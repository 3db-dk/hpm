//! HTTP API-based registry client.
//!
//! Connects to registries that serve package metadata over HTTP,
//! such as `https://api.3db.dk/v1/registry`.

use super::types::{RegistryEntry, SearchResults};
use super::{Registry, RegistryError};
use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use tracing::{debug, info};

/// An HTTP API-based package registry.
///
/// Expects endpoints:
/// - `GET {base_url}/config` -> RegistryConfig
/// - `GET {base_url}/packages?q={query}` -> SearchResults
/// - `GET {base_url}/packages/{creator}/{slug}` -> VersionsResponse
/// - `GET {base_url}/packages/{creator}/{slug}/{version}` -> BuildsResponse
///   (one or more platform-specific entries; the client picks the build that
///   matches the host platform — see [`select_build`])
pub struct ApiRegistry {
    /// Display name for this registry
    display_name: String,
    /// Base URL (e.g., "https://api.3db.dk/v1/registry")
    base_url: String,
    /// HTTP client
    client: reqwest::Client,
}

impl ApiRegistry {
    /// Create a new API registry client (anonymous requests).
    pub fn new(
        name: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Result<Self, RegistryError> {
        Self::with_auth_token(name, base_url, None)
    }

    /// Create a new API registry client, optionally injecting a bearer token
    /// on every request via the `Authorization` header.
    ///
    /// When `token` is `None`, behavior is identical to [`Self::new`]: the
    /// underlying client is built without an `Authorization` header. When
    /// `Some`, the token is attached as `Authorization: Bearer <token>` for
    /// every request the client issues, and the header value is flagged
    /// sensitive so reqwest does not log it.
    ///
    /// Note: the token is baked into the client at construction time. Callers
    /// that need to track a refreshing token should rebuild the registry
    /// (or the enclosing `RegistrySet`) when the token changes.
    pub fn with_auth_token(
        name: impl Into<String>,
        base_url: impl Into<String>,
        token: Option<&str>,
    ) -> Result<Self, RegistryError> {
        let mut builder = reqwest::Client::builder()
            .user_agent("hpm/0.1.0")
            .timeout(std::time::Duration::from_secs(30));

        if let Some(token) = token {
            let mut value = HeaderValue::from_str(&format!("Bearer {}", token)).map_err(|e| {
                RegistryError::ParseError(format!("invalid auth token for registry: {}", e))
            })?;
            value.set_sensitive(true);
            let mut headers = HeaderMap::new();
            headers.insert(AUTHORIZATION, value);
            builder = builder.default_headers(headers);
        }

        let client = builder.build()?;

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

    /// Create with a custom reqwest client (for testing).
    ///
    /// For attaching an auth token, prefer [`Self::with_auth_token`].
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

#[derive(serde::Deserialize)]
struct BuildsResponse {
    builds: Vec<RegistryEntry>,
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
        let url = format!("{}/packages/{}", self.base_url, encode_package_path(name));
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
            encode_package_path(name),
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
        let wrapper: BuildsResponse = response
            .json()
            .await
            .map_err(|e| RegistryError::ParseError(e.to_string()))?;

        if wrapper.builds.is_empty() {
            return Err(RegistryError::VersionNotFound {
                name: name.to_string(),
                version: version.to_string(),
            });
        }

        let host = hpm_package::Platform::current();
        select_build(&wrapper.builds, host).cloned().ok_or_else(|| {
            RegistryError::NoCompatibleBuild {
                name: name.to_string(),
                version: version.to_string(),
                host: host.map(|p| p.as_str().to_string()).unwrap_or_else(|| {
                    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
                }),
            }
        })
    }

    async fn refresh(&self) -> Result<(), RegistryError> {
        // API registries don't need local cache refresh - data is always live
        debug!("API registry '{}' refresh (no-op)", self.display_name);
        Ok(())
    }

    fn name(&self) -> &str {
        &self.display_name
    }
}

/// Simple percent-encoding for URL path segments.
fn urlencoded(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// Encode a package name for use in URL paths.
///
/// For scoped paths (`creator/slug`), each segment is encoded individually
/// so the `/` separator is preserved in the URL. For flat names, the entire
/// name is encoded as a single segment.
fn encode_package_path(name: &str) -> String {
    if let Some((creator, slug)) = name.split_once('/') {
        format!("{}/{}", urlencoded(creator), urlencoded(slug))
    } else {
        urlencoded(name)
    }
}

/// Map any platform identifier a registry might emit to hpm's canonical
/// [`hpm_package::Platform`]. Accepts both the canonical long form
/// (`"windows-x86_64"`) and the short OS form (`"WINDOWS"`/`"linux"`/etc.,
/// case-insensitive). Returns `None` for anything else, including the
/// universal sentinels handled separately by [`is_universal`].
fn normalize_platform(s: &str) -> Option<hpm_package::Platform> {
    use hpm_package::Platform;
    if let Ok(p) = s.parse::<Platform>() {
        return Some(p);
    }
    match s.to_ascii_lowercase().as_str() {
        "windows" => Some(Platform::WindowsX86_64),
        "linux" => Some(Platform::LinuxX86_64),
        "macos" => Some(Platform::MacosUniversal),
        _ => None,
    }
}

/// A build entry counts as universal when the registry omits the platform
/// or tags it `"universal"` (case-insensitive).
fn is_universal(platform: Option<&str>) -> bool {
    match platform {
        None => true,
        Some(s) => s.eq_ignore_ascii_case("universal"),
    }
}

/// Pick the best build for the host: exact platform match first, then a
/// universal entry. No silent positional fallback — if the registry
/// annotates every build but none match the host, the caller should error.
fn select_build(
    builds: &[RegistryEntry],
    host: Option<hpm_package::Platform>,
) -> Option<&RegistryEntry> {
    if let Some(host) = host
        && let Some(b) = builds.iter().find(|b| {
            b.platform
                .as_deref()
                .and_then(normalize_platform)
                .is_some_and(|bp| bp == host)
        })
    {
        return Some(b);
    }
    builds.iter().find(|b| is_universal(b.platform.as_deref()))
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
    fn test_encode_package_path_scoped() {
        assert_eq!(
            encode_package_path("tumblehead/tumble-rig"),
            "tumblehead/tumble-rig"
        );
    }

    #[test]
    fn test_encode_package_path_flat() {
        assert_eq!(encode_package_path("simple-name"), "simple-name");
    }

    #[test]
    fn test_api_registry_url_normalization() {
        let reg = ApiRegistry::new("test", "https://example.com/v1/registry/").unwrap();
        assert_eq!(reg.base_url, "https://example.com/v1/registry");
    }

    #[test]
    fn with_auth_token_none_is_equivalent_to_new() {
        let reg =
            ApiRegistry::with_auth_token("test", "https://example.com/v1/registry/", None).unwrap();
        assert_eq!(reg.base_url, "https://example.com/v1/registry");
        assert_eq!(reg.display_name, "test");
    }

    #[test]
    fn with_auth_token_some_builds_successfully() {
        // A well-formed token should not fail header construction; bad chars
        // would surface as ParseError.
        let reg = ApiRegistry::with_auth_token(
            "test",
            "https://example.com/v1/registry",
            Some("supabase-access-token-xyz"),
        )
        .unwrap();
        assert_eq!(reg.base_url, "https://example.com/v1/registry");
    }

    #[test]
    fn with_auth_token_rejects_tokens_with_invalid_header_bytes() {
        // Newline in a header value is not legal; reqwest rejects it on build.
        let result = ApiRegistry::with_auth_token(
            "test",
            "https://example.com/v1/registry",
            Some("bad\ntoken"),
        );
        assert!(matches!(result, Err(RegistryError::ParseError(_))));
    }

    use hpm_package::Platform;

    fn build_with(platform: Option<&str>, dl: &str) -> RegistryEntry {
        RegistryEntry {
            name: "tumblehead/tumblerig".to_string(),
            version: "1.0.0".to_string(),
            deps: vec![],
            cksum: None,
            dl: dl.to_string(),
            sig: None,
            kid: None,
            houdini_compat: None,
            platform: platform.map(|s| s.to_string()),
            yanked: false,
            description: None,
            author: None,
            created_at: None,
        }
    }

    #[test]
    fn normalize_accepts_canonical_long_form() {
        assert_eq!(
            normalize_platform("windows-x86_64"),
            Some(Platform::WindowsX86_64)
        );
        assert_eq!(
            normalize_platform("linux-x86_64"),
            Some(Platform::LinuxX86_64)
        );
        assert_eq!(
            normalize_platform("macos-universal"),
            Some(Platform::MacosUniversal)
        );
    }

    #[test]
    fn normalize_accepts_short_os_case_insensitive() {
        assert_eq!(normalize_platform("WINDOWS"), Some(Platform::WindowsX86_64));
        assert_eq!(normalize_platform("Linux"), Some(Platform::LinuxX86_64));
        assert_eq!(normalize_platform("macos"), Some(Platform::MacosUniversal));
    }

    #[test]
    fn normalize_rejects_unknown_and_universal() {
        // "universal" is intentionally not a Platform — it's handled by is_universal.
        assert_eq!(normalize_platform("universal"), None);
        assert_eq!(normalize_platform(""), None);
        assert_eq!(normalize_platform("linux-arm64"), None);
    }

    #[test]
    fn select_picks_host_match_against_short_os_tags() {
        // The bug from issue #3: TumbleTrove API emits "LINUX"/"WINDOWS".
        // Linux is first in the array; a Windows host must still pick Windows.
        let builds = vec![
            build_with(Some("LINUX"), "linux.zip"),
            build_with(Some("WINDOWS"), "windows.zip"),
        ];
        let picked = select_build(&builds, Some(Platform::WindowsX86_64)).unwrap();
        assert_eq!(picked.dl, "windows.zip");
    }

    #[test]
    fn select_picks_host_match_against_canonical_tags() {
        let builds = vec![
            build_with(Some("linux-x86_64"), "linux.zip"),
            build_with(Some("windows-x86_64"), "windows.zip"),
        ];
        let picked = select_build(&builds, Some(Platform::WindowsX86_64)).unwrap();
        assert_eq!(picked.dl, "windows.zip");
    }

    #[test]
    fn select_falls_back_to_universal_when_no_host_match() {
        let builds = vec![
            build_with(Some("LINUX"), "linux.zip"),
            build_with(None, "any.zip"),
        ];
        let picked = select_build(&builds, Some(Platform::WindowsX86_64)).unwrap();
        assert_eq!(picked.dl, "any.zip");
    }

    #[test]
    fn select_treats_explicit_universal_string_as_universal() {
        let builds = vec![
            build_with(Some("LINUX"), "linux.zip"),
            build_with(Some("UNIVERSAL"), "any.zip"),
        ];
        let picked = select_build(&builds, Some(Platform::WindowsX86_64)).unwrap();
        assert_eq!(picked.dl, "any.zip");
    }

    #[test]
    fn select_returns_none_when_no_match_and_no_universal() {
        // The defense-in-depth case: every build is platform-tagged, none
        // match the host. Must NOT silently fall through to builds[0].
        let builds = vec![
            build_with(Some("LINUX"), "linux.zip"),
            build_with(Some("MACOS"), "macos.zip"),
        ];
        assert!(select_build(&builds, Some(Platform::WindowsX86_64)).is_none());
    }

    #[test]
    fn select_returns_none_when_host_unknown_and_no_universal() {
        let builds = vec![
            build_with(Some("LINUX"), "linux.zip"),
            build_with(Some("WINDOWS"), "windows.zip"),
        ];
        assert!(select_build(&builds, None).is_none());
    }

    #[test]
    fn select_ignores_unknown_platform_strings() {
        // Unknown platform string + no universal + no match => None.
        let builds = vec![build_with(Some("plan9-amd64"), "plan9.zip")];
        assert!(select_build(&builds, Some(Platform::LinuxX86_64)).is_none());
    }
}
