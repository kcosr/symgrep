# Symsemantic Code Search CLI – Implementation Plan (Overview)

> Goal: Build a Rust-based CLI that acts like a “semantic grep” for codebases (TS/JS/C++ first, extensible later) with LLM-friendly output and rich context controls.

This overview describes the **high-level architecture, guardrails, tech stack, and project scaffolding**.  
Detailed phase-by-phase tasks live in separate files:

- `PHASES_01-03.md` – Phase 1 (skeleton CLI) through Phase 3 (C++ backend)
- `PHASES_04-06.md` – Phase 4 (parent context + DSL) through Phase 6 (indexing backends)
- `PHASES_07-08.md` – Phase 7 (LLM integration) and Phase 8 (future/daemon roadmap)

---

## 0. High-Level Goals & Scope

### 0.1 Primary Goals

- Provide a CLI that can:
  - Search **text** (like `grep` / `ripgrep`).
  - Search **symbols** (functions, variables, classes, etc.).
  - Return **configurable context**:
    - Declaration-only
    - Full definition
    - Parent context (e.g. enclosing function/class/module)
  - Search:
    - Across entire repos
    - Within specific files / globs
  - Output:
    - Plain text (human-readable)
    - Table
    - JSON (LLM/machine-friendly)

- Support languages:
  - Short term: TypeScript, JavaScript, C++
  - Longer term: additional tree-sitter languages (Python, Go, Rust, etc.)

- Optimize UX for **LLM coding agents**:
  - Stable JSON schemas
  - Predictable CLI interface
  - Clear, machine-friendly error codes

### 0.2 Non-Goals (for v1)

- Full semantic understanding of the code (e.g., full type resolution, advanced refactoring).
- Full LSP integration (tsserver/clangd/etc.) – may come later as optional enhancement.
- IDE plugin integration – out of scope for initial CLI, but keep APIs extensible.

---

## 1. Architecture Overview

### 1.1 Core Components

1. **CLI Frontend**
   - `symgrep` (working name).
   - Argument parsing, subcommand routing, config loading.
   - Output formatting (text/table/json).

2. **Search Engine Layer**
   - Orchestrates:
     - Fast text search (ripgrep-like).
     - Symbol/AST search (tree-sitter).
   - Combines results with requested context and formats return values for the CLI / daemon.

3. **Language Backends**
   - Implement a common interface, e.g.:
     - `LanguageBackend::parse_file(...)`
     - `LanguageBackend::index_symbols(...)`
     - `LanguageBackend::get_context(...)`
   - Backends for:
     - TypeScript/JavaScript
     - C++
     - Others later.

4. **Index, Caching & Storage Backends**
   - Optional, pluggable indexing layer used to avoid reparsing and rescanning for every query.
   - Responsibilities:
     - Store file metadata (path, language, hash/mtime).
     - Store symbol records (name, kind, location, language).
     - Later: store reference records (usage sites).
   - Backends:
     - `file` backend (JSON/JSONL files).
     - `sqlite` backend (single DB file).
     - Future: Postgres/other networked backends.

### 1.2 Deployment Modes: CLI vs Daemon

From the beginning, the design should support two primary ways of running `symgrep`:

1. **Local one-shot mode (no daemon)**  
   - CLI directly:
     - Walks the filesystem.
     - Parses files with tree-sitter.
     - Optionally uses on-disk indexes (`file` / `sqlite`).
   - Best for:
     - Simple, ad-hoc queries.
     - CI pipelines that want a single command.
     - Environments where a daemon is not desirable.

2. **Daemon / service mode (later phase)**
   - Long-running `symgrep` process (e.g. `symgrep serve`) that:
     - Hosts indexes and caches in memory.
     - Exposes an HTTP+JSON API (e.g. `/v1/search`, `/v1/index`, `/v1/health`).
   - CLI and LLM agents can:
     - Connect via `--server <URL>` or `SYMGREP_SERVER_URL`.
     - Offload heavy indexing and parsing to the daemon.
   - Supports:
     - Local daemon on `localhost`.
     - Remote/shared daemon (later).

3. **CLI behavior**
   - By default, the CLI:
     - Runs in **local one-shot mode** if no server is configured.
   - If a server is configured:
     - CLI acts as a **thin client** that:
       - Serializes arguments into JSON.
       - Sends a request to the daemon.
       - Renders the response according to `--format`.
   - A `--no-server` flag can force local behavior even if a server URL is configured.

---

## 2. Core Guardrails & Separation of Concerns

These are the **engineering guardrails** that should be followed from Phase 1 onward to make later daemonization and refactors painless.

### 2.1 Core “search as a function” API

All real logic should flow through a small, stable library API:

```rust
pub struct SearchConfig { /* built from CLI or HTTP */ }
pub struct SearchResult { /* what we return / serialize */ }

pub fn run_search(config: SearchConfig) -> anyhow::Result<SearchResult> { /* core */ }

pub struct IndexConfig { /* index params */ }
pub struct IndexSummary { /* stats & metadata */ }

pub fn run_index(config: IndexConfig) -> anyhow::Result<IndexSummary> { /* core */ }
```

- **CLI**:
  - Parse arguments → build `SearchConfig` / `IndexConfig`.
  - Call `run_search` / `run_index`.
  - Pretty-print `SearchResult` / `IndexSummary`.

- **Daemon (later)**:
  - HTTP handlers parse JSON → build `SearchConfig` / `IndexConfig`.
  - Call the same core functions.
  - Serialize results as JSON and return.

### 2.2 No printing inside core logic

- Only the CLI and daemon HTTP layer should:
  - `println!` / write to stdout.
  - Format text or tables.
- Core modules (`search`, `language`, `index`, etc.) should:
  - Return structured values and errors, **never** print directly.
