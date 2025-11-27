# symgrep

Symsemantic code search CLI currently supporting TypeScript, JavaScript, C++, and Rust.

`symgrep` combines fast text search with symbol-aware, AST-backed queries and
LLM-friendly JSON output. It can run as a one-shot CLI, reuse on-disk indexes
for larger repos, or delegate work to a long‑lived HTTP daemon.

## Features

- Text search (`--mode text`) with grep-like output.
- Symbol/AST search (`--mode symbol` or `--mode auto`) across TS/JS/C++/Rust.
- Configurable symbol views:
  - `--view=meta` – symbol metadata only (no context snippets, no per-symbol matches).
  - `--view=decl` – declaration/signature snippets.
  - `--view=def` – full definition bodies.
  - `--view=parent` – enclosing scopes (function, class, namespace, file).
  - `--view=comment` – doc comments only.
  - `--view=matches` – matching lines within the chosen region.
- Multiple output formats:
  - `--format=text` – grep-style lines.
  - `--format=table` – compact, aligned table view.
  - `--format=json` – structured, versioned schema for tools and agents.
- Literal and DSL-aware matching:
  - `--literal` for whole-identifier text matches and exact symbol
    name matches.
  - Indexing backends:
  - File backend (`--index-backend=file`) using `.symgrep/` JSON files.
  - SQLite backend (`--index-backend=sqlite`) for larger repos.
  - `symgrep index` to build/update indexes, `--use-index` to reuse them.
- Daemon / server mode:
  - `symgrep serve` starts an HTTP+JSON daemon.
  - `--server` / `SYMGREP_SERVER_URL` send CLI requests to the daemon.
- Project-local configuration:
  - `.symgrep/config.toml` can set defaults for CLI flags so commands like
    `symgrep search foo` can reuse project-specific paths, formats, and other
    options without repeating them on the command line.

## Install / Build

`symgrep` is a Rust project targeting the stable toolchain.

Prerequisites:

- A recent Rust toolchain via [`rustup`](https://rustup.rs).

For first-time setups where no default toolchain is configured:

```bash
rustup default stable
```

Build the binary:

```bash
cargo build --release
```

This produces `target/release/symgrep`. During development you can also run
directly via Cargo:

```bash
cargo run -- search "foo" --path tests/fixtures/text_repo --format text
```

## Quickstart

All examples assume you are in the `symgrep` repo root. You can replace the
fixture paths with your own project directories (for example, `--path .`).

### 1. Basic text search

Search for the string `"foo"` in a small text fixture repo:

```bash
symgrep search "foo" \
  --path tests/fixtures/text_repo \
  --format text
```

This prints grep-like lines:

```text
tests/fixtures/text_repo/a.txt:1:1: foo
```

For JSON output instead:

```bash
symgrep search "foo" \
  --path tests/fixtures/text_repo \
  --format json
```

To restrict matches to whole identifiers (so `foo` does not match
`foobar`), add `--literal`:

```bash
symgrep search "foo" \
  --path tests/fixtures/text_repo \
  --format text \
  --literal
```

The JSON schema (structure, fields, and versioning) is documented in
`docs/JSON_SCHEMA.md`. To check the current schema version:

```bash
symgrep --schema-version
```

Example output:

```text
Search result JSON schema version: 1.2.0
```

### 2. Symbol search with views

Run a symbol-mode search over a small mixed-language fixture repo, returning
declaration snippets for each `add` function:

```bash
symgrep search "name:add kind:function" \
  --path tests/fixtures/mixed_repo \
  --mode symbol \
  --view decl \
  --format json
```

The pattern `"name:add kind:function"` uses a simple DSL supporting `name:`,
`kind:`, and other filters. Symbol mode also accepts plain text patterns that
match symbol names.

To target a specific language, use either `--language` or a `language:`
filter in the pattern. For example, to search Rust symbols in the small
fixture repo:

```bash
symgrep search "name:increment kind:method language:rust" \
  --path tests/fixtures/rust_repo \
  --mode symbol \
  --view parent \
  --format json
```

This returns the Rust method `increment` with a `parent` context snippet
covering its enclosing `impl` block and a `parent_chain` that includes the
file, module, and struct. For Rust, symgrep classifies free functions
(`fn foo(...)`) and associated functions in `impl` blocks that do not take
`self` as `kind:function`, while inherent and trait methods that take any
form of `self` are reported as `kind:method`.

Key flags:

- `--mode symbol` – use the symbol/AST engine instead of pure text.
- `--view meta|decl|def|parent|comment|matches` – control what is shown per symbol:
  - `meta` – metadata only (no context snippets or match lines).
  - `decl` – declaration/signature only.
  - `def` – full definition/body.
  - `parent` – enclosing scope with `parent_chain`.
  - `comment` – doc comment only.
  - `matches` – matching lines within the chosen region or attributes.

Other useful flags for controlling result size:

- `--limit N` – stop after N matches.
- `--max-lines N` – truncate snippets to at most N lines (use `--max-lines 0`
  to disable snippets entirely).

### Query DSL overview

`symgrep search` accepts a small query DSL in the `pattern` string:

- Fields:
  - `content:` – content to search for (lines in text mode, symbol surface/snippets in symbol mode).
  - `name:` – symbol name.
  - `kind:` – symbol kind (`function`, `method`, `class`, `interface`, `variable`, `namespace`).
  - `language:` – language identifier (e.g. `typescript`, `javascript`, `cpp`, `rust`).
  - `file:` – file path substring.
- AND / OR:
  - `A B` → `A AND B` (whitespace).
  - `A|B` → `A OR B` (within a group).
  - `field:x|y|z` → `field:x OR field:y OR field:z`
    (e.g. `kind:function|method`, `language:typescript|javascript`).
- Bare patterns:
  - If there is no `field:` at all, the pattern is treated as `content:...`:
    - `foo` → `content:foo`, `foo|bar` → `content:foo OR content:bar`.
  - To filter by symbol name, prefer `name:` explicitly:
    - `name:add kind:function`.
- Exact vs substring:
  - `field:value` → substring/contains match.
  - `field:=value` (value starting with `=`) → exact match, e.g. `name:=add`.

### 3. Indexing and `--use-index`

For larger repos, build an index once and reuse it across searches. The example
below uses the TypeScript/JavaScript fixture repo and a SQLite index under
`target/`:

```bash
# Build a SQLite index once
symgrep index \
  --path tests/fixtures/ts_js_repo \
  --index-backend sqlite \
  --index-path target/symgrep/index.sqlite

# Run a symbol search that reuses the index
symgrep search "add" \
  --path tests/fixtures/ts_js_repo \
  --language typescript \
  --mode symbol \
  --view decl \
  --format json \
  --use-index \
  --index-backend sqlite \
  --index-path target/symgrep/index.sqlite
```

Example text output from `symgrep index`:

```text
Indexed 8 files and 8 symbols using Sqlite backend at target/symgrep/index.sqlite
```

Behavior notes:

- Omitting `--use-index` runs directly on files (no index).
- With `--use-index` but no explicit backend/path, `symgrep` automatically
  checks for an existing index at `.symgrep/index.sqlite` (SQLite) or
  `.symgrep/` (file), falling back to non-indexed search if neither exists.
- Indexed and non-indexed symbol searches are designed to be semantically
  equivalent; indexes only improve performance.

To inspect an existing index without modifying it, use `symgrep index-info`:

```bash
symgrep index-info \
  --path tests/fixtures/ts_js_repo \
  --index-backend sqlite \
  --index-path target/symgrep/index.sqlite \
  --format text
```

Example text output:

```text
backend      : sqlite
index_path   : target/symgrep/index.sqlite
root_path    : /workspace/devtools/symgrep/tests/fixtures/ts_js_repo
schema       : 2
tool_version : 0.0.0
created_at   : 2025-11-25T23:58:33Z
updated_at   : 2025-11-25T23:58:33Z
files        : 8
symbols      : 8
```

### 4. Daemon mode (`symgrep serve` + `--server`)

Start a long-lived HTTP daemon:

```bash
symgrep serve --addr 127.0.0.1:7878
```

CLI output:

```text
Starting symgrep HTTP server on http://127.0.0.1:7878
```

In another shell, send search requests to the daemon:

```bash
symgrep search "foo" \
  --path tests/fixtures/text_repo \
  --format json \
  --server http://127.0.0.1:7878
```

You can also configure the server URL via environment:

```bash
export SYMGREP_SERVER_URL=http://127.0.0.1:7878
symgrep search "foo" --path tests/fixtures/text_repo --format json
```

The daemon HTTP endpoints and payloads are described in `docs/DAEMON_API.md`.

### 5. Project-local TOML config

You can configure per-project defaults for all CLI-exposed options using a
TOML file under `.symgrep/config.toml` in your project root (or any ancestor
directory of your current working directory).

A complete example that exercises all CLI options is available at
`docs/config.example.toml`.

Example:

```toml
[search]
paths = ["."]
exclude = ["target", "node_modules"]
mode = "symbol"            # text|symbol|auto
view = ["parent"]          # meta|decl|def|parent|comment|matches
format = "json"            # text|table|json
use_index = true           # equivalent to --use-index
index_backend = "sqlite"   # file|sqlite
index_path = ".symgrep/index.sqlite"
# reindex_on_search = false   # when true, rebuild the index before each symbol search
language = "typescript"

[index]
paths = ["."]
backend = "sqlite"
index_path = ".symgrep/index.sqlite"

[index_info]
paths = ["."]
backend = "sqlite"
index_path = ".symgrep/index.sqlite"

[serve]
addr = "127.0.0.1:7878"

[http]
server_url = "http://127.0.0.1:7878"
```

Precedence rules:

- Built-in CLI defaults are used first.
- Project config from `.symgrep/config.toml` provides defaults when CLI flags
  or environment variables are not set.
- Environment variables (for example `SYMGREP_SERVER_URL`) override config.
- Explicit CLI flags and args always take precedence over config and env.

This allows commands like:

```bash
symgrep search "foo"
```

to behave as if you had explicitly passed `--path`, `--format`, `--use-index`,
and related options according to your project’s config, while remaining fully
overridable via the command line when needed.

## Further reading

Architecture & roadmap:

- `IMPLEMENTATION_PLAN.md` – high-level goals, architecture, and guardrails.
- `PHASES_01-03.md` – CLI skeleton, core search, and C++ backend.
- `PHASES_04-06.md` – context modes, DSL, indexing backends.
- `PHASES_07-08.md` – agent integration and daemon roadmap.

Agent and API-focused docs:

- `docs/AGENT_GUIDE.md` – recommended CLI flags, JSON usage, and agent recipes.
- `docs/JSON_SCHEMA.md` – structured JSON output schema and versioning.
- `docs/DAEMON_API.md` – HTTP/JSON daemon endpoints and request/response
  shapes.

Testing and development:

- `docs/TESTING.md` – testing philosophy, fixtures, and how to run the suite.
