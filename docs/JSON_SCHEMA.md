# `symgrep` Search Result JSON Schema

This document describes the JSON structures produced by:

- `symgrep search ... --format=json` (search results).
- `symgrep follow ... --format=json` (callers/callees exploration).

These schemas are treated as stable APIs for tools and LLM agents.

The authoritative Rust types live in `src/models/mod.rs`:

- `SearchResult` / `SearchMatch` / `SearchSummary`
- `Symbol` / `ContextInfo` / `ContextNode`
- `CallRef`
- `FollowResult` / `FollowTarget` / `FollowEdge`
- `FollowSymbolRef` / `FollowCallSite`

`SearchResult` payloads are versioned via the `SEARCH_RESULT_VERSION`
constant; `FollowResult` payloads are versioned via the
`FOLLOW_RESULT_VERSION` constant.

---

## 1. Schema Versioning

Each `SearchResult` or `FollowResult` payload includes a top-level
`version` field:

- Type: string
- Current `SearchResult` value: `"1.2.0"`
- Current `FollowResult` value: `"1.0.0"`
- Sources: `SEARCH_RESULT_VERSION` / `FOLLOW_RESULT_VERSION` in `src/models/mod.rs`

Versioning follows semantic versioning for each schema independently:

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
  "version": "1.2.0",
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
  "signature": "export function add(a: number, b: number): number",
  "attributes": {
    "comment": "Adds two numbers.",
    "comment_range": {
      "start_line": 1,
      "start_column": 1,
      "end_line": 3,
      "end_column": 2
    },
    "keywords": ["math", "example"],
    "description": "Simple example function used in tests."
  },
  "def_line_count": 5,
  "matches": [
    {
      "line": 4,
      "column": 3,
      "snippet": "return a + b;"
    }
  ]
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

- `attributes` (`object`, optional)  
  Optional per-symbol attributes, used for richer search and
  annotation workflows. When present:
  - `comment` (`string`, optional) – leading doc comment or comment
    block extracted from source code.
  - `comment_range` (`TextRange`, optional) – source range covering
    the original leading comment block, suitable for reconstructing
    original formatting in text-based views.
  - `keywords` (`array<string>`, optional, default `[]`) – external
    tags/keywords owned by an external tool or service.
  - `description` (`string`, optional) – longer free-form description
  managed by an external owner.
  This field is additive and may be missing in older payloads; when
  omitted, clients should treat attributes as empty.

- `def_line_count` (`integer`, optional)  
  Number of lines in the symbol’s definition/body snippet when a
  `Def` context has been materialized (for example, when `--view def`
  or `--view def,matches` is requested). This value is derived from
  the `ContextInfo.range` for the `def` context:
  `end_line - start_line + 1`. When no definition context is
  constructed (e.g. `--view decl` or metadata-only queries), this
  field is omitted. Because it is computed from the full Def context
  range, `def_line_count` is unaffected by CLI/daemon `max_lines`
  truncation used for text/table output.

- `matches` (`array<SymbolMatch>`, optional, default `[]`)  
  Optional per-symbol matches used in symbol-mode views when the
  `matches` view is requested via `SearchConfig.symbol_views`. Each
  entry describes a single matching line within the chosen context or
  attributes. When the `meta` view is requested without `matches`,
  this array stays empty and no match lines are computed. This field
  is additive and may be missing in older payloads or when
  match-line views are not used.

- `calls` (`array<CallRef>`, optional, default `[]`)  
  Outgoing call edges from this symbol to other functions/methods,
  expressed as best-effort name-based references. Each entry describes
  a single call site within the symbol body. This field is additive
  and may be missing or empty when call information is not available.

- `called_by` (`array<CallRef>`, optional, default `[]`)  
  Incoming call edges describing which symbols call this symbol. Each
  entry describes a single caller symbol and call site. This field is
  additive and may be missing or empty when call information is not
  available.

### 4.1 `SymbolMatch`

Represents a single match location within a symbol-oriented view:

```json
{
  "line": 57,
  "column": 9,
  "snippet": "if (foo && bar) {"
}
```

Fields:

- `line` (`integer`, required)  
  1-based line number of the match within the source file.

- `column` (`integer`, optional)  
  1-based column number of the match within the line. May be `null`
  when not computed.

- `snippet` (`string`, required)  
  Snippet text around the match, typically the full line.

The matches array is populated only for symbol-mode searches when the
`matches` view is requested. When both context snippets and matches
are present, `Symbol.matches[*].line` / `column` refer to locations
within the same file and region as the primary context.

### 4.2 `CallRef`

Represents a single caller or callee in a direct call edge:

```json
{
  "name": "bar",
  "file": "tests/fixtures/call_graph_repo/ts_calls.ts",
  "line": 5,
  "kind": "function"
}
```

Fields:

- `name` (`string`, required)  
  Name of the caller or callee symbol (e.g. `"foo"`, `"bar"`).

- `file` (`string`, required)  
  File containing the call site. For both `calls` and `called_by`
  edges this is the file where the call expression appears.

- `line` (`integer`, optional)  
  1-based line number of the call site or symbol. May be omitted or
  `null` when not available.

- `kind` (`string`, optional)  
  Optional symbol kind for the caller/callee, using the same
  lowercased values as `Symbol.kind` (`"function"`, `"method"`,
  `"class"`, `"interface"`, `"variable"`, `"namespace"`). May be
  omitted when the kind is unknown.

Semantics and limitations:

- Call edges are **name-based and best-effort**:
  - No type or overload resolution is performed.
  - Edges are constructed by matching the callee name at the call
    head against symbol names visible to the language backend.
