# Symgrep – Phases 7–8

This document contains detailed tasks for:

- Phase 7 – LLM/Agent Integration Guides
- Phase 8 – Future Enhancements & Daemon / Service Mode (roadmap-level)

---

## Phase 7 – LLM/Agent Integration Guides

**Goals:**

- Make it easy for LLM agents and external tools to use `symgrep`.
- Provide practical examples and best practices for CLI and JSON usage.

**Tasks:**

1. **Agent-oriented documentation**
   - Create `docs/AGENT_GUIDE.md` covering:
     - How to call `symgrep` from:
       - Shell scripts.
       - Node/TypeScript.
       - Python.
     - Recommended flags:
       - `--format=json`
       - `--context=decl|def` for focused snippets.
       - `--limit` and `--max-lines` to control output volume.
       - `--use-index` for large repos.

2. **Common recipes**
   - Document and test:
     - “Find declaration and all call sites of function X.”
     - “Find where variable Y is used inside function Z.”
     - “Get the parent function/class for a symbol and print its full body.”
   - Represent each recipe as:
     - Example CLI command(s).
     - Example JSON output snippet.
     - Short explanation of how an agent can parse and use it.

3. **Thin language wrappers**
   - Optional but useful:
     - A Node/TypeScript wrapper module:
       - Spawns `symgrep` as a child process.
       - Wraps results in typed interfaces.
     - A Python wrapper:
       - Provides a `run_symgrep_search(config) -> dict` API.

4. **Tests**
   - Examples used in `AGENT_GUIDE.md` should:
     - Be covered by snapshot tests.
     - Stay in sync with actual behavior.
   - When CLI or JSON format changes:
     - Update tests and docs together.

**Deliverable:**  
`v0.7.0` – documented agent integration patterns with working examples.

---

## Phase 8 – Future Enhancements & Daemon / Service Mode (Roadmap)

> Note: This phase is a roadmap-level description and may be implemented incrementally after the core CLI + indexing features are stable.

**Long-term goals:**

- Introduce a `symgrep serve` mode that:
  - Runs as a long-lived daemon.
  - Exposes an HTTP+JSON API mirroring the CLI’s `--format=json`.
  - Holds indexes and parsed structures in memory for very fast repeated queries.
- Allow CLI to:
  - Use local one-shot mode (default).
  - Use daemon mode via `--server` or environment config.

**Key ideas:**

1. **Server module**
   - Add `src/server/mod.rs` with:
     - HTTP server setup.
     - Handlers for:
       - `POST /v1/search` → `SearchConfig` → `SearchResult`.
       - `POST /v1/index` → `IndexConfig` → `IndexSummary`.
       - `GET /v1/health`.

2. **Reusing core engine**
   - HTTP handlers must:
     - Use the same `SearchConfig`, `SearchResult`, `IndexConfig`, `IndexSummary` types as the CLI.
   - No duplicated search logic – only one core in `search::engine`.

3. **CLI client mode**
   - Extend CLI to support:
     - `--server <URL>` and `SYMGREP_SERVER_URL`.
     - `--no-server` to override defaults.
   - Implement an `HttpSearchBackend`:
     - Serializes configs to JSON.
     - Sends HTTP requests.
     - Deserializes JSON responses.

4. **Additional documentation**
   - Add `docs/DAEMON_API.md` with:
     - Endpoint list.
     - Request/response schemas.
     - Versioning and compatibility notes.

5. **Testing direction**
   - Test matrix should eventually cover:
     - Local CLI without indexing.
     - CLI with local index backends.
     - CLI talking to local daemon.
   - Ensure semantic equivalence between:
     - CLI local search vs CLI+daemon search on the same repo and query.

**Deliverable (roadmap):**  
`v0.8.0+` – daemon/service mode and CLI/HTTP integration, implemented when needed without disrupting existing CLI and agent workflows.
