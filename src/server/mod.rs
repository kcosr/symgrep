//! HTTP daemon/server mode for `symgrep`.
//!
//! This module exposes a small HTTP+JSON API that mirrors the core
//! `run_search` and `run_index` functions:
//!
//! - `POST /v1/search` – accepts a JSON-encoded `SearchConfig` and
//!   returns a `SearchResult`.
//! - `POST /v1/index` – accepts a JSON-encoded `IndexConfig` and
//!   returns an `IndexSummary`.
//! - `GET /v1/health` – simple health check endpoint.
//!
//! The server is intentionally thin: it performs JSON
//! (de)serialization, delegates to the core engine, and converts
//! errors into JSON HTTP responses.

use std::net::SocketAddr;

use anyhow::Result;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use tokio::net::TcpListener;

use crate::models::{
    IndexConfig, IndexSummary, SearchConfig, SearchResult, SymbolAttributesRequest,
    SymbolAttributesResponse,
};
use crate::search::engine;

/// Simple health-check response payload.
#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
}

/// JSON error body returned by the API.
#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

/// Error type used by HTTP handlers to map internal failures into
/// JSON error responses.
#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        let message = err.to_string();
        if message.starts_with("index not found at ") {
            Self {
                status: StatusCode::NOT_FOUND,
                message,
            }
        } else {
            // For now, treat all other engine errors as client-visible
            // bad requests; this can be refined later if needed with
            // more structured error types.
            ApiError::bad_request(message)
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(ErrorResponse {
            error: self.message,
        });
        (self.status, body).into_response()
    }
}

/// Build the Axum router for the Symgrep HTTP API.
pub fn router() -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/search", post(search))
        .route("/v1/index", post(index))
        .route("/v1/index/info", post(index_info))
        .route("/v1/symbol/attributes", post(symbol_attributes))
}

/// Run the HTTP server bound to the provided socket address.
///
/// This is used by the CLI `symgrep serve` subcommand.
pub async fn run(addr: SocketAddr) -> Result<()> {
    let listener = TcpListener::bind(addr).await?;
    serve_with_listener(listener).await
}

