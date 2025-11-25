# Symbol Attributes (`comment`, `keywords`, `description`)

This document explains how per-symbol attributes work in `symgrep`:

- How attributes are shaped in JSON results.
- How they are stored and preserved in indexes.
- How to query and update them via the CLI and HTTP API.

The goal is to let external systems (ownership services, code review
bots, etc.) annotate symbols with semantic metadata (tags, behavior
descriptions) without touching source files, and to make that metadata
first-class in the query DSL.

---

## 1. Data Model

### 1.1 `SymbolAttributes`

The core JSON shape is defined in `src/models/mod.rs` and surfaced as
an optional `attributes` field on `Symbol`:

```json
{
  "name": "add",
  "kind": "function",
  "language": "typescript",
  "file": "src/math.ts",
  "range": { "start_line": 3, "start_column": 1, "end_line": 5, "end_column": 2 },
  "signature": "export function add(a: number, b: number): number",
  "attributes": {
    "comment": "Adds two numbers.",
    "comment_range": { "start_line": 1, "start_column": 1, "end_line": 3, "end_column": 2 },
    "keywords": ["math", "example"],
    "description": "Simple example function used in tests."
  }
}
```

Fields:

- `comment` (`string`, optional)  
  Leading doc comment or comment block attached to the symbol,
  extracted from source code by the language backend.

- `comment_range` (`TextRange`, optional)  
  Half-open source range covering the original leading comment block
  for the symbol. This preserves delimiters, indentation, and blank
  lines so that clients (and the CLI) can reconstruct the exact
  comment layout when needed. When absent, clients should fall back
  to using only `comment`.

- `keywords` (`array<string>`, optional, default `[]`)  
  Short tags/keywords owned by an external tool or service.
  Examples: `["auth", "jwt", "rate-limit"]`.

- `description` (`string`, optional)  
  Longer free-form description (1–3 sentences) managed by an external
  owner. Examples:
  `"Performs user authentication and issues JWTs"` or
  `"Primary entrypoint for HTTP request logging and metrics."`.

`Symbol.attributes` is optional and **additive**:

- Older payloads may omit it entirely.
- Missing fields should be treated as empty (`comment = null`,
  `keywords = []`, `description = null`).

The public JSON schema for search results is versioned via
`SEARCH_RESULT_VERSION` and is currently `0.3.0`.

---

## 2. Index Storage & Identity

### 2.1 Where attributes are stored

Attributes are persisted in the index as part of each `SymbolRecord`
via the `extra` field (`serde_json::Value`):

```json
{
  "id": 42,
  "file_id": 7,
  "name": "loginUser",
  "kind": "function",
  "language": "typescript",
  "range": { "start_line": 42, "start_column": 1, "end_line": 60, "end_column": 1 },
  "signature": "export async function loginUser(...)",
  "extra": {
    "comment": "Handles user authentication and JWT issuance.",
    "keywords": ["auth", "login", "jwt"],
    "description": "Performs user authentication and issues JWTs."
  }
}
```

Backends:

- **File backend** (`.symgrep/` directory):
  - `symbols.jsonl` stores one JSON object per `SymbolRecord`, including
    the `extra` payload.
  - See `src/index/file.rs`.

- **SQLite backend** (`index.sqlite`):
  - `symbols.extra` is a JSON-encoded string representing the same
    logical payload.
  - See `src/index/sqlite.rs`.

When serving search results from an index (`--use-index` or daemon),
`Symbol.attributes` is **hydrated from `extra`** and then used for
query evaluation and JSON output.

### 2.2 Identity key & merge semantics

On each `symgrep index` run, symbols for a file are rebuilt from
scratch and written back to the index. To avoid losing external
annotations (keywords/description) on reindex, `symgrep` uses a stable
identity key to match new symbols to existing records:

```text
SymbolIdentity = (
  kind,
  name,
  start_line,
  end_line,
  signature
)
```

Where:

- `kind` – `function`, `method`, `class`, `interface`, `variable`,
  `namespace`.
