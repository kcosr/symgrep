//! Index backend abstraction and helpers.
//!
//! The `IndexBackend` trait provides a common interface that the
//! search engine and CLI can use without depending on concrete
//! implementations. Phase 6A ships with a file-backed implementation;
//! later phases will add a SQLite backend.

use std::path::Path;

use anyhow::Result;

use crate::index::models::{FileRecord, IndexMeta, NewSymbolRecord, SymbolQuery, SymbolRecord};
use crate::models::{IndexBackendKind, IndexConfig};

/// Pluggable index backend used by the core engine.
pub trait IndexBackend {
    /// Kind of backend implementation.
    fn kind(&self) -> IndexBackendKind;

    /// Root path for the on-disk index.
    fn index_path(&self) -> &Path;

    /// Load index metadata.
    fn load_meta(&self) -> Result<IndexMeta>;

    /// Persist index metadata.
    fn save_meta(&mut self, meta: &IndexMeta) -> Result<()>;

    /// List all known files.
    fn list_files(&self) -> Result<Vec<FileRecord>>;

    /// Look up a file record by path.
    fn get_file_by_path(&self, path: &Path) -> Result<Option<FileRecord>>;

    /// Look up a file record by id.
    fn get_file_by_id(&self, id: u64) -> Result<Option<FileRecord>>;

    /// Create or update a file record.
    fn upsert_file(
        &mut self,
        path: &Path,
        language: &str,
        hash: Option<&str>,
        mtime: i64,
        size: u64,
    ) -> Result<FileRecord>;

    /// Remove a file and any associated symbols.
    fn remove_file_by_path(&mut self, path: &Path) -> Result<()>;

    /// Replace all symbols for a given file with new records.
    fn set_file_symbols(&mut self, file_id: u64, symbols: &[NewSymbolRecord]) -> Result<()>;

    /// Query symbols using basic filters.
    fn query_symbols(&self, query: &SymbolQuery) -> Result<Vec<SymbolRecord>>;
}

/// Helper to construct an appropriate backend from a generic config.
pub fn open_backend(config: &IndexConfig) -> Result<Box<dyn IndexBackend>> {
    match config.backend {
        IndexBackendKind::File => Ok(Box::new(crate::index::FileIndexBackend::open(
            &config.index_path,
        )?)),
        IndexBackendKind::Sqlite => Ok(Box::new(crate::index::SqliteIndexBackend::open(
            &config.index_path,
        )?)),
    }
}
