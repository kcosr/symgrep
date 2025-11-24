# Repository-specific instructions

## Development guidelines

- Keep core engine modules (`search`, `language`, `index`, `models`) free of CLI concerns: no printing, env access, or argument parsing in core logic.
- CLI code (`src/cli`) is responsible for parsing args, building configs, and formatting output; it must call `run_search` / `run_index` rather than duplicating behavior.
- Treat JSON output as a stable API: backward-incompatible changes require a schema version bump and docs update under `docs/`.
- For any new feature or behavior change, add or update tests and run `cargo test` before opening a PR (once the Rust project exists); tests must be deterministic and offline.
- For any new feature or behavior change, update documentation appropriately. See docs and README.md.
