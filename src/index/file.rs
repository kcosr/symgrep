//! File-based index backend.
//!
//! This backend stores index data under a `.symgrep/` directory:
//! - `meta.json`
//! - `files.jsonl`
//! - `symbols.jsonl`
//!
//! The implementation is intentionally simple and optimized for
//! clarity rather than micro-performance. It uses sequential scans
//! and full rewrites of the JSONL files when updating symbols.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::index::backend::IndexBackend;
use crate::index::build_globset;
use crate::index::models::{FileRecord, IndexMeta, NewSymbolRecord, SymbolQuery, SymbolRecord};
use crate::models::IndexBackendKind;

type FileMaps = (
    Vec<FileRecord>,
    HashMap<PathBuf, FileRecord>,
    HashMap<u64, FileRecord>,
);

/// File-backed implementation of `IndexBackend`.
pub struct FileIndexBackend {
    root: PathBuf,
    meta: Option<IndexMeta>,
    files: Vec<FileRecord>,
    files_by_path: HashMap<PathBuf, FileRecord>,
    files_by_id: HashMap<u64, FileRecord>,
    next_file_id: u64,
    next_symbol_id: u64,
}

impl FileIndexBackend {
    /// Open (or create) a file-based index at the given path.
    pub fn open(index_path: &Path) -> Result<Self> {
        fs::create_dir_all(index_path)?;

        let meta_path = index_path.join("meta.json");
        let meta = if meta_path.exists() {
            let file = File::open(&meta_path)?;
            let meta: IndexMeta = serde_json::from_reader(file)?;
            if meta.schema_version != "1" {
                anyhow::bail!(
                    "unsupported index schema version {}; expected 1",
                    meta.schema_version
                );
            }
            Some(meta)
        } else {
            None
        };

        let (files, files_by_path, files_by_id) = Self::load_files(index_path)?;
        let (next_file_id, next_symbol_id) = Self::compute_next_ids(index_path, &files)?;

        Ok(Self {
            root: index_path.to_path_buf(),
            meta,
            files,
            files_by_path,
            files_by_id,
            next_file_id,
            next_symbol_id,
        })
    }

    fn meta_path(&self) -> PathBuf {
        self.root.join("meta.json")
    }

    fn files_path(&self) -> PathBuf {
        self.root.join("files.jsonl")
    }

    fn symbols_path(&self) -> PathBuf {
        self.root.join("symbols.jsonl")
    }

    fn load_files(root: &Path) -> Result<FileMaps> {
        let path = root.join("files.jsonl");
        if !path.exists() {
            return Ok((Vec::new(), HashMap::new(), HashMap::new()));
        }

        let file = File::open(&path)?;
        let reader = BufReader::new(file);

        let mut files = Vec::new();
        let mut by_path = HashMap::new();
        let mut by_id = HashMap::new();

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            let record: FileRecord = serde_json::from_str(&line)?;
            by_path.insert(record.path.clone(), record.clone());
            by_id.insert(record.id, record.clone());
            files.push(record);
        }

        Ok((files, by_path, by_id))
    }

    fn compute_next_ids(root: &Path, files: &[FileRecord]) -> Result<(u64, u64)> {
        let mut max_file_id = files.iter().map(|f| f.id).max().unwrap_or(0);
        let mut max_symbol_id: u64 = 0;

        let symbols_path = root.join("symbols.jsonl");
        if symbols_path.exists() {
            let file = File::open(&symbols_path)?;
            let reader = BufReader::new(file);

            for line in reader.lines() {
                let line = line?;
                if line.trim().is_empty() {
                    continue;
                }

                let record: SymbolRecord = serde_json::from_str(&line)?;
                if record.id > max_symbol_id {
                    max_symbol_id = record.id;
                }
                if record.file_id > max_file_id {
                    max_file_id = record.file_id;
                }
            }
        }

        let next_file_id = max_file_id.saturating_add(1);
        let next_symbol_id = max_symbol_id.saturating_add(1);

        Ok((next_file_id, next_symbol_id))
    }

    fn persist_files(&self) -> Result<()> {
        let path = self.files_path();
        let tmp_path = path.with_extension("jsonl.tmp");

        let file = File::create(&tmp_path)?;
        let mut writer = BufWriter::new(file);

        for record in &self.files {
            serde_json::to_writer(&mut writer, record)?;
            writer.write_all(b"\n")?;
        }

        writer.flush()?;
        fs::rename(tmp_path, path)?;
        Ok(())
    }

    fn persist_meta(&self, meta: &IndexMeta) -> Result<()> {
        let path = self.meta_path();
        let file = File::create(&path)?;
        serde_json::to_writer(file, meta)?;
        Ok(())
    }

    fn rewrite_symbols_excluding_file(&self, file_id: u64) -> Result<()> {
        let path = self.symbols_path();
        if !path.exists() {
            return Ok(());
        }

        let tmp_path = path.with_extension("jsonl.tmp");

        let in_file = File::open(&path)?;
        let reader = BufReader::new(in_file);
        let out_file = File::create(&tmp_path)?;
        let mut writer = BufWriter::new(out_file);

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            let record: SymbolRecord = serde_json::from_str(&line)?;
            if record.file_id == file_id {
                continue;
            }

            serde_json::to_writer(&mut writer, &record)?;
            writer.write_all(b"\n")?;
        }

        writer.flush()?;
        fs::rename(tmp_path, path)?;

        Ok(())
    }

    fn allocate_file_id(&mut self) -> u64 {
        let id = self.next_file_id;
        self.next_file_id = self.next_file_id.saturating_add(1);
        id
    }
}

