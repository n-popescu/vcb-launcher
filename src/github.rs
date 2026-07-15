//! Fetch a repo's latest release WITHOUT the rate-limited REST API.
//!
//! Unauthenticated `api.github.com` is capped at **60 requests/hour/IP**, and the launcher fans
//! out one call per installed mod on every update check — enough to hit the cap and get an HTTP
//! 403. The GitHub **releases Atom feed** (`https://github.com/<owner>/<repo>/releases.atom`) is
//! served from github.com, not the API, so it isn't subject to that 60/hour limit; its newest
//! `<entry>` carries the latest release's tag. Pairing it with the stable
//! `github.com/<owner>/<repo>/releases/latest/download/<asset>` CDN URL (also github.com, not the
//! API) lets a mod's version check *and* download happen with zero REST-API calls.
//!
//! Caveat: the Atom feed lists pre-releases too, whereas the API's `releases/latest` skips them.
//! The VCB mods here don't publish pre-releases, so the newest Atom entry is the real latest; the
//! `releases/latest/download/` redirect likewise resolves to the newest non-prerelease asset.

use crate::net;

/// GET the repo's releases Atom feed and return the newest release's tag (e.g. `v1.4.0`).
pub fn latest_release_tag(owner: &str, repo: &str) -> Result<String, String> {
    let url = format!("https://github.com/{owner}/{repo}/releases.atom");
    let xml = net::get_text(&url, "application/atom+xml")?;
    parse_latest_tag(&xml).ok_or_else(|| format!("no releases found for {owner}/{repo}"))
}

/// A github.com CDN download URL for an asset on the repo's latest (non-prerelease) release. This
/// is a stable redirect served by github.com — NOT the REST API — so it doesn't count against the
/// API rate limit. `net::get_bytes` follows the redirect to the actual file.
pub fn latest_asset_download_url(owner: &str, repo: &str, asset: &str) -> String {
    format!("https://github.com/{owner}/{repo}/releases/latest/download/{asset}")
}

/// Extract the newest release's tag from a GitHub releases Atom feed. The first `<entry>` is the
/// newest release; its alternate link is `…/releases/tag/<TAG>`, so the first `/releases/tag/`
/// occurrence in the feed yields the latest tag.
pub fn parse_latest_tag(atom_xml: &str) -> Option<String> {
    const MARKER: &str = "/releases/tag/";
    let start = atom_xml.find(MARKER)? + MARKER.len();
    let rest = &atom_xml[start..];
    let end = rest
        .find(['"', '\'', '<', '>', ' ', '\n', '\r', '\t'])
        .unwrap_or(rest.len());
    let tag = rest[..end].trim();
    if tag.is_empty() {
        None
    } else {
        Some(decode_min_entities(tag))
    }
}

/// Minimal XML entity decode for the few that can appear in a tag path inside an href.
fn decode_min_entities(s: &str) -> String {
    s.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ATOM: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xml:lang="en-US">
  <id>tag:github.com,2008:https://github.com/n-popescu/vcb-modmenu/releases</id>
  <title>Release notes from vcb-modmenu</title>
  <updated>2024-05-01T00:00:00Z</updated>
  <entry>
    <id>tag:github.com,2008:Repository/123456789/v1.5.0</id>
    <updated>2024-05-01T00:00:00Z</updated>
    <link rel="alternate" type="text/html" href="https://github.com/n-popescu/vcb-modmenu/releases/tag/v1.5.0"/>
    <title>v1.5.0</title>
    <content type="html">notes</content>
  </entry>
  <entry>
    <id>tag:github.com,2008:Repository/123456789/v1.4.0</id>
    <link rel="alternate" type="text/html" href="https://github.com/n-popescu/vcb-modmenu/releases/tag/v1.4.0"/>
    <title>v1.4.0</title>
  </entry>
</feed>"#;

    #[test]
    fn parses_the_newest_tag() {
        assert_eq!(parse_latest_tag(ATOM).as_deref(), Some("v1.5.0"));
    }

    #[test]
    fn handles_a_bare_tag_without_v_prefix() {
        let atom = r#"<entry><link href="https://github.com/o/r/releases/tag/2.0.1"/></entry>"#;
        assert_eq!(parse_latest_tag(atom).as_deref(), Some("2.0.1"));
    }

    #[test]
    fn empty_feed_has_no_tag() {
        let empty = r#"<feed><id>tag:github.com,2008:https://github.com/o/r/releases</id></feed>"#;
        assert!(parse_latest_tag(empty).is_none());
    }

    #[test]
    fn download_url_is_cdn_not_api() {
        let u = latest_asset_download_url("n-popescu", "vcb-comment-block", "npopescu-VCBCommentBlock.zip");
        assert_eq!(
            u,
            "https://github.com/n-popescu/vcb-comment-block/releases/latest/download/npopescu-VCBCommentBlock.zip"
        );
        assert!(!u.contains("api.github.com"));
    }
}
