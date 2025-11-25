use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Deserialize;

use crate::models::{
    IndexBackendKind, IndexConfig, SearchConfig, SearchMode, SymbolAttributesRequest,
    SymbolAttributesUpdate, SymbolKind, SymbolSelector,
};
/// Top-level CLI entrypoint for `symgrep`.
#[derive(Parser, Debug)]
#[command(
    name = "symgrep",
    about = "Symsemantic code search CLI (skeleton)",
    author = "symgrep developers",
    subcommand_required = false,
    arg_required_else_help = false
)]
pub struct Cli {
    /// Print the JSON schema version used for `--format=json` output
    /// and exit.
    #[arg(long = "schema-version")]
    pub schema_version: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Top-level CLI subcommands.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Search code (text or symbols).
    Search(SearchArgs),
    /// Build or update an index.
    Index(IndexArgs),
    /// Inspect an existing index without modifying it.
    IndexInfo(IndexInfoArgs),
    /// Run a long-lived HTTP+JSON daemon.
    Serve(ServeArgs),
    /// Explore callers/callees for symbols.
    Follow(FollowArgs),
    /// Update symbol attributes (keywords, description) in an index.
    Annotate(AnnotateArgs),
}

/// Arguments specific to the `search` subcommand.
#[derive(Args, Debug)]
pub struct SearchArgs {
    /// Search pattern (syntax not implemented yet).
    pub pattern: String,

    /// Paths to search (defaults to current directory if omitted).
    #[arg(short = 'p', long = "path")]
    pub paths: Vec<PathBuf>,

    /// Inclusion globs applied to candidate files.
    #[arg(long = "glob")]
    pub globs: Vec<String>,

    /// Exclusion globs applied to candidate files.
    #[arg(long = "exclude")]
    pub exclude_globs: Vec<String>,

    /// Optional language hint or filter (e.g. "typescript").
    #[arg(long = "language")]
    pub language: Option<String>,

    /// Interpret the pattern as a literal identifier/word.
    ///
    /// - In text mode, restricts matches to whole identifiers (e.g. `foo`
    ///   matches `foo` but not `foobar`).
    /// - In symbol mode, requires exact symbol-name matches for `name:`
    ///   terms and bare patterns.
    #[arg(long = "literal")]
    pub literal: bool,

    /// Search mode (text, symbol, or auto).
    #[arg(long = "mode", value_enum, default_value_t = SearchModeArg::Text)]
    pub mode: SearchModeArg,

    /// Symbol views to materialize in symbol mode.
    ///
    /// Values: `meta`, `decl`, `def`, `parent`, `comment`, `matches`.
    /// Multiple views can be combined via commas or repeated flags,
    /// e.g. `--view decl,comment` or `--view decl --view comment`.
    #[arg(long = "view", value_delimiter = ',')]
    pub view: Vec<SymbolViewArg>,

    /// Maximum number of matches to return.
    #[arg(long = "limit")]
    pub limit: Option<usize>,

    /// Maximum number of lines per match snippet.
    #[arg(long = "max-lines")]
    pub max_lines: Option<usize>,

    /// Number of context lines to show before and after each match
    /// line in text output.
    ///
    /// This applies to `--format text` only:
    /// - In text mode, it expands each matching line into a window of
    ///   surrounding source lines per file.
    /// - In symbol mode with match views (e.g. `--view def,matches`),
    ///   it expands match lines within the primary context snippet
    ///   for each symbol.
    ///
    /// JSON output (`--format json`) is unchanged; match context is a
    /// CLI-only presentation feature.
    #[arg(short = 'C', long = "context")]
    pub context: Option<usize>,

    /// Whether to use an existing index for symbol searches.
    #[arg(long = "use-index")]
    pub use_index: bool,

    /// Rebuild or update the configured index before running a symbol
    /// search when `--use-index` is enabled and the search will use
    /// an index backend.
    ///
    /// This is an opt-in flag intended for workflows where other tools
    /// modify source files between searches and callers always want the
    /// index to reflect the latest on-disk state.
    #[arg(long = "reindex-on-search")]
    pub reindex_on_search: bool,

    /// Index backend to use when `--use-index` is enabled.
    ///
    /// When omitted, the search engine may automatically choose an
    /// appropriate backend based on existing indexes (preferring a
    /// SQLite index at the default path when present).
    #[arg(long = "index-backend", value_enum)]
    pub index_backend: Option<IndexBackendArg>,

