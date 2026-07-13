//! Minimal blocking HTTP(S) helpers for the update checker.
//!
//! Everything here is deliberately tiny: a shared `ureq` agent with a timeout and a
//! User-Agent (GitHub's API rejects requests without one), plus text and byte GETs that
//! follow redirects (release-asset downloads redirect to a CDN). Errors are flattened to
//! `String` so the UI can show them verbatim.

use std::io::Read;
use std::time::Duration;

/// GitHub requires a User-Agent on every API request; use a stable, identifying one.
const USER_AGENT: &str = concat!("vcb-launcher/", env!("CARGO_PKG_VERSION"));

/// Cap a single download so a bad/huge asset can't exhaust memory (release binaries are a
/// few MB; give generous headroom).
const MAX_BODY: u64 = 128 * 1024 * 1024;

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout(Duration::from_secs(60))
        .user_agent(USER_AGENT)
        .build()
}

fn map_err(url: &str, e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, _) => format!("{url} → HTTP {code}"),
        ureq::Error::Transport(t) => format!("{url} → {t}"),
    }
}

/// GET a URL as text (used for the GitHub JSON API). `accept` sets the Accept header.
pub fn get_text(url: &str, accept: &str) -> Result<String, String> {
    let resp = agent()
        .get(url)
        .set("Accept", accept)
        .call()
        .map_err(|e| map_err(url, e))?;
    let mut buf = String::new();
    resp.into_reader()
        .take(MAX_BODY)
        .read_to_string(&mut buf)
        .map_err(|e| format!("{url} → {e}"))?;
    Ok(buf)
}

/// GET a URL as raw bytes (used for release-asset / zipball downloads).
pub fn get_bytes(url: &str) -> Result<Vec<u8>, String> {
    let resp = agent().get(url).call().map_err(|e| map_err(url, e))?;
    let mut buf = Vec::new();
    resp.into_reader()
        .take(MAX_BODY)
        .read_to_end(&mut buf)
        .map_err(|e| format!("{url} → {e}"))?;
    Ok(buf)
}
