use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde::Serialize;

use crate::models::{
    IndexConfig, IndexSummary, SearchConfig, SearchResult, SymbolAttributesRequest,
    SymbolAttributesResponse,
};

/// HTTP client backend that delegates search and index operations to a
/// running `symgrep` daemon.
pub struct HttpSearchBackend {
    client: Client,
    base_url: String,
}

impl HttpSearchBackend {
    /// Create a new HTTP backend targeting the given base URL
    /// (e.g. "http://127.0.0.1:7878").
    pub fn new<S: Into<String>>(base_url: S) -> Result<Self> {
        let base_url = base_url.into();
        let base_url = base_url.trim_end_matches('/').to_string();

        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .context("failed to build HTTP client")?;

        Ok(Self { client, base_url })
    }

    /// Execute a search via `POST /v1/search`, returning a
    /// deserialized `SearchResult`.
    pub fn search(&self, config: SearchConfig) -> Result<SearchResult> {
        self.post_json("/v1/search", &config)
    }

    /// Execute an index operation via `POST /v1/index`, returning a
    /// deserialized `IndexSummary`.
    pub fn index(&self, config: IndexConfig) -> Result<IndexSummary> {
        self.post_json("/v1/index", &config)
    }

    /// Execute an index introspection via `POST /v1/index/info`, returning a
    /// deserialized `IndexSummary`.
    pub fn index_info(&self, config: IndexConfig) -> Result<IndexSummary> {
        self.post_json("/v1/index/info", &config)
    }

    /// Update attributes for a single symbol via
    /// `POST /v1/symbol/attributes`, returning the updated symbol.
    pub fn update_symbol_attributes(
        &self,
        request: SymbolAttributesRequest,
    ) -> Result<SymbolAttributesResponse> {
        self.post_json("/v1/symbol/attributes", &request)
    }

    fn post_json<T, R>(&self, path: &str, body: &T) -> Result<R>
    where
        T: Serialize,
        R: serde::de::DeserializeOwned,
    {
        let url = self.url_for(path);
        let response = self
            .client
            .post(&url)
            .json(body)
            .send()
            .with_context(|| format!("failed to send request to {}", url))?
            .error_for_status()
            .with_context(|| format!("server returned error for {}", url))?;

        let value = response
            .json::<R>()
            .context("failed to decode JSON response from server")?;

        Ok(value)
    }

    fn url_for(&self, path: &str) -> String {
        format!("{}/{}", self.base_url, path.trim_start_matches('/'))
    }
}