    /// Location for on-disk index data used with `--use-index`.
    ///
    /// For the file backend this should be a directory (e.g. ".symgrep").
    /// For the SQLite backend this is typically a database file path
    /// such as ".symgrep/index.sqlite".
    #[arg(long = "index-path")]
    pub index_path: Option<PathBuf>,

    /// Output format (text, table, or json).
    #[arg(long = "format", value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,

    /// Optional server URL for delegating search to a daemon.
    ///
    /// When set (either via this flag or the `SYMGREP_SERVER_URL`
    /// environment variable), the CLI sends the search configuration
    /// to the HTTP server instead of running a local search. Use
    /// `--no-server` to override this and force local execution.
    #[arg(long = "server", env = "SYMGREP_SERVER_URL")]
    pub server: Option<String>,

    /// Disable use of any configured server and force local search.
    #[arg(long = "no-server")]
    pub no_server: bool,
}

/// Direction for the `follow` subcommand.
#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FollowDirectionArg {
    Callers,
    Callees,
    Both,
}

impl FollowDirectionArg {
    pub fn to_model(self) -> crate::models::FollowDirection {
        match self {
            FollowDirectionArg::Callers => crate::models::FollowDirection::Callers,
            FollowDirectionArg::Callees => crate::models::FollowDirection::Callees,
            FollowDirectionArg::Both => crate::models::FollowDirection::Both,
        }
    }
}

/// Arguments specific to the `follow` subcommand.
#[derive(Args, Debug)]
pub struct FollowArgs {
    /// Search pattern used to select target symbols.
    pub pattern: String,

    /// Paths to search (defaults to current directory if omitted).
    #[arg(short = 'p', long = "path")]
    pub paths: Vec<PathBuf>,

    /// Inclusion globs applied to candidate files.
    #[arg(long = "glob")]
    pub globs: Vec<String>,

    /// Exclusion globs applied to candidate files.
    #[arg(long = "exclude")]
    pub exclude_globs: Vec<String>,

    /// Optional language hint or filter (e.g. "typescript").
    #[arg(long = "language")]
    pub language: Option<String>,

    /// Interpret the pattern as a literal identifier when matching
    /// target symbol names.
    ///
    /// This affects only the initial symbol query and does not change
    /// how call sites are displayed.
    #[arg(long = "literal")]
    pub literal: bool,

    /// Maximum number of target symbols to process.
    ///
    /// This bounds the number of `FollowTarget` entries produced from
    /// the initial symbol search. Within each target, all caller and
    /// callee edges are still considered.
    #[arg(long = "limit")]
    pub limit: Option<usize>,

    /// Direction for call relationships to follow.
    ///
    /// - callers: show who calls the target(s).
    /// - callees: show what the target(s) call.
    /// - both: include both directions in JSON output.
    #[arg(long = "direction", value_enum, default_value_t = FollowDirectionArg::Callers)]
    pub direction: FollowDirectionArg,

    /// Number of context lines to show before and after each call
    /// site in text output.
    ///
    /// This is a CLI-only presentation feature and does not affect
    /// JSON output.
    #[arg(short = 'C', long = "context")]
    pub context: Option<usize>,

    /// Maximum number of lines per caller/callee context block in
    /// text output.
    ///
    /// When set to 0, only headers are printed without any context
    /// lines.
    #[arg(long = "max-lines")]
    pub max_lines: Option<usize>,

    /// Output format (text or json).
    #[arg(long = "format", value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,

    /// Optional server URL for delegating follow to a daemon.
    ///
    /// When set (either via this flag or the `SYMGREP_SERVER_URL`
    /// environment variable), the CLI sends an internal symbol search
    /// request to the HTTP server instead of running a local search.
    #[arg(long = "server", env = "SYMGREP_SERVER_URL")]
    pub server: Option<String>,

    /// Disable use of any configured server and force local follow.
    #[arg(long = "no-server")]
    pub no_server: bool,
}

/// CLI representation of search mode.
#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchModeArg {
    Text,
    Symbol,
    Auto,
}

/// CLI representation of symbol views for symbol mode.
#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SymbolViewArg {
    Meta,
    Decl,
    Def,
    Parent,
    Comment,
    Matches,
}

impl SymbolViewArg {
    pub fn to_model(self) -> crate::models::SymbolView {
        match self {
            SymbolViewArg::Meta => crate::models::SymbolView::Meta,
            SymbolViewArg::Decl => crate::models::SymbolView::Decl,
            SymbolViewArg::Def => crate::models::SymbolView::Def,
            SymbolViewArg::Parent => crate::models::SymbolView::Parent,
            SymbolViewArg::Comment => crate::models::SymbolView::Comment,
            SymbolViewArg::Matches => crate::models::SymbolView::Matches,
        }
    }
}

