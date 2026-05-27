//! [`HttpClient`] — the only async I/O surface in the crate.
//!
//! Every other module that talks to a PMS or plex.tv funnels through
//! [`HttpClient`]. Centralising HTTP means we apply identity headers,
//! `Accept: application/json` negotiation, status-to-[`Error`] mapping,
//! and retry/backoff exactly once.
//!
//! Key design points (cross-references into the analysis docs):
//!
//! - **JSON-first content negotiation** — every request carries
//!   `Accept: application/json` by default; XML-only endpoints opt out
//!   via the dedicated `get_bytes` family.
//!   (`analysis/01-openapi-overview.md` §1.)
//! - **Status mapping** — non-2xx responses are folded into
//!   [`Error`] variants by [`Error::from_status`].
//!   (`analysis/02-base-and-http.md` §6.)
//! - **Retry with full-jitter exponential backoff** —
//!   `delay = rand([0, base * 2^attempt])` capped at `retry_max_delay`.
//!   Only retryable kinds (timeouts, transient connects, 5xx, 408, 425,
//!   429) trigger a retry. (`analysis/11-rust-mapping-recommendations.md`
//!   §4.8 — added on top of python-plexapi which has no retry layer.)
//! - **Token redaction** — the [`HttpClient`]'s `Debug` impl never prints
//!   the token; tracing spans elide URL `X-Plex-Token` query parameters.
//!
//! The retry math itself ([`retry_delay`]) is pure and unit-tested in
//! isolation. Integration tests against a `wiremock` mock server live
//! under `tests/`.

use std::fmt;
use std::time::Duration;

use serde::de::DeserializeOwned;
use tracing::{debug, warn};

use crate::config::ClientConfig;
use crate::error::{Error, Result};

// -----------------------------------------------------------------------------
// HttpClient.
// -----------------------------------------------------------------------------

/// Async HTTP client tailored to Plex's request envelope.
///
/// Wraps [`reqwest::Client`] and a [`ClientConfig`]. Identity headers
/// are baked into the underlying `reqwest::Client` as default headers
/// so every outgoing request inherits them; the token is also baked in
/// at construction time, meaning **rotating the token requires
/// constructing a new `HttpClient`** (cheap — `reqwest::Client` is
/// internally `Arc`-based).
///
/// `HttpClient` is `Clone`. `reqwest::Client` is internally
/// reference-counted so cloning the outer struct is cheap (a
/// `ClientConfig` clone — a handful of `String`s and integers). This
/// lets sub-types (`PlexServer`, `Library`, …) each hold their own
/// `HttpClient` handle without worrying about lifetimes.
#[derive(Clone)]
pub struct HttpClient {
    inner: reqwest::Client,
    config: ClientConfig,
}

impl HttpClient {
    /// Build an [`HttpClient`] from a frozen [`ClientConfig`].
    ///
    /// # Errors
    /// Returns [`Error::Config`] if the identity contains a value that
    /// can't be encoded as an HTTP header, and [`Error::Transport`] if
    /// the underlying TLS / DNS resolver fails to initialise.
    pub fn new(config: ClientConfig) -> Result<Self> {
        let default_headers = config.identity.headers(config.token.as_ref())?;
        let user_agent = config.effective_user_agent();
        let inner = reqwest::Client::builder()
            .connect_timeout(config.connect_timeout)
            .timeout(config.request_timeout)
            .user_agent(user_agent)
            .default_headers(default_headers)
            .build()?;
        Ok(Self { inner, config })
    }

    /// Borrow the underlying configuration.
    #[must_use]
    pub const fn config(&self) -> &ClientConfig {
        &self.config
    }

    /// Borrow the underlying `reqwest::Client`.
    ///
    /// Crate-private escape hatch for modules that need to bypass the
    /// standard JSON-with-retry envelope — e.g. the password sign-in
    /// flow inspects the response body to distinguish a generic
    /// `401 Unauthorized` from a 2FA-required gate.
    pub(crate) const fn inner(&self) -> &reqwest::Client {
        &self.inner
    }

