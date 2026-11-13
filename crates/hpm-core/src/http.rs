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
    base_builder().timeout(timeout)
}

/// A builder for downloads of any size: no cap on the whole transfer (a
/// large package on a slow link must finish), but a connection that cannot
/// be made, or that sends nothing for two minutes, fails.
pub(crate) fn download_client_builder() -> reqwest::ClientBuilder {
    base_builder().read_timeout(std::time::Duration::from_secs(120))
}

fn base_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(std::time::Duration::from_secs(30))
        .redirect(redirect_policy())
}

/// Whether `url` names this machine: `localhost`, `127.0.0.0/8` or `::1`.
fn is_loopback(url: &url::Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

/// The one transport rule for everything hpm fetches (registry answers,
/// package archives): https, or plain http only to this machine (a local
/// mirror or a test server). Plain http elsewhere lets anyone on the path
/// read and replace what is fetched.
pub(crate) fn check_transport(raw: &str) -> Result<(), String> {
    let url = url::Url::parse(raw.trim()).map_err(|e| format!("invalid URL '{raw}': {e}"))?;
    match url.scheme() {
        "https" => Ok(()),
        "http" if is_loopback(&url) => Ok(()),
        "http" => Err(format!(
            "'{raw}' is plain http to another machine; use https"
        )),
        other => Err(format!(
            "'{raw}' uses unsupported scheme '{other}'; use https"
        )),
    }
}

/// Follow redirects under [`check_transport`], and never from https down to
/// http, even on this machine; at most 10 hops.
fn redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 10 {
            return attempt.error("too many redirects");
        }
        let from_https = attempt
            .previous()
            .last()
            .is_some_and(|prev| prev.scheme() == "https");
        if from_https && attempt.url().scheme() != "https" {
            let message = format!("refusing redirect from https to {}", attempt.url());
            return attempt.error(message);
        }
        match check_transport(attempt.url().as_str()) {
            Ok(()) => attempt.follow(),
            Err(e) => attempt.error(format!("refusing redirect: {e}")),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// A 127.0.0.1 server answering every request with a redirect to
    /// `location`.
    fn redirecting_to(location: &'static str) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/start", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let _ = write!(
                    stream,
                    "HTTP/1.1 302 Found\r\nlocation: {location}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                );
            }
        });
        url
    }

    /// A redirect to a plain-http host off this machine is refused before
    /// any connection is made to it: the downloaded bytes would be readable
    /// and replaceable by anyone on the path.
    #[tokio::test]
    async fn a_redirect_to_plain_http_elsewhere_is_refused() {
        let start = redirecting_to("http://hpm-redirect-target.invalid/archive.zip");
        let client = client_builder(std::time::Duration::from_secs(10))
            .build()
            .unwrap();
        let err = client.get(&start).send().await.unwrap_err();
        assert!(err.is_redirect(), "followed the redirect: {err}");
    }
}
