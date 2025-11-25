//! Indexing backends and related types.
//!
//! This module defines the shared logical index model plus the
//! pluggable backend abstraction used by both the CLI `symgrep
//! index` command and `--use-index` search integration.
//!
//! Phase 6A provides a simple file-based backend that stores its
//! data under a `.symgrep/` directory using JSON/JSONL files. Later
//! phases will add a SQLite backend with the same logical model.

mod backend;
mod file;
pub mod models;
mod sqlite;

pub use backend::{open_backend, IndexBackend};
pub use file::FileIndexBackend;
pub use models::{FileRecord, IndexMeta, NewSymbolRecord, ProjectIndex, SymbolQuery, SymbolRecord};
pub use sqlite::SqliteIndexBackend;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Result};
use globset::{Glob, GlobSet};
use ignore::WalkBuilder;

use crate::language::{backend_for_language, backend_for_path};
use crate::models::{
    IndexConfig, IndexSummary, Symbol, SymbolAttributes, SymbolAttributesRequest,
    SymbolAttributesResponse, SymbolKind,
};
use serde_json::Value;

/// Run indexing for the given configuration using the configured backend.
///
/// This function is the core entry point used by the CLI and tests.
pub fn run_index(config: IndexConfig) -> Result<IndexSummary> {
    let mut backend = backend::open_backend(&config)?;
    build_index(backend.as_mut(), &config)
}

/// Read-only helper to inspect an existing index without modifying it.
///
/// This function opens the configured backend, reads its metadata, and
/// computes aggregate file and symbol counts. It does not create or
/// update any on-disk index data.
pub fn get_index_info(config: &IndexConfig) -> Result<IndexSummary> {
    match config.backend {
        crate::models::IndexBackendKind::File => {
            if !config.index_path.exists() {
                bail!("index not found at {}", config.index_path.display());
            }
            if !config.index_path.is_dir() {
                bail!(
                    "file backend requires index_path to be a directory; got {}",
                    config.index_path.display()
                );
            }
        }
        crate::models::IndexBackendKind::Sqlite => {
            if !config.index_path.exists() {
                bail!("index not found at {}", config.index_path.display());
            }
            if !config.index_path.is_file() {
                bail!(
                    "sqlite backend requires index_path to be a file; got {}",
                    config.index_path.display()
                );
            }
        }
    }

    let backend = backend::open_backend(config)?;

    let meta = backend.load_meta().unwrap_or_else(|_| {
        let now = current_epoch_seconds();
        IndexMeta {
            schema_version: "2".to_string(),
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            root_path: String::new(),
            created_at: now,
            updated_at: now,
        }
    });

    let files = backend.list_files()?;
    let files_indexed = files.len() as u64;

    let symbol_query = SymbolQuery {
        name_substring: None,
        language: None,
        paths: Vec::new(),
        globs: Vec::new(),
        exclude_globs: Vec::new(),
    };
    let symbols = backend.query_symbols(&symbol_query)?;
    let symbols_indexed = symbols.len() as u64;

    let created_at_iso = format_timestamp_iso8601(meta.created_at);
    let updated_at_iso = format_timestamp_iso8601(meta.updated_at);

    let root_path_opt = if meta.root_path.is_empty() {
        None
    } else {
        Some(meta.root_path)
    };

    Ok(IndexSummary {
        backend: backend.kind(),
        index_path: config.index_path.clone(),
        files_indexed,
        symbols_indexed,
        root_path: root_path_opt,
        schema_version: Some(meta.schema_version),
        tool_version: Some(meta.tool_version),
        created_at: created_at_iso,
        updated_at: updated_at_iso,
    })
}