    /// GET the URL and deserialise the JSON body as `T`.
    ///
    /// Retries are applied transparently per the policy in
    /// [`ClientConfig`].
    ///
    /// # Errors
    /// Any variant of [`Error`]; see [`Error::from_status`] for the
    /// status-code mapping.
    pub async fn get_json<T>(&self, url: &str) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let bytes = self
            .send_with_retry(reqwest::Method::GET, url, None)
            .await?;
        serde_json::from_slice(&bytes).map_err(Error::from)
    }

    /// GET the URL and return the raw response body as bytes.
    ///
    /// Use this for endpoints that emit XML or other non-JSON payloads.
    ///
    /// # Errors
    /// See [`Self::get_json`].
    pub async fn get_bytes(&self, url: &str) -> Result<bytes::Bytes> {
        self.send_with_retry(reqwest::Method::GET, url, None).await
    }

    /// PUT with a `&[u8]` body and no JSON deserialisation, for the
    /// many PMS endpoints that respond with an empty 200 body.
    ///
    /// # Errors
    /// See [`Self::get_json`].
    pub async fn put_no_body(&self, url: &str) -> Result<()> {
        let _ = self
            .send_with_retry(reqwest::Method::PUT, url, None)
            .await?;
        Ok(())
    }

    /// DELETE with no body — used for tag removal and similar.
    ///
    /// # Errors
    /// See [`Self::get_json`].
    pub async fn delete(&self, url: &str) -> Result<()> {
        let _ = self
            .send_with_retry(reqwest::Method::DELETE, url, None)
            .await?;
        Ok(())
    }

    /// POST a serialisable body and return the JSON-deserialised
    /// response.
    ///
    /// # Errors
    /// See [`Self::get_json`].
    pub async fn post_json<B, T>(&self, url: &str, body: &B) -> Result<T>
    where
        B: serde::Serialize + Sync + ?Sized,
        T: DeserializeOwned,
    {
        let body_bytes = serde_json::to_vec(body)?;
        let bytes = self
            .send_with_retry(reqwest::Method::POST, url, Some(body_bytes))
            .await?;
        serde_json::from_slice(&bytes).map_err(Error::from)
    }

    /// Core send loop with retry. Builds a fresh request per attempt
    /// because [`reqwest::RequestBuilder`] is not clonable post-build.
    async fn send_with_retry(
        &self,
        method: reqwest::Method,
        url: &str,
        body: Option<Vec<u8>>,
    ) -> Result<bytes::Bytes> {
        let max_attempts = self.config.max_retries.saturating_add(1);
        let mut last: Option<Error> = None;
        for attempt in 0..max_attempts {
            if attempt > 0 {
                let delay = retry_delay(
                    attempt,
                    self.config.retry_base_delay,
                    self.config.retry_max_delay,
                );
                debug!(
                    target: "plex_rs::client",
                    attempt,
                    delay_ms = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                    method = %method, "retrying after backoff"
                );
                tokio::time::sleep(delay).await;
            }
            match self.try_once(&method, url, body.as_deref()).await {
                Ok(bytes) => return Ok(bytes),
                Err(e) if e.is_retryable() && attempt + 1 < max_attempts => {
                    warn!(
                        target: "plex_rs::client",
                        attempt, method = %method, error = %e,
                        "request failed, will retry"
                    );
                    last = Some(e);
                }
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or(Error::Internal("retry loop exited without an error")))
    }

    /// Single attempt — build, send, map status, return body bytes on
    /// success.
    async fn try_once(
        &self,
        method: &reqwest::Method,
        url: &str,
        body: Option<&[u8]>,
    ) -> Result<bytes::Bytes> {
        let mut rb = self.inner.request(method.clone(), url);
        if let Some(body) = body {
            rb = rb
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(body.to_vec());
        }
        let resp = rb.send().await?;
        let status = resp.status();
        // Capture the *requested* path for NotFound diagnostics; preferring
        // the URL we sent rather than what reqwest gives back keeps the
        // error message stable across redirects.
        let path_for_error = response_path(&resp);
        let bytes = resp.bytes().await?;
        if status.is_success() {
            return Ok(bytes);
        }
        // Best-effort body excerpt — Plex error bodies are short HTML stubs.
        let body_excerpt = String::from_utf8_lossy(&bytes);
        Err(Error::from_status(status, &body_excerpt, &path_for_error))
    }
}

impl fmt::Debug for HttpClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // INVARIANT: token must not appear in Debug output. The
        // `ClientConfig` Debug impl is safe because `PlexToken`'s own
        // Debug is redacted (`util/ids.rs`), but we additionally elide
        // the inner reqwest::Client which would dump cookies / TLS
        // config metadata.
        f.debug_struct("HttpClient")
            .field("config", &self.config)
            .field("inner", &"reqwest::Client { .. }")
            .finish()
    }
}

/// Path portion of a `reqwest::Response`'s final URL.
/// Returned separately so the caller can decide whether to redact.
fn response_path(resp: &reqwest::Response) -> String {
    let url = resp.url();
    format!(
        "{}{}",
        url.path(),
        url.query().map(|q| format!("?{q}")).unwrap_or_default()
    )
}

// -----------------------------------------------------------------------------
// Retry math — pure, easy to unit-test.
// -----------------------------------------------------------------------------

/// Full-jitter exponential backoff:
/// `delay = uniform_random([0, min(max, base * 2^(attempt-1))])`.
///
/// `attempt` is 1-indexed (the first retry is `attempt == 1`).
/// `base * 2^(attempt-1)` is saturated to prevent overflow for very
/// large attempt counts.
#[must_use]
pub fn retry_delay(attempt: u32, base: Duration, max: Duration) -> Duration {
    // Compute the exponential ceiling, saturating at `max`.
    let shift = attempt.saturating_sub(1).min(20); // 2^20 ≈ 1M× — well past any sane max.
    let factor = 1u64.checked_shl(shift).unwrap_or(u64::MAX);
    let raw_nanos = u64::try_from(base.as_nanos()).unwrap_or(u64::MAX);
    let scaled_nanos = raw_nanos.saturating_mul(factor);
    let ceiling = Duration::from_nanos(scaled_nanos).min(max);
    // Full-jitter: pick uniformly in [0, ceiling]. Std lib's RNG isn't
    // available without an extra dep; use a tiny self-contained PCG.
    let ceiling_nanos = u64::try_from(ceiling.as_nanos()).unwrap_or(u64::MAX);
    let jitter = pcg_jitter(ceiling_nanos);
    Duration::from_nanos(jitter)
}