/// CLI representation of output format.
#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Text,
    Table,
    Json,
}

/// CLI representation of index backend kind.
#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IndexBackendArg {
    File,
    Sqlite,
}

/// CLI representation of symbol kind for attribute updates.
#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SymbolKindArg {
    Function,
    Method,
    Class,
    Interface,
    Variable,
    Namespace,
}

impl SymbolKindArg {
    pub fn to_model(self) -> SymbolKind {
        match self {
            SymbolKindArg::Function => SymbolKind::Function,
            SymbolKindArg::Method => SymbolKind::Method,
            SymbolKindArg::Class => SymbolKind::Class,
            SymbolKindArg::Interface => SymbolKind::Interface,
            SymbolKindArg::Variable => SymbolKind::Variable,
            SymbolKindArg::Namespace => SymbolKind::Namespace,
        }
    }
}

/// Arguments specific to the `index` subcommand.
#[derive(Args, Debug)]
pub struct IndexArgs {
    /// Paths to index (defaults to current directory if omitted).
    #[arg(short = 'p', long = "path")]
    pub paths: Vec<PathBuf>,

    /// Inclusion globs applied to candidate files.
    #[arg(long = "glob")]
    pub globs: Vec<String>,

    /// Exclusion globs applied to candidate files.
    #[arg(long = "exclude")]
    pub exclude_globs: Vec<String>,

    /// Optional language hint or filter (e.g. "typescript").
    #[arg(long = "language")]
    pub language: Option<String>,

    /// Index backend to use.
    ///
    /// When omitted, the file backend is used by default.
    #[arg(long = "index-backend", value_enum)]
    pub backend: Option<IndexBackendArg>,

    /// Location for on-disk index data.
    ///
    /// For the file backend this should be a directory (e.g. ".symgrep").
    /// For the SQLite backend this is typically a database file path
    /// such as ".symgrep/index.sqlite".
    #[arg(long = "index-path")]
    pub index_path: Option<PathBuf>,

    /// Optional server URL for delegating indexing to a daemon.
    ///
    /// When set (either via this flag or the `SYMGREP_SERVER_URL`
    /// environment variable), the CLI sends the index configuration
    /// to the HTTP server instead of running local indexing. Use
    /// `--no-server` to override this and force local execution.
    #[arg(long = "server", env = "SYMGREP_SERVER_URL")]
    pub server: Option<String>,

    /// Disable use of any configured server and force local indexing.
    #[arg(long = "no-server")]
    pub no_server: bool,
}

/// Arguments specific to the `index-info` subcommand.
#[derive(Args, Debug)]
pub struct IndexInfoArgs {
    /// Paths to index (defaults to current directory if omitted).
    #[arg(short = 'p', long = "path")]
    pub paths: Vec<PathBuf>,

    /// Inclusion globs applied to candidate files.
    #[arg(long = "glob")]
    pub globs: Vec<String>,

    /// Exclusion globs applied to candidate files.
    #[arg(long = "exclude")]
    pub exclude_globs: Vec<String>,

    /// Optional language hint or filter (e.g. "typescript").
    #[arg(long = "language")]
    pub language: Option<String>,

    /// Index backend to use.
    ///
    /// When omitted, the file backend is used by default.
    #[arg(long = "index-backend", value_enum)]
    pub backend: Option<IndexBackendArg>,

    /// Location for on-disk index data.
    ///
    /// For the file backend this should be a directory (e.g. ".symgrep").
    /// For the SQLite backend this is typically a database file path
    /// such as ".symgrep/index.sqlite".
    #[arg(long = "index-path")]
    pub index_path: Option<PathBuf>,

    /// Output format (text or json).
    #[arg(long = "format", value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,

    /// Optional server URL for delegating index introspection to a daemon.
    ///
    /// When set (either via this flag or the `SYMGREP_SERVER_URL`
    /// environment variable), the CLI sends the index configuration
    /// to the HTTP server instead of running local introspection. Use
    /// `--no-server` to override this and force local execution.
    #[arg(long = "server", env = "SYMGREP_SERVER_URL")]
    pub server: Option<String>,

    /// Disable use of any configured server and force local index introspection.
    #[arg(long = "no-server")]
    pub no_server: bool,
}

