# `symgrep` Daemon / HTTP API

This document describes the initial HTTP+JSON API exposed by the
`symgrep` daemon (`symgrep serve`). The daemon is a thin wrapper
around the core `run_search` / `run_index` functions and reuses the
same JSON models as the CLI.

For detailed field definitions see `docs/JSON_SCHEMA.md`; this file
focuses on HTTP endpoints and request/response shapes.

---

## 1. Overview

- Protocol: HTTP/1.1
- Content type: `application/json` for requests and responses.
- Base URL: typically `http://127.0.0.1:7878` (configurable via
  `symgrep serve --addr`).

The daemon provides three primary endpoints:

- `GET /v1/health` – health check.
- `POST /v1/search` – execute a search and return a `SearchResult`.
- `POST /v1/index` – build or update an index and return an
  `IndexSummary`.

Clients should treat the JSON payloads as identical to the CLI’s
`--format=json` output; the same schema version applies.

---

## 2. Health Check

### `GET /v1/health`

Simple liveness probe.

- Request body: none.
- Response: `200 OK` with:

```json
{ "status": "ok" }
```

---

## 3. Search Endpoint

### `POST /v1/search`

Execute a search using the same `SearchConfig` type that the CLI
builds from `symgrep search` arguments.

- Request body: JSON `SearchConfig` (see `docs/JSON_SCHEMA.md`).
- Response:
  - `200 OK` with a JSON `SearchResult` on success.
  - `400 Bad Request` with an error JSON object when validation or
    engine errors occur (e.g. missing paths).

#### Example Request

```http
POST /v1/search HTTP/1.1
Content-Type: application/json

{
  "pattern": "foo",
  "paths": ["tests/fixtures/text_repo"],
  "globs": [],
  "exclude_globs": [],
  "language": null,
  "mode": "text",
   "literal": false,
  "context": "none",
  "limit": null,
  "max_lines": null,
  "index": null,
  "query_expr": null
}
```

#### Example Success Response

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

#### Example Error Response

```json
{
  "error": "search path does not exist: tests/fixtures/missing_repo"
}
```

---

## 4. Index Endpoint

### `POST /v1/index`

Build or update an index using the same `IndexConfig` type that the
CLI builds from `symgrep index` arguments.

- Request body: JSON `IndexConfig`.
- Response:
  - `200 OK` with a JSON `IndexSummary` on success.
  - `400 Bad Request` with an error JSON object when validation fails
    (e.g. non-existent index paths).

#### Example Request

```http
POST /v1/index HTTP/1.1
Content-Type: application/json

{
  "paths": ["tests/fixtures/ts_js_repo"],
  "globs": [],
  "exclude_globs": [],
  "backend": "file",
  "index_path": ".symgrep",
  "language": "typescript"
}
```

#### Example Success Response

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

The additional metadata fields (`root_path`, `schema_version`,
`tool_version`, `created_at`, `updated_at`) are additive and optional.

#### Example Error Response

```json
{
  "error": "index path does not exist: tests/fixtures/missing_repo"
}
```

---

## 5. Index Info Endpoint

### `POST /v1/index/info`

Read-only endpoint for inspecting an existing index without
modifying it. Mirrors the CLI `symgrep index-info` command and
returns the same `IndexSummary` shape as `POST /v1/index`.

- Request body: JSON `IndexConfig` (backend and index_path are
  required; paths/globs are used for context only).
- Response:
  - `200 OK` with a JSON `IndexSummary` on success.
  - `404 Not Found` when the requested index does not exist.
  - `400 Bad Request` for schema/version mismatches or other
    validation errors.

Example request:

```http
POST /v1/index/info HTTP/1.1
Content-Type: application/json

{
  "paths": ["tests/fixtures/ts_js_repo"],
  "globs": [],
  "exclude_globs": [],
  "backend": "sqlite",
  "index_path": ".symgrep/index.sqlite",
  "language": "typescript"
}
```

Example success response:

```json
{
  "backend": "sqlite",
  "index_path": ".symgrep/index.sqlite",
  "files_indexed": 3,
  "symbols_indexed": 42,
  "root_path": "/abs/path/to/project",
  "schema_version": "1",
  "tool_version": "0.1.0",
  "created_at": "2025-11-23T07:30:00Z",
  "updated_at": "2025-11-23T09:05:00Z"
}
```

Example error response for a missing index:

```json
{
  "error": "index not found at .symgrep/index.sqlite"
}
```

---

## 6. Versioning & Compatibility

The daemon reuses the same JSON schema version as the CLI’s
`--format=json` output:

- `SearchResult.version` indicates the schema version
  (e.g. `"0.1.0"`).
- Backward-incompatible changes to `SearchResult` or `IndexSummary`
  will bump this version and be reflected in both CLI and daemon
  responses.

Client guidance:

- Treat unknown fields as optional.
- Use the schema version string defensively when parsing.
- Expect that the HTTP API and CLI JSON formats evolve together.

---

## 6. CLI Integration (`--server` / `--no-server`)

The CLI can act as a thin HTTP client when a server URL is provided:

- `symgrep search ... --server http://127.0.0.1:7878`
- `symgrep index ... --server http://127.0.0.1:7878`
- Or via environment: `SYMGREP_SERVER_URL=http://127.0.0.1:7878`.
- `--no-server` forces local execution even when the environment
  variable is set.

When run with `--server`:

- `symgrep search` serializes its `SearchConfig` to JSON and sends it
  to `POST /v1/search`, then renders the returned `SearchResult` using
  the usual `--format` flag.
- `symgrep index` serializes its `IndexConfig` to JSON and sends it to
  `POST /v1/index`, then prints a human-readable summary derived from
  the returned `IndexSummary`.

Provided the daemon and CLI are built from the same version of
`symgrep`, local and daemon-backed searches/indexing should be
semantically equivalent for the same configuration.
