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
- Symbol/AST search over TS/JS/C++ with contexts (decl/def/parent).
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
  - `symgrep search 'name:Foo kind:function|method' --path . --mode symbol --context def --format text`
- Filter functions by what appears in their body:
  - `symgrep search 'kind:function text:\"some phrase\"' --path . --mode symbol --context def --format text`
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
- `--context none|decl|def|parent` – how much surrounding code to return for symbols:
  - `none` – metadata only (fastest, smallest payload).
  - `decl` – declaration/signature only.
  - `def` – full definition/body.
  - `parent` – enclosing scope (e.g. file, namespace, class, or function).

Use `--schema-version` to discover the JSON schema version:

```bash
symgrep --schema-version
```

Agents should read the version string and handle minor version bumps
by treating unknown fields as optional.

### 2.2 Controlling result size

Recommended flags for bounding output:

- `--limit N` – stop after N matches (sets `summary.truncated = true`).
- `--max-lines N` – truncate each `snippet` to at most N lines.
  - `--max-lines 0` disables snippets entirely (snippets become `null`).

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
  --context decl \
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

- `text:` – generic content to search for (lines in text mode, snippets/bodies in symbol mode).
- `name:` – symbol name (function/method/class/interface/variable/namespace).
- `kind:` – symbol kind (`function`, `method`, `class`, `interface`, `variable`, `namespace`; aliases like `func`, `struct`, `ns` also work).
- `language:` – language identifier (e.g. `typescript`, `javascript`, `cpp`).
- `file:` – file path substring.

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

Bare patterns:

- If the pattern contains **no `field:` at all**, it is treated as a `text:` query:
  - `foo` → `text:foo`.
  - `foo|bar` → `text:foo OR text:bar`.
- To search by symbol name, prefer explicit `name:`:
  - `name:add kind:function` instead of bare `add`.

Precedence (informal):

- Quotes bind first (keep spaces inside a term), then `|` forms OR-groups inside a token, then whitespace combines groups with AND.

`--literal`:

- In **text mode**, `--literal` enables whole-identifier matching for the underlying `text:` value.
- In **symbol mode**, `--literal` controls exact vs substring matching for `name:` when you do not use `name:=value`.
- For new code and agent prompts, prefer `name:=foo` / `text:=foo` for explicit exact matches.

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
  "version": "0.1.0",
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
  --context decl \
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
  --context parent \
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
  context?: "none" | "decl" | "def" | "parent";
  language?: string;
  literal?: boolean;
  limit?: number;
  maxLines?: number;
  useIndex?: boolean;
  indexBackend?: "file" | "sqlite";
  indexPath?: string;
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
  if (config.context) args.push("--context", config.context);
  if (config.language) args.push("--language", config.language);
  if (config.literal) args.push("--literal");
  if (config.limit != null) args.push("--limit", String(config.limit));
  if (config.maxLines != null) args.push("--max-lines", String(config.maxLines));

  if (config.useIndex) {
    args.push("--use-index");
    if (config.indexBackend) args.push("--index-backend", config.indexBackend);
    if (config.indexPath) args.push("--index-path", config.indexPath);
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
        "context": "decl",
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
  - `mode`, `context`, `language`, `limit`, `max_lines`, `use_index`,
    `index_backend`, `index_path`, `literal` → corresponding flags.
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
combination of symbol search and text search.

1. Find declarations of `add` in a mixed-language repo:

   ```bash
   symgrep search "name:add kind:function" \
     --path tests/fixtures/mixed_repo \
     --mode symbol \
     --context decl \
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

### 6.2 Find where a variable is used inside a given function

Goal: given a function `increment`, find lines mentioning `value` in
its body.

1. Get the full definition for `increment`:

   ```bash
   symgrep search "name:increment kind:method" \
     --path tests/fixtures/cpp_repo \
     --language cpp \
     --mode symbol \
     --context def \
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
  --context parent \
  --format json
```

Interpretation:

- Take `contexts[0].snippet` as the full body of the enclosing
  `struct Widget { ... }`.
- Use `parent_chain` to understand the nesting:
  file → namespace → class → method.
- Combine this with the `symbols` array to build a richer view of the
  class (e.g. list all methods, or suggest new ones).

---

## 7. Recommended Agent Practices

- Prefer `--format json` for all automation.
- Use `--limit` and `--max-lines` aggressively to keep payloads small.
- Use `--context decl` for quick overviews, `def` when you need
  implementation details, and `parent` when you need surrounding
  class/module context.
- For large repos:
  - Build an index once via `symgrep index`.
  - Use `--use-index` plus a stable `--index-path` in scripts/CI.
- Always check:
  - `version` for schema compatibility.
  - `summary.truncated` to decide whether to refine or paginate
    follow-up queries.