- `name` – simple symbol name.
- `start_line`, `end_line` – inclusive 1-based line range of the
  symbol (`TextRange`).
- `signature` – optional human-readable signature when available.

**Merge behavior per symbol**:

- `comment` – always taken from the **fresh AST extraction** for the
  new symbol (reflects current source code).
- `keywords` – carried forward from the existing record’s `extra` when
  a matching identity key exists, otherwise default `[]`.
- `description` – carried forward from the existing record’s `extra`
  when a matching identity key exists, otherwise `null`.

This happens independently for each file in `build_index` in
`src/index/mod.rs`.

#### 2.2.1 When attributes are preserved

Attributes are preserved across reindex runs when:

- The symbol still exists in the file.
- `kind`, `name`, `start_line`, `end_line`, and `signature` are the
  same as before (or at least map to the same identity key).

Examples:

- Adding or changing code **inside** a function body → attributes
  preserved.
- Small edits that move a function up/down but keep `start_line` /
  `end_line` stable (e.g. edits inside the body only) → preserved.

#### 2.2.2 When attributes may be lost

Attributes are intentionally **best-effort** and may be lost when:

- A symbol is **renamed** (e.g. `loginUser` → `authenticateUser`).
- A symbol’s range changes significantly (e.g. large refactors, code
  moved to a different file or split across functions).
- `signature` changes materially (e.g. parameter list or return type
  changes in languages that populate `signature`).

In those cases, the new symbol will be treated as a different identity
and will not inherit `keywords`/`description`. External systems should
be prepared to reapply annotations as part of their own workflows after
major refactors.

---

## 3. Query DSL & Search Behavior

### 3.1 New fields

The query DSL (see `src/search/query.rs`) introduces three new fields:

- `comment:` – matches the leading doc comment extracted for a symbol.
- `keyword:` – matches per-symbol keywords/tags.
- `desc:` / `description:` – matches the free-form description.

Existing fields (`content:`, `name:`, `kind:`, `language:`, `file:`)
remain unchanged.

### 3.2 Matching rules

Given a `Symbol` with attributes:

```json
{
  "attributes": {
    "comment": "Handles user authentication and JWT issuance.",
    "keywords": ["auth", "login", "jwt"],
    "description": "Performs user authentication and issues JWTs for API clients."
  }
}
```

The DSL behaves as follows:

- `comment:auth`  
  Matches if `"auth"` appears as a substring of the `comment`.
  - `comment:=Handles user authentication and JWT issuance.` –
    exact match of the whole comment string.

- `keyword:auth`  
  Matches if `"auth"` is **exactly equal** to one of the keywords in
  the list (list membership).
  - `keyword:=auth` – also exact list membership.
  - `keyword:~jwt` – substring match within keywords (matches
    `"jwt-token"`).

- `desc:authentication` or `description:authentication`  
  Matches if `"authentication"` appears as a substring of the
  description.
  - `desc:=Performs user authentication and issues JWTs for API clients.` –
    exact description match.

### 3.3 `content:` search surface

When evaluating `content:` terms in symbol mode, the engine builds a
composite search surface per symbol:

- `symbol.name`
- `symbol.signature` (when present)
- `attributes.comment`
- `attributes.keywords.join(" ")`
- `attributes.description`
- Context snippet text (decl/def/parent), when available

Thus:

- `content:authentication kind:function` will match functions whose name,
  signature, comment, description, or decl/def snippet includes the
  word `"authentication"`.
- For more precise targeting, prefer `comment:`, `keyword:`, or
  `desc:`/`description:`.

`--literal` continues to control **name** semantics (exact vs
substring) and does not change how attribute fields are matched beyond
`=`-prefixed exact matches described above.

---

## 4. Update API (HTTP & CLI)

### 4.1 HTTP: `POST /v1/symbol/attributes`

The daemon exposes an endpoint for updating attributes in-place in an
existing index (no code edits):

- Path: `POST /v1/symbol/attributes`
- Request: `SymbolAttributesRequest`
- Response: `SymbolAttributesResponse`