/// Arguments specific to the `annotate` subcommand.
#[derive(Args, Debug)]
pub struct AnnotateArgs {
    /// File containing the target symbol.
    #[arg(long = "file")]
    pub file: PathBuf,

    /// Logical language identifier for the symbol (e.g. "typescript").
    #[arg(long = "language")]
    pub language: String,

    /// Kind of symbol to update.
    #[arg(long = "kind", value_enum)]
    pub kind: SymbolKindArg,

    /// Simple symbol name.
    #[arg(long = "name")]
    pub name: String,

    /// 1-based starting line for the symbol's range (inclusive).
    #[arg(long = "start-line")]
    pub start_line: u32,

    /// 1-based ending line for the symbol's range (inclusive).
    #[arg(long = "end-line")]
    pub end_line: u32,

    /// Comma-separated or repeated keywords to attach to the symbol.
    #[arg(long = "keywords", value_delimiter = ',')]
    pub keywords: Vec<String>,

    /// Free-form description for the symbol.
    #[arg(long = "description")]
    pub description: Option<String>,

    /// Path to a file whose contents should be used as the symbol description.
    #[arg(long = "description-file")]
    pub description_file: Option<PathBuf>,

    /// Index backend to use.
    ///
    /// When omitted, the file backend is used by default.
    #[arg(long = "index-backend", value_enum)]
    pub index_backend: Option<IndexBackendArg>,

    /// Location for on-disk index data.
    ///
    /// For the file backend this should be a directory (e.g. ".symgrep").
    /// For the SQLite backend this is typically a database file path
    /// such as ".symgrep/index.sqlite".
    #[arg(long = "index-path")]
    pub index_path: Option<PathBuf>,

    /// Optional server URL for delegating annotation to a daemon.
    ///
    /// When set (either via this flag or the `SYMGREP_SERVER_URL`
    /// environment variable), the CLI sends the request to the HTTP
    /// server instead of updating the index locally. Use `--no-server`
    /// to override this and force local execution.
    #[arg(long = "server", env = "SYMGREP_SERVER_URL")]
    pub server: Option<String>,

    /// Disable use of any configured server and force local updates.
    #[arg(long = "no-server")]
    pub no_server: bool,
}

/// Arguments specific to the `serve` subcommand.
#[derive(Args, Debug)]
pub struct ServeArgs {
    /// Address to bind the HTTP server to, e.g. "127.0.0.1:7878".
    #[arg(long = "addr", default_value = "127.0.0.1:7878")]
    pub addr: String,
}

/// Build a core `SearchConfig` from CLI `SearchArgs`.
pub fn search_config_from_args(args: &SearchArgs) -> Result<SearchConfig> {
    let paths = if args.paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        args.paths.clone()
    };

    let mode = match args.mode {
        SearchModeArg::Text => SearchMode::Text,
        SearchModeArg::Symbol => SearchMode::Symbol,
        SearchModeArg::Auto => SearchMode::Auto,
    };

    let index = if args.use_index {
        let backend_arg = match (&args.index_backend, &args.index_path) {
            (Some(kind), _) => *kind,
            (None, Some(path)) => {
                if path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|ext| ext.eq_ignore_ascii_case("sqlite"))
                    .unwrap_or(false)
                {
                    IndexBackendArg::Sqlite
                } else {
                    IndexBackendArg::File
                }
            }
            (None, None) => IndexBackendArg::File,
        };

        let backend = match backend_arg {
            IndexBackendArg::File => IndexBackendKind::File,
            IndexBackendArg::Sqlite => IndexBackendKind::Sqlite,
        };

        let index_path = match (&args.index_path, backend_arg) {
            (Some(path), _) => path.clone(),
            (None, IndexBackendArg::File) => PathBuf::from(".symgrep"),
            (None, IndexBackendArg::Sqlite) => PathBuf::from(".symgrep").join("index.sqlite"),
        };

        Some(IndexConfig {
            paths: paths.clone(),
            globs: args.globs.clone(),
            exclude_globs: args.exclude_globs.clone(),
            backend,
            index_path,
            language: args.language.clone(),
        })
    } else {
        None
    };

    let symbol_views = args.view.iter().map(|v| v.to_model()).collect();

    Ok(SearchConfig {
        pattern: args.pattern.clone(),
        paths,
        globs: args.globs.clone(),
        exclude_globs: args.exclude_globs.clone(),
        language: args.language.clone(),
        mode,
        literal: args.literal,
        symbol_views,
        limit: args.limit,
        max_lines: args.max_lines,
        reindex_on_search: args.reindex_on_search,
        query_expr: None,
        index,
    })
}

