# `symgrep` Agent Integration Guide

This guide is for LLM agents and humans who want to drive `symgrep`
programmatically. It focuses on the JSON interface and small,
deterministic examples that map directly to the test fixtures under
`tests/fixtures/`.

All examples assume:

- You are in the `symgrep` repo root.
- `symgrep` is available on `PATH` (e.g. via `cargo run --` or a built binary).

For full JSON field details see `docs/JSON_SCHEMA.md`.

---

## 1. Agent Quickstart

`symgrep` is a code-aware search tool for agents:

- Text search over files (fast, grep-like).
- Symbol/AST search over TS/JS/C++/Rust with contexts (decl/def/parent).
- JSON output designed for tools/LLMs.

Prefer `symgrep` over ad-hoc `cat`/`grep`/large file reads when you need to:

- Find where a concept or config is used.
- See the function(s) that own a behavior.
- Get just enough context (signatures, bodies, parent scopes) without dragging whole files into the prompt.

### 1.1 Drill-down recipe

When exploring an unfamiliar repo, use this pattern:

- Start broad with bounded text search:
  - `symgrep search 'word1|word2' --path . --format text`
  - Use `|` to OR related terms in one call.
- Zoom to the symbol that owns the behavior:
  - `symgrep search 'name:Foo kind:function|method' --path . --mode symbol --view def --format text`
- Filter functions by what appears in their body:
  - `symgrep search 'kind:function content:\"some phrase\"' --path . --mode symbol --view def,matches --format text`
- Use JSON when you need structure or precise locations:
  - Same patterns with `--format json` and inspect `symbols[*]` / `contexts[*].snippet`.

## 2. CLI Basics for Agents

### 2.1 Core search command

Text search over a small fixture repo:

```bash
symgrep search "foo" \
  --path tests/fixtures/text_repo \
  --format json
```

Key flags:

- `--format json` – always use this for agents; output matches `SearchResult` in `docs/JSON_SCHEMA.md`.
- `--mode text|symbol|auto` – text matches vs symbol/AST search.
- `--view meta|decl|def|parent|comment|matches` – symbol views:
  - `meta` – symbol metadata only (no context snippets, no per-symbol matches when used alone).
  - `decl` – declaration/signature only.
  - `def` – full definition/body.
  - `parent` – enclosing scope (e.g. file, namespace, class, or function).
  - `comment` – doc comment / leading comment only.
  - `matches` – matching lines within the chosen region/comment/description.

Use `--schema-version` to discover the JSON schema version:

```bash
symgrep --schema-version
```

Agents should read the version string and handle minor version bumps
by treating unknown fields as optional.

### 2.2 Controlling result size

Recommended flags for bounding output:

- `--limit N` – stop after N matches (sets `summary.truncated = true`).
- `--max-lines N` – bound snippet height:
  - In **text mode**, controls `matches[*].snippet` in JSON (`--max-lines 0` makes `snippet` `null`; any other value keeps the single-line snippet used today).
  - In **symbol/auto modes**, acts as a CLI presentation cap: it limits the number of context/def/match lines printed per symbol in text/table output, but does **not** truncate `contexts[*].snippet` or `def_line_count` in JSON.
 - `--context N` / `-C N` – add **N lines of real source context before and after each match line** in human-readable text output:
   - In **text mode** with `--format text`, groups matches by file and prints merged context windows per file.
   - In **symbol mode** with match views like `--view def,matches` and `--format text`, expands match lines within the primary context snippet (e.g. the Def body) into merged context windows per symbol.
   - Has **no effect on `--format json`**; JSON schemas and fields remain unchanged.

Example (limited JSON payload, snapshotted in tests):

```bash
symgrep search "foo" \
  --path tests/fixtures/text_repo \
  --format json \
  --limit 1 \
  --max-lines 0
```

This returns a single match, with `snippet: null` and
`summary.truncated: true`.

Example (CLI-only match context in text output):

```bash
symgrep search "foo" \
  --path tests/fixtures/text_repo \
  --format text \
  --context 1
```

This prints, per file, the matching line(s) plus one line of surrounding
source context before and after each match, merging overlapping windows
so nearby matches do not duplicate lines. JSON output for the same query
is unaffected.

