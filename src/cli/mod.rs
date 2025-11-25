use std::net::SocketAddr;

use anyhow::Result;
use clap::{CommandFactory, Parser};

use crate::models::SEARCH_RESULT_VERSION;
use crate::search::engine;
use crate::server;

mod args;
mod follow;
mod format;
mod http_backend;
mod config;

pub use args::{
    AnnotateArgs, Cli, Commands, FollowArgs, IndexArgs, IndexInfoArgs, OutputFormat, SearchArgs,
    ServeArgs,
};

use config::{
    apply_annotate_config_defaults, apply_follow_config_defaults, apply_index_config_defaults,
    apply_index_info_config_defaults, apply_search_config_defaults, apply_serve_config_defaults,
    load_cli_config,
};
use http_backend::HttpSearchBackend;

/// Entry point for the CLI binary.
pub fn run() -> Result<()> {
    let cli = Cli::parse();

    if cli.schema_version {
        println!(
            "Search result JSON schema version: {}",
            SEARCH_RESULT_VERSION
        );
        return Ok(());
    }

    let cli_config = load_cli_config()?;

    match cli.command {
        Some(Commands::Search(mut search_args)) => {
            if let Some(ref config) = cli_config {
                apply_search_config_defaults(config, &mut search_args);
            }

            let config = args::search_config_from_args(&search_args)?;
            let result = if let Some(server_url) =
                effective_server_url(
                    search_args.server.as_deref(),
                    search_args.no_server,
                )
            {
                let backend = HttpSearchBackend::new(server_url)?;
                backend.search(config)?
            } else {
                engine::run_search(config)?
            };

            match search_args.format {
                OutputFormat::Text => format::print_text(&result, &search_args),
                OutputFormat::Table => format::print_table(&result),
                OutputFormat::Json => {
                    serde_json::to_writer(std::io::stdout(), &result)?;
                    println!();
                    Ok(())
                }
            }
        }
        Some(Commands::Index(mut index_args)) => {
            if let Some(ref config) = cli_config {
                apply_index_config_defaults(config, &mut index_args);
            }

            let config = args::index_config_from_args(&index_args)?;
            let summary = if let Some(server_url) =
                effective_server_url(
                    index_args.server.as_deref(),
                    index_args.no_server,
                )
            {
                let backend = HttpSearchBackend::new(server_url)?;
                backend.index(config)?
            } else {
                engine::run_index(config)?
            };

            println!(
                "Indexed {} files and {} symbols using {:?} backend at {}",
                summary.files_indexed,
                summary.symbols_indexed,
                summary.backend,
                summary.index_path.display()
            );

            Ok(())
        }
        Some(Commands::IndexInfo(mut info_args)) => {
            if let Some(ref config) = cli_config {
                apply_index_info_config_defaults(config, &mut info_args);
            }

            let config = args::index_info_config_from_args(&info_args)?;
            let summary = if let Some(server_url) =
                effective_server_url(
                    info_args.server.as_deref(),
                    info_args.no_server,
                )
            {
                let backend = HttpSearchBackend::new(server_url)?;
                backend.index_info(config)?
            } else {
                crate::index::get_index_info(&config)?
            };

            match info_args.format {
                OutputFormat::Text | OutputFormat::Table => {
                    format::print_index_summary_text(&summary)
                }
                OutputFormat::Json => {
                    serde_json::to_writer(std::io::stdout(), &summary)?;
                    println!();
                    Ok(())
                }
            }
        }
        Some(Commands::Follow(mut follow_args)) => {
            if let Some(ref config) = cli_config {
                apply_follow_config_defaults(config, &mut follow_args);
            }

            let search_config = args::follow_search_config_from_args(&follow_args)?;
            let search_result = if let Some(server_url) = effective_server_url(
                follow_args.server.as_deref(),
                follow_args.no_server,
            ) {
                let backend = HttpSearchBackend::new(server_url)?;
                backend.search(search_config)?
            } else {
                engine::run_search(search_config)?
            };

            let direction = follow_args.direction.to_model();
            let follow_result = follow::build_follow_result(&search_result, direction);

            match follow_args.format {
                OutputFormat::Json => {
                    serde_json::to_writer(std::io::stdout(), &follow_result)?;
                    println!();
                    Ok(())
                }
                // Table output is not currently specialized for follow;
                // treat it as text for now.
                OutputFormat::Text | OutputFormat::Table => {
                    follow::print_follow_text(&follow_result, &follow_args)
                }
            }
        }
        Some(Commands::Serve(mut serve_args)) => {
            if let Some(ref config) = cli_config {
                apply_serve_config_defaults(config, &mut serve_args);
            }

            let addr: SocketAddr = serve_args.addr.parse()?;
            println!("Starting symgrep HTTP server on http://{addr}");

            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;

            runtime.block_on(server::run(addr))?;
            Ok(())
        }
        Some(Commands::Annotate(mut annotate_args)) => {
            if let Some(ref config) = cli_config {
                apply_annotate_config_defaults(config, &mut annotate_args);
            }

            let request = args::symbol_attributes_request_from_args(&annotate_args)?;

            let response = if let Some(server_url) = effective_server_url(
                annotate_args.server.as_deref(),
                annotate_args.no_server,
            ) {
                let backend = HttpSearchBackend::new(server_url)?;
                backend.update_symbol_attributes(request)?
            } else {
                crate::index::update_symbol_attributes(request)?
            };

            serde_json::to_writer(std::io::stdout(), &response)?;
            println!();
            Ok(())
        }
        None => {
            let mut cmd = Cli::command();
            cmd.print_help()?;
            println!();
            Ok(())
        }
    }
}

fn effective_server_url(
    server_flag: Option<&str>,
    no_server: bool,
) -> Option<String> {
    if no_server {
        None
    } else {
        server_flag.map(|s| s.to_string())
    }
}
