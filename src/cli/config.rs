use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::cli::args::{
    FollowDirectionArg, IndexBackendArg, OutputFormat, SearchModeArg, SymbolViewArg,
};
use crate::cli::{AnnotateArgs, FollowArgs, IndexArgs, IndexInfoArgs, SearchArgs, ServeArgs};

/// Top-level representation of `.symgrep/config.toml`.
#[derive(Debug, Default, Deserialize)]
pub struct CliConfig {
    #[serde(default)]
    pub search: Option<SearchSection>,

    #[serde(default)]
    pub index: Option<IndexSection>,

    #[serde(default, rename = "index_info")]
    pub index_info: Option<IndexInfoSection>,

    #[serde(default)]
    pub serve: Option<ServeSection>,

    #[serde(default)]
    pub follow: Option<FollowSection>,

    #[serde(default)]
    pub http: Option<HttpSection>,
}

#[derive(Debug, Default, Deserialize)]
pub struct SearchSection {
    #[serde(default)]
    pub paths: Vec<PathBuf>,
    #[serde(default)]
    pub globs: Vec<String>,
    #[serde(default, alias = "exclude")]
    pub exclude_globs: Vec<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub literal: Option<bool>,
    #[serde(default)]
    pub mode: Option<SearchModeArg>,
    #[serde(default)]
    pub view: Option<Vec<SymbolViewArg>>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub max_lines: Option<usize>,
    #[serde(default)]
    pub context: Option<usize>,
    #[serde(default)]
    pub use_index: Option<bool>,
    #[serde(default)]
    pub reindex_on_search: Option<bool>,
    #[serde(default)]
    pub index_backend: Option<IndexBackendArg>,
    #[serde(default)]
    pub index_path: Option<PathBuf>,
    #[serde(default)]
    pub format: Option<OutputFormat>,
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default)]
    pub no_server: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
pub struct IndexSection {
    #[serde(default)]
    pub paths: Vec<PathBuf>,
    #[serde(default)]
    pub globs: Vec<String>,
    #[serde(default, alias = "exclude")]
    pub exclude_globs: Vec<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub backend: Option<IndexBackendArg>,
    #[serde(default)]
    pub index_path: Option<PathBuf>,
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default)]
    pub no_server: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
pub struct IndexInfoSection {
    #[serde(default)]
    pub paths: Vec<PathBuf>,
    #[serde(default)]
    pub globs: Vec<String>,
    #[serde(default, alias = "exclude")]
    pub exclude_globs: Vec<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub backend: Option<IndexBackendArg>,
    #[serde(default)]
    pub index_path: Option<PathBuf>,
    #[serde(default)]
    pub format: Option<OutputFormat>,
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default)]
    pub no_server: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ServeSection {
    #[serde(default)]
    pub addr: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct FollowSection {
    #[serde(default)]
    pub paths: Vec<PathBuf>,
    #[serde(default)]
    pub globs: Vec<String>,
    #[serde(default, alias = "exclude")]
    pub exclude_globs: Vec<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub literal: Option<bool>,
    #[serde(default)]
    pub direction: Option<FollowDirectionArg>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub max_lines: Option<usize>,
    #[serde(default)]
    pub context: Option<usize>,
    #[serde(default)]
    pub format: Option<OutputFormat>,
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default)]
    pub no_server: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
pub struct HttpSection {
    #[serde(default)]
    pub server_url: Option<String>,
}

/// Discover and load a project-local `.symgrep/config.toml` (or
/// `.symgrep/symgrep.toml`) starting from the current working
/// directory and walking up parent directories.
pub fn load_cli_config() -> Result<Option<CliConfig>> {
    let cwd = std::env::current_dir().context("failed to read current directory")?;
    let config_path = find_project_config(&cwd);

    let Some(path) = config_path else {
        return Ok(None);
    };

    let contents = fs::read_to_string(&path)
        .with_context(|| format!("failed to read config file at {}", path.display()))?;
    let config: CliConfig = toml::from_str(&contents)
        .with_context(|| format!("failed to parse TOML config at {}", path.display()))?;

    Ok(Some(config))
}

fn find_project_config(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);

    while let Some(current) = dir {
        let symgrep_dir = current.join(".symgrep");
        let config_toml = symgrep_dir.join("config.toml");
        if config_toml.is_file() {
            return Some(config_toml);
        }

        let symgrep_toml = symgrep_dir.join("symgrep.toml");
        if symgrep_toml.is_file() {
            return Some(symgrep_toml);
        }

        dir = current.parent();
    }

    None
}