### 2.3 View semantics and combinations

Symbol views are composable:

- `--view meta` – return only `symbols[*]` (name/kind/language/file/range/attributes); `contexts` stays empty and `matches` stays empty.
- `--view decl|def|parent` – choose the primary region for context snippets; `contexts[*].kind` reflects the requested region.
- `--view def,matches` – use the definition as the primary region and populate `symbol.matches` with matching lines from that region; text output prints only those lines (not the full body).
- `--view comment,matches` – populate `symbol.matches` from comment/description attributes.
- `--view meta,matches` – behaves like a matches view: the engine computes match lines and may fetch a primary region when needed, so this is not metadata-only despite including `meta`.

When multiple region views are present, precedence is:

- `def` > `decl` > `parent`

If only `comment`/`matches` are present and the query uses `content:` terms, the engine falls back to the definition region internally so it can compute match lines. For pure metadata queries (e.g. only `name:`/`kind:`/`file:`), no snippets are needed and `contexts` can stay empty even without `meta`.

When a definition/body snippet is materialized (for example, with
`--view def` or `--view def,matches`), each symbol also exposes a
`def_line_count` field in JSON. This is the number of lines in the
`Def` context snippet (`end_line - start_line + 1`) and gives agents a
cheap size signal (e.g. “only fetch full defs under 50 lines”). When
no `Def` context is constructed (e.g. `--view decl` only), this field
is omitted. `def_line_count` is derived from the full Def context and
is not affected by CLI-only truncation via `--max-lines`.

### 2.3 Indexing and `--use-index`

For large repos, build an index once and reuse it:

```bash
# File-backed index at the default location (.symgrep/)
symgrep index --path .

# Explicit SQLite index under target/ (recommended for scripts/CI)
symgrep index \
  --path . \
  --index-backend sqlite \
  --index-path target/symgrep/index.sqlite
```

To use an existing index during symbol search:

```bash
symgrep search "add" \
  --path . \
  --language typescript \
  --mode symbol \
  --view decl \
  --format json \
  --use-index \
  --index-backend sqlite \
  --index-path target/symgrep/index.sqlite
```

Behavior notes for agents:

- If `--use-index` is omitted, `symgrep` runs directly on files.
- When `--use-index` is set without an explicit backend/path, the
  engine prefers an existing SQLite index at `.symgrep/index.sqlite`,
  then falls back to a file backend at `.symgrep/`, and finally to
  non-indexed search if no index exists.
- Indexed and non-indexed symbol searches are designed to be
  semantically equivalent; indexes only improve performance.

### 2.4 Query DSL quick reference

The `pattern` string is parsed as a small DSL that works consistently
across text and symbol searches.

Fields:

- `content:` – generic content to search for (lines in text mode, symbol surface/snippets in symbol mode).
- `name:` – symbol name (function/method/class/interface/variable/namespace).
- `kind:` – symbol kind (`function`, `method`, `class`, `interface`, `variable`, `namespace`; aliases like `func`, `struct`, `ns` also work).
- `language:` – language identifier (e.g. `typescript`, `javascript`, `cpp`, `rust`). For Rust specifically, symgrep treats free functions and associated functions in `impl` blocks that do not take `self` as `kind:function`, and inherent or trait methods that take any form of `self` as `kind:method`.
- `file:` – file path substring.
- `comment:` – leading doc comment attached to a symbol (when available).
- `keyword:` – external per-symbol keyword/tag (exact match on list elements; use `keyword:~foo` for substring matches within keywords).
- `desc:` / `description:` – longer free-form description attached via the attributes API.
 - `calls:` – callee name(s) referenced from within a symbol’s body, matched against `symbols[*].calls[*].name`.
 - `called-by:` / `callers:` – caller name(s) that invoke a symbol, matched against `symbols[*].called_by[*].name`.

Operators:

- `field:value` – substring/contains match.
- `field:=value` – exact match (when the value starts with `=`).

Composition:

- AND – whitespace between groups:
  - `name:add kind:function` → `name:add AND kind:function`.
- OR – `|` inside a group:
  - `name:add|sum` → `name:add OR name:sum`.