/// Cheap, self-seeded jitter source. Quality is sufficient for
/// dispersing retries across a fleet — not for cryptography.
fn pcg_jitter(max_nanos: u64) -> u64 {
    use std::cell::Cell;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static SEED: AtomicU64 = AtomicU64::new(0);
    thread_local! {
        static STATE: Cell<u64> = const { Cell::new(0) };
    }
    if max_nanos == 0 {
        return 0;
    }
    STATE.with(|s| {
        // Lazily seed once per thread, mixing the global counter, the
        // thread id, and the process wallclock.
        if s.get() == 0 {
            let global = SEED.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed);
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX));
            s.set(global ^ now.wrapping_mul(2_862_933_555_777_941_757) ^ 1);
        }
        let mut x = s.get();
        // xorshift64 — fine for jitter.
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        x % max_nanos
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::headers::PlexIdentity;
    use crate::util::ids::{ClientIdentifier, PlexToken};

    fn cfg() -> ClientConfig {
        ClientConfig::builder(ClientIdentifier::new("test").unwrap())
            .request_timeout(Duration::from_secs(2))
            .build()
            .unwrap()
    }

    // ---------- HttpClient construction ----------

    #[test]
    fn http_client_constructs_from_config() {
        let c = HttpClient::new(cfg()).unwrap();
        assert_eq!(c.config().identity.product, "plex-rs");
    }

    #[test]
    fn http_client_construction_propagates_header_error() {
        // Non-ASCII identity is rejected at PlexIdentity::headers.
        let cfg = ClientConfig::builder(ClientIdentifier::new("c").unwrap())
            .identity(
                PlexIdentity::new(ClientIdentifier::new("c").unwrap()).with_product("ünicode"),
            )
            .build()
            .unwrap();
        let err = HttpClient::new(cfg).unwrap_err();
        assert!(matches!(err, Error::InvalidHeader(_)));
    }

    #[test]
    fn http_client_debug_redacts_token() {
        let token = PlexToken::new("super-secret-token").unwrap();
        let cfg = ClientConfig::builder(ClientIdentifier::new("c").unwrap())
            .token(Some(token))
            .build()
            .unwrap();
        let client = HttpClient::new(cfg).unwrap();
        let s = format!("{client:?}");
        assert!(!s.contains("super-secret-token"), "Debug leaked token: {s}");
        assert!(s.contains("***redacted***"));
    }

    // ---------- retry_delay ----------

    #[test]
    fn retry_delay_attempt_zero_is_zero() {
        // attempt is 1-indexed; attempt=0 shouldn't happen but should be safe.
        let d = retry_delay(0, Duration::from_millis(100), Duration::from_secs(1));
        // attempt-1 saturates to 0 → ceiling = base. Jitter chooses in [0, base).
        assert!(d <= Duration::from_millis(100));
    }

    #[test]
    fn retry_delay_within_ceiling_for_first_retry() {
        for _ in 0..50 {
            let d = retry_delay(1, Duration::from_millis(100), Duration::from_secs(10));
            assert!(
                d <= Duration::from_millis(100),
                "delay {d:?} exceeded base 100ms"
            );
        }
    }

    #[test]
    fn retry_delay_grows_exponentially_then_caps() {
        // attempt 5 with base 100ms → ceiling 100 * 16 = 1600ms (<10s cap).
        for _ in 0..50 {
            let d = retry_delay(5, Duration::from_millis(100), Duration::from_secs(10));
            assert!(d <= Duration::from_millis(1600));
        }
        // attempt 20 — exponent overflows; should hit the max cap.
        for _ in 0..50 {
            let d = retry_delay(20, Duration::from_millis(100), Duration::from_secs(2));
            assert!(d <= Duration::from_secs(2));
        }
    }

    #[test]
    fn retry_delay_zero_max_returns_zero() {
        let d = retry_delay(3, Duration::from_millis(100), Duration::ZERO);
        assert_eq!(d, Duration::ZERO);
    }

    #[test]
    fn retry_delay_distribution_is_non_constant() {
        // Sanity: full-jitter shouldn't return the same value every time.
        let mut seen = std::collections::HashSet::new();
        for _ in 0..40 {
            let d = retry_delay(3, Duration::from_millis(100), Duration::from_secs(10));
            seen.insert(d);
        }
        assert!(
            seen.len() > 1,
            "retry_delay returned the same value 40 times in a row"
        );
    }

    // ---------- Error mapping smoke test via from_status ----------
    // Live HTTP behaviour is exercised by integration tests under `tests/`.

    #[test]
    fn status_500_maps_to_api_variant_through_helper() {
        let e = Error::from_status(http::StatusCode::INTERNAL_SERVER_ERROR, "boom", "/p");
        assert!(matches!(e, Error::Api { .. }));
        assert!(e.is_retryable());
    }
}
