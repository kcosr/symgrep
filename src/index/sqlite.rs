//! SQLite-based index backend.
//!
//! This backend stores the logical index model in a single SQLite
//! database file with the following schema:
//!
//! - `meta(key TEXT PRIMARY KEY, value TEXT NOT NULL)`
//! - `files(id INTEGER PRIMARY KEY, path TEXT UNIQUE, language TEXT, hash TEXT, mtime INTEGER, size INTEGER)`
//! - `symbols(id INTEGER PRIMARY KEY, file_id INTEGER, name TEXT, kind TEXT, language TEXT,
//!            start_line INTEGER, start_col INTEGER, end_line INTEGER, end_col INTEGER,
//!            signature TEXT, extra TEXT)`
//!
//! The schema is intentionally close to the file-based backend's
//! logical model. The backend uses write transactions for index
//! updates and read-only queries for search. The SQLite connection
//! is configured with:
//!
//! - `journal_mode = WAL` for concurrent readers and a single writer.
//! - `synchronous = NORMAL` as a balance between safety and speed.
//! - `busy_timeout` to avoid transient `database is locked` errors.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

use crate::index::backend::IndexBackend;
use crate::index::build_globset;
use crate::index::models::{FileRecord, IndexMeta, NewSymbolRecord, SymbolQuery, SymbolRecord};
use crate::models::{IndexBackendKind, SymbolKind, TextRange};

/// SQLite-backed implementation of `IndexBackend`.
pub struct SqliteIndexBackend {
    path: PathBuf,
    conn: Connection,
}

impl SqliteIndexBackend {
    /// Open (or create) a SQLite index at the given path.
    pub fn open(index_path: &Path) -> Result<Self> {
        if let Some(parent) = index_path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }

        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE;
        let conn = Connection::open_with_flags(index_path, flags)?;

        // Enable basic pragmas suitable for concurrent read-heavy workloads.
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_millis(5000))?;

        Self::initialize_schema(&conn)?;

        Ok(Self {
            path: index_path.to_path_buf(),
            conn,
        })
    }

    fn initialize_schema(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS meta (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS files (
                id       INTEGER PRIMARY KEY,
                path     TEXT NOT NULL UNIQUE,
                language TEXT NOT NULL,
                hash     TEXT,
                mtime    INTEGER NOT NULL,
                size     INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS symbols (
                id          INTEGER PRIMARY KEY,
                file_id     INTEGER NOT NULL,
                name        TEXT NOT NULL,
                kind        TEXT NOT NULL,
                language    TEXT NOT NULL,
                start_line  INTEGER NOT NULL,
                start_col   INTEGER NOT NULL,
                end_line    INTEGER NOT NULL,
                end_col     INTEGER NOT NULL,
                signature   TEXT,
                extra       TEXT,
                FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_symbols_name
                ON symbols(name);
            CREATE INDEX IF NOT EXISTS idx_symbols_kind
                ON symbols(kind);
            CREATE INDEX IF NOT EXISTS idx_symbols_language
                ON symbols(language);
            CREATE INDEX IF NOT EXISTS idx_symbols_file_id
                ON symbols(file_id);
        "#,
        )?;

        Ok(())
    }

    fn load_files(&self) -> Result<Vec<FileRecord>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, path, language, hash, mtime, size FROM files ORDER BY id ASC")?;

        let rows = stmt.query_map([], |row| {
            let id: i64 = row.get(0)?;
            let path: String = row.get(1)?;
            let language: String = row.get(2)?;
            let hash: Option<String> = row.get(3)?;
            let mtime: i64 = row.get(4)?;
            let size: i64 = row.get(5)?;

            Ok(FileRecord {
                id: id as u64,
                path: PathBuf::from(path),
                language,
                hash,
                mtime,
                size: size as u64,
            })
        })?;

        let mut files = Vec::new();
        for row in rows {
            files.push(row?);
        }

        Ok(files)
    }

    fn symbol_kind_to_str(kind: SymbolKind) -> &'static str {
        match kind {
            SymbolKind::Function => "function",
            SymbolKind::Method => "method",
            SymbolKind::Class => "class",
            SymbolKind::Interface => "interface",
            SymbolKind::Variable => "variable",
            SymbolKind::Namespace => "namespace",
        }
    }

    fn symbol_kind_from_str(s: &str) -> Result<SymbolKind> {
        match s {
            "function" => Ok(SymbolKind::Function),
            "method" => Ok(SymbolKind::Method),
            "class" => Ok(SymbolKind::Class),
            "interface" => Ok(SymbolKind::Interface),
            "variable" => Ok(SymbolKind::Variable),
            "namespace" => Ok(SymbolKind::Namespace),
            other => bail!("unknown symbol kind in sqlite index: {other}"),
        }
    }
}

