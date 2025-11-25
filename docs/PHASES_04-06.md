# Symgrep – Phases 4–6

This document contains detailed tasks for:

- Phase 4 – Parent Context & Improved DSL
- Phase 5 – Output Polish & Table Format
- Phase 6 – Indexing, Caching & Configurable Backends (File + SQLite)

---

## Phase 4 – Parent Context & Improved DSL

**Goals:**

- Implement `--context=parent`.
- Implement query DSL with OR (`|`) and AND semantics.
- Improve text+symbol hybrid behavior.

**Tasks:**

1. **Parent context resolution**
   - In each language backend:
     - Implement AST parent traversal to determine the enclosing context node:
       - For variables: enclosing function/method.
       - For functions/methods: enclosing class/namespace/file.
       - For classes: enclosing module/file.
   - Represent parent chain as `ContextInfo.parent_chain`:
     - Ordered from outermost (file/module) to innermost (function).

2. **Context kinds**
   - Refine behavior:
     - `decl`:
       - Only declaration line(s).
     - `def`:
       - Entire definition body (function/class).
     - `parent`:
       - One level up from the matched symbol or usage site.
     - `none`:
       - No snippets, metadata only.

3. **DSL parser improvements**
   - Implement real parsing of:
     - `name:foo|bar kind:function`
     - `name:Foo kind:class|interface`
   - Define `QueryExpr` with AND/OR combinators:
     - `A B` → `And(A, B)`
     - `A|B` → `Or(A, B)`
   - Handle basic precedence:
     - OR groups first, then AND inside groups.
     - Parentheses may still be omitted in MVP.

4. **Link query DSL to engine**
   - Extend `SearchConfig` to include a parsed `QueryExpr`.
   - Use this expression to:
     - Filter files (via `file:` terms).
     - Filter languages (via `language:` terms).
     - Filter symbols (via `name:` and `kind:`).

5. **Hybrid text+symbol behavior**
   - For queries containing both text and symbol terms:
     - Use symbol filters to narrow symbol candidates.
     - For each symbol match, optionally:
       - Run text search within its context (if a text term is present).

6. **Tests**
   - Unit tests:
     - DSL parsing for a variety of combinations (AND, OR, mixed).
     - Parent context resolution for TS/JS/C++.
   - Integration tests:
     - Queries like:
       - `name:Limiter kind:class|interface`
       - `name:process|handle content:"rate limit"`
       - Use `--context=parent` and validate snippets.
   - Run full test suite to confirm no regressions.

**Deliverable:**  
`v0.4.0` – full set of context modes and richer symsemantic queries.

---

## Phase 5 – Output Polish & Table Format

**Goals:**

- Improve human-readable output.
- Implement `--format=table`.
- Make JSON output schema stable and documented.

**Tasks:**

1. **Text output polish**
   - Define a standard text format:
     - `path:line:col: kind name`
     - Followed by indented snippet lines.
   - Include optional:
     - Line numbers.
     - Context kind (`decl` / `def` / `parent`).

2. **Table output**
   - Implement `--format=table`:
     - Columns:
       - FILE
       - LINE
       - KIND
       - NAME
       - CONTEXT (e.g. function or class name)
   - Align columns dynamically for terminal width (best effort).

3. **JSON schema documentation**
   - Extract the JSON structure used in `--format=json` into a separate `docs/JSON_SCHEMA.md`:
     - Document:
       - Top-level structure.
       - `results` entries.
       - `match_info`, `context`, `snippet`, `summary`.
     - Mention `version` field and versioning policy.

4. **Schema versioning**
   - Add `--schema-version` flag:
     - Prints current JSON schema version and a short description.
   - Ensure:
     - Backwards-incompatible changes bump the schema version.
     - Minor additive changes note new fields.

5. **Tests**
   - Snapshot tests:
     - Store golden text and table outputs for representative queries.
   - JSON schema tests:
     - Validate that JSON outputs conform to expected structure (optionally using a JSON schema tool).
   - After formatting changes:
     - Review snapshot diffs.
     - Update snapshots only when changes are intentional.

**Deliverable:**  
`v0.5.0` – polished text/table output with a documented, versioned JSON schema.

---

## Phase 6 – Indexing, Caching & Configurable Backends (File + SQLite)

**Goals:**

- Improve performance for large repos by reusing parsed structure across runs.
- Introduce a pluggable index backend:
  - File-based JSON/JSONL backend.
  - SQLite backend.
- Provide `symgrep index` command for pre-indexing.
- Make `--use-index` a first-class option for symbol queries.

### 6.1 Shared logical index model

