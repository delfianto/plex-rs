//! Webhook registration on `plex.tv`.
//!
//! The complement to [`crate::webhook`]: where the latter receives
//! webhook deliveries, this module manages the URL list on the
//! plex.tv side. Add a URL here and PMS will POST events to it
//! whenever the configured triggers fire.
//!
//! Wire endpoint: `https://plex.tv/api/v2/user/webhooks`.
//!
//! - `GET` returns the current list. Plex's v2 endpoint returns
//!   either a top-level array of `{ url }` objects or a wrapped
//!   `{ webhooks: [...] }` shape depending on Accept negotiation;
//!   we parse both.
//! - `POST` with `application/x-www-form-urlencoded` body
//!   `urls[]=u1&urls[]=u2&…` replaces the full list. Passing an
//!   empty form clears every webhook.
//!
//! There is no per-webhook delete endpoint; mutation is always a
//! full-list replace. [`MyPlexClient::add_webhook`] /
//! [`delete_webhook`](MyPlexClient::delete_webhook) build the new
//! list and POST it.

use serde::Deserialize;

use crate::client::HttpClient;
use crate::error::{Error, Result};
use crate::myplex::MyPlexClient;

impl MyPlexClient {
    /// Fetch the current list of webhook URLs.
    ///
    /// # Errors
    /// Any [`Error`] variant. [`Error::Unauthorized`] signals a
    /// stale account token.
    pub async fn webhooks(&self) -> Result<Vec<String>> {
        let url = self.webhooks_url();
        let bytes = self.http().get_bytes(&url).await?;
        let body = std::str::from_utf8(&bytes)
            .map_err(|e| Error::Config(format!("webhooks body not utf-8: {e}")))?;
        Ok(parse_webhook_list(body))
    }

    /// Append `webhook_url` to the existing list and POST the new
    /// list. Idempotent — if `webhook_url` is already registered,
    /// no duplicate is added.
    ///
    /// Returns the fresh server-side list.
    ///
    /// # Errors
    /// As for [`Self::webhooks`].
    pub async fn add_webhook(&self, webhook_url: &str) -> Result<Vec<String>> {
        let mut urls = self.webhooks().await?;
        if urls.iter().any(|u| u == webhook_url) {
            return Ok(urls);
        }
        urls.push(webhook_url.to_owned());
        self.set_webhooks(&urls).await
    }

    /// Remove `webhook_url` from the list and POST the new list.
    ///
    /// # Errors
    /// - [`Error::NotFound`] if `webhook_url` isn't registered.
    /// - Any [`Error`] variant.
    pub async fn delete_webhook(&self, webhook_url: &str) -> Result<Vec<String>> {
        let mut urls = self.webhooks().await?;
        let before = urls.len();
        urls.retain(|u| u != webhook_url);
        if urls.len() == before {
            return Err(Error::NotFound {
                resource: format!("webhook {webhook_url}"),
            });
        }
        self.set_webhooks(&urls).await
    }

    /// Replace the entire webhook URL list. Passing an empty slice
    /// clears every webhook (matching python-plexapi's empty-list
    /// semantics).
    ///
    /// Returns the fresh server-side list.
    ///
    /// # Errors
    /// Any [`Error`] variant.
    pub async fn set_webhooks(&self, urls: &[String]) -> Result<Vec<String>> {
        let body = encode_form(urls);
        post_form(self.http(), &self.webhooks_url(), body).await?;
        // The POST response shape varies; reload to canonicalise.
        self.webhooks().await
    }

    /// Convenience: full webhook endpoint URL.
    fn webhooks_url(&self) -> String {
        format!("{}/api/v2/user/webhooks", self.base())
    }
}

/// Build the `application/x-www-form-urlencoded` body Plex expects.
///
/// Empty `urls` produces a single `urls=` pair, which plex.tv
/// interprets as "clear all". Non-empty `urls` is encoded as
/// `urls[]=u1&urls[]=u2&...`.
fn encode_form(urls: &[String]) -> Vec<u8> {
    let mut s = url::form_urlencoded::Serializer::new(String::new());
    if urls.is_empty() {
        s.append_pair("urls", "");
    } else {
        for u in urls {
            s.append_pair("urls[]", u);
        }
    }
    s.finish().into_bytes()
}