pub fn apply_search_config_defaults(config: &CliConfig, args: &mut SearchArgs) {
    if let Some(search) = &config.search {
        if args.paths.is_empty() && !search.paths.is_empty() {
            args.paths = search.paths.clone();
        }

        if args.globs.is_empty() && !search.globs.is_empty() {
            args.globs = search.globs.clone();
        }

        if args.exclude_globs.is_empty() && !search.exclude_globs.is_empty() {
            args.exclude_globs = search.exclude_globs.clone();
        }

        if args.language.is_none() {
            if let Some(language) = &search.language {
                args.language = Some(language.clone());
            }
        }

        if !args.literal {
            if let Some(true) = search.literal {
                args.literal = true;
            }
        }

        // Apply config mode only when CLI mode is still at its default value (Text).
        // This allows config to set a project-wide mode while CLI --mode always overrides.
        if matches!(args.mode, SearchModeArg::Text) {
            if let Some(mode) = search.mode {
                args.mode = mode;
            }
        }

        // Apply config view only for non-text modes (Symbol and Auto).
        // Auto mode may resolve to Symbol at runtime, so we treat it as symbol-capable.
        // CLI --view always overrides when explicitly provided (args.view non-empty).
        if args.view.is_empty() && !matches!(args.mode, SearchModeArg::Text) {
            if let Some(view) = &search.view {
                args.view = view.clone();
            }
        }

        if args.limit.is_none() {
            if let Some(limit) = search.limit {
                args.limit = Some(limit);
            }
        }

        if args.max_lines.is_none() {
            if let Some(max_lines) = search.max_lines {
                args.max_lines = Some(max_lines);
            }
        }

        if args.context.is_none() {
            if let Some(context) = search.context {
                args.context = Some(context);
            }
        }

        if !args.use_index {
            if let Some(true) = search.use_index {
                args.use_index = true;
            }
        }

        if !args.reindex_on_search {
            if let Some(true) = search.reindex_on_search {
                args.reindex_on_search = true;
            }
        }

        if args.index_backend.is_none() {
            if let Some(backend) = search.index_backend {
                args.index_backend = Some(backend);
            }
        }

        if args.index_path.is_none() {
            if let Some(index_path) = &search.index_path {
                args.index_path = Some(index_path.clone());
            }
        }

        if matches!(args.format, OutputFormat::Text) {
            if let Some(format) = search.format {
                args.format = format;
            }
        }

        if args.server.is_none() {
            if let Some(server) = &search.server {
                args.server = Some(server.clone());
            } else if let Some(http) = &config.http {
                if let Some(url) = &http.server_url {
                    args.server = Some(url.clone());
                }
            }
        }

        if !args.no_server {
            if let Some(true) = search.no_server {
                args.no_server = true;
            }
        }
    } else if args.server.is_none() {
        // If there is no per-search section, fall back to a global
        // HTTP server URL when present.
        if let Some(http) = &config.http {
            if let Some(url) = &http.server_url {
                args.server = Some(url.clone());
            }
        }
    }
}

