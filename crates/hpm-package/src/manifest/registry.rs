//! Registry references inside `hpm.toml`'s `[[registries]]` array.

use serde::{Deserialize, Serialize};

/// The type of registry backend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RegistryType {
    Api,
    Git,
}

impl std::str::FromStr for RegistryType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "api" => Ok(Self::Api),
            "git" => Ok(Self::Git),
            other => Err(format!(
                "unknown registry type '{other}'; expected 'api' or 'git'"
            )),
        }
    }
}

/// URL schemes a git registry remote may use. `git://` and `http://` are
/// unauthenticated plaintext; every other scheme names a git transport
/// helper, which hpm never runs on a registry's behalf.
const GIT_REMOTE_SCHEMES: [&str; 3] = ["https", "ssh", "file"];

/// Check a git registry remote before anything hands it to `git`.
///
/// Accepted: `https://` and `ssh://` URLs, `file://` URLs, scp-style
/// `user@host:path`, and local or network paths (studio indexes on a NAS).
/// Refused: anything `git` would read as an option (a leading `-`), the
/// `<transport>::<address>` helper syntax (`ext::`, `fd::`), schemes other
/// than the ones above, control characters, and leading or trailing
/// whitespace.
pub fn validate_git_remote(url: &str) -> Result<(), String> {
    if url.is_empty() {
        return Err("git registry url is empty".to_string());
    }
    if url.trim() != url {
        return Err("git registry url has leading or trailing whitespace".to_string());
    }
    if url.chars().any(char::is_control) {
        return Err("git registry url contains a control character".to_string());
    }
    if url.starts_with('-') {
        return Err("git registry url must not start with '-'".to_string());
    }
    if let Some((scheme, rest)) = url.split_once("://") {
        let scheme = scheme.to_ascii_lowercase();
        if !GIT_REMOTE_SCHEMES.contains(&scheme.as_str()) {
            return Err(format!(
                "git registry url scheme '{scheme}' is not allowed (use https, ssh or file)"
            ));
        }
        return check_remote_host(rest.split('/').next().unwrap_or(rest));
    }
    // `<transport>::<address>` asks git to run a remote helper. A `::` before
    // the first path separator is that syntax; a path may contain `::` later.
    let head = url.split(['/', '\\']).next().unwrap_or(url);
    if head.contains("::") {
        return Err("git registry url uses the '<transport>::' helper syntax".to_string());
    }
    // scp-like `[user@]host:path`: a colon before any separator, after more
    // than one character (`C:\...` is a Windows path).
    if let Some((authority, _)) = head.split_once(':')
        && authority.len() > 1
    {
        return check_remote_host(authority);
    }
    Ok(())
}

/// Refuse a host git would hand to ssh as an option (`-oProxyCommand=...`).
/// No host name starts with `-`.
fn check_remote_host(authority: &str) -> Result<(), String> {
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    if host.starts_with('-') {
        return Err(format!(
            "git registry url host '{}' must not start with '-'",
            host.escape_debug()
        ));
    }
    Ok(())
}

/// Check a registry's url for its backend. Git remotes are validated by
/// [`validate_git_remote`]; API urls only ever reach the HTTP client.
pub fn validate_registry_url(registry_type: &RegistryType, url: &str) -> Result<(), String> {
    match registry_type {
        RegistryType::Git => validate_git_remote(url),
        RegistryType::Api => Ok(()),
    }
}

/// A registry declared in hpm.toml's `[[registries]]` array.
///
/// The url is checked against its backend when the manifest is loaded, so a
/// bad entry fails naming the registry instead of reaching `git`.
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, try_from = "RawRegistryConfig")]
pub struct RegistryConfig {
    pub name: String,
    pub url: String,
    #[serde(rename = "type")]
    pub registry_type: RegistryType,
}

/// The unchecked on-disk shape of a [`RegistryConfig`].
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRegistryConfig {
    name: String,
    url: String,
    #[serde(rename = "type")]
    registry_type: RegistryType,
}

impl TryFrom<RawRegistryConfig> for RegistryConfig {
    type Error = String;

    fn try_from(raw: RawRegistryConfig) -> Result<Self, Self::Error> {
        validate_registry_url(&raw.registry_type, &raw.url)
            .map_err(|e| format!("registry '{}': {e}", raw.name))?;
        Ok(Self {
            name: raw.name,
            url: raw.url,
            registry_type: raw.registry_type,
        })
    }
}