/// Core indexing routine shared between the CLI, tests, and future daemon.
pub(crate) fn build_index(
    backend: &mut dyn backend::IndexBackend,
    config: &IndexConfig,
) -> Result<IndexSummary> {
    if config.paths.is_empty() {
        bail!("at least one index path is required");
    }

    for path in &config.paths {
        if !path.exists() {
            bail!("index path does not exist: {}", path.display());
        }
    }

    let canonical_root = config.paths[0]
        .canonicalize()
        .unwrap_or_else(|_| config.paths[0].clone());

    // Load or initialize index metadata and enforce root_path semantics.
    let mut meta = backend.load_meta().unwrap_or_else(|_| {
        let now = current_epoch_seconds();
        IndexMeta {
            schema_version: "2".to_string(),
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            root_path: String::new(),
            created_at: now,
            updated_at: now,
        }
    });

    if meta.root_path.is_empty() {
        meta.root_path = canonical_root.to_string_lossy().to_string();
    } else if let Ok(stored_root) = PathBuf::from(&meta.root_path).canonicalize() {
        if stored_root != canonical_root {
            bail!(
                "index root_path mismatch: index was created with root {}, but {} was requested",
                stored_root.display(),
                canonical_root.display()
            );
        }
    }

    // Upgrade older index metadata to the current logical schema
    // version while preserving other fields.
    if meta.schema_version != "2" {
        // Older schema versions (e.g. "1") are still readable but
        // are upgraded in-place on the next successful index run.
        if meta.schema_version != "1" {
            bail!(
                "unsupported index schema version {}; expected 1 or 2",
                meta.schema_version
            );
        }
        meta.schema_version = "2".to_string();
    }

    let include_globs = build_globset(&config.globs)?;
    let exclude_globs = build_globset(&config.exclude_globs)?;

    let existing_files = backend.list_files()?;
    let mut existing_by_path: HashMap<PathBuf, FileRecord> = existing_files
        .iter()
        .map(|f| (f.path.clone(), f.clone()))
        .collect();
    let mut seen_paths = HashSet::new();

    let mut builder = WalkBuilder::new(&config.paths[0]);
    for path in config.paths.iter().skip(1) {
        builder.add(path);
    }
    let walker = builder.build();

    let mut files_indexed: u64 = 0;
    let mut symbols_indexed: u64 = 0;

    for entry_result in walker {
        let entry = match entry_result {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();

        if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
            continue;
        }

        if let Some(set) = &include_globs {
            if !set.is_match(path) {
                continue;
            }
        }
        if let Some(set) = &exclude_globs {
            if set.is_match(path) {
                continue;
            }
        }

        let language_backend = if let Some(lang) = &config.language {
            let backend = backend_for_language(lang).ok_or_else(|| {
                anyhow::anyhow!(
                    "indexing is only supported for known languages (e.g., typescript, javascript, cpp); got {}",
                    lang
                )
            })?;

            // If the file extension is not supported by the selected backend,
            // skip this file.
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !backend
                .file_extensions()
                .iter()
                .any(|e| e.eq_ignore_ascii_case(ext))
            {
                continue;
            }

            backend
        } else {
            match backend_for_path(path) {
                Some(b) => b,
                None => continue,
            }
        };

        let metadata = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let size = metadata.len();

        let path_buf = path.to_path_buf();
        seen_paths.insert(path_buf.clone());

        let needs_reindex = match existing_by_path.get(&path_buf) {
            Some(file_record) => file_record.mtime != mtime || file_record.size != size,
            None => true,
        };

        if !needs_reindex {
            continue;
        }

        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let parsed = match language_backend.parse_file(path, &source) {
            Ok(p) => p,
            Err(_) => continue,
        };

        let symbols = match language_backend.index_symbols(&parsed) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let file_record = backend.upsert_file(path, language_backend.id(), None, mtime, size)?;

        existing_by_path.insert(file_record.path.clone(), file_record.clone());

        // Load existing symbols for this file so we can preserve
        // externally-managed attributes (keywords, descriptions)
        // across reindex runs.
        let existing_symbols = backend.query_symbols(&SymbolQuery {
            name_substring: None,
            language: Some(file_record.language.clone()),
            paths: vec![file_record.path.clone()],
            globs: Vec::new(),
            exclude_globs: Vec::new(),
        })?;

        let mut existing_by_identity = std::collections::HashMap::new();
        for record in existing_symbols {
            let identity = SymbolIdentity::from_record(&record);
            existing_by_identity.insert(identity, record);
        }

        let new_symbols: Vec<NewSymbolRecord> = symbols
            .into_iter()
            .map(|s| {
                let identity = SymbolIdentity::from_symbol(&s);
                let existing = existing_by_identity.get(&identity);
                let merged_attrs = merge_symbol_attributes_for_index(existing, &s);

                NewSymbolRecord {
                    file_id: file_record.id,
                    name: s.name,
                    kind: s.kind,
                    language: s.language,
                    range: s.range,
                    signature: s.signature,
                    extra: symbol_attributes_to_extra(&merged_attrs),
                }
            })
            .collect();

        backend.set_file_symbols(file_record.id, &new_symbols)?;

        files_indexed += 1;
        symbols_indexed += new_symbols.len() as u64;
    }

    // Remove stale entries for files that no longer exist under the
    // indexed paths.
    for file in existing_files {
        if !seen_paths.contains(&file.path) && path_within_any(&file.path, &config.paths) {
            backend.remove_file_by_path(&file.path)?;
        }
    }

    meta.updated_at = current_epoch_seconds();
    backend.save_meta(&meta)?;

    let created_at_iso = format_timestamp_iso8601(meta.created_at);
    let updated_at_iso = format_timestamp_iso8601(meta.updated_at);

    Ok(IndexSummary {
        backend: backend.kind(),
        index_path: config.index_path.clone(),
        files_indexed,
        symbols_indexed,
        root_path: Some(meta.root_path),
        schema_version: Some(meta.schema_version),
        tool_version: Some(meta.tool_version),
        created_at: created_at_iso,
        updated_at: updated_at_iso,
    })
}

