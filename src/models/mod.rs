//! Shared data models for search configs, results, symbols, and context.
//!
//! These types form the stable JSON API surface used by the CLI
//! and future daemon/server modes.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Schema version for `SearchResult` JSON payloads.
///
/// This version follows semver semantics (MAJOR.MINOR.PATCH):
/// - MAJOR: Breaking changes to required fields or field semantics.
/// - MINOR: Backward-compatible additions (new optional fields).
/// - PATCH: Documentation or internal changes only.
///
/// Clients consuming `--format=json` output should check this version
/// to ensure compatibility and handle newer minor versions
/// conservatively.
pub const SEARCH_RESULT_VERSION: &str = "0.1.0";

/// High-level search mode.
///
/// Text mode behaves like a traditional grep, symbol mode operates on
/// language-aware symbol indexes, and auto chooses based on the query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchMode {
    Text,
    Symbol,
    Auto,
}

/// Controls how much contextual code is returned around each match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchContext {
    /// No additional context beyond the raw match.
    None,
    /// Declaration-only context (e.g., signature line).
    Decl,
    /// Full definition/body of the symbol or construct.
    Def,
    /// Enclosing/parent context. For TS/JS in Phase 2 this behaves
    /// like `Def`; richer parent semantics are planned for later
    /// phases.
    Parent,
}

/// Kind of a symbol in a source file.
///
/// This initial set is intentionally small and focused on the TS/JS
/// backends; additional kinds can be added in later phases as more
/// languages come online.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Interface,
    Variable,
    Namespace,
}

/// Kind of context snippet returned for a symbol.
///
/// `Decl` covers declarations or signatures, `Def` covers full
/// definitions/bodies, and `Parent` will be used in later phases for
/// enclosing scopes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContextKind {
    Decl,
    Def,
    Parent,
}

/// A single node in the enclosing context chain for a symbol or match.
///
/// The chain is ordered from outermost (e.g., file/module/namespace)
/// to innermost (e.g., class, method, function).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextNode {
    /// Name of this enclosing context (file, module, namespace, class, etc.).
    pub name: String,
    /// Optional high-level kind for symbol-like contexts.
    ///
    /// For file-level or other non-symbol contexts this may be `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<SymbolKind>,
}

/// A half-open range in a source file, expressed as 1-based
/// line/column positions.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TextRange {
    /// 1-based starting line (inclusive).
    pub start_line: u32,
    /// 1-based starting column (inclusive, byte offset).
    pub start_column: u32,
    /// 1-based ending line (inclusive).
    pub end_line: u32,
    /// 1-based ending column (exclusive, byte offset).
    pub end_column: u32,
}

/// A language-level symbol such as a function, method, or class.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    /// Simple name of the symbol (function name, class name, etc.).
    pub name: String,
    /// High-level kind of symbol.
    pub kind: SymbolKind,
    /// Stable language identifier (e.g., "typescript").
    pub language: String,
    /// Path of the file that defines the symbol.
    pub file: PathBuf,
    /// Source range covering the symbol's declaration/definition.
    pub range: TextRange,
    /// Optional human-readable signature for the symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

/// A concrete snippet of source representing a particular context
/// view for a symbol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextInfo {
    /// Kind of context captured in this snippet.
    pub kind: ContextKind,
    /// File containing the snippet.
    pub file: PathBuf,
    /// Range of source code covered by the snippet.
    pub range: TextRange,
    /// Snippet contents as a single multi-line string.
    pub snippet: String,
    /// Index into the `SearchResult.symbols` array that this context
    /// is associated with, when applicable. This creates an explicit
    /// linkage between symbols and contexts instead of relying on
    /// positional correspondence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_index: Option<usize>,
    /// Enclosing AST/context chain for the symbol or match location,
    /// ordered from outermost (file/module/namespace) to innermost
    /// (e.g., class/method/function).
    ///
    /// This field is additive and may be absent in older payloads.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parent_chain: Vec<ContextNode>,
}

/// Field selectors supported by the structured query DSL.
///
/// These map to different aspects of a match or symbol and are used
/// by `QueryExpr` to express filters such as `name:foo` or
/// `kind:function`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QueryField {
    Name,
    Kind,
    File,
    Language,
    Text,
}

/// A single atomic query term such as `name:foo` or `kind:function`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryTerm {
    /// Field this term applies to (name, kind, file, language, text).
    pub field: QueryField,
    /// Raw value associated with the field, after basic parsing.
    pub value: String,
}

/// High-level query expression with AND/OR combinators.
///
/// Parsing rules (Phase 4):
/// - Space-separated groups are combined with AND.
/// - `A|B` within a group is treated as OR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueryExpr {
    /// A single atomic term.
    Term(QueryTerm),
    /// Logical AND of multiple sub-expressions.
    And(Vec<QueryExpr>),
    /// Logical OR of multiple sub-expressions.
    Or(Vec<QueryExpr>),
}

