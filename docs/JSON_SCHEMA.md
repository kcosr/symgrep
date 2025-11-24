# `symgrep` Search Result JSON Schema

This document describes the JSON structure produced by
`symgrep search ... --format=json`. The schema is treated as a
stable API for tools and LLM agents.

The authoritative Rust types live in `src/models/mod.rs`:

- `SearchResult`
- `SearchMatch`
- `Symbol`
- `ContextInfo`
- `ContextNode`
- `SearchSummary`

and are versioned via the `SEARCH_RESULT_VERSION` constant.

---

## 1. Schema Versioning

Each `SearchResult` payload includes a top-level `version` field:

- Type: string
- Current value: `"0.1.0"`
- Source: `SEARCH_RESULT_VERSION` in `src/models/mod.rs`

Versioning follows semantic versioning:

- **MAJOR** – Backward-incompatible changes to required fields or
  field semantics.
- **MINOR** – Backward-compatible additions (new optional fields).
- **PATCH** – Documentation or internal representation changes only.

Compatibility rules:

- Consumers **must** check `version` and handle newer minor versions
  defensively, treating unknown fields as optional.
- Backward-incompatible changes (e.g., removing/renaming fields or
  changing types) require a MAJOR bump and a corresponding update
  to this document.
- Additive fields are introduced with `#[serde(default)]` and/or
  `skip_serializing_if` to keep older consumers working.

---

## 2. Top-Level `SearchResult` Object

Top-level JSON object:

```json
{
  "version": "0.1.0",
  "query": "foo",
  "matches": [ /* SearchMatch */ ],
  "symbols": [ /* Symbol */ ],
  "contexts": [ /* ContextInfo */ ],
  "summary": { /* SearchSummary */ }
}
```

Fields:

- `version` (`string`, required)  
  Schema version for this payload (see above).

- `query` (`string`, required)  
  The raw pattern or query string provided by the user (e.g. `"foo"`,
  `"name:add kind:function"`). This is preserved as typed.

- `matches` (`array<SearchMatch>`, required, default `[]`)  
  Concrete text matches for text-mode searches. Symbol-mode searches
  typically leave this empty.

- `symbols` (`array<Symbol>`, required, default `[]`)  
  Language-aware symbols returned for symbol/DSL-based searches.
  Text-only searches typically leave this empty.

- `contexts` (`array<ContextInfo>`, required, default `[]`)  
  Context snippets associated with symbols (or, in later phases,
  text matches). Each context may carry a `parent_chain` describing
  enclosing scopes.

- `summary` (`SearchSummary`, required)  
  Aggregate statistics about the search run.

---

## 3. `SearchMatch` – Text Matches

Represents a single text match:

```json
{
  "path": "tests/fixtures/text_repo/a.txt",
  "line": 1,
  "column": 1,
  "snippet": "foo"
}
```

Fields:

- `path` (`string`, required)  
  File path containing the match (relative to the search root).

- `line` (`integer`, required)  
  1-based line number of the match.

- `column` (`integer`, optional)  
  1-based byte offset within the line. May be `null` when not
  computed.

- `snippet` (`string`, optional)  
  A short snippet corresponding to the match. For Phase 1/2 this is
  usually the full matching line. May be `null` when snippets are
  disabled (e.g. `--max-lines 0`).

---

## 4. `Symbol` – Language-Level Symbols

Represents a symbol such as a function, method, class, or variable:

```json
{
  "name": "add",
  "kind": "function",
  "language": "typescript",
  "file": "tests/fixtures/ts_js_repo/simple.ts",
  "range": {
    "start_line": 3,
    "start_column": 1,
    "end_line": 5,
    "end_column": 2
  },
  "signature": "export function add(a: number, b: number): number"
}
```

Fields:

- `name` (`string`, required)  
  Simple symbol name (e.g. function or class name).

- `kind` (`string`, required)  
  Symbol kind, lowercased. Current values:
  - `"function"`
  - `"method"`
  - `"class"`
  - `"interface"`
  - `"variable"`
  - `"namespace"`

- `language` (`string`, required)  
  Stable language identifier (e.g. `"typescript"`, `"javascript"`,
  `"cpp"`).

- `file` (`string`, required)  
  File path where the symbol is defined.

- `range` (`TextRange`, required)  
  Half-open source range for the symbol. "Half-open" means:
  - `start_line`, `start_column` (1-based, inclusive)
  - `end_line`, `end_column` (1-based, exclusive)

- `signature` (`string`, optional)  
  Human-readable signature, when available. May be omitted or `null`.

---

## 5. `ContextInfo` – Context Snippets

Represents a concrete snippet corresponding to a symbol’s declaration,
definition, or parent context:

