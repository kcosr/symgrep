# Symgrep – Phases 1–3

This document contains detailed tasks for:

- Phase 1 – Project Setup & Skeleton CLI
- Phase 2 – Tree-sitter Integration & TypeScript/JavaScript Backend
- Phase 3 – C++ Backend

The high-level architecture, guardrails, and scaffolding are defined in `IMPLEMENTATION_PLAN.md`.

---

## Phase 1 – Project Setup & Skeleton CLI

**Goals:**

- Set up Rust project and initial scaffolding.
- Implement basic CLI with `search` command stub.
- Implement basic text search mode (no AST, language-agnostic).
- Establish core `SearchConfig → SearchResult` and `IndexConfig → IndexSummary` APIs.

**Tasks:**

1. **Scaffold the project**
   - Create the cargo project `symgrep` as a bin+lib crate.
   - Create the directory layout as described in the overview:
     - `src/main.rs`, `src/lib.rs`
     - `src/cli/`, `src/search/`, `src/language/`, `src/index/`, `src/models/`
   - Add minimal code for:
     - `cli::run()` that:
       - Parses a placeholder `search` subcommand.
       - Calls `search::run_search()` with a stub `SearchConfig`.

2. **Define core models**
   - Add `SearchConfig`, `SearchResult`, `IndexConfig`, `IndexSummary` structs in `search::engine` or `models`:
     - `SearchConfig` should contain:
       - `pattern`, `path`, `glob`, `exclude`, `language`, `mode`, `context`, `limit`, `max_lines`, etc.
     - `SearchResult` should contain:
       - Original query string.
       - A list of matches (even if minimal).
       - A summary struct with `total_matches` and `truncated` flags.
   - Implement `SearchConfig::from_args(&SearchArgs)` in a separate module (e.g. `cli::args`).

3. **Implement skeleton CLI**
   - Use `clap` to define:
     - `symgrep search <pattern> [--path ...] [--format text|json]`.
   - In `cli::run()`:
     - Build `SearchConfig` from args.
     - Call `search::run_search(config)`.

4. **Implement naive text search**
   - Implement `run_search` to:
     - Walk files under `path` using `ignore` crate.
     - Filter by globs and excludes.
     - For each file, search for the `pattern` as plain text (line-by-line).
     - Collect matches into `SearchResult`.
   - Implement a minimal `Match` struct with:
     - `file`, `line_number`, and the line text.

5. **Output formats**
   - Implement:
     - `--format=text`:
       - Print lines like `path:line: text`.
     - `--format=json`:
       - Serialize `SearchResult` with `serde_json`.
   - Ensure formatting is done only in the CLI layer, not in `search::engine`.

6. **Tests**
   - Unit tests:
     - `SearchConfig::from_args` behavior.
     - Naive text search on small in-memory or temporary files.
   - Integration test:
     - Use a small fixture directory and verify:
       - `symgrep search pattern --format=json` returns matches as expected.
   - Run `cargo test` after each change and keep expanding coverage as features are added.

**Deliverable:**  
`v0.1.0` – text-based grep-like CLI with JSON output, no language semantics.

---

## Phase 2 – Tree-sitter Integration & TypeScript/JavaScript Backend

**Goals:**

- Integrate tree-sitter.
- Implement TypeScript/JavaScript language backends.
- Support symbol indexing and `--context=decl|def` for TS/JS.

**Tasks:**

1. **Add tree-sitter dependencies**
   - Add `tree-sitter` and:
     - `tree-sitter-typescript` (TS/TSX)
     - `tree-sitter-javascript` (JS/JSX)
   - Create `language::typescript` and `language::javascript` modules.

2. **Define `LanguageBackend` trait**
   - In `language::mod`:
     - Define the trait as in the overview (`id`, `file_extensions`, `parse_file`, `index_symbols`, `get_context_snippet`, etc.).
   - Implement a registry that:
     - Maps file extensions to a backend.
     - Is used by `search::engine` to pick the correct backend.

3. **Implement TS/JS backends (MVP)**
   - For each backend:
     - Use tree-sitter to parse a file into an AST.
     - Implement `index_symbols` to extract:
       - Functions (free and methods).
       - Classes.
       - Top-level `const`/`let`/`var` declarations.
     - Implement `get_context_snippet`:
       - For `decl`: return the declaration line(s).
       - For `def`: return the full function or class body.

4. **Wire symbol mode into `run_search`**
   - Extend `SearchConfig` with a `mode` field (`text` | `symbol` | `auto`).
   - For `mode=symbol` or `auto` when symbol filters are used:
     - Use language backends to:
       - Parse and index symbols.
       - Filter symbols by name/kind where applicable.
     - Populate `SearchResult` with symbol-based matches and snippets.

5. **Context control flags**
   - Add `--context=decl|def|parent|none` to `SearchArgs`.
   - Implement:
     - `decl` and `def` for TS/JS.
     - `parent` can temporarily be mapped to `def` in this phase (true parent resolution comes later in Phase 4).

6. **JSON output extensions**
   - Extend `SearchResult`:
     - Include `Symbol` and `ContextInfo` data where applicable.
   - Ensure JSON output remains stable and versioned (e.g., `version: "0.3.0"`).

7. **Tests**
   - Unit tests:
     - TS/JS symbol extraction on small fixtures.
     - `get_context_snippet` behavior for `decl` and `def`.
   - Integration tests:
     - Run `symgrep search name:Foo --language typescript --format=json`.
     - Verify that:
       - The symbol is found.
       - The snippet lines match expectations.
   - Keep using `cargo test` after each change.

**Deliverable:**  
`v0.3.0` – TS/JS symbol-aware search with declaration/definition context.

---

## Phase 3 – C++ Backend

**Goals:**

- Add C++ support.
- Handle basic C++ constructs and context retrieval.

**Tasks:**

1. **Add C++ tree-sitter backend**
   - Add `tree-sitter-cpp` dependency.
   - Implement `language::cpp::Backend`:
     - Map C++ file extensions (`.cpp`, `.cc`, `.cxx`, `.hpp`, `.hh`, `.hxx`).
     - Parse files using tree-sitter.
     - Extract:
       - Free functions and methods.
       - Classes/structs.
       - Namespaces.
       - Variables where practical.

2. **Integrate C++ backend with registry**
   - Register the C++ backend in `language::mod` so it is chosen based on file extension or `--language cpp`.

3. **Implement context for C++**
   - `decl`:
     - Function / method / class / struct declarations.
   - `def`:
     - Full function or class body, where possible.
   - `parent`:
     - For Phase 3, `parent` can behave like `def` or a minimal approximation; full parent chain will be fleshed out in Phase 4.

4. **CLI enhancements**
   - Add `--language cpp` support to `SearchArgs`.
   - Ensure auto-detection of C++ files by extension.
   - Update help text to mention C++.

5. **Tests**
   - Unit tests:
     - C++ symbol extraction on small fixtures.
     - Context snippet extraction for `decl` and `def`.
   - Integration tests:
     - Mixed repo fixture with TS/JS/C++.
     - Run `symgrep search name:Foo --language cpp` and validate results.
   - Run full tests (including TS/JS) to ensure no regressions.

**Deliverable:**  
`v0.3.0` – C++ support with symbol-aware search.
