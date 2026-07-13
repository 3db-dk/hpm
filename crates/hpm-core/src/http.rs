//! Shared HTTP client construction.
//!
//! Every outbound HTTP client hpm builds goes through here so the
//! user-agent (`hpm/<version>`) is consistent across the registry API,
//! archive downloads, and tool bootstrap.

/// Canonical user-agent for all hpm HTTP traffic.
pub(crate) const USER_AGENT: &str = concat!("hpm/", env!("CARGO_PKG_VERSION"));

/// A `reqwest::ClientBuilder` preconfigured with hpm's user-agent and the
/// given request timeout. Callers add their own headers (e.g. auth) and
/// `build()`.
pub(crate) fn client_builder(timeout: std::time::Duration) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(timeout)
}