- `ProjectIndex` containing:
  - `files`: `FileRecord`
  - `symbols`: `SymbolRecord`
  - `refs` (later): `ReferenceRecord`
  - `meta`: `IndexMeta`

**FileRecord**:

- `id`, `path`, `language`, `hash`, `mtime`, `size?`.

**SymbolRecord**:

- `id`, `file_id`, `name`, `kind`, `language`, `range`, `signature?`, `extra?`.

**IndexMeta**:

- `schema_version`, `tool_version`, `created_at`, `updated_at`.

### 6.2 Backend abstraction

- Implement `BackendKind` and `IndexBackend` trait (see overview).
- Engine (`search::engine`) should:
  - Depend only on `IndexBackend`, not on concrete file/SQLite implementations.

---

### Phase 6A – File-Based Backend

**Goals:**

- Simple, debuggable file-based index backend using JSON/JSONL.
- Good enough for moderate-sized repos and early performance wins.

**Tasks:**

1. **File backend layout**
   - Use `.symgrep/` in project root:
     - `meta.json`
     - `files.jsonl`
     - `symbols.jsonl`
     - `refs.jsonl` (later)

2. **Implement `FileIndexBackend`**
   - Implement:
     - `open`, `initialize`, `load_meta`, `save_meta`.
     - `upsert_file`, `list_files`, `get_file_by_path`.
     - `bulk_insert_symbols`, `query_symbols`.
   - Initial `query_symbols` can:
     - Scan `symbols.jsonl` sequentially.
     - Filter in memory.

3. **`symgrep index` command**
   - Add:
     - `symgrep index --path . --index-backend file --index-path .symgrep`.
   - Logic:
     - Walk files under `path`.
     - For each file:
       - Compare `mtime`/`size`/hash against stored `FileRecord`.
       - If new or changed:
         - Parse with language backend.
         - Recompute symbols and update `symbols.jsonl`.
     - Remove stale entries for deleted files.

4. **Engine integration**
   - When `--use-index` is enabled:
     - Use `FileIndexBackend::query_symbols` as a prefilter.
     - Only parse files that:
       - Are new.
       - Have stale index entries.
     - Ensure results are semantically identical to non-indexed search.

5. **Tests**
   - Unit tests:
     - File backend read/write.
     - Round-trip `FileRecord` / `SymbolRecord`.
   - Integration tests:
     - Build index on a fixture repo.
     - Compare `--use-index` vs non-indexed searches.
   - Run full test suite to confirm no behavior changes.

**Deliverable:**  
Interim `v0.6.0-file` – file-based indexing backend.

---

### Phase 6B – SQLite Backend

**Goals:**

- Provide a more scalable index with indexed queries.
- Improve performance on very large repos.

**Tasks:**

1. **SQLite schema**
   - Implement tables:
     - `meta(key TEXT PRIMARY KEY, value TEXT NOT NULL)`
     - `files(id INTEGER PRIMARY KEY, path TEXT UNIQUE, language, hash, mtime, size)`
     - `symbols(id INTEGER PRIMARY KEY, file_id, name, kind, language, start_line, start_col, end_line, end_col, signature, extra)`
     - `refs(...)` (later)
   - Add indexes on:
     - `symbols.name`
     - `symbols.kind`
     - `symbols.language`
     - `symbols.file_id`

2. **`SqliteIndexBackend`**
   - Implement:
     - `open`, `initialize`, `load_meta`, `save_meta`.
     - `upsert_file`, `list_files`, `get_file_by_path`.
     - `bulk_insert_symbols` using batched transactions.
     - `query_symbols` by translating `SymbolQuery` into SQL.

3. **Extend `symgrep index`**
   - Support:
     - `--index-backend sqlite --index-path .symgrep/index.sqlite`.
   - Reuse the same change-detection logic as the file backend.

4. **Engine integration**
   - Allow configuration:
     - If `--index-backend` is given, use it explicitly.
     - If omitted:
       - Prefer SQLite if index exists.
       - Else fall back to file backend if present.
   - Maintain semantic parity with non-indexed search.

5. **Concurrency & locking**
   - Use:
     - Read-only transactions for search.
     - Write transactions for indexing.
   - Document behavior when index is being updated while being read.

6. **Tests**
   - Unit tests:
     - CRUD and query operations for SQLite backend.
   - Integration tests:
     - Indexed searches on large-ish fixtures.
     - Comparison with file backend and non-indexed runs.
   - Run full suite and benchmark performance.

**Deliverable:**  
`v0.6.0` – SQLite-backed index with configurable backends (file/SQLite).
