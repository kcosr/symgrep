# Adding a new language backend to `symgrep`

This document is for contributors who want to add or extend language
support in `symgrep` (for example, to support Rust). It describes how
language backends fit into the architecture and provides a checklist
for implementing a new backend.

The examples assume you are familiar with Rust and comfortable working
in this repository.

---

## 1. Architecture overview

At a high level, a language backend is responsible for:

- Parsing source files for a given language.
- Extracting `Symbol` records from the parsed representation.
- Providing context snippets (`decl` / `def` / `parent`) for symbols.

The main components involved are:

- `src/language/*` – per-language backends implementing the
  `LanguageBackend` trait.
- `src/search/engine.rs` – orchestrates text vs symbol search,
  calls into language backends for symbol indexing and context.
- `src/index/*` – file-backed and SQLite-backed index implementations
  that store and query symbols.
- `src/cli/*` and `src/server/*` – build `SearchConfig` from CLI/HTTP
  input and serialize `SearchResult` as JSON or text.

Queries use a small fielded DSL (`text:`, `name:`, `kind:`, `language:`,
`file:`). Language backends must respect the semantics of this DSL so
that agents can reuse patterns across languages.

---

## 2. Steps to add a new language backend

This section outlines the concrete steps to add support for a new
language `<lang>` with file extension(s) like `.ext`.

### 2.1 Create the backend module

1. Add a new source file under `src/language/<lang>.rs`.
2. Implement the `LanguageBackend` trait for a new backend type, for
   example:

   ```rust
   pub struct LangBackend {
       // parser state, configuration, etc.
   }

   impl LanguageBackend for LangBackend {
       // parse_file, index_symbols, get_context_snippet
   }
   ```

3. Choose an appropriate parser:
   - For many languages, a tree-sitter grammar is a good fit.
   - The backend should expose a small, stable API to the rest of the
     code (do not leak parser-specific types outside `src/language`).

### 2.2 Register the backend

Update `src/language/mod.rs` to wire in the new backend:

- Add a module declaration:

  ```rust
  pub mod <lang>;
  ```

- Update `backend_for_language` to return your backend for
  `language: "<lang>"` (lowercase string used in the DSL/CLI).
- Update `backend_for_path` to select the backend for relevant file
  extensions (for example, `.rs` for Rust).
- Add your backend to the `BACKENDS` array so it participates in
  lookup by both language ID and extension.

This ensures that:

- `--language <lang>` on the CLI maps to your backend.
- Files discovered by the search engine with the appropriate
  extension are parsed with your backend.

---

## 3. Symbol modeling guidelines

Backends must emit `Symbol` values that fit the shared model in
`src/models/mod.rs`:

- `name` – simple identifier (function name, type name, etc.).
- `kind` – one of the `SymbolKind` variants:
  - `function`
  - `method`
  - `class`
  - `interface`
  - `variable`
  - `namespace`
  - Additional kinds may be added in the future (for example, `enum`
    or `trait`).
- `language` – stable lowercase language identifier (e.g. `"rust"`).
- `file` – `PathBuf` for the source file where the symbol is defined.
- `range` – half-open range covering the symbol definition.
- `signature` – optional, human-readable declaration/signature.

`SymbolKind` is defined as a Rust enum (PascalCase variants like
`Function`, `Method`) but is serialized to JSON as lowercase strings
(`"function"`, `"method"`, etc.). The query DSL uses these lowercase
strings in `kind:` filters, and `parse_symbol_kind` in
`src/search/query.rs` maps them back to `SymbolKind`. When adding a
new kind, you must update both the enum and `parse_symbol_kind` so
`kind:newkind` queries work correctly.

When mapping language constructs to `SymbolKind`:

- Prefer the shared kinds above so queries like `kind:function` and
  `kind:class` work across languages.
- If your language has constructs that do not fit cleanly (for example,
  traits or enums), you can:
  - Map them to the closest existing kind (e.g. `struct`/`enum` to
    `class`) for a first iteration, or
  - Propose an additive change to `SymbolKind` and `parse_symbol_kind`
    in `src/search/query.rs` so they can be addressed explicitly via
    `kind:` in the DSL.

Symbols are used both directly in search results and indirectly via
indexes, so the mapping must be stable.

---

## 4. Context snippets and parent chains

Language backends are also responsible for producing `ContextInfo`
entries for symbols. The search engine requests context via the
`SearchContext` enum (`none`, `decl`, `def`, `parent`), which is
mapped to an internal `ContextKind` and passed to
`LanguageBackend::get_context_snippet`.

Recommended behavior:

- `Decl` (declaration/signature):
  - For functions/methods: the `fn`/function signature without the
    body.
  - For types: the struct/class/enum/trait header.