```json
{
  "kind": "parent",
  "file": "tests/fixtures/mixed_repo/sample.cpp",
  "range": { "start_line": 1, "start_column": 1, "end_line": 40, "end_column": 1 },
  "snippet": "struct Widget { ... }",
  "symbol_index": 0,
  "parent_chain": [
    { "name": "sample.cpp", "kind": null },
    { "name": "util", "kind": "namespace" },
    { "name": "Widget", "kind": "class" }
  ]
}
```

Fields:

- `kind` (`string`, required)  
  Context kind, lowercased:
  - `"decl"` – declaration/signature-only context.
  - `"def"` – full definition/body.
  - `"parent"` – enclosing context (e.g. file, namespace, class).

- `file` (`string`, required)  
  File containing the snippet.

- `range` (`TextRange`, required)  
  Half-open range for the snippet in the file.

- `snippet` (`string`, required)  
  Multi-line snippet contents.

- `symbol_index` (`integer`, optional)  
  Index into `SearchResult.symbols` referring to the associated
  symbol. Omitted or `null` when the context is not tied to a
  specific symbol.

- `parent_chain` (`array<ContextNode>`, optional, default `[]`)  
  Ordered chain of enclosing contexts, from outermost to innermost.
  This field is **additive** and may be missing in older payloads or
  when context information is not available.

### 5.1 `ContextNode`

Each entry in `parent_chain`:

```json
{
  "name": "Widget",
  "kind": "class"
}
```

Fields:

- `name` (`string`, required)  
  Name of the enclosing context (file/module/namespace/class/etc.).

- `kind` (`string`, optional)  
  Optional symbol-like kind for the context. Uses the same lowercased
  string values as `Symbol.kind` when present (e.g. `"function"`,
  `"class"`, `"namespace"`); omitted (`null`) for file-level or
  non-symbol contexts.

---

## 6. `SearchSummary` – Aggregate Statistics

Summary object:

```json
{
  "total_matches": 2,
  "truncated": false
}
```

Fields:

- `total_matches` (`integer`, required)  
  Total number of matches discovered. When a limit is applied,
  this is the number of matches up to the cut-off.

- `truncated` (`bool`, required)  
  `true` when results were truncated due to `--limit` or other caps;
  `false` otherwise.

---

## 7. `IndexSummary` – Index Metadata

`IndexSummary` is returned by indexing operations (`symgrep index` and
`POST /v1/index`) and by index introspection (`symgrep index-info`,
`POST /v1/index/info`).

Canonical JSON representation:

```json
{
  "backend": "file",
  "index_path": ".symgrep",
  "files_indexed": 3,
  "symbols_indexed": 42,
  "root_path": "/abs/path/to/project",
  "schema_version": "1",
  "tool_version": "0.1.0",
  "created_at": "2025-11-23T07:30:00Z",
  "updated_at": "2025-11-23T09:05:00Z"
}
```

Fields:

- `backend` (`string`, required)  
  Index backend kind, lowercased:
  - `"file"`
  - `"sqlite"`

- `index_path` (`string`, required)  
  On-disk location of the index:
  - For the file backend, a directory (e.g. `.symgrep`).
  - For the SQLite backend, a database file path
    (e.g. `.symgrep/index.sqlite`).

- `files_indexed` (`integer`, required)  
  Number of files currently recorded in the index.

- `symbols_indexed` (`integer`, required)  
  Number of symbols currently recorded in the index.

- `root_path` (`string`, optional)  
  Canonical project root for this index, stored as an absolute path.
  Omitted or `null` for indexes created before this field existed.

- `schema_version` (`string`, optional)  
  Logical index schema version (e.g. `"1"`). May be omitted for
  older indexes.

- `tool_version` (`string`, optional)  
  Version of the `symgrep` tool that last wrote index metadata.

- `created_at` (`string`, optional)  
  ISO-8601 timestamp (UTC) for when the index was first created.

- `updated_at` (`string`, optional)  
  ISO-8601 timestamp (UTC) for the last successful index update.

All new fields are additive and optional; older payloads may only
contain `backend`, `index_path`, `files_indexed`, and
`symbols_indexed`.

---

## 8. Stability & Future Extensions

- The structures above describe the JSON produced in Phase 4–5 with
  version `"0.1.0"`.
- Future phases may:
  - Add optional fields (e.g. richer symbol metadata, index
    information, or additional context views).
  - Introduce new top-level objects (e.g. for `symgrep index`).
- Existing fields will not be removed or renamed without:
  - A MAJOR version bump of `SEARCH_RESULT_VERSION`.
  - A corresponding update to this document.

Consumers relying on this schema should:

- Treat unknown fields as optional.
- Guard on `version` for significant behavior changes. When
  encountering a newer **MAJOR** version than expected, consumers
  should treat the payload as potentially incompatible (e.g. fail
  fast or require an explicit opt-in) rather than assuming
  best-effort parsing will succeed.
- Use `parent_chain` and `contexts` when present, but remain robust
  when they are absent.
