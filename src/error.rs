//! Crate-wide [`enum@Error`] and [`Result`] alias.
//!
//! Every fallible public function in `plex-rs` returns
//! [`Result<T>`](Result). Transport errors are mapped to typed variants
//! at the boundary of `HttpClient` so callers never see a raw
//! [`reqwest::Error`].
//!
//! The variants intentionally mirror the categories surfaced by
//! `python-plexapi`'s exception hierarchy
//! (`analysis/02-base-and-http.md` §6) but use typed status codes and
//! `#[from]` conversions rather than string sniffing.

use std::time::Duration;

use thiserror::Error;

/// All errors produced by `plex-rs`.
///
/// Variants are organised roughly from "user-recoverable" (e.g.
/// [`Unauthorized`](Error::Unauthorized) — refresh the token) to
/// "programmer error" ([`Internal`](Error::Internal) — invariant
/// violated). Match exhaustively; this enum is `#[non_exhaustive]` so
/// future additions are not breaking.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// The HTTP transport itself failed (DNS, TCP, TLS, connection
    /// reset, etc.). Includes the underlying [`reqwest::Error`].
    #[error("transport error: {0}")]
    Transport(#[from] reqwest::Error),

    /// The server returned an HTTP status that does not map to a more
    /// specific variant. `message` is the response body verbatim (Plex
    /// often returns short HTML stubs — see
    /// `analysis/01-openapi-overview.md` §4.4).
    #[error("plex api error {status}: {message}")]
    Api {
        /// HTTP status code returned by the server.
        status: http::StatusCode,
        /// Body excerpt (truncated to a reasonable length).
        message: String,
    },

    /// HTTP `401 Unauthorized` — the `X-Plex-Token` was missing,
    /// invalid, or expired. Always retryable after re-auth.
    #[error("unauthorized: missing or invalid X-Plex-Token")]
    Unauthorized,

    /// HTTP `403 Forbidden` — the token is valid but the calling
    /// account is not allowed to perform this action.
    #[error("forbidden: {0}")]
    Forbidden(String),

    /// HTTP `404 Not Found`. `resource` is a hint (the request path).
    #[error("not found: {resource}")]
    NotFound {
        /// The resource path that was requested.
        resource: String,
    },

    /// Authentication failed for a reason other than `401`. Used by the
    /// `MyPlex` sign-in flows when the API surfaces a structured error
    /// (e.g. 2FA required, account locked).
    #[error("authentication failed: {0}")]
    Auth(String),

    /// Two-factor verification is required to complete sign-in. The
    /// caller should re-issue the request with a `verification_code`.
    ///
    /// Distinct from [`Unauthorized`](Error::Unauthorized) so callers
    /// can distinguish "no/bad credentials" from "credentials OK but
    /// 2FA gate". See `analysis/03-myplex-and-auth.md` §2.3.
    #[error("two-factor authentication required")]
    TwoFactorRequired,

    /// The request did not complete within the configured timeout.
    #[error("timeout after {0:?}")]
    Timeout(Duration),

    /// XML parsing failed. Wraps [`quick_xml::DeError`].
    #[error("xml parse error: {0}")]
    Xml(#[from] quick_xml::DeError),

    /// JSON parsing failed. Wraps [`serde_json::Error`].
    #[error("json parse error: {0}")]
    Json(#[from] serde_json::Error),

    /// URL construction or parsing failed.
    #[error("invalid url: {0}")]
    Url(#[from] url::ParseError),

    /// An HTTP header value could not be constructed from user input
    /// (e.g. a non-ASCII X-Plex-Device-Name).
    #[error("invalid http header: {0}")]
    InvalidHeader(String),

    /// A configuration value was rejected (e.g. an empty
    /// `client_identifier`).
    #[error("invalid configuration: {0}")]
    Config(String),

    /// An internal invariant was violated. This is always a bug in
    /// `plex-rs` itself; please file an issue.
    #[error("internal invariant violated: {0}")]
    Internal(&'static str),
}

/// Convenience alias for `std::result::Result<T, Error>`.
pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    /// Map a status code + body excerpt to the most specific variant.
    ///
    /// Bodies are truncated at [`Self::MAX_BODY_LEN`] bytes to keep
    /// `Display` impls readable; Plex's error bodies are short HTML
    /// stubs in practice (see `analysis/01-openapi-overview.md` §4.4).
    #[must_use]
    pub fn from_status(status: http::StatusCode, body: &str, path: &str) -> Self {
        let trimmed = if body.len() > Self::MAX_BODY_LEN {
            // Snap to a UTF-8 boundary, then append the truncation marker.
            let mut end = Self::MAX_BODY_LEN;
            while !body.is_char_boundary(end) {
                end -= 1;
            }
            let mut s = body[..end].to_owned();
            s.push('…');
            s
        } else {
            body.to_owned()
        };
        match status.as_u16() {
            401 => Self::Unauthorized,
            403 => Self::Forbidden(trimmed),
            404 => Self::NotFound {
                resource: path.to_owned(),
            },
            _ => Self::Api {
                status,
                message: trimmed,
            },
        }
    }

    /// Truncation limit for error message bodies, in bytes.
    pub const MAX_BODY_LEN: usize = 1024;

    /// Returns `true` when the error is plausibly transient and the
    /// caller can retry after a backoff.
    ///
    /// Used by `HttpClient`'s retry middleware; see
    /// `analysis/11-rust-mapping-recommendations.md` §4.8.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Timeout(_) => true,
            Self::Transport(e) => e.is_timeout() || e.is_connect(),
            Self::Api { status, .. } => {
                matches!(status.as_u16(), 408 | 425 | 429 | 500 | 502 | 503 | 504)
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_status_maps_401_to_unauthorized() {
        let e = Error::from_status(http::StatusCode::UNAUTHORIZED, "no token", "/identity");
        assert!(matches!(e, Error::Unauthorized));
    }

    #[test]
    fn from_status_maps_403_to_forbidden() {
        let e = Error::from_status(http::StatusCode::FORBIDDEN, "nope", "/identity");
        assert!(matches!(e, Error::Forbidden(ref msg) if msg == "nope"));
    }

    #[test]
    fn from_status_maps_404_to_not_found_with_path() {
        let e = Error::from_status(
            http::StatusCode::NOT_FOUND,
            "missing",
            "/library/sections/99",
        );
        match e {
            Error::NotFound { resource } => assert_eq!(resource, "/library/sections/99"),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn from_status_maps_other_codes_to_api_with_truncated_body() {
        let huge = "x".repeat(2 * Error::MAX_BODY_LEN);
        let e = Error::from_status(http::StatusCode::INTERNAL_SERVER_ERROR, &huge, "/");
        match e {
            Error::Api { status, message } => {
                assert_eq!(status, http::StatusCode::INTERNAL_SERVER_ERROR);
                // Body must be truncated; ellipsis sentinel preserved.
                assert!(message.len() <= Error::MAX_BODY_LEN + 4);
                assert!(message.ends_with('…'));
            }
            other => panic!("expected Api, got {other:?}"),
        }
    }

    #[test]
    fn from_status_does_not_truncate_short_body() {
        let body = "short";
        let e = Error::from_status(http::StatusCode::SERVICE_UNAVAILABLE, body, "/");
        match e {
            Error::Api { message, .. } => assert_eq!(message, body),
            other => panic!("expected Api, got {other:?}"),
        }
    }

    #[test]
    fn is_retryable_timeout() {
        assert!(Error::Timeout(Duration::from_secs(1)).is_retryable());
    }

    #[test]
    fn is_retryable_for_5xx_and_429() {
        for code in [429u16, 500, 502, 503, 504, 408, 425] {
            let e = Error::Api {
                status: http::StatusCode::from_u16(code).unwrap(),
                message: String::new(),
            };
            assert!(e.is_retryable(), "{code} should be retryable");
        }
    }

    #[test]
    fn is_retryable_skips_client_errors() {
        for code in [400u16, 401, 403, 404, 422] {
            let e = Error::Api {
                status: http::StatusCode::from_u16(code).unwrap(),
                message: String::new(),
            };
            assert!(!e.is_retryable(), "{code} must not be retryable");
        }
        assert!(!Error::Unauthorized.is_retryable());
        assert!(!Error::TwoFactorRequired.is_retryable());
    }

    #[test]
    fn url_parse_error_converts_via_from() {
        let parse_err = url::Url::parse("not a url").unwrap_err();
        let e: Error = parse_err.into();
        assert!(matches!(e, Error::Url(_)));
    }

    #[test]
    fn json_error_converts_via_from() {
        let json_err = serde_json::from_str::<u32>("not json").unwrap_err();
        let e: Error = json_err.into();
        assert!(matches!(e, Error::Json(_)));
    }

    #[test]
    fn display_format_is_stable() {
        let e = Error::NotFound {
            resource: "/library".into(),
        };
        assert_eq!(e.to_string(), "not found: /library");
    }
}