/// Build a core `SearchConfig` from CLI `FollowArgs`.
///
/// Follow always uses symbol-mode search and currently ignores
/// index-backed search in order to ensure call metadata is available.
pub fn follow_search_config_from_args(args: &FollowArgs) -> Result<SearchConfig> {
    let paths = if args.paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        args.paths.clone()
    };

    Ok(SearchConfig {
        pattern: args.pattern.clone(),
        paths,
        globs: args.globs.clone(),
        exclude_globs: args.exclude_globs.clone(),
        language: args.language.clone(),
        mode: SearchMode::Symbol,
        literal: args.literal,
        // Follow operates over call metadata and does not require
        // symbol views or snippet truncation hints; CLI `--context`
        // and `--max-lines` are applied at presentation time.
        symbol_views: Vec::new(),
        limit: args.limit,
        max_lines: None,
        reindex_on_search: false,
        index: None,
        query_expr: None,
    })
}

/// Build a core `IndexConfig` from CLI `IndexArgs`.
pub fn index_config_from_args(args: &IndexArgs) -> Result<IndexConfig> {
    let paths = if args.paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        args.paths.clone()
    };

    let backend_arg = match (&args.backend, &args.index_path) {
        (Some(kind), _) => *kind,
        (None, Some(path)) => {
            if path
                .extension()
                .and_then(|e| e.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("sqlite"))
                .unwrap_or(false)
            {
                IndexBackendArg::Sqlite
            } else {
                IndexBackendArg::File
            }
        }
        (None, None) => IndexBackendArg::File,
    };

    let backend = match backend_arg {
        IndexBackendArg::File => IndexBackendKind::File,
        IndexBackendArg::Sqlite => IndexBackendKind::Sqlite,
    };

    let index_path = match (&args.index_path, backend_arg) {
        (Some(path), _) => path.clone(),
        (None, IndexBackendArg::File) => PathBuf::from(".symgrep"),
        (None, IndexBackendArg::Sqlite) => PathBuf::from(".symgrep").join("index.sqlite"),
    };

    Ok(IndexConfig {
        paths,
        globs: args.globs.clone(),
        exclude_globs: args.exclude_globs.clone(),
        backend,
        index_path,
        language: args.language.clone(),
    })
}

/// Build a core `IndexConfig` from CLI `IndexInfoArgs`.
pub fn index_info_config_from_args(args: &IndexInfoArgs) -> Result<IndexConfig> {
    let paths = if args.paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        args.paths.clone()
    };

    let backend_arg = match (&args.backend, &args.index_path) {
        (Some(kind), _) => *kind,
        (None, Some(path)) => {
            if path
                .extension()
                .and_then(|e| e.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("sqlite"))
                .unwrap_or(false)
            {
                IndexBackendArg::Sqlite
            } else {
                IndexBackendArg::File
            }
        }
        (None, None) => IndexBackendArg::File,
    };

    let backend = match backend_arg {
        IndexBackendArg::File => IndexBackendKind::File,
        IndexBackendArg::Sqlite => IndexBackendKind::Sqlite,
    };

    let index_path = match (&args.index_path, backend_arg) {
        (Some(path), _) => path.clone(),
        (None, IndexBackendArg::File) => PathBuf::from(".symgrep"),
        (None, IndexBackendArg::Sqlite) => PathBuf::from(".symgrep").join("index.sqlite"),
    };

    Ok(IndexConfig {
        paths,
        globs: args.globs.clone(),
        exclude_globs: args.exclude_globs.clone(),
        backend,
        index_path,
        language: args.language.clone(),
    })
}