fn path_within_any(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| path.starts_with(root))
}

/// Internal identity key used to match symbols across index runs for
/// a single file.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SymbolIdentity {
    kind: SymbolKind,
    name: String,
    start_line: u32,
    end_line: u32,
    signature: Option<String>,
}

impl SymbolIdentity {
    fn from_symbol(symbol: &Symbol) -> Self {
        Self {
            kind: symbol.kind,
            name: symbol.name.clone(),
            start_line: symbol.range.start_line,
            end_line: symbol.range.end_line,
            signature: symbol.signature.clone(),
        }
    }

    fn from_record(record: &SymbolRecord) -> Self {
        Self {
            kind: record.kind,
            name: record.name.clone(),
            start_line: record.range.start_line,
            end_line: record.range.end_line,
            signature: record.signature.clone(),
        }
    }
}

fn empty_symbol_attributes() -> SymbolAttributes {
    SymbolAttributes {
        comment: None,
        comment_range: None,
        keywords: Vec::new(),
        description: None,
    }
}

fn merge_symbol_attributes_for_index(
    existing: Option<&SymbolRecord>,
    symbol: &Symbol,
) -> SymbolAttributes {
    let (new_comment, new_comment_range) = match symbol.attributes.as_ref() {
        Some(attrs) => (attrs.comment.clone(), attrs.comment_range),
        None => (None, None),
    };

    let mut merged = empty_symbol_attributes();
    merged.comment = new_comment;
    merged.comment_range = new_comment_range;

    if let Some(record) = existing {
        if let Some(existing_attrs) = symbol_attributes_from_extra(&record.extra) {
            // Preserve externally-owned attributes across reindex
            // runs; comments always come from fresh AST extraction.
            merged.keywords = existing_attrs.keywords;
            merged.description = existing_attrs.description;
        }
    }

    merged
}

/// Convert optional symbol attributes into a serialized `extra`
/// payload for the index.
fn symbol_attributes_to_extra(attrs: &SymbolAttributes) -> Option<Value> {
    let has_comment = attrs.comment.is_some();
    let has_comment_range = attrs.comment_range.is_some();
    let has_keywords = !attrs.keywords.is_empty();
    let has_desc = attrs.description.is_some();

    if !has_comment && !has_comment_range && !has_keywords && !has_desc {
        return None;
    }

    serde_json::to_value(attrs).ok()
}

/// Hydrate `SymbolAttributes` from an indexed `extra` payload.
pub(crate) fn symbol_attributes_from_extra(extra: &Option<Value>) -> Option<SymbolAttributes> {
    extra
        .as_ref()
        .and_then(|v| {
            if v.is_object() {
                serde_json::from_value(v.clone()).ok()
            } else {
                None
            }
        })
}

