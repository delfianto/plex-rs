//! Client-side configuration shared by every HTTP request.
//!
//! [`ClientConfig`] is the single immutable bundle of identity,
//! timeouts, and retry policy passed into `HttpClient` (landing in
//! M0.12). It is constructed via [`ClientConfig::builder`] and then
//! frozen — callers wanting different settings spin up a new client.
//!
//! Builder pattern over `pub` fields because several invariants
//! (`max_retries <= 8`, `request_timeout >= 1ms`) need enforcing at
//! construction time. See [`ClientConfig::builder`] for the entry
//! point.

use std::time::Duration;

use crate::error::{Error, Result};
use crate::headers::PlexIdentity;
use crate::util::ids::{ClientIdentifier, PlexToken};

/// Default per-request timeout when the builder doesn't override.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Default connect timeout — separate from the full-request timeout so
/// a slow PMS doesn't masquerade as a hung connection.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Default maximum retry attempts (0 = no retries; only the first attempt).
pub const DEFAULT_MAX_RETRIES: u32 = 3;

/// Default base backoff between retries; the actual delay is
/// `base * 2^attempt * jitter` per `analysis/11` §4.8.
pub const DEFAULT_RETRY_BASE_DELAY: Duration = Duration::from_millis(250);

/// Default ceiling on a single retry delay.
pub const DEFAULT_RETRY_MAX_DELAY: Duration = Duration::from_secs(10);

/// Hard upper bound on `max_retries`. Anything higher is almost
/// certainly a misconfiguration (compounded backoff hits >30s well
/// before this limit).
pub const RETRY_HARD_CAP: u32 = 8;

// -----------------------------------------------------------------------------
// ClientConfig — the frozen-after-construction request envelope.
// -----------------------------------------------------------------------------

/// Frozen client configuration.
///
/// Construct via [`ClientConfig::builder`]; once built, the fields are
/// read-only. Fields are public so callers can introspect them, but
/// mutation goes through a fresh builder.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ClientConfig {
    /// Identity headers sent on every request.
    pub identity: PlexIdentity,
    /// Optional auth token; absent until sign-in completes.
    pub token: Option<PlexToken>,
    /// Per-request total timeout.
    pub request_timeout: Duration,
    /// Connect-only timeout.
    pub connect_timeout: Duration,
    /// Maximum retry attempts (excluding the first try).
    pub max_retries: u32,
    /// Base delay for the exponential backoff.
    pub retry_base_delay: Duration,
    /// Hard ceiling on a single retry delay (after exponential growth + jitter).
    pub retry_max_delay: Duration,
    /// Optional User-Agent override; falls back to the [`PlexIdentity`]
    /// `product/version platform` triple when absent.
    pub user_agent: Option<String>,
}

impl ClientConfig {
    /// Start a new builder.
    #[must_use]
    pub fn builder(client_identifier: ClientIdentifier) -> ClientConfigBuilder {
        ClientConfigBuilder {
            identity: PlexIdentity::new(client_identifier),
            token: None,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            max_retries: DEFAULT_MAX_RETRIES,
            retry_base_delay: DEFAULT_RETRY_BASE_DELAY,
            retry_max_delay: DEFAULT_RETRY_MAX_DELAY,
            user_agent: None,
        }
    }

    /// Returns the effective User-Agent string. Falls back to a
    /// `{product}/{version} ({platform})` triple constructed from
    /// [`PlexIdentity`].
    #[must_use]
    pub fn effective_user_agent(&self) -> String {
        self.user_agent.clone().unwrap_or_else(|| {
            format!(
                "{}/{} ({})",
                self.identity.product, self.identity.version, self.identity.platform,
            )
        })
    }
}

// -----------------------------------------------------------------------------
// ClientConfigBuilder.
// -----------------------------------------------------------------------------

/// Builder for [`ClientConfig`].
///
/// Build via [`ClientConfig::builder`] and finalise with
/// [`ClientConfigBuilder::build`].
#[derive(Debug, Clone)]
pub struct ClientConfigBuilder {
    identity: PlexIdentity,
    token: Option<PlexToken>,
    request_timeout: Duration,
    connect_timeout: Duration,
    max_retries: u32,
    retry_base_delay: Duration,
    retry_max_delay: Duration,
    user_agent: Option<String>,
}

impl ClientConfigBuilder {
    /// Replace the entire identity bundle.
    #[must_use]
    pub fn identity(mut self, identity: PlexIdentity) -> Self {
        self.identity = identity;
        self
    }

    /// Customise the identity in-place.
    #[must_use]
    pub fn map_identity(mut self, f: impl FnOnce(PlexIdentity) -> PlexIdentity) -> Self {
        self.identity = f(self.identity);
        self
    }

    /// Set the auth token. Pass [`None`] to clear (e.g. before sign-in).
    #[must_use]
    pub fn token(mut self, token: Option<PlexToken>) -> Self {
        self.token = token;
        self
    }

    /// Set the full-request timeout.
    #[must_use]
    pub const fn request_timeout(mut self, t: Duration) -> Self {
        self.request_timeout = t;
        self
    }

    /// Set the connect-only timeout.
    #[must_use]
    pub const fn connect_timeout(mut self, t: Duration) -> Self {
        self.connect_timeout = t;
        self
    }

    /// Set the retry attempt cap.
    #[must_use]
    pub const fn max_retries(mut self, n: u32) -> Self {
        self.max_retries = n;
        self
    }

