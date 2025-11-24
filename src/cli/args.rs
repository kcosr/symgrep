use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Deserialize;

use crate::models::{IndexBackendKind, IndexConfig, SearchConfig, SearchContext, SearchMode};
use crate::search::query::parse_query_expr;

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

    /// Requested context for each match.
    #[arg(long = "context", value_enum, default_value_t = ContextArg::None)]
    pub context: ContextArg,

    /// Maximum number of matches to return.
    #[arg(long = "limit")]
    pub limit: Option<usize>,

    /// Maximum number of lines per match snippet.
    #[arg(long = "max-lines")]
    pub max_lines: Option<usize>,

    /// Whether to use an existing index for symbol searches.
    #[arg(long = "use-index")]
    pub use_index: bool,

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

/// CLI representation of search mode.
#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchModeArg {
    Text,
    Symbol,
    Auto,
}

/// CLI representation of context mode.
#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContextArg {
    None,
    Decl,
    Def,
    Parent,
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

    let context = match args.context {
        ContextArg::None => SearchContext::None,
        ContextArg::Decl => SearchContext::Decl,
        ContextArg::Def => SearchContext::Def,
        ContextArg::Parent => SearchContext::Parent,
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

    Ok(SearchConfig {
        pattern: args.pattern.clone(),
        paths,
        globs: args.globs.clone(),
        exclude_globs: args.exclude_globs.clone(),
        language: args.language.clone(),
        mode,
        literal: args.literal,
        context,
        limit: args.limit,
        max_lines: args.max_lines,
        query_expr: parse_query_expr(&args.pattern),
        index,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{IndexBackendKind, SearchContext, SearchMode};

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
            context: ContextArg::None,
            limit: None,
            max_lines: None,
            use_index: false,
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
        assert_eq!(config.context, SearchContext::None);
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
            context: ContextArg::Def,
            limit: Some(10),
            max_lines: Some(5),
            use_index: true,
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
        assert_eq!(config.context, SearchContext::Def);
        assert_eq!(config.limit, Some(10));
        assert_eq!(config.max_lines, Some(5));

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