/// Build a `SymbolAttributesRequest` from CLI `AnnotateArgs`.
pub fn symbol_attributes_request_from_args(args: &AnnotateArgs) -> Result<SymbolAttributesRequest> {
    let backend_arg = match (&args.index_backend, &args.index_path) {
        (Some(kind), _) => *kind,
        (None, Some(path)) => {
            if path
                .extension()
                .and_then(|e| e.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("sqlite"))
                .unwrap_or(false)
            {
                IndexBackendArg::Sqlite
            } else {
                IndexBackendArg::File
            }
        }
        (None, None) => IndexBackendArg::File,
    };

    let backend = match backend_arg {
        IndexBackendArg::File => IndexBackendKind::File,
        IndexBackendArg::Sqlite => IndexBackendKind::Sqlite,
    };

    let index_path = match (&args.index_path, backend_arg) {
        (Some(path), _) => path.clone(),
        (None, IndexBackendArg::File) => PathBuf::from(".symgrep"),
        (None, IndexBackendArg::Sqlite) => PathBuf::from(".symgrep").join("index.sqlite"),
    };

    let index = IndexConfig {
        // The concrete paths are not used by the attribute update
        // API when opening an existing index; they are included for
        // completeness and potential future use.
        paths: vec![PathBuf::from(".")],
        globs: Vec::new(),
        exclude_globs: Vec::new(),
        backend,
        index_path,
        language: None,
    };

    let selector = SymbolSelector {
        file: args.file.clone(),
        language: args.language.clone(),
        kind: args.kind.to_model(),
        name: args.name.clone(),
        start_line: args.start_line,
        end_line: args.end_line,
    };

    let description = match (&args.description, &args.description_file) {
        (Some(_), Some(_)) => {
            bail!("--description and --description-file are mutually exclusive");
        }
        (Some(desc), None) => Some(desc.clone()),
        (None, Some(path)) => Some(fs::read_to_string(path)?),
        (None, None) => None,
    };

    let attributes = SymbolAttributesUpdate {
        keywords: args.keywords.clone(),
        description,
    };

    Ok(SymbolAttributesRequest {
        index,
        selector,
        attributes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{IndexBackendKind, SearchMode};

    #[test]
    fn search_config_defaults_path_to_current_dir() {
        let args = SearchArgs {
            pattern: "foo".to_string(),
            paths: Vec::new(),
            globs: Vec::new(),
            exclude_globs: Vec::new(),
            language: None,
            literal: false,
            mode: SearchModeArg::Text,
            view: Vec::new(),
            limit: None,
            max_lines: None,
            context: None,
            use_index: false,
            reindex_on_search: false,
            index_backend: None,
            index_path: None,
            format: OutputFormat::Text,
            server: None,
            no_server: false,
        };

        let config = search_config_from_args(&args).expect("config");

        assert_eq!(config.pattern, "foo");
        assert_eq!(config.paths, vec![PathBuf::from(".")]);
        assert!(config.globs.is_empty());
        assert!(config.exclude_globs.is_empty());
        assert_eq!(config.language, None);
        assert_eq!(config.mode, SearchMode::Text);
        assert_eq!(config.limit, None);
        assert_eq!(config.max_lines, None);
        assert!(config.index.is_none());
    }

    #[test]
    fn search_config_respects_all_fields() {
        let args = SearchArgs {
            pattern: "bar".to_string(),
            paths: vec![PathBuf::from("src"), PathBuf::from("tests")],
            globs: vec!["*.rs".to_string()],
            exclude_globs: vec!["target/*".to_string()],
            language: Some("rust".to_string()),
            literal: true,
            mode: SearchModeArg::Symbol,
            view: vec![SymbolViewArg::Def],
            limit: Some(10),
            max_lines: Some(5),
            context: Some(2),
            use_index: true,
            reindex_on_search: true,
            index_backend: Some(IndexBackendArg::File),
            index_path: Some(PathBuf::from(".symgrep")),
            format: OutputFormat::Json,
            server: Some("http://localhost:7878".to_string()),
            no_server: false,
        };

        let config = search_config_from_args(&args).expect("config");

        assert_eq!(config.pattern, "bar");
        assert_eq!(
            config.paths,
            vec![PathBuf::from("src"), PathBuf::from("tests")]
        );
        assert_eq!(config.globs, vec!["*.rs".to_string()]);
        assert_eq!(config.exclude_globs, vec!["target/*".to_string()]);
        assert_eq!(config.language.as_deref(), Some("rust"));
        assert_eq!(config.mode, SearchMode::Symbol);
        assert_eq!(config.limit, Some(10));
        assert_eq!(config.max_lines, Some(5));
        assert!(config.reindex_on_search);

        let index = config.index.expect("index config");
        assert_eq!(
            index.paths,
            vec![PathBuf::from("src"), PathBuf::from("tests")]
        );
        assert_eq!(index.globs, vec!["*.rs".to_string()]);
        assert_eq!(index.exclude_globs, vec!["target/*".to_string()]);
        assert_eq!(index.backend, IndexBackendKind::File);
        assert_eq!(index.index_path, PathBuf::from(".symgrep"));
        assert_eq!(index.language.as_deref(), Some("rust"));
    }
}
