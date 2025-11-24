use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::cli::args::{ContextArg, IndexBackendArg, OutputFormat, SearchModeArg};
use crate::cli::{IndexArgs, IndexInfoArgs, SearchArgs, ServeArgs};

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
    pub context: Option<ContextArg>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub max_lines: Option<usize>,
    #[serde(default)]
    pub use_index: Option<bool>,
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

        if matches!(args.mode, SearchModeArg::Text) {
            if let Some(mode) = search.mode {
                args.mode = mode;
            }
        }

        if matches!(args.context, ContextArg::None) {
            if let Some(context) = search.context {
                args.context = context;
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

        if !args.use_index {
            if let Some(true) = search.use_index {
                args.use_index = true;
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