- Field inheritance inside a group:
  - If the first alternative has a field, later bare alternatives share it:
    - `kind:function|method` → `kind:function OR kind:method`.
    - `language:typescript|javascript` → `language:typescript OR language:javascript`.
     - `called-by:foo|bar` → `called-by:foo OR called-by:bar`.

Bare patterns:

- If the pattern contains **no `field:` at all**, it is treated as a `content:` query:
- `foo` → `content:foo`.
- `foo|bar` → `content:foo OR content:bar`.
- To search by symbol name, prefer explicit `name:`:
  - `name:add kind:function` instead of bare `add`.

`content:` interaction with views:

- When the query includes `content:` terms, the engine may need a primary region snippet even if you do not request one explicitly.
- With `--view meta` alone and `content:` terms, the engine still evaluates `content:` against symbol surface/attributes and does **not** fetch snippets or populate `contexts`/`matches`.
- With `--view matches` (with or without `meta`), `content:` terms drive the per-symbol match lines computed from the selected primary region.

Precedence (informal):

- Quotes bind first (keep spaces inside a term), then `|` forms OR-groups inside a token, then whitespace combines groups with AND.

`--literal`:

- In **text mode**, `--literal` enables whole-identifier matching for the underlying content value.
- In **symbol mode**, `--literal` controls exact vs substring matching for `name:` when you do not use `name:=value`.
- For new code and agent prompts, prefer `name:=foo` / `content:=foo` for explicit exact matches.
 - For `calls:` / `called-by:` filters, use `calls:=foo` / `called-by:=foo` for exact callee/caller names, or plain `calls:foo` / `called-by:foo` for substring matches.

### 2.5 Project config defaults

Projects can set defaults for CLI flags via `.symgrep/config.toml`:

```toml
[search]
paths = ["."]
mode = "symbol"
view = ["def", "matches"]
```

- The `[search].view` array controls default symbol views for `symgrep search`.
- Command-line `--view` always overrides the TOML default.
- When the effective mode is plain text, `view` is ignored.

### 2.6 Following callers and callees

`symgrep follow` lets agents explore direct callers/callees for
symbol targets using the call graph metadata exposed in `Symbol.calls`
and `Symbol.called_by`:

```bash
symgrep follow "name:foo kind:function" \
  --path tests/fixtures/call_graph_repo \
  --language typescript \
  --direction callers \
  --format json
```

Behavior:

- Internally, `follow` runs a symbol-mode search using the same query
  DSL as `search`, then builds a `FollowResult` (see
  `docs/JSON_SCHEMA.md`) from the call edges attached to each symbol.
- `--direction callers|callees|both` controls which relationships are
  included:
  - `callers` – who calls the target(s) (`Symbol.called_by`).
  - `callees` – what the target(s) call (`Symbol.calls`).
- `--context N` and `--max-lines N` are **text-only** options:
  - In `--format text`, follow prints per-target blocks with
    per-caller/per-callee context windows around each call site,
    merging overlapping windows and capping to `max-lines` per block.
  - In `--format json`, follow returns the full `FollowResult`
    structure without truncation; `--context` and `--max-lines` have
    no effect on JSON.
- `--limit N` caps the number of target symbols (i.e. `FollowTarget`
  entries) produced from the initial symbol search; within each
  target, all caller/callee edges are still considered.

Example (TS call graph fixture, callees of `foo`):

```bash
symgrep follow "name:foo kind:function" \
  --path tests/fixtures/call_graph_repo/ts_calls.ts \
  --language typescript \
  --direction callees \
  --format json
```

This yields a single `FollowTarget` whose `symbol.name` is `"foo"`
and whose `callees[*].symbol.name` contains `"bar"` and `"baz"`,
with `call_sites[*]` entries pointing at the concrete call lines in
`ts_calls.ts`. In text mode with `--context 1`, the same query prints
call-site-centric context blocks around those lines.

Limitations:

- Call edges are **per-file** in this phase: callers and callees are
  resolved within a single file; cross-file edges are not included.
- Call edges are **name-based and best-effort**: no type/overload
  resolution is performed, and member calls are handled heuristically.
- Column information for call sites (when shown in text output) is
  computed via simple substring search and may be approximate when
  the symbol name appears multiple times on the same line.

