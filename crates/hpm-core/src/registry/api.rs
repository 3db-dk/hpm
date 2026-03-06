//! HTTP API-based registry client.
//!
//! Connects to registries that serve package metadata over HTTP,
//! such as `https://api.3db.dk/v1/registry`.

use super::types::{RegistryConfig, RegistryEntry, SearchResults};
use super::{Registry, RegistryError};
use async_trait::async_trait;
use tracing::{debug, info};

/// An HTTP API-based package registry.
///
/// Expects endpoints:
/// - `GET {base_url}/config` -> RegistryConfig
/// - `GET {base_url}/packages?q={query}` -> SearchResults
/// - `GET {base_url}/packages/{name}` -> VersionsResponse
/// - `GET {base_url}/packages/{name}/{version}` -> RegistryEntry
pub struct ApiRegistry {
    /// Display name for this registry
    display_name: String,
    /// Base URL (e.g., "https://api.3db.dk/v1/registry")
    base_url: String,
    /// HTTP client
    client: reqwest::Client,
}

impl ApiRegistry {
    /// Create a new API registry client.
    pub fn new(
        name: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Result<Self, RegistryError> {
        let client = reqwest::Client::builder()
            .user_agent("hpm/0.1.0")
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        let mut url = base_url.into();
        // Normalize: remove trailing slash
        while url.ends_with('/') {
            url.pop();
        }

        Ok(Self {
            display_name: name.into(),
            base_url: url,
            client,
        })
    }

    /// Create with a custom reqwest client (for testing or auth tokens).
    pub fn with_client(
        name: impl Into<String>,
        base_url: impl Into<String>,
        client: reqwest::Client,
    ) -> Self {
        let mut url = base_url.into();
        while url.ends_with('/') {
            url.pop();
        }
        Self {
            display_name: name.into(),
            base_url: url,
            client,
        }
    }
}

#[derive(serde::Deserialize)]
struct VersionsResponse {
    versions: Vec<RegistryEntry>,
}

#[async_trait]
impl Registry for ApiRegistry {
    async fn search(&self, query: &str) -> Result<SearchResults, RegistryError> {
        let url = format!("{}/packages?q={}", self.base_url, urlencoded(query));
        debug!("API registry search: {}", url);

        let response = self.client.get(&url).send().await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(SearchResults {
                packages: vec![],
                total: 0,
            });
        }

        let response = response.error_for_status()?;
        let results: SearchResults = response
            .json()
            .await
            .map_err(|e| RegistryError::ParseError(e.to_string()))?;

        info!(
            "Registry '{}' search for '{}': {} results",
            self.display_name, query, results.total
        );

        Ok(results)
    }

    async fn get_versions(&self, name: &str) -> Result<Vec<RegistryEntry>, RegistryError> {
        let url = format!("{}/packages/{}", self.base_url, urlencoded(name));
        debug!("API registry get_versions: {}", url);

        let response = self.client.get(&url).send().await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(RegistryError::PackageNotFound {
                name: name.to_string(),
            });
        }

        let response = response.error_for_status()?;
        let wrapper: VersionsResponse = response
            .json()
            .await
            .map_err(|e| RegistryError::ParseError(e.to_string()))?;

        Ok(wrapper.versions)
    }

    async fn get_version(&self, name: &str, version: &str) -> Result<RegistryEntry, RegistryError> {
        let url = format!(
            "{}/packages/{}/{}",
            self.base_url,
            urlencoded(name),
            urlencoded(version)
        );
        debug!("API registry get_version: {}", url);

        let response = self.client.get(&url).send().await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(RegistryError::VersionNotFound {
                name: name.to_string(),
                version: version.to_string(),
            });
        }

        let response = response.error_for_status()?;
        let entry: RegistryEntry = response
            .json()
            .await
            .map_err(|e| RegistryError::ParseError(e.to_string()))?;

        Ok(entry)
    }

    async fn refresh(&self) -> Result<(), RegistryError> {
        // API registries don't need local cache refresh - data is always live
        debug!("API registry '{}' refresh (no-op)", self.display_name);
        Ok(())
    }

    async fn config(&self) -> Result<RegistryConfig, RegistryError> {
        let url = format!("{}/config", self.base_url);
        debug!("API registry config: {}", url);

        let response = self.client.get(&url).send().await?;
        let response = response.error_for_status()?;
        let config: RegistryConfig = response
            .json()
            .await
            .map_err(|e| RegistryError::ParseError(e.to_string()))?;

        Ok(config)
    }

    fn name(&self) -> &str {
        &self.display_name
    }
}

/// Simple percent-encoding for URL path segments.
fn urlencoded(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_urlencoded() {
        assert_eq!(urlencoded("simple-name"), "simple-name");
        assert_eq!(urlencoded("name with spaces"), "name+with+spaces");
        assert_eq!(urlencoded("name@1.0"), "name%401.0");
    }

    #[test]
    fn test_api_registry_url_normalization() {
        let reg = ApiRegistry::new("test", "https://example.com/v1/registry/").unwrap();
        assert_eq!(reg.base_url, "https://example.com/v1/registry");
    }
}