impl IndexBackend for SqliteIndexBackend {
    fn kind(&self) -> IndexBackendKind {
        IndexBackendKind::Sqlite
    }

    fn index_path(&self) -> &Path {
        &self.path
    }

    fn load_meta(&self) -> Result<IndexMeta> {
        let mut stmt = self.conn.prepare("SELECT key, value FROM meta")?;
        let rows = stmt.query_map([], |row| {
            let key: String = row.get(0)?;
            let value: String = row.get(1)?;
            Ok((key, value))
        })?;

        let mut map = HashMap::new();
        for row in rows {
            let (key, value) = row?;
            map.insert(key, value);
        }

        if map.is_empty() {
            let now = crate::index::current_epoch_seconds();
            return Ok(IndexMeta {
                schema_version: "2".to_string(),
                tool_version: env!("CARGO_PKG_VERSION").to_string(),
                root_path: String::new(),
                created_at: now,
                updated_at: now,
            });
        }

        let schema_version = map
            .get("schema_version")
            .cloned()
            .unwrap_or_else(|| "1".to_string());

        if schema_version != "1" && schema_version != "2" {
            bail!(
                "unsupported index schema version {}; expected 1 or 2",
                schema_version
            );
        }

        let tool_version = map
            .get("tool_version")
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());

        let root_path = map.get("root_path").cloned().unwrap_or_default();

        let created_at = map
            .get("created_at")
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        let updated_at = map
            .get("updated_at")
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(created_at);