## 3. Shell Integration

Agents and scripts can treat `symgrep` as a pure function:
CLI arguments → JSON on stdout.

### 3.1 Basic text JSON search

```bash
symgrep search "foo" \
  --path tests/fixtures/text_repo \
  --format json
```

Example JSON shape (abridged):

```json
{
  "version": "1.1.0",
  "query": "foo",
  "matches": [
    {
      "path": "tests/fixtures/text_repo/a.txt",
      "line": 1,
      "column": 1,
      "snippet": "foo"
    }
  ],
  "symbols": [],
  "contexts": [],
  "summary": {
    "total_matches": 2,
    "truncated": false
  }
}
```

### 3.2 Symbol search with declaration context

Find `function`-like symbols named `add` across the mixed-language
fixture, keeping a small declaration snippet:

```bash
symgrep search "name:add kind:function" \
  --path tests/fixtures/mixed_repo \
  --mode symbol \
  --view decl \
  --format json
```

Shape of the JSON (abridged, one entry per language):

```json
{
  "symbols": [
    {
      "name": "add",
      "kind": "function",
      "language": "typescript",
      "file": "tests/fixtures/mixed_repo/simple.ts",
      "range": { "start_line": 1, "start_column": 1, "...": "..." }
    }
  ],
  "contexts": [
    {
      "kind": "decl",
      "file": "tests/fixtures/mixed_repo/simple.ts",
      "snippet": "export function add(a: number, b: number): number { ... }",
      "symbol_index": 0
    }
  ]
}
```

Agents can:

- Use `symbols[*].file` / `range` to locate precise positions.
- Use `contexts[*].snippet` when `context != none` to reason about
  implementations without re-reading files.

### 2.3 Parent context for methods/classes

Get the enclosing struct/class for the C++ method `increment`:

```bash
symgrep search "name:increment kind:method" \
  --path tests/fixtures/cpp_repo \
  --language cpp \
  --mode symbol \
  --view parent \
  --format json
```

Key fields to inspect:

- `contexts[0].kind = "parent"` – indicates we asked for enclosing context.
- `contexts[0].snippet` – contains the full `struct Widget { ... }` body.
- `contexts[0].parent_chain` – ordered chain of enclosing scopes, e.g.:

  ```json
  [
    { "name": "sample.cpp", "kind": null },
    { "name": "util", "kind": "namespace" },
    { "name": "Widget", "kind": "class" }
  ]
  ```

This pattern is useful when an agent needs to understand all methods
on a class or the surrounding namespace.

---

## 4. Node / TypeScript Integration

From Node/TypeScript you can spawn the CLI and parse JSON.

### 4.1 Minimal wrapper-style helper

```ts
import { spawn } from "node:child_process";

export interface SymgrepSearchConfig {
  pattern: string;
  paths?: string[];
  mode?: "text" | "symbol" | "auto";
  view?: ("meta" | "decl" | "def" | "parent" | "comment" | "matches")[];
  language?: string;
  literal?: boolean;
  limit?: number;
  maxLines?: number;
  useIndex?: boolean;
  indexBackend?: "file" | "sqlite";
  indexPath?: string;
  reindexOnSearch?: boolean;
}

export interface SearchResult {
  version: string;
  query: string;
  matches: unknown[];
  symbols: unknown[];
  contexts: unknown[];
  summary: { total_matches: number; truncated: boolean };
}

export function runSymgrepSearch(config: SymgrepSearchConfig): Promise<SearchResult> {
  const args = ["search", config.pattern];
  const paths = config.paths && config.paths.length ? config.paths : ["."];
  for (const p of paths) args.push("--path", p);

  if (config.mode) args.push("--mode", config.mode);
  if (config.view && config.view.length) {
    args.push("--view", config.view.join(","));
  }
  if (config.language) args.push("--language", config.language);
  if (config.literal) args.push("--literal");
  if (config.limit != null) args.push("--limit", String(config.limit));
  if (config.maxLines != null) args.push("--max-lines", String(config.maxLines));

  if (config.useIndex) {
    args.push("--use-index");
    if (config.indexBackend) args.push("--index-backend", config.indexBackend);
    if (config.indexPath) args.push("--index-path", config.indexPath);
  }

  if (config.reindexOnSearch) {
    args.push("--reindex-on-search");
  }

  args.push("--format", "json");

  return new Promise((resolve, reject) => {
    const child = spawn("symgrep", args, { stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";

    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk) => (stdout += chunk));
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (chunk) => (stderr += chunk));

    child.on("error", (err) => reject(err));
    child.on("close", (code) => {
      if (code !== 0) {
        return reject(new Error(`symgrep exited with code ${code}: ${stderr.trim()}`));
      }
      try {
        resolve(JSON.parse(stdout) as SearchResult);
      } catch (err) {
        reject(new Error(`failed to parse symgrep JSON: ${(err as Error).message}`));
      }
    });
  });
}
```