impl IndexBackend for FileIndexBackend {
    fn kind(&self) -> IndexBackendKind {
        IndexBackendKind::File
    }

    fn index_path(&self) -> &Path {
        &self.root
    }

    fn load_meta(&self) -> Result<IndexMeta> {
        if let Some(meta) = &self.meta {
            Ok(meta.clone())
        } else {
            let now = crate::index::current_epoch_seconds();
            Ok(IndexMeta {
                schema_version: "1".to_string(),
                tool_version: env!("CARGO_PKG_VERSION").to_string(),
                root_path: String::new(),
                created_at: now,
                updated_at: now,
            })
        }
    }

    fn save_meta(&mut self, meta: &IndexMeta) -> Result<()> {
        self.meta = Some(meta.clone());
        self.persist_meta(meta)
    }

    fn list_files(&self) -> Result<Vec<FileRecord>> {
        Ok(self.files.clone())
    }

    fn get_file_by_path(&self, path: &Path) -> Result<Option<FileRecord>> {
        Ok(self.files_by_path.get(path).cloned())
    }

    fn get_file_by_id(&self, id: u64) -> Result<Option<FileRecord>> {
        Ok(self.files_by_id.get(&id).cloned())
    }

    fn upsert_file(
        &mut self,
        path: &Path,
        language: &str,
        hash: Option<&str>,
        mtime: i64,
        size: u64,
    ) -> Result<FileRecord> {
        let path_buf = path.to_path_buf();
        let hash_value = hash.map(|h| h.to_string());

        let record = if let Some(existing) = self.files_by_path.get(&path_buf).cloned() {
            let mut record = existing;
            record.language = language.to_string();
            record.hash = hash_value;
            record.mtime = mtime;
            record.size = size;
            record
        } else {
            FileRecord {
                id: self.allocate_file_id(),
                path: path_buf.clone(),
                language: language.to_string(),
                hash: hash_value,
                mtime,
                size,
            }
        };

        // Update in-memory collections.
        if let Some(existing) = self.files_by_path.get(&path_buf) {
            let id = existing.id;
            if let Some(slot) = self.files.iter_mut().find(|f| f.id == id) {
                *slot = record.clone();
            }
        } else {
            self.files.push(record.clone());
        }

        self.files_by_path.insert(path_buf, record.clone());
        self.files_by_id.insert(record.id, record.clone());

        self.persist_files()?;

        Ok(record)
    }

    fn remove_file_by_path(&mut self, path: &Path) -> Result<()> {
        let path_buf = path.to_path_buf();

        if let Some(record) = self.files_by_path.remove(&path_buf) {
            self.files.retain(|f| f.id != record.id);
            self.files_by_id.remove(&record.id);
            self.persist_files()?;
            self.rewrite_symbols_excluding_file(record.id)?;
        }

        Ok(())
    }