- Benefits:
  - Easier testing (no need to capture stdout).
  - HTTP server becomes trivial to layer on later.

### 2.3 Backends must not know about CLI

- Language backends and index backends must **not**:
  - Read environment variables directly.
  - Parse CLI flags.
  - Print or handle I/O.
- They should only:
  - Receive typed configs / options.
  - Return typed results / errors.
- All user-facing concerns (flags, env vars, colors) stay strictly in the CLI or daemon layers.

### 2.4 JSON schema as the shared contract

- The `SearchResult` and `IndexSummary` types should:
  - Be `serde`-serializable.
  - Represent the **single source of truth** for:
    - CLI `--format=json`.
    - Future daemon HTTP responses.
- Any backward-incompatible change to these types must go through:
  - Schema version bump and documentation update.

---

## 3. Tech Stack & Dependencies

(Same content as before, but summarized.)

- **Rust 2021**.
- `clap`, `anyhow`, `thiserror` for CLI & errors.
- `ignore`, `globset`, `regex` for file search.
- `tree-sitter` + language-specific grammars.
- `rayon` for parallelism.
- `serde` + `serde_json` for structured data.
- `rusqlite` (later) for indexing.
- HTTP crate (e.g. `axum`, `hyper`) for daemon endpoint.

(See phase files for dependencies introduced per phase.)

---

## 4. Project Scaffolding Layout

The repository structure should be established early (Phase 1) to reflect the layered architecture and guardrails.

### 4.1 Repository layout

```text
symgrep/
  Cargo.toml
  src/
    main.rs          # CLI entrypoint: parses args, calls symgrep::cli::run()
    lib.rs           # pub mod cli; search; language; index; models; server (later)

    cli/
      mod.rs         # top-level CLI wiring (subcommands, dispatch)
      args.rs        # SearchArgs, IndexArgs, common flags

    search/
      mod.rs         # re-exports & high-level orchestration
      engine.rs      # run_search / run_index implementations
      query.rs       # DSL query parsing

    language/
      mod.rs         # backend registration
      typescript.rs  # tree-sitter TS backend
      javascript.rs  # tree-sitter JS backend
      cpp.rs         # tree-sitter C++ backend

    index/
      mod.rs         # high-level index API
      backend.rs     # BackendKind + IndexBackend trait + shared types
      file_backend.rs
      sqlite_backend.rs

    models/
      mod.rs
      symbol.rs      # Symbol, SymbolKind
      match.rs       # Match, MatchKind
      context.rs     # ContextInfo, ContextNode

    server/          # added in daemon phase
      mod.rs         # HTTP server, request/response structs

  tests/
    cli_search.rs    # integration tests for `symgrep search`
    cli_index.rs     # integration tests for `symgrep index`
    fixtures/        # small example repos (TS/JS/C++/mixed)
    snapshots/       # snapshot/golden outputs for text/JSON/table

  docs/
    IMPLEMENTATION_PLAN.md     # this overview
    PHASES_01-03.md
    PHASES_04-06.md
    PHASES_07-08.md
    TESTING.md                 # testing philosophy & layout
    CLI_REFERENCE.md           # CLI flags & examples (later)
    JSON_SCHEMA.md             # JSON structure & schema (later)
    INDEX_BACKENDS.md          # file/sqlite index formats (later)
    DAEMON_API.md              # HTTP API (later)
```

### 4.2 Guardrail summary for scaffolding

- `search/engine.rs` is the **only place** that knows how to:
  - Call language backends.
  - Call index backends.
  - Implement the full `SearchConfig → SearchResult` logic.
- `cli/`:
  - Knows about flags, environment, and formatting.
  - Does **not** contain any parsing or indexing logic beyond building configs and rendering.
- `server/` (later):
  - Only responsible for HTTP → config and result → JSON.

---

## 5. Phase Files

Detailed, step-by-step tasks and deliverables are split into separate documents:

- **`PHASES_01-03.md`**
  - Phase 1 – Project Setup & Skeleton CLI
  - Phase 2 – Tree-sitter Integration & TypeScript/JavaScript Backend
  - Phase 3 – C++ Backend

- **`PHASES_04-06.md`**
  - Phase 4 – Parent Context & Improved DSL
  - Phase 5 – Output Polish & Table Format
  - Phase 6 – Indexing, Caching & Configurable Backends (File + SQLite)

- **`PHASES_07-08.md`**
  - Phase 7 – LLM/Agent Integration Guides
  - Phase 8 – Future Enhancements / Daemon roadmap (interactive mode & service mode details)

Each phase file follows the pattern:

- **Goals**
- **Tasks**
- **Deliverable**

and assumes the guardrails and scaffolding described here.

---

## 6. Testing & QA (High-Level)

- After each significant change:
  - Run `cargo test`.
  - Add/extend unit & integration tests for new behaviors.
- Critical invariants:
  - CLI `--format=json` output remains stable (or versioned).
  - Search results are identical between:
    - non-indexed vs indexed runs (where applicable).
    - local CLI vs future daemon mode (where applicable).

(See `PHASES_01-03.md` and `docs/TESTING.md` for phase-specific testing details.)

---

## 7. Initial Milestones Summary

- **M1** – Basic CLI + text search (`v0.1.0`)
- **M2** – TS/JS tree-sitter backend + symbol context (`v0.2.0`)
- **M3** – C++ backend (`v0.3.0`)
- **M4** – Parent context + DSL (`v0.4.0`)
- **M5** – Polished output + JSON schema docs (`v0.5.0`)
- **M6** – Indexing + configurable backends (file + SQLite) (`v0.6.0`)
- **M7** – LLM integration guides (`v0.7.0`)
- **M8** – Daemon / interactive mode & service architecture (beyond initial CLI) (`v0.8.0+`)