Notes:

- Always include `--format json`.
- Treat a non-zero exit code as an error (invalid flags, missing paths, etc.).
- Use `summary.truncated` to decide whether to narrow the query or
  issue follow-up calls with tighter filters.

---

## 5. Python Integration

### 5.1 Thin wrapper helper

This repository includes a small Python helper at
`wrappers/python/symgrep_agent.py`:

```python
from typing import Any, Dict

from symgrep_agent import run_symgrep_search

result: Dict[str, Any] = run_symgrep_search(
    {
        "pattern": "add",
        "paths": ["tests/fixtures/ts_js_repo"],
        "mode": "symbol",
        "view": ["decl"],
        "language": "typescript",
        "limit": 5,
    }
)

for symbol in result["symbols"]:
    print(symbol["file"], symbol["name"], symbol["kind"])
```

Wrapper contract:

- `run_symgrep_search(config) -> dict` where `config` keys map to CLI
  flags:
  - `pattern` (required) → search pattern.
  - `paths` → repeated `--path`.
  - `mode`, `view`, `language`, `limit`, `max_lines`, `use_index`,
    `index_backend`, `index_path`, `literal`, `reindex_on_search`
    → corresponding flags.
  - `symgrep_bin` (optional) → override the executable name/path
    (defaults to `"symgrep"`).
- Returns a decoded `SearchResult` JSON object as a `dict`.
- Raises `RuntimeError` with stderr attached if the subprocess exits
  non‑zero or outputs invalid JSON.

Because the wrapper is just a thin subprocess call, it stays aligned
with the CLI’s behavior and JSON schema.

---

## 6. Recipes for Common Agent Tasks

These recipes use the fixtures under `tests/fixtures/` so they stay
covered by snapshot tests.

### 6.1 Find declaration and approximate call sites of a function

Goal: locate where `add` is defined and where it is used, using a
combination of symbol search and text/call-graph search.

1. Find declarations of `add` in a mixed-language repo:

   ```bash
   symgrep search "name:add kind:function" \
     --path tests/fixtures/mixed_repo \
     --mode symbol \
     --view decl \
     --format json
   ```

   - Look at `symbols[*]` for the function definitions in each language.
   - Use `contexts[*].snippet` to see the signatures.

2. Approximate call sites by text search for `add(`:

   ```bash
   symgrep search "add(" \
     --path tests/fixtures/mixed_repo \
     --format json \
     --max-lines 3
   ```

   - `matches[*].path` and `line` give candidate call locations.
   - An agent can post-filter out lines that correspond to
     definitions rather than calls (e.g. by inspecting the snippet).