/// POST a form body using the inner reqwest client (bypasses the
/// JSON envelope, mirrors the password sign-in pattern).
async fn post_form(http: &HttpClient, url: &str, body: Vec<u8>) -> Result<()> {
    let resp = http
        .inner()
        .post(url)
        .header(
            http::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .header(http::header::ACCEPT, "application/json")
        .body(body)
        .send()
        .await?;
    let status = resp.status();
    let path = resp.url().path().to_owned();
    if status.is_success() {
        return Ok(());
    }
    let bytes = resp.bytes().await?;
    let body_excerpt = String::from_utf8_lossy(&bytes);
    Err(Error::from_status(status, &body_excerpt, &path))
}

/// Parse the GET response. Plex's webhook endpoint has flipped
/// shape across versions: sometimes a top-level array
/// `[{"url":"..."}]`, sometimes `{"webhooks":[{"url":"..."}]}`,
/// sometimes XML. We accept all three by trying each parse in
/// order and returning the first that succeeds; if none parse,
/// we return an empty list (treating the call as informational).
fn parse_webhook_list(body: &str) -> Vec<String> {
    // Try top-level array first.
    if let Ok(items) = serde_json::from_str::<Vec<WebhookDto>>(body) {
        return items.into_iter().map(|w| w.url).collect();
    }
    // Wrapped: { "webhooks": [...] }
    if let Ok(env) = serde_json::from_str::<WebhooksEnvelope>(body) {
        return env.webhooks.into_iter().map(|w| w.url).collect();
    }
    // XML fallback: extract every `url="..."` attribute.
    if body.trim_start().starts_with('<') {
        return parse_xml_url_attrs(body);
    }
    Vec::new()
}

/// Best-effort `url="..."` attribute scraper for the XML fallback.
/// Plex's older XML response shape is `<webhook url="..." />`
/// repeated, with no nested structure. A regex would do but we
/// avoid the dep just for this.
fn parse_xml_url_attrs(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let needle = "url=\"";
    let mut cursor = 0;
    while let Some(rel) = body[cursor..].find(needle) {
        let start = cursor + rel + needle.len();
        if let Some(end_rel) = body[start..].find('"') {
            out.push(body[start..start + end_rel].to_owned());
            cursor = start + end_rel + 1;
        } else {
            break;
        }
    }
    out
}

// -----------------------------------------------------------------------------
// DTOs.
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct WebhookDto {
    url: String,
}

#[derive(Debug, Deserialize)]
struct WebhooksEnvelope {
    webhooks: Vec<WebhookDto>,
}

// -----------------------------------------------------------------------------
// Tests.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_form_with_multiple_urls_uses_bracket_keys() {
        let body = encode_form(&["https://a/".to_owned(), "https://b/".to_owned()]);
        let s = String::from_utf8(body).unwrap();
        assert!(s.contains("urls%5B%5D=https%3A%2F%2Fa%2F"), "{s}");
        assert!(s.contains("urls%5B%5D=https%3A%2F%2Fb%2F"), "{s}");
    }

    #[test]
    fn encode_form_empty_produces_clear_marker() {
        let body = encode_form(&[]);
        let s = String::from_utf8(body).unwrap();
        assert_eq!(s, "urls=");
    }

    #[test]
    fn parse_webhook_list_handles_top_level_array() {
        let json = r#"[{"url":"https://a/"},{"url":"https://b/"}]"#;
        let urls = parse_webhook_list(json);
        assert_eq!(urls, vec!["https://a/", "https://b/"]);
    }

    #[test]
    fn parse_webhook_list_handles_wrapped_envelope() {
        let json = r#"{"webhooks":[{"url":"https://a/"},{"url":"https://b/"}]}"#;
        let urls = parse_webhook_list(json);
        assert_eq!(urls, vec!["https://a/", "https://b/"]);
    }

    #[test]
    fn parse_webhook_list_handles_xml_fallback() {
        let xml = "<webhooks><webhook url=\"https://a/\"/><webhook url=\"https://b/\"/></webhooks>";
        let urls = parse_webhook_list(xml);
        assert_eq!(urls, vec!["https://a/", "https://b/"]);
    }

    #[test]
    fn parse_webhook_list_empty_array() {
        assert_eq!(parse_webhook_list("[]"), Vec::<String>::new());
        assert_eq!(
            parse_webhook_list("{\"webhooks\":[]}"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn parse_webhook_list_unparseable_returns_empty() {
        assert_eq!(parse_webhook_list("not json or xml"), Vec::<String>::new());
    }
}