/// Run the HTTP server using an existing `TcpListener`.
///
/// This is primarily used in tests to bind to an ephemeral port.
pub async fn serve_with_listener(listener: TcpListener) -> Result<()> {
    let app = router();
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn search(Json(config): Json<SearchConfig>) -> Result<Json<SearchResult>, ApiError> {
    let result = engine::run_search(config).map_err(ApiError::from)?;
    Ok(Json(result))
}

async fn index(Json(config): Json<IndexConfig>) -> Result<Json<IndexSummary>, ApiError> {
    let summary = engine::run_index(config).map_err(ApiError::from)?;
    Ok(Json(summary))
}

async fn index_info(Json(config): Json<IndexConfig>) -> Result<Json<IndexSummary>, ApiError> {
    let summary = crate::index::get_index_info(&config).map_err(ApiError::from)?;
    Ok(Json(summary))
}

async fn symbol_attributes(
    Json(request): Json<SymbolAttributesRequest>,
) -> Result<Json<SymbolAttributesResponse>, ApiError> {
    let response = crate::index::update_symbol_attributes(request).map_err(ApiError::from)?;
    Ok(Json(response))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;
    use std::path::PathBuf;

    #[tokio::test]
    async fn health_endpoint_returns_ok_status() {
        let response = health().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn search_endpoint_executes_text_search() {
        let config = SearchConfig {
            pattern: "foo".to_string(),
            paths: vec![PathBuf::from("tests/fixtures/text_repo")],
            globs: Vec::new(),
            exclude_globs: Vec::new(),
            language: None,
            mode: crate::models::SearchMode::Text,
            literal: false,
            reindex_on_search: false,
            limit: None,
            max_lines: None,
            query_expr: None,
            index: None,
            symbol_views: Vec::new(),
        };

        let Json(result) = search(Json(config)).await.expect("search result");
        assert_eq!(result.query, "foo");
        assert!(result.summary.total_matches >= 1);
    }

    #[tokio::test]
    async fn index_endpoint_builds_index() {
        use tempfile::tempdir;

        let tmp = tempdir().expect("tempdir");
        let repo_root = tmp.path().join("repo");
        std::fs::create_dir_all(&repo_root).expect("create repo root");

        // Create a simple file to index.
        let file_path = repo_root.join("sample.ts");
        std::fs::write(
            &file_path,
            "export function add(a: number, b: number): number { return a + b; }",
        )
        .expect("write sample");

        let index_root = tmp.path().join(".symgrep");

        let config = IndexConfig {
            paths: vec![repo_root],
            globs: Vec::new(),
            exclude_globs: Vec::new(),
            backend: crate::models::IndexBackendKind::File,
            index_path: index_root.clone(),
            language: Some("typescript".to_string()),
        };

        let Json(summary) = index(Json(config)).await.expect("index summary");
        assert_eq!(summary.backend, crate::models::IndexBackendKind::File);
        assert_eq!(summary.index_path, index_root);
        assert!(summary.files_indexed >= 1);
        assert!(summary.symbols_indexed >= 1);
    }

    #[tokio::test]
    async fn index_info_endpoint_returns_summary() {
        use tempfile::tempdir;

        let tmp = tempdir().expect("tempdir");
        let repo_root = tmp.path().join("repo");
        std::fs::create_dir_all(&repo_root).expect("create repo root");

        let file_path = repo_root.join("sample.ts");
        std::fs::write(
            &file_path,
            "export function add(a: number, b: number): number { return a + b; }",
        )
        .expect("write sample");

        let index_root = tmp.path().join(".symgrep");

        let config = IndexConfig {
            paths: vec![repo_root],
            globs: Vec::new(),
            exclude_globs: Vec::new(),
            backend: crate::models::IndexBackendKind::File,
            index_path: index_root.clone(),
            language: Some("typescript".to_string()),
        };

        // Build the index once using the core engine.
        let _ = engine::run_index(config.clone()).expect("index summary");

        let Json(info) = index_info(Json(config)).await.expect("index info summary");
        assert_eq!(info.backend, crate::models::IndexBackendKind::File);
        assert_eq!(info.index_path, index_root);
        assert!(info.files_indexed >= 1);
        assert!(info.symbols_indexed >= 1);
        assert!(info.root_path.is_some());
        assert!(info.created_at.is_some());
        assert!(info.updated_at.is_some());
    }

    #[tokio::test]
    async fn symbol_attributes_endpoint_updates_symbol_attributes() {
        use crate::models::{IndexBackendKind, SymbolAttributesUpdate, SymbolKind, SymbolSelector};
        use tempfile::tempdir;

        let tmp = tempdir().expect("tempdir");
        let repo_root = tmp.path().join("repo");
        std::fs::create_dir_all(&repo_root).expect("create repo root");

        let file_path = repo_root.join("sample.ts");
        std::fs::write(
            &file_path,
            "export function add(a: number, b: number): number { return a + b; }",
        )
        .expect("write sample");

        let index_root = tmp.path().join(".symgrep");

        let index_config = IndexConfig {
            paths: vec![repo_root.clone()],
            globs: Vec::new(),
            exclude_globs: Vec::new(),
            backend: IndexBackendKind::File,
            index_path: index_root,
            language: Some("typescript".to_string()),
        };

        // Build the index once using the core engine.
        let _ = engine::run_index(index_config.clone()).expect("index summary");

        let selector = SymbolSelector {
            file: file_path,
            language: "typescript".to_string(),
            kind: SymbolKind::Function,
            name: "add".to_string(),
            start_line: 1,
            end_line: 1,
        };

        let attributes = SymbolAttributesUpdate {
            keywords: vec!["auth".to_string(), "login".to_string()],
            description: Some("Performs user authentication and issues JWTs".to_string()),
        };

        let request = SymbolAttributesRequest {
            index: index_config,
            selector,
            attributes,
        };

        let Json(response) = symbol_attributes(Json(request))
            .await
            .expect("attributes response");

        let attrs = response.symbol.attributes.expect("attributes");
        assert_eq!(attrs.keywords, vec!["auth".to_string(), "login".to_string()]);
        assert_eq!(
            attrs.description.as_deref(),
            Some("Performs user authentication and issues JWTs")
        );
    }

    #[tokio::test]
    async fn error_responses_are_returned_as_json() {
        // Use a non-existent path to trigger an error in the engine.
        let config = SearchConfig {
            pattern: "foo".to_string(),
            paths: vec![PathBuf::from("definitely/does/not/exist")],
            globs: Vec::new(),
            exclude_globs: Vec::new(),
            language: None,
            mode: crate::models::SearchMode::Text,
            literal: false,
            reindex_on_search: false,
            limit: None,
            max_lines: None,
            query_expr: None,
            index: None,
            symbol_views: Vec::new(),
        };

        let err = search(Json(config)).await.expect_err("expected error");
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