3. When call metadata is available (for example in the small
   `tests/fixtures/call_graph_repo` fixture), you can use structured
   call filters instead of text heuristics:

   ```bash
   # Symbols that call `foo`
   symgrep search "calls:foo" \
     --path tests/fixtures/call_graph_repo \
     --language typescript \
     --mode symbol \
     --view meta \
     --format json

   # Symbols that are called by `foo`
   symgrep search "called-by:foo" \
     --path tests/fixtures/call_graph_repo \
     --language typescript \
     --mode symbol \
     --view meta \
     --format json
   ```

   In the TypeScript call-graph fixture:

   - `foo` calls `bar` and `baz`, so `called-by:foo` returns their
     symbols.
   - `qux` calls `foo`, so `calls:foo` returns the `qux` symbol.

   Current behavior and limitations for `calls:` / `called-by:`:

   - **Name-based only**: call edges are matched by simple symbol name
     without type or overload resolution. Overloaded functions and
     methods with the same name will all share the same call edges.
   - **Per-file call graph**: edges are computed within individual
     files; cross-file and cross-language calls are not represented.
   - **Language coverage**:
     - TypeScript/JavaScript: handle common patterns like `foo(...)`
       and `obj.foo(...)` in the fixtures.
     - C++: current implementation focuses on simple identifier calls
       (`foo();`). More complex forms (e.g. `obj.method()`,
       `ns::func()`, templates) are not yet recorded in the call
       metadata.
   - **Index interaction**: any query that uses `calls:` or
     `called-by:` / `callers:` automatically runs a non-indexed
     symbol search, even when `--use-index` is set. This ensures
     results always see fresh call metadata at the cost of full-file
     parsing. For large repos, prefer narrowing `paths`, `language`,
     or combining with `name:`/`kind:` filters to bound work.

### 6.2 Find where a variable is used inside a given function

Goal: given a function `increment`, find lines mentioning `value` in
its body.

1. Get the full definition for `increment`:

```bash
symgrep search "name:increment kind:method" \
  --path tests/fixtures/cpp_repo \
  --language cpp \
  --mode symbol \
  --view def \
  --format json
   ```

2. In the returned JSON:

   - Find the symbol for `increment`.
   - Follow `contexts[symbol_index].snippet`.
   - Search within that snippet for the substring `value` and record
     the surrounding lines.

Agents can repeat this pattern for any variable name by adjusting the
substring search inside the `snippet` text; no extra CLI features are
required.

### 6.3 Get the parent function/class for a symbol and print its body

Goal: given the method `increment`, retrieve the enclosing `Widget`
struct/class.

```bash
symgrep search "name:increment kind:method" \
  --path tests/fixtures/cpp_repo \
  --language cpp \
  --mode symbol \
  --view parent \
  --format json
```

Interpretation:

- Take `contexts[0].snippet` as the full body of the enclosing
  `struct Widget { ... }`.
- Use `parent_chain` to understand the nesting:
  file → namespace → class → method.
- Combine this with the `symbols` array to build a richer view of the
  class (e.g. list all methods, or suggest new ones).

### 6.4 Attach keywords/description to a symbol and search by them

Goal: tag a symbol with external keywords and a description, then
query using those attributes.

1. Build an index for the TS/JS fixture:

   ```bash
   symgrep index \
     --path tests/fixtures/ts_js_repo \
     --index-backend file \
     --index-path .symgrep
   ```

2. Annotate the `addWithDoc` function with keywords and a description:

   ```bash
   symgrep annotate \
     --file tests/fixtures/ts_js_repo/doc_comments.ts \
     --language typescript \
     --kind function \
     --name addWithDoc \
     --start-line 5 \
     --end-line 7 \
     --keywords auth,login,jwt \
     --description "Performs user authentication and issues JWTs" \
     --index-backend file \
     --index-path .symgrep
   ```

   The command prints a JSON `SymbolAttributesResponse` containing the
   updated symbol and its `attributes`.

3. Search for symbols using the new attributes:

   ```bash
   symgrep search "comment:auth keyword:jwt desc:authentication" \
     --path tests/fixtures/ts_js_repo \
     --language typescript \
     --mode symbol \
     --view decl \
     --format json \
     --use-index \
     --index-backend file \
     --index-path .symgrep
   ```

   - Use `comment:` to filter by leading doc comments.
   - Use `keyword:` for exact keyword matches (list membership).
   - Use `desc:` / `description:` for substring matches within the
     free-form description.

---

## 7. Recommended Agent Practices

- Prefer `--format json` for all automation.
- Use `--limit` and `--max-lines` aggressively to keep payloads small.
- Use `--view decl` for quick overviews, `def` when you need
  implementation details, `parent` for surrounding class/module
  context, and `meta` when you only need symbol metadata.
- For large repos:
  - Build an index once via `symgrep index`.
  - Use `--use-index` plus a stable `--index-path` in scripts/CI.
- Always check:
  - `version` for schema compatibility.
  - `summary.truncated` to decide whether to refine or paginate
    follow-up queries.