- `Def` (definition/body):
  - Full function/method body.
  - For types: the full type definition where appropriate.
- `Parent` (enclosing scope):
  - The surrounding scope relevant to the symbol, such as:
    - File-level block.
    - Module/namespace.
    - Enclosing type (class/struct/enum/trait).
    - `impl` block for methods (for languages like Rust).

`ContextInfo.parent_chain` should describe the nesting from outermost
to innermost, for example:

```json
[
  { "name": "sample.cpp", "kind": null },
  { "name": "util", "kind": "namespace" },
  { "name": "Widget", "kind": "class" }
]
```

Guidelines:

- Always include a file-level node as the first element (with `kind`
  usually `null`).
- Use `kind` strings consistent with `SymbolKind` when representing
  nested scopes like classes, namespaces, or functions.
- Keep the snippet reasonably small; the CLI/agent layer controls
  line limits via `--max-lines`, but backends should avoid including
  unrelated code when possible.

---

## 5. Index integration

`symgrep` can use either a file backend or a SQLite backend to store
symbols on disk. New language backends must work with both.

Key points:

- File backend:
  - Implemented in `src/index/file.rs`.
  - Serializes `Symbol` records (including `language` and `kind`) into
    JSON-like data under `.symgrep/`.
- SQLite backend:
  - Implemented in `src/index/sqlite.rs`.
  - Stores symbol rows with language and kind columns.

As long as your backend emits `Symbol` instances using supported
`SymbolKind` variants and a stable `language` string, the index
layers should work without changes. When new kinds are added, make
sure they round-trip through both backends and add tests as needed.

---

## 6. Tests and fixtures

When adding a new language backend, include tests and fixtures so
behavior stays stable over time.

Recommended steps:

- Fixtures:
  - Add a directory under `tests/fixtures/<lang>_repo`.
  - Include representative constructs:
    - Free functions.
    - Methods (if applicable).
    - Types (classes/structs/enums/traits).
    - Modules/namespaces.
  - Keep fixtures small and focused; they should be easy to reason
    about in tests and snapshots.

- CLI tests (`tests/cli_search.rs`):
  - Text search over the new fixture repo (to ensure basic file
    discovery works).
  - Symbol search:
    - `name:foo kind:function --language <lang>`.
    - Context behavior with `--context decl|def|parent`.
  - If appropriate, add the new language to any “mixed repo” tests
    that exercise cross-language patterns.

- Index tests:
  - When adding a new backend, verify that symbols from the new
    language appear in index-related tests (or add new ones that
    exercise `--use-index` for `<lang>`).

---

## 7. Agent-facing considerations

Language backends are behind the CLI boundary, but they directly
impact how agents experience `symgrep`. When designing or extending a
backend, keep the following in mind:

- DSL:
  - `name:` and `kind:` filters should behave consistently across
    languages.
  - `language:<lang>` should always work as a filter, both in the DSL
    and via `--language <lang>`.
- JSON output:
  - Respect the `SearchResult` and `Symbol` schema in
    `docs/JSON_SCHEMA.md`.
  - Ensure new language-specific features are additive and do not
    break existing consumers.
- Context size:
  - Keep snippets concise; agents often combine `symgrep` output with
    other tools and benefit from smaller, well-targeted contexts.
  - Use `parent_chain` to give agents a high-level view of where a
    symbol lives without forcing them to read entire files.

Following these guidelines will make new language backends predictable
for both humans and agents and keep `symgrep`’s behavior consistent as
the set of supported languages grows.

---

## 8. Common pitfalls

When adding or modifying a backend, watch out for these pitfalls:

- Parser leakage:
  - Keep tree-sitter (or other parser) types inside `src/language`;
    do not expose them via public APIs.
  - Use `BackendError` and `BackendResult<T>` for error reporting
    instead of panicking.
- Kind mapping consistency:
  - Ensure each construct in your language maps to the same
    `SymbolKind` everywhere.
  - When adding a new kind, update `SymbolKind`, `parse_symbol_kind`,
    and any docs/tests that refer to `kind:` filters for that kind.
- Extension variants:
  - Make sure `file_extensions()` and `backend_for_path` cover all
    relevant extensions for your language (for example, `.cc`/`.cxx`
    as well as `.cpp`).
- Parent chains:
  - Always include a file-level node in `parent_chain`.
  - If you cannot compute richer context for a symbol, still return a
    valid `ContextInfo` with at least the file-level parent rather
    than leaving `parent_chain` empty.
- Syntax errors:
  - `parse_file` should return a `BackendError` when the parser
    reports syntax errors or cannot produce a tree, instead of
    emitting partial or inconsistent symbols.
  - Callers will treat these as backend failures and skip the file
    gracefully.