- Edges are currently computed **per file**:
  - `calls[*].file` / `called_by[*].file` always point to the call
    site’s file.
  - Cross-file and cross-language edges are not included in this
    version.
- Language backends apply conservative heuristics:
  - TypeScript / JavaScript handle plain identifiers and common member
    expressions (`foo(...)`, `obj.foo(...)`).
  - C++ currently focuses on simple identifier calls (e.g. `foo()` in
    `foo();`); more complex patterns such as `obj.method()`,
    `ns::func()`, or template instantiations are not yet described in
    `CallRef` and may be added in future minor versions.

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
  Multi-line snippet contents. In symbol/auto modes this always
  contains the full region selected by the engine (decl/def/parent);
  CLI/daemon `max_lines` settings do **not** truncate this field.

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
  "schema_version": "2",
  "tool_version": "0.3.0",
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

## 8. `FollowResult` – Callers/Callees Exploration

`FollowResult` is returned by the `symgrep follow` subcommand when
`--format=json` is used. It describes target symbols plus their
direct callers and/or callees, grouped by symbol.

Top-level object:

```json
{
  "version": "1.0.0",
  "direction": "callers",
  "query": "name:foo kind:function",
  "targets": [ /* FollowTarget */ ]
}
```

Fields:

- `version` (`string`, required)  
  Schema version for this payload (see Versioning above).

- `direction` (`string`, required)  
  Direction requested by the caller, lowercased:
  - `"callers"`
  - `"callees"`
  - `"both"`

- `query` (`string`, required)  
  Raw follow pattern as provided to the CLI (e.g.
  `"name:foo kind:function"`). This is the same DSL as search.

- `targets` (`array<FollowTarget>`, required, default `[]`)  
  Target symbols selected by the follow pattern and their immediate
  call relationships.

### 8.1 `FollowTarget`

Per-target structure:

```json
{
  "symbol": { /* Symbol */ },
  "callers": [ /* FollowEdge */ ],
  "callees": [ /* FollowEdge */ ]
}
```

Fields:

- `symbol` (`Symbol`, required)  
  The symbol that matched the follow pattern. This reuses the same
  `Symbol` shape described earlier in this document.

- `callers` (`array<FollowEdge>`, optional, default `[]`)  
  Direct callers of `symbol`, grouped by caller symbol. Present when
  `direction` is `"callers"` or `"both"`.

- `callees` (`array<FollowEdge>`, optional, default `[]`)  
  Direct callees of `symbol`, grouped by callee symbol. Present when
  `direction` is `"callees"` or `"both"`.

### 8.2 `FollowEdge`

Represents call-site information for a single caller or callee:

```json
{
  "symbol": { /* FollowSymbolRef */ },
  "call_sites": [
    { "file": "tests/fixtures/call_graph_repo/ts_calls.ts", "line": 6 }
  ]
}
```

Fields:

- `symbol` (`FollowSymbolRef`, required)  
  Lightweight description of the caller/callee symbol.

- `call_sites` (`array<FollowCallSite>`, optional, default `[]`)  
  One or more concrete call sites that connect the target symbol to
  this caller/callee.

### 8.3 `FollowSymbolRef`

Lightweight symbol descriptor used in follow responses:

```json
{
  "name": "foo",
  "kind": "function",
  "file": "tests/fixtures/call_graph_repo/ts_calls.ts"
}
```

Fields:

- `name` (`string`, required)  
  Simple symbol name (function/method/class/etc.).

- `kind` (`string`, optional)  
  Optional symbol kind, using the same lowercased values as
  `Symbol.kind` when present (`"function"`, `"method"`, etc.).

- `file` (`string`, required)  
  File containing the symbol and its call sites.

### 8.4 `FollowCallSite`

Concrete call-site location in source code:

```json
{
  "file": "tests/fixtures/call_graph_repo/ts_calls.ts",
  "line": 6,
  "column": null
}
```

Fields:

- `file` (`string`, required)  
  File containing the call expression.

- `line` (`integer`, required)  
  1-based line number of the call expression.

- `column` (`integer`, optional)  
  1-based column number of the call expression. This field is
  currently omitted in CLI output but may be populated in future
  versions; clients should treat it as optional.

  `column` values are computed via best-effort substring matching in
  CLI text output and may be approximate when the symbol name appears
  multiple times on the same line.

Semantics:

- Follow responses are **call-site centric**: each `FollowCallSite`
  pinpoints a concrete call location based on the underlying call
  metadata (`Symbol.calls` / `Symbol.called_by`).
- CLI `--context` / `--max-lines` flags only affect human-readable
  text output; `FollowResult` JSON is never truncated by these flags.

---

## 9. Stability & Future Extensions

- The structures above describe the JSON produced starting with
  `SearchResult.version = "1.2.0"` and `FollowResult.version = "1.0.0"`.
- Future phases may:
  - Add optional fields (e.g. richer symbol metadata, index
    information, additional follow details, or extra context views).
  - Introduce new top-level objects (e.g. for additional commands).
- Existing fields will not be removed or renamed without:
  - A MAJOR version bump of the corresponding schema version
    constant.
  - A corresponding update to this document.

Consumers relying on these schemas should:

- Treat unknown fields as optional.
- Guard on `version` for significant behavior changes. When
  encountering a newer **MAJOR** version than expected, consumers
  should treat the payload as potentially incompatible (e.g. fail
  fast or require an explicit opt-in) rather than assuming
  best-effort parsing will succeed.
- Use `parent_chain`, `contexts`, and follow-specific fields when
  present, but remain robust when they are absent.