    fn set_file_symbols(&mut self, file_id: u64, symbols: &[NewSymbolRecord]) -> Result<()> {
        let path = self.symbols_path();
        let tmp_path = path.with_extension("jsonl.tmp");

        let mut next_id = self.next_symbol_id;

        if path.exists() {
            let in_file = File::open(&path)?;
            let reader = BufReader::new(in_file);
            let out_file = File::create(&tmp_path)?;
            let mut writer = BufWriter::new(out_file);

            for line in reader.lines() {
                let line = line?;
                if line.trim().is_empty() {
                    continue;
                }

                let record: SymbolRecord = serde_json::from_str(&line)?;
                if record.file_id == file_id {
                    continue;
                }

                serde_json::to_writer(&mut writer, &record)?;
                writer.write_all(b"\n")?;
            }

            for symbol in symbols {
                let record = SymbolRecord {
                    id: next_id,
                    file_id,
                    name: symbol.name.clone(),
                    kind: symbol.kind,
                    language: symbol.language.clone(),
                    range: symbol.range,
                    signature: symbol.signature.clone(),
                    extra: symbol.extra.clone(),
                };

                next_id = next_id.saturating_add(1);

                serde_json::to_writer(&mut writer, &record)?;
                writer.write_all(b"\n")?;
            }

            writer.flush()?;
        } else {
            let out_file = File::create(&tmp_path)?;
            let mut writer = BufWriter::new(out_file);

            for symbol in symbols {
                let record = SymbolRecord {
                    id: next_id,
                    file_id,
                    name: symbol.name.clone(),
                    kind: symbol.kind,
                    language: symbol.language.clone(),
                    range: symbol.range,
                    signature: symbol.signature.clone(),
                    extra: symbol.extra.clone(),
                };

                next_id = next_id.saturating_add(1);

                serde_json::to_writer(&mut writer, &record)?;
                writer.write_all(b"\n")?;
            }

            writer.flush()?;
        }

        fs::rename(tmp_path, path)?;

        self.next_symbol_id = next_id;

        Ok(())
    }

    fn query_symbols(&self, query: &SymbolQuery) -> Result<Vec<SymbolRecord>> {
        let path = self.symbols_path();
        if !path.exists() {
            return Ok(Vec::new());
        }

        let include_globs = build_globset(&query.globs)?;
        let exclude_globs = build_globset(&query.exclude_globs)?;

        let file = File::open(&path)?;
        let reader = BufReader::new(file);

        let mut results = Vec::new();

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            let record: SymbolRecord = serde_json::from_str(&line)?;

            if let Some(sub) = &query.name_substring {
                if !record.name.contains(sub) {
                    continue;
                }
            }

            if let Some(lang) = &query.language {
                if !record.language.eq_ignore_ascii_case(lang) {
                    continue;
                }
            }

            let file_record = match self.files_by_id.get(&record.file_id) {
                Some(f) => f,
                None => continue,
            };

            let file_path = &file_record.path;

            if !query.paths.is_empty()
                && !query.paths.iter().any(|root| file_path.starts_with(root))
            {
                continue;
            }

            if let Some(set) = &include_globs {
                if !set.is_match(file_path) {
                    continue;
                }
            }

            if let Some(set) = &exclude_globs {
                if set.is_match(file_path) {
                    continue;
                }
            }

            results.push(record);
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::models::{NewSymbolRecord, SymbolQuery};
    use crate::models::{SymbolKind, TextRange};
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn file_backend_persists_files_and_symbols() {
        let dir = tempdir().expect("tempdir");
        let index_root = dir.path().join(".symgrep");

        let mut backend = FileIndexBackend::open(&index_root).expect("backend");

        let file = backend
            .upsert_file(
                Path::new("src/lib.rs"),
                "typescript",
                None,
                1_700_000_000,
                42,
            )
            .expect("file record");

        assert_eq!(file.id, 1);
        assert_eq!(file.path, PathBuf::from("src/lib.rs"));

        let files = backend.list_files().expect("list files");
        assert_eq!(files.len(), 1);

        let fetched = backend
            .get_file_by_path(Path::new("src/lib.rs"))
            .expect("by path")
            .expect("record");
        assert_eq!(fetched.id, file.id);

        let symbols = vec![NewSymbolRecord {
            file_id: file.id,
            name: "add".to_string(),
            kind: SymbolKind::Function,
            language: "typescript".to_string(),
            range: TextRange {
                start_line: 1,
                start_column: 1,
                end_line: 1,
                end_column: 10,
            },
            signature: Some("add(a: number, b: number): number".to_string()),
            extra: None,
        }];

        backend
            .set_file_symbols(file.id, &symbols)
            .expect("set symbols");

        let query = SymbolQuery {
            name_substring: Some("add".to_string()),
            language: Some("typescript".to_string()),
            paths: vec![PathBuf::from("src")],
            globs: Vec::new(),
            exclude_globs: Vec::new(),
        };

        let results = backend.query_symbols(&query).expect("query symbols");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "add");

        // Removing the file should also remove its symbols.
        backend
            .remove_file_by_path(Path::new("src/lib.rs"))
            .expect("remove file");

        let files_after = backend.list_files().expect("list files");
        assert!(files_after.is_empty());

        let results_after = backend.query_symbols(&query).expect("query symbols");
        assert!(results_after.is_empty());
    }
}