pub(crate) fn build_globset(patterns: &[String]) -> Result<Option<GlobSet>> {
    if patterns.is_empty() {
        return Ok(None);
    }

    let mut builder = globset::GlobSetBuilder::new();
    for pat in patterns {
        builder.add(Glob::new(pat)?);
    }
    Ok(Some(builder.build()?))
}

fn current_epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn format_timestamp_iso8601(secs: u64) -> Option<String> {
    use time::{format_description::well_known::Rfc3339, OffsetDateTime};

    let ts = secs as i64;
    let dt = OffsetDateTime::from_unix_timestamp(ts).ok()?;
    Some(dt.format(&Rfc3339).unwrap_or_else(|_| dt.to_string()))
}

/// Update keywords/description attributes for a single symbol in an
/// existing index, identified by a `SymbolSelector`.
pub fn update_symbol_attributes(
    request: SymbolAttributesRequest,
) -> Result<SymbolAttributesResponse> {
    let mut backend = open_backend(&request.index)?;

    let selector = request.selector;
    let update = request.attributes;

    let file_record = backend
        .get_file_by_path(&selector.file)?
        .ok_or_else(|| anyhow::anyhow!("symbol file not found in index: {}", selector.file.display()))?;

    let symbol_query = SymbolQuery {
        name_substring: None,
        language: Some(selector.language.clone()),
        paths: vec![selector.file.clone()],
        globs: Vec::new(),
        exclude_globs: Vec::new(),
    };

    let mut records = backend.query_symbols(&symbol_query)?;

    if records.is_empty() {
        anyhow::bail!(
            "no symbols found in index for file {} and language {}",
            selector.file.display(),
            selector.language
        );
    }

    let mut target_idx: Option<usize> = None;

    for (idx, record) in records.iter().enumerate() {
        if record.file_id != file_record.id {
            continue;
        }

        if record.kind != selector.kind {
            continue;
        }

        if record.name != selector.name {
            continue;
        }

        if record.range.start_line != selector.start_line
            || record.range.end_line != selector.end_line
        {
            continue;
        }

        if let Some(prev) = target_idx {
            anyhow::bail!(
                "selector matched multiple symbols in index (at least ids {} and {})",
                records[prev].id,
                record.id
            );
        }

        target_idx = Some(idx);
    }

    let target_idx =
        target_idx.ok_or_else(|| anyhow::anyhow!("no symbol matched the provided selector"))?;

    // Compute updated attributes for the target symbol.
    let target_record = &records[target_idx];
    let target_range = target_record.range;
    let target_signature = target_record.signature.clone();
    let mut target_attrs = symbol_attributes_from_extra(&target_record.extra)
        .unwrap_or_else(empty_symbol_attributes);
    target_attrs.keywords = update.keywords;
    target_attrs.description = update.description;

    // Rewrite all symbols for this file, updating only the target
    // symbol's attributes.
    let mut new_symbols: Vec<NewSymbolRecord> = Vec::with_capacity(records.len());

    for (idx, record) in records.into_iter().enumerate() {
        let attrs = if idx == target_idx {
            target_attrs.clone()
        } else {
            symbol_attributes_from_extra(&record.extra).unwrap_or_else(empty_symbol_attributes)
        };

        let extra = symbol_attributes_to_extra(&attrs);

        new_symbols.push(NewSymbolRecord {
            file_id: record.file_id,
            name: record.name,
            kind: record.kind,
            language: record.language,
            range: record.range,
            signature: record.signature,
            extra,
        });
    }

    backend.set_file_symbols(file_record.id, &new_symbols)?;

    let updated_symbol = Symbol {
        name: selector.name,
        kind: selector.kind,
        language: selector.language,
        file: selector.file,
        range: target_range,
        signature: target_signature,
        attributes: Some(target_attrs),
        def_line_count: None,
        matches: Vec::new(),
         calls: Vec::new(),
         called_by: Vec::new(),
    };

    Ok(SymbolAttributesResponse {
        symbol: updated_symbol,
    })
}
