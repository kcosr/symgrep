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
use crate::models::{IndexConfig, IndexSummary};

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

    let mut backend = backend::open_backend(config)?;

    let meta = backend.load_meta().unwrap_or_else(|_| {
        let now = current_epoch_seconds();
        IndexMeta {
            schema_version: "1".to_string(),
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
            schema_version: "1".to_string(),
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

        let new_symbols: Vec<NewSymbolRecord> = symbols
            .into_iter()
            .map(|s| NewSymbolRecord {
                file_id: file_record.id,
                name: s.name,
                kind: s.kind,
                language: s.language,
                range: s.range,
                signature: s.signature,
                extra: None,
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