Example request:

```json
{
  "index": {
    "paths": ["."],
    "globs": [],
    "exclude_globs": [],
    "backend": "file",
    "index_path": ".symgrep",
    "language": "typescript"
  },
  "selector": {
    "file": "src/auth/login.ts",
    "language": "typescript",
    "kind": "function",
    "name": "loginUser",
    "start_line": 42,
    "end_line": 60
  },
  "attributes": {
    "keywords": ["auth", "login", "jwt"],
    "description": "Performs user authentication and issues JWTs"
  }
}
```

Semantics:

- The `index` block selects the on-disk index to open (file or
  SQLite).
- The `selector` identifies exactly one symbol by file path, language,
  kind, name, and line range.
  - 0 matches → `400 Bad Request` with a clear error.
  - >1 matches → `400 Bad Request` (ambiguous selector).
- The `attributes` payload **replaces**:
  - `keywords` – full list is replaced.
  - `description` – replaced (may be set to `null`).
- The `comment` field is **not** changed by this API; it is always
  derived from the source code at index time.

Response (abridged):

```json
{
  "symbol": {
    "name": "loginUser",
    "kind": "function",
    "language": "typescript",
    "file": "src/auth/login.ts",
    "range": { "start_line": 42, "start_column": 1, "end_line": 60, "end_column": 1 },
    "attributes": {
      "comment": "Handles user authentication and JWT issuance.",
      "keywords": ["auth", "login", "jwt"],
      "description": "Performs user authentication and issues JWTs"
    }
  }
}
```

### 4.2 CLI: `symgrep annotate`

The CLI wrapper mirrors the HTTP API and is implemented in
`src/cli/args.rs` / `src/cli/mod.rs` as the `annotate` subcommand.

Example:

```bash
symgrep annotate \
  --file src/auth/login.ts \
  --language typescript \
  --kind function \
  --name loginUser \
  --start-line 42 \
  --end-line 60 \
  --keywords auth,login,jwt \
  --description "Performs user authentication and issues JWTs" \
  --index-backend sqlite \
  --index-path .symgrep/index.sqlite
```

Behavior:

- Builds a `SymbolAttributesRequest` from CLI flags and either:
  - Sends it to `POST /v1/symbol/attributes` when `--server` /
    `SYMGREP_SERVER_URL` is set (and `--no-server` is not).
  - Applies it directly against the index using
    `crate::index::update_symbol_attributes` in local mode.
- Prints the `SymbolAttributesResponse` JSON to stdout.

Flags:

- `--file`, `--language`, `--kind`, `--name`, `--start-line`,
  `--end-line` – define the `SymbolSelector`.
- `--keywords` – comma-separated list or repeated flag; builds
  `attributes.keywords`.
- `--description` / `--description-file` – mutually exclusive; one is
  used to populate `attributes.description`.
- `--index-backend` / `--index-path` – select the target index.
- `--server` / `--no-server` – choose daemon vs local index update.

---

## 5. Recommended Practices & Caveats

- Treat attributes as **external annotations**, not as source of truth:
  - Source code (comments, function bodies) remains canonical.
  - Attributes are best-effort and may be lost under major refactors.

- When designing external systems that write attributes:
  - Prefer **idempotent** workflows (e.g. periodically reapply tags
    based on your own source of truth).
  - Use stable paths and be cautious about aggressive renames that
    change symbol identity en masse.

- Use the DSL fields for clarity:
  - `comment:` for doc comments / leader lines.
  - `keyword:` for coarse tagging (`owner:payments`, `tier:critical`).
  - `desc:` / `description:` for more natural-language queries.

- For large-scale changes to identity semantics (e.g. adding a
  signature hash component or tolerance windows around line numbers),
  consider bumping the index schema version and/or performing an
  index rebuild with a clear migration path for external systems.

This feature is intentionally conservative and additive. It should be
safe to use in production workflows, with clear and predictable
behavior across reindex runs and CLI/HTTP entry points.  