        Ok(IndexMeta {
            schema_version,
            tool_version,
            root_path,
            created_at,
            updated_at,
        })
    }

    fn save_meta(&mut self, meta: &IndexMeta) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM meta", [])?;

        {
            let mut stmt = tx.prepare("INSERT INTO meta (key, value) VALUES (?1, ?2)")?;

            let rows = [
                ("schema_version", meta.schema_version.as_str()),
                ("tool_version", meta.tool_version.as_str()),
                ("root_path", meta.root_path.as_str()),
                ("created_at", &meta.created_at.to_string()),
                ("updated_at", &meta.updated_at.to_string()),
            ];

            for (key, value) in rows {
                stmt.execute(params![key, value])?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    fn list_files(&self) -> Result<Vec<FileRecord>> {
        self.load_files()
    }

    fn get_file_by_path(&self, path: &Path) -> Result<Option<FileRecord>> {
        let path_str = path.to_string_lossy().to_string();
        let mut stmt = self
            .conn
            .prepare("SELECT id, path, language, hash, mtime, size FROM files WHERE path = ?1")?;

        let row = stmt
            .query_row(params![path_str], |row| {
                let id: i64 = row.get(0)?;
                let path: String = row.get(1)?;
                let language: String = row.get(2)?;
                let hash: Option<String> = row.get(3)?;
                let mtime: i64 = row.get(4)?;
                let size: i64 = row.get(5)?;

                Ok(FileRecord {
                    id: id as u64,
                    path: PathBuf::from(path),
                    language,
                    hash,
                    mtime,
                    size: size as u64,
                })
            })
            .optional()?;

        Ok(row)
    }

    fn get_file_by_id(&self, id: u64) -> Result<Option<FileRecord>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, path, language, hash, mtime, size FROM files WHERE id = ?1")?;

        let row = stmt
            .query_row(params![id as i64], |row| {
                let id: i64 = row.get(0)?;
                let path: String = row.get(1)?;
                let language: String = row.get(2)?;
                let hash: Option<String> = row.get(3)?;
                let mtime: i64 = row.get(4)?;
                let size: i64 = row.get(5)?;

                Ok(FileRecord {
                    id: id as u64,
                    path: PathBuf::from(path),
                    language,
                    hash,
                    mtime,
                    size: size as u64,
                })
            })
            .optional()?;

        Ok(row)
    }

    fn upsert_file(
        &mut self,
        path: &Path,
        language: &str,
        hash: Option<&str>,
        mtime: i64,
        size: u64,
    ) -> Result<FileRecord> {
        let path_str = path.to_string_lossy().to_string();
        let hash_value = hash.map(|h| h.to_string());

        let tx = self.conn.transaction()?;

        let existing: Option<(i64, String, Option<String>, i64, i64)> = {
            let mut stmt =
                tx.prepare("SELECT id, language, hash, mtime, size FROM files WHERE path = ?1")?;

            stmt.query_row(params![path_str], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .optional()?
        };

        let record =
            if let Some((id, _existing_lang, _existing_hash, _existing_mtime, _existing_size)) =
                existing
            {
                tx.execute(
                "UPDATE files SET language = ?1, hash = ?2, mtime = ?3, size = ?4 WHERE id = ?5",
                params![language, hash_value, mtime, size as i64, id],
            )?;

                FileRecord {
                    id: id as u64,
                    path: PathBuf::from(&path_str),
                    language: language.to_string(),
                    hash: hash_value,
                    mtime,
                    size,
                }
            } else {
                tx.execute(
                "INSERT INTO files (path, language, hash, mtime, size) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![path_str, language, hash_value, mtime, size as i64],
            )?;

                let id = tx.last_insert_rowid();

                FileRecord {
                    id: id as u64,
                    path: PathBuf::from(&path_str),
                    language: language.to_string(),
                    hash: hash.map(|h| h.to_string()),
                    mtime,
                    size,
                }
            };

        tx.commit()?;
        Ok(record)
    }

    fn remove_file_by_path(&mut self, path: &Path) -> Result<()> {
        let path_str = path.to_string_lossy().to_string();
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM files WHERE path = ?1", params![path_str])?;
        tx.commit()?;
        Ok(())
    }

    fn set_file_symbols(&mut self, file_id: u64, symbols: &[NewSymbolRecord]) -> Result<()> {
        let tx = self.conn.transaction()?;

        tx.execute(
            "DELETE FROM symbols WHERE file_id = ?1",
            params![file_id as i64],
        )?;

        {
            let mut stmt = tx.prepare(
                "INSERT INTO symbols (
                    file_id,
                    name,
                    kind,
                    language,
                    start_line,
                    start_col,
                    end_line,
                    end_col,
                    signature,
                    extra
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;

            for symbol in symbols {
                let kind_str = Self::symbol_kind_to_str(symbol.kind);

                let extra_json = match &symbol.extra {
                    Some(value) => Some(serde_json::to_string(value)?),
                    None => None,
                };

                stmt.execute(params![
                    file_id as i64,
                    symbol.name,
                    kind_str,
                    symbol.language,
                    symbol.range.start_line as i64,
                    symbol.range.start_column as i64,
                    symbol.range.end_line as i64,
                    symbol.range.end_column as i64,
                    symbol.signature,
                    extra_json,
                ])?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    fn query_symbols(&self, query: &SymbolQuery) -> Result<Vec<SymbolRecord>> {
        // Preload file paths so we can apply path/glob filters in memory.
        let mut files_by_id: HashMap<u64, PathBuf> = HashMap::new();
        {
            let mut stmt = self
                .conn
                .prepare("SELECT id, path FROM files ORDER BY id ASC")?;
            let rows = stmt.query_map([], |row| {
                let id: i64 = row.get(0)?;
                let path: String = row.get(1)?;
                Ok((id as u64, PathBuf::from(path)))
            })?;

            for row in rows {
                let (id, path) = row?;
                files_by_id.insert(id, path);
            }
        }

        let include_globs = build_globset(&query.globs)?;
        let exclude_globs = build_globset(&query.exclude_globs)?;

        let sql = String::from(
            "SELECT
                id,
                file_id,
                name,
                kind,
                language,
                start_line,
                start_col,
                end_line,
                end_col,
                signature,
                extra
             FROM symbols
             WHERE (?1 IS NULL OR name LIKE '%' || ?1 || '%')
               AND (?2 IS NULL OR LOWER(language) = LOWER(?2))",
        );

        let mut stmt = self.conn.prepare(&sql)?;

        let name_param: Option<&str> = query.name_substring.as_deref();
        let lang_param: Option<&str> = query.language.as_deref();

        let rows = stmt.query_map(params![name_param, lang_param], |row| {
            let id: i64 = row.get(0)?;
            let file_id: i64 = row.get(1)?;
            let name: String = row.get(2)?;
            let kind: String = row.get(3)?;
            let language: String = row.get(4)?;
            let start_line: i64 = row.get(5)?;
            let start_col: i64 = row.get(6)?;
            let end_line: i64 = row.get(7)?;
            let end_col: i64 = row.get(8)?;
            let signature: Option<String> = row.get(9)?;
            let extra: Option<String> = row.get(10)?;

            Ok((
                id, file_id, name, kind, language, start_line, start_col, end_line, end_col,
                signature, extra,
            ))
        })?;

        let mut results = Vec::new();

        for row in rows {
            let (
                id,
                file_id,
                name,
                kind,
                language,
                start_line,
                start_col,
                end_line,
                end_col,
                signature,
                extra,
            ) = row?;

            let file_id_u64 = file_id as u64;
            let file_path = match files_by_id.get(&file_id_u64) {
                Some(p) => p,
                None => continue,
            };

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

            let kind_enum = Self::symbol_kind_from_str(&kind)?;

            let extra_value = if let Some(json) = extra {
                Some(serde_json::from_str(&json)?)
            } else {
                None
            };

            let range = TextRange {
                start_line: start_line as u32,
                start_column: start_col as u32,
                end_line: end_line as u32,
                end_column: end_col as u32,
            };

            results.push(SymbolRecord {
                id: id as u64,
                file_id: file_id_u64,
                name,
                kind: kind_enum,
                language,
                range,
                signature,
                extra: extra_value,
            });
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::models::{NewSymbolRecord, SymbolQuery};
    use crate::models::{SymbolKind, TextRange};
    use tempfile::tempdir;

    #[test]
    fn sqlite_backend_persists_files_and_symbols() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("index.sqlite");

        let mut backend = SqliteIndexBackend::open(&db_path).expect("backend");

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

        backend
            .remove_file_by_path(Path::new("src/lib.rs"))
            .expect("remove file");

        let files_after = backend.list_files().expect("list files");
        assert!(files_after.is_empty());

        let results_after = backend.query_symbols(&query).expect("query symbols");
        assert!(results_after.is_empty());
    }

    #[test]
    fn sqlite_backend_initializes_and_reuses_meta() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("index.sqlite");

        {
            let mut backend = SqliteIndexBackend::open(&db_path).expect("backend");
            let mut meta = backend.load_meta().expect("load meta");
            assert_eq!(meta.schema_version, "2");
            meta.tool_version = "0.0.0-test".to_string();
            backend.save_meta(&meta).expect("save meta");
        }

        {
            let backend = SqliteIndexBackend::open(&db_path).expect("backend");
            let meta = backend.load_meta().expect("load meta");
            assert_eq!(meta.schema_version, "2");
            assert_eq!(meta.tool_version, "0.0.0-test");
        }
    }
}