/// Core configuration for a search operation.
///
/// This struct is built from CLI or daemon inputs and is consumed by the
/// core search engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchConfig {
    /// Raw search pattern/string provided by the user.
    pub pattern: String,
    /// One or more filesystem roots to search under.
    pub paths: Vec<PathBuf>,
    /// Inclusion globs applied to candidate files.
    #[serde(default)]
    pub globs: Vec<String>,
    /// Exclusion globs applied to candidate files.
    #[serde(default)]
    pub exclude_globs: Vec<String>,
    /// Optional language hint or filter (e.g. "typescript").
    pub language: Option<String>,
    /// Search mode (text, symbol, or auto).
    pub mode: SearchMode,
    /// Whether to interpret the pattern as a literal identifier/word.
    ///
    /// - In text mode, this enables whole-identifier matching (e.g. `foo`
    ///   matches `foo` but not `foobar`).
    /// - In symbol mode, this enables exact symbol-name matching for
    ///   `name:` terms and bare patterns.
    #[serde(default)]
    pub literal: bool,
    /// Requested context for each match.
    pub context: SearchContext,
    /// Maximum number of matches to return (None = unlimited).
    pub limit: Option<usize>,
    /// Maximum number of lines per match snippet.
    pub max_lines: Option<usize>,
    /// Optional index configuration to use during search.
    ///
    /// When present and the backend is available, symbol-mode
    /// searches may use the index as a pre-filter. This field is
    /// optional to keep the JSON configuration format backward
    /// compatible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<IndexConfig>,
    /// Parsed representation of the structured query/DSL, when used.
    ///
    /// This is built from the raw `pattern` string by the CLI or
    /// daemon layer and consumed by the search engine. It is optional
    /// to keep the JSON configuration format backward compatible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query_expr: Option<QueryExpr>,
}

/// A single search match.
///
/// This is intentionally minimal for Phase 1 and will be extended in
/// later phases with richer symbol and context information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMatch {
    /// Path of the file containing the match.
    pub path: PathBuf,
    /// 1-based line number of the match.
    pub line: u32,
    /// Optional 1-based column number of the match.
    pub column: Option<u32>,
    /// Optional line or snippet text for the match.
    pub snippet: Option<String>,
}

/// Summary information for a search result set.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SearchSummary {
    /// Total number of matches found while the search was running.
    ///
    /// When `limit` is set, the engine stops scanning once that many
    /// matches have been found, so this will equal the number of
    /// concrete matches returned when `truncated` is `true`.
    pub total_matches: u64,
    /// True if results were truncated due to a limit or other cap.
    pub truncated: bool,
}

/// Top-level result for a search invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// Schema version for this result payload.
    pub version: String,
    /// The original pattern or query string.
    pub query: String,
    /// Concrete matches returned by the engine.
    #[serde(default)]
    pub matches: Vec<SearchMatch>,
    /// Symbols associated with this search, when symbol-aware modes
    /// are used. For plain text searches this will be empty.
    #[serde(default)]
    pub symbols: Vec<Symbol>,
    /// Context snippets associated with symbols or matches. This is
    /// reserved for symbol/context-aware modes; plain text searches
    /// leave it empty.
    #[serde(default)]
    pub contexts: Vec<ContextInfo>,
    /// Aggregate summary of the result set.
    pub summary: SearchSummary,
}

/// Backend kind for indexing.
///
/// Additional backends can be added in later phases; JSON uses
/// lowercase strings for stability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IndexBackendKind {
    File,
    Sqlite,
}

/// Configuration for building or updating an index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexConfig {
    /// Filesystem roots to index.
    pub paths: Vec<PathBuf>,
    /// Inclusion globs applied to candidate files.
    #[serde(default)]
    pub globs: Vec<String>,
    /// Exclusion globs applied to candidate files.
    #[serde(default)]
    pub exclude_globs: Vec<String>,
    /// Selected backend implementation.
    pub backend: IndexBackendKind,
    /// Location for on-disk index data (directory or file path).
    pub index_path: PathBuf,
    /// Optional language filter for indexing.
    pub language: Option<String>,
}

/// Summary information about an index operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexSummary {
    /// Backend used for the index.
    pub backend: IndexBackendKind,
    /// Location of the index on disk.
    pub index_path: PathBuf,
    /// Number of files indexed.
    pub files_indexed: u64,
    /// Number of symbols indexed.
    pub symbols_indexed: u64,
    /// Canonical project root for this index (absolute path).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_path: Option<String>,
    /// Logical schema version for the index.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<String>,
    /// Version of the symgrep tool that wrote the index.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_version: Option<String>,
    /// ISO-8601 creation timestamp for this index.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// ISO-8601 last-updated timestamp for this index.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}