    /// Set the retry exponential-backoff base delay.
    #[must_use]
    pub const fn retry_base_delay(mut self, d: Duration) -> Self {
        self.retry_base_delay = d;
        self
    }

    /// Set the cap on any single retry delay.
    #[must_use]
    pub const fn retry_max_delay(mut self, d: Duration) -> Self {
        self.retry_max_delay = d;
        self
    }

    /// Override the User-Agent string entirely.
    #[must_use]
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = Some(ua.into());
        self
    }

    /// Finalise into a [`ClientConfig`], validating invariants.
    ///
    /// # Errors
    /// Returns [`Error::Config`] when any of the following hold:
    /// - `request_timeout` is zero
    /// - `connect_timeout` is zero
    /// - `max_retries > 8` (see [`RETRY_HARD_CAP`])
    /// - `retry_max_delay < retry_base_delay`
    pub fn build(self) -> Result<ClientConfig> {
        if self.request_timeout.is_zero() {
            return Err(Error::Config("request_timeout must be > 0".to_owned()));
        }
        if self.connect_timeout.is_zero() {
            return Err(Error::Config("connect_timeout must be > 0".to_owned()));
        }
        if self.max_retries > RETRY_HARD_CAP {
            return Err(Error::Config(format!(
                "max_retries {} exceeds hard cap {RETRY_HARD_CAP}",
                self.max_retries,
            )));
        }
        if self.retry_max_delay < self.retry_base_delay {
            return Err(Error::Config(
                "retry_max_delay must be >= retry_base_delay".to_owned(),
            ));
        }
        Ok(ClientConfig {
            identity: self.identity,
            token: self.token,
            request_timeout: self.request_timeout,
            connect_timeout: self.connect_timeout,
            max_retries: self.max_retries,
            retry_base_delay: self.retry_base_delay,
            retry_max_delay: self.retry_max_delay,
            user_agent: self.user_agent,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cid() -> ClientIdentifier {
        ClientIdentifier::new("test-id").unwrap()
    }

    #[test]
    fn defaults_are_sane() {
        let cfg = ClientConfig::builder(cid()).build().unwrap();
        assert_eq!(cfg.request_timeout, DEFAULT_REQUEST_TIMEOUT);
        assert_eq!(cfg.connect_timeout, DEFAULT_CONNECT_TIMEOUT);
        assert_eq!(cfg.max_retries, DEFAULT_MAX_RETRIES);
        assert_eq!(cfg.retry_base_delay, DEFAULT_RETRY_BASE_DELAY);
        assert_eq!(cfg.retry_max_delay, DEFAULT_RETRY_MAX_DELAY);
        assert!(cfg.token.is_none());
        assert!(cfg.user_agent.is_none());
    }

    #[test]
    fn token_can_be_attached() {
        let token = PlexToken::new("secret").unwrap();
        let cfg = ClientConfig::builder(cid())
            .token(Some(token))
            .build()
            .unwrap();
        assert_eq!(cfg.token.as_ref().unwrap().expose(), "secret");
    }

    #[test]
    fn identity_setters_compose() {
        let cfg = ClientConfig::builder(cid())
            .map_identity(|id| id.with_product("custom").with_device("Mac"))
            .build()
            .unwrap();
        assert_eq!(cfg.identity.product, "custom");
        assert_eq!(cfg.identity.device, "Mac");
    }

    #[test]
    fn timeout_setters_apply() {
        let cfg = ClientConfig::builder(cid())
            .request_timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        assert_eq!(cfg.request_timeout, Duration::from_secs(60));
        assert_eq!(cfg.connect_timeout, Duration::from_secs(5));
    }

    #[test]
    fn rejects_zero_request_timeout() {
        let err = ClientConfig::builder(cid())
            .request_timeout(Duration::ZERO)
            .build()
            .unwrap_err();
        assert!(matches!(err, Error::Config(_)));
    }

    #[test]
    fn rejects_zero_connect_timeout() {
        let err = ClientConfig::builder(cid())
            .connect_timeout(Duration::ZERO)
            .build()
            .unwrap_err();
        assert!(matches!(err, Error::Config(_)));
    }

    #[test]
    fn rejects_excess_retries() {
        let err = ClientConfig::builder(cid())
            .max_retries(RETRY_HARD_CAP + 1)
            .build()
            .unwrap_err();
        assert!(matches!(err, Error::Config(_)));
    }

    #[test]
    fn rejects_inverted_retry_window() {
        let err = ClientConfig::builder(cid())
            .retry_base_delay(Duration::from_secs(5))
            .retry_max_delay(Duration::from_secs(1))
            .build()
            .unwrap_err();
        assert!(matches!(err, Error::Config(_)));
    }

    #[test]
    fn user_agent_override_round_trips() {
        let cfg = ClientConfig::builder(cid())
            .user_agent("MyApp/1.0")
            .build()
            .unwrap();
        assert_eq!(cfg.effective_user_agent(), "MyApp/1.0");
    }

    #[test]
    fn effective_user_agent_default_format() {
        let cfg = ClientConfig::builder(cid())
            .map_identity(|i| {
                i.with_product("prod")
                    .with_version("9.9")
                    .with_platform("test-os")
            })
            .build()
            .unwrap();
        assert_eq!(cfg.effective_user_agent(), "prod/9.9 (test-os)");
    }
}
