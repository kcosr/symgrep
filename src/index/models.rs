//! Shared logical index model used by index backends.
//!
//! These types represent the persisted project index stored by
//! various backend implementations (file, SQLite, etc.).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::models::{SymbolKind, TextRange};

/// Metadata for the entire project index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexMeta {
    /// Schema version for the index on disk.
    pub schema_version: String,
    /// Version of the symgrep tool that wrote the index.
    pub tool_version: String,
    /// Canonical project root for this index, stored as an absolute path.
    ///
    /// Older indexes may omit this field; in that case it will be
    /// populated from the indexing configuration on the next update.
    #[serde(default)]
    pub root_path: String,
    /// Unix timestamp (seconds since epoch) when the index was created.
    pub created_at: u64,
    /// Unix timestamp (seconds since epoch) when the index was last updated.
    pub updated_at: u64,
}

/// Logical record for a single file in the index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRecord {
    /// Stable numeric identifier for this file within the index.
    pub id: u64,
    /// Path to the file, as stored by the index.
    pub path: PathBuf,
    /// Logical language identifier (e.g., "typescript", "javascript").
    pub language: String,
    /// Optional content hash for change detection.
    ///
    /// For the file-based backend in Phase 6A this may be omitted and
    /// change detection will rely on `mtime` and `size` instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    /// Last modification time, in seconds since Unix epoch.
    pub mtime: i64,
    /// File size in bytes.
    pub size: u64,
}

/// Logical record for a single symbol in the index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolRecord {
    /// Stable numeric identifier for this symbol within the index.
    pub id: u64,
    /// Foreign key reference to the owning file.
    pub file_id: u64,
    /// Symbol name (function, method, class, etc.).
    pub name: String,
    /// High-level kind of symbol.
    pub kind: SymbolKind,
    /// Logical language identifier (e.g., "typescript").
    pub language: String,
    /// Source range for the symbol definition/declaration.
    pub range: TextRange,
    /// Optional human-readable signature for the symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    /// Optional backend-specific or language-specific payload.
    ///
    /// This field allows backends to attach extra data without
    /// affecting the core JSON schema.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<serde_json::Value>,
}

/// Non-persisted representation of a symbol ready to be inserted.
#[derive(Debug, Clone)]
pub struct NewSymbolRecord {
    pub file_id: u64,
    pub name: String,
    pub kind: SymbolKind,
    pub language: String,
    pub range: TextRange,
    pub signature: Option<String>,
    pub extra: Option<serde_json::Value>,
}

/// Query parameters for retrieving symbols from an index backend.
#[derive(Debug, Clone)]
pub struct SymbolQuery {
    /// Optional substring to match against symbol names.
    pub name_substring: Option<String>,
    /// Optional language filter.
    pub language: Option<String>,
    /// One or more filesystem roots to restrict matches to.
    pub paths: Vec<PathBuf>,
    /// Inclusion globs applied to candidate files.
    pub globs: Vec<String>,
    /// Exclusion globs applied to candidate files.
    pub exclude_globs: Vec<String>,
}

/// In-memory representation of the project index.
///
/// This is primarily useful for debugging and tests; most backends
/// operate directly on their on-disk representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectIndex {
    pub meta: IndexMeta,
    pub files: Vec<FileRecord>,
    pub symbols: Vec<SymbolRecord>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{SymbolKind, TextRange};

    #[test]
    fn file_record_round_trips_with_serde() {
        let record = FileRecord {
            id: 1,
            path: PathBuf::from("src/lib.rs"),
            language: "typescript".to_string(),
            hash: Some("abc123".to_string()),
            mtime: 1_700_000_000,
            size: 42,
        };

        let json = serde_json::to_string(&record).expect("serialize");
        let decoded: FileRecord = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(decoded.id, record.id);
        assert_eq!(decoded.path, record.path);
        assert_eq!(decoded.language, record.language);
        assert_eq!(decoded.hash, record.hash);
        assert_eq!(decoded.mtime, record.mtime);
        assert_eq!(decoded.size, record.size);
    }

    #[test]
    fn symbol_record_round_trips_with_serde() {
        let range = TextRange {
            start_line: 1,
            start_column: 1,
            end_line: 1,
            end_column: 10,
        };

        let record = SymbolRecord {
            id: 1,
            file_id: 2,
            name: "add".to_string(),
            kind: SymbolKind::Function,
            language: "typescript".to_string(),
            range,
            signature: Some("add(a: number, b: number): number".to_string()),
            extra: None,
        };

        let json = serde_json::to_string(&record).expect("serialize");
        let decoded: SymbolRecord = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(decoded.id, record.id);
        assert_eq!(decoded.file_id, record.file_id);
        assert_eq!(decoded.name, record.name);
        assert_eq!(decoded.kind, record.kind);
        assert_eq!(decoded.language, record.language);
        assert_eq!(decoded.range.start_line, record.range.start_line);
        assert_eq!(decoded.range.end_column, record.range.end_column);
        assert_eq!(decoded.signature, record.signature);
    }

    #[test]
    fn index_meta_round_trips_with_serde() {
        let meta = IndexMeta {
            schema_version: "2".to_string(),
            tool_version: "0.0.0".to_string(),
            root_path: "/path/to/project".to_string(),
            created_at: 1_700_000_000,
            updated_at: 1_700_000_500,
        };

        let json = serde_json::to_string(&meta).expect("serialize");
        let decoded: IndexMeta = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(decoded.schema_version, meta.schema_version);
        assert_eq!(decoded.tool_version, meta.tool_version);
        assert_eq!(decoded.root_path, meta.root_path);
        assert_eq!(decoded.created_at, meta.created_at);
        assert_eq!(decoded.updated_at, meta.updated_at);
    }
}
