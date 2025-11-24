# Testing Strategy & Philosophy

This document describes the long-term testing strategy for the **Symsemantic Code Search CLI** (`symgrep`): how tests are organized, what they cover, and how they should evolve alongside the codebase.

The goal is to treat tests as a **first-class part of the design**, not an afterthought.

---

## 1. Testing Philosophy

### 1.1 Core Principles

- **Tests are incremental and continuous**
  - Every new feature or refactor should come with new or updated tests.
  - No “big bang” test phases at the end of milestones.
- **CLI behavior and JSON output are APIs**
  - These are used by LLM agents and scripts; changes must be intentional.
  - Use snapshot tests and schema tests to keep them stable.
- **Fast feedback**
  - `cargo test` should run fast enough to encourage frequent execution.
  - Expensive benchmarks can be separated (e.g., only on CI, or manually).
- **Deterministic and isolated**
  - Tests must not depend on global machine state, network access, or external services.
  - Use temporary directories and fixtures for reproducible environments.

### 1.2 Testing Expectations

For each meaningful change (feature, bugfix, refactor):

1. Add or update **unit tests** covering the behavior.
2. Add or update **integration tests** if the CLI surface or end-to-end behavior is affected.
3. Run the full suite (`cargo test`) and fix regressions before merging.
4. If CLI output or JSON changes, update snapshots intentionally and review diffs.

---

## 2. Test Layout & Directory Structure

We follow standard Rust conventions with some additional directories for fixtures and snapshots:

```text
/ (repo root)
  src/
    main.rs / lib.rs
    ...
    backend/
      mod.rs          
    query/
      mod.rs          

  tests/              
    cli_search.rs     
    cli_index.rs      
    backends.rs       
    fixtures/         
      ts_js_repo/
      cpp_repo/
      mixed_repo/
    snapshots/        

  docs/
    TESTING.md        
```

### 2.1 Unit Tests (in `src/`)

- **Location**: inline in each module, under `mod tests { ... }`.
- **Purpose**:
  - Test internal logic in isolation:
    - Query parsing / DSL.
    - Language backends (symbol extraction, context).
    - Index backends (file/SQLite behavior).
    - Utility functions.
- **Characteristics**:
  - Fast.
  - No filesystem or external process dependencies.

### 2.2 Integration Tests (in `tests/`)

- Exercise the compiled binary end-to-end.
- Validate command-line behavior and actual parsing/indexing.
- Assert on:
  - Exit code
  - JSON/text output
  - Side effects (index creation, updates)

### 2.3 Fixtures

Small representative repos stored under version control:

- `ts_js_repo/`
- `cpp_repo/`
- `mixed_repo/`

Used across integration and some backend tests.

### 2.4 Snapshots

Used for stable output verification:

- JSON output
- Text/table formats

Stored in `tests/snapshots/`.

Only update snapshots intentionally after reviewing diffs.

---

## 3. Test Types

### 3.1 Unit Tests
- Query parser tests  
- Language backend symbol/context tests  
- Index backend CRUD/query tests  
- Utility function tests  

### 3.2 Integration Tests
- `symgrep search` across fixtures  
- `symgrep index` + `--use-index` parity checks  
- Multi-language detection tests  

### 3.3 Snapshot Tests
- Compare CLI output to golden files  
- Store snapshots under version control  
- Use when output structure/format is stable  

### 3.4 Performance & Benchmark Tests
- Optional; use Criterion  
- Measure:
  - Cold runs (no index)
  - Warm runs (file backend)
  - Warm runs (SQLite backend)

---

## 4. Recommended Crates

- `assert_cmd` – Run CLI commands  
- `predicates` – Output assertions  
- `tempfile` – Isolated FS environments  
- `insta` – Snapshot testing (optional)  
- `criterion` – Benchmarks (optional)  

---

## 5. Running Tests

### 5.1 Standard run

```bash
cargo test
```

### 5.2 With all features

```bash
cargo test --all-features
```

### 5.3 Specific test

```bash
cargo test --test cli_search
```

---

## 6. CI Integration

CI should:  
- Run `cargo fmt --check`  
- Run `cargo clippy -- -D warnings`  
- Run `cargo test` (all features)  
- Optionally run benchmarks  
- Build release binaries for tags  

---

## 7. Best Practices Over Time

- Keep fixtures small and focused  
- Never disable tests to fix CI  
- Treat snapshots like API changes: review diffs  
- Every regression leads to a stronger test suite  

---

## 8. Summary

This testing strategy ensures `symgrep` remains stable, predictable, and LLM-friendly.  
Unit + integration + snapshot tests together guard functionality, output formats, and long-term reliability.