pub fn apply_follow_config_defaults(config: &CliConfig, args: &mut FollowArgs) {
    if let Some(follow) = &config.follow {
        if args.paths.is_empty() && !follow.paths.is_empty() {
            args.paths = follow.paths.clone();
        }

        if args.globs.is_empty() && !follow.globs.is_empty() {
            args.globs = follow.globs.clone();
        }

        if args.exclude_globs.is_empty() && !follow.exclude_globs.is_empty() {
            args.exclude_globs = follow.exclude_globs.clone();
        }

        if args.language.is_none() {
            if let Some(language) = &follow.language {
                args.language = Some(language.clone());
            }
        }

        if !args.literal {
            if let Some(true) = follow.literal {
                args.literal = true;
            }
        }

        if args.limit.is_none() {
            if let Some(limit) = follow.limit {
                args.limit = Some(limit);
            }
        }

        if matches!(args.direction, FollowDirectionArg::Callers) {
            if let Some(direction) = follow.direction {
                args.direction = direction;
            }
        }

        if args.max_lines.is_none() {
            if let Some(max_lines) = follow.max_lines {
                args.max_lines = Some(max_lines);
            }
        }

        if args.context.is_none() {
            if let Some(context) = follow.context {
                args.context = Some(context);
            }
        }

        if matches!(args.format, OutputFormat::Text) {
            if let Some(format) = follow.format {
                args.format = format;
            }
        }

        if args.server.is_none() {
            if let Some(server) = &follow.server {
                args.server = Some(server.clone());
            } else if let Some(http) = &config.http {
                if let Some(url) = &http.server_url {
                    args.server = Some(url.clone());
                }
            }
        }

        if !args.no_server {
            if let Some(true) = follow.no_server {
                args.no_server = true;
            }
        }
    } else if args.server.is_none() {
        if let Some(http) = &config.http {
            if let Some(url) = &http.server_url {
                args.server = Some(url.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::{OutputFormat, SearchArgs};

    fn empty_search_args_with_mode(mode: SearchModeArg) -> SearchArgs {
        SearchArgs {
            pattern: "foo".to_string(),
            paths: Vec::new(),
            globs: Vec::new(),
            exclude_globs: Vec::new(),
            language: None,
            literal: false,
            mode,
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
        }
    }

    #[test]
    fn search_config_applies_view_for_symbol_mode() {
        let config = CliConfig {
            search: Some(SearchSection {
                view: Some(vec![SymbolViewArg::Def, SymbolViewArg::Matches]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let mut args = empty_search_args_with_mode(SearchModeArg::Symbol);

        apply_search_config_defaults(&config, &mut args);

        assert_eq!(
            args.view,
            vec![SymbolViewArg::Def, SymbolViewArg::Matches]
        );
    }

    #[test]
    fn search_config_does_not_apply_view_in_text_mode() {
        let config = CliConfig {
            search: Some(SearchSection {
                view: Some(vec![SymbolViewArg::Def]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let mut args = empty_search_args_with_mode(SearchModeArg::Text);

        apply_search_config_defaults(&config, &mut args);

        assert!(args.view.is_empty());
    }

    #[test]
    fn search_config_respects_cli_view_override() {
        let config = CliConfig {
            search: Some(SearchSection {
                view: Some(vec![SymbolViewArg::Def]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let mut args = empty_search_args_with_mode(SearchModeArg::Symbol);
        args.view = vec![SymbolViewArg::Decl];

        apply_search_config_defaults(&config, &mut args);

        assert_eq!(args.view, vec![SymbolViewArg::Decl]);
    }

    #[test]
    fn search_config_applies_reindex_on_search_flag() {
        let config = CliConfig {
            search: Some(SearchSection {
                reindex_on_search: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };

        let mut args = empty_search_args_with_mode(SearchModeArg::Symbol);

        apply_search_config_defaults(&config, &mut args);

        assert!(args.reindex_on_search);
    }
}

pub fn apply_index_config_defaults(config: &CliConfig, args: &mut IndexArgs) {
    if let Some(index) = &config.index {
        if args.paths.is_empty() && !index.paths.is_empty() {
            args.paths = index.paths.clone();
        }

        if args.globs.is_empty() && !index.globs.is_empty() {
            args.globs = index.globs.clone();
        }

        if args.exclude_globs.is_empty() && !index.exclude_globs.is_empty() {
            args.exclude_globs = index.exclude_globs.clone();
        }

        if args.language.is_none() {
            if let Some(language) = &index.language {
                args.language = Some(language.clone());
            }
        }

        if args.backend.is_none() {
            if let Some(backend) = index.backend {
                args.backend = Some(backend);
            }
        }

        if args.index_path.is_none() {
            if let Some(index_path) = &index.index_path {
                args.index_path = Some(index_path.clone());
            }
        }

        if args.server.is_none() {
            if let Some(server) = &index.server {
                args.server = Some(server.clone());
            } else if let Some(http) = &config.http {
                if let Some(url) = &http.server_url {
                    args.server = Some(url.clone());
                }
            }
        }

        if !args.no_server {
            if let Some(true) = index.no_server {
                args.no_server = true;
            }
        }
    } else if args.server.is_none() {
        if let Some(http) = &config.http {
            if let Some(url) = &http.server_url {
                args.server = Some(url.clone());
            }
        }
    }
}

pub fn apply_index_info_config_defaults(config: &CliConfig, args: &mut IndexInfoArgs) {
    if let Some(info) = &config.index_info {
        if args.paths.is_empty() && !info.paths.is_empty() {
            args.paths = info.paths.clone();
        }

        if args.globs.is_empty() && !info.globs.is_empty() {
            args.globs = info.globs.clone();
        }

        if args.exclude_globs.is_empty() && !info.exclude_globs.is_empty() {
            args.exclude_globs = info.exclude_globs.clone();
        }

        if args.language.is_none() {
            if let Some(language) = &info.language {
                args.language = Some(language.clone());
            }
        }

        if args.backend.is_none() {
            if let Some(backend) = info.backend {
                args.backend = Some(backend);
            }
        }

        if args.index_path.is_none() {
            if let Some(index_path) = &info.index_path {
                args.index_path = Some(index_path.clone());
            }
        }

        if matches!(args.format, OutputFormat::Text) {
            if let Some(format) = info.format {
                args.format = format;
            }
        }

        if args.server.is_none() {
            if let Some(server) = &info.server {
                args.server = Some(server.clone());
            } else if let Some(http) = &config.http {
                if let Some(url) = &http.server_url {
                    args.server = Some(url.clone());
                }
            }
        }

        if !args.no_server {
            if let Some(true) = info.no_server {
                args.no_server = true;
            }
        }
    } else if args.server.is_none() {
        if let Some(http) = &config.http {
            if let Some(url) = &http.server_url {
                args.server = Some(url.clone());
            }
        }
    }
}

pub fn apply_serve_config_defaults(config: &CliConfig, args: &mut ServeArgs) {
    if let Some(serve) = &config.serve {
        if args.addr == "127.0.0.1:7878" {
            if let Some(addr) = &serve.addr {
                args.addr = addr.clone();
            }
        }
    }
}

pub fn apply_annotate_config_defaults(config: &CliConfig, args: &mut AnnotateArgs) {
    if args.server.is_none() {
        if let Some(http) = &config.http {
            if let Some(url) = &http.server_url {
                args.server = Some(url.clone());
            }
        }
    }
}
