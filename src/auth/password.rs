//! [`MyPlexPasswordLogin`] — username + password (+ optional 2FA) sign-in
//! against plex.tv.
//!
//! Wire format:
//!
//! ```text
//! POST https://plex.tv/api/v2/users/signin
//! Content-Type: application/x-www-form-urlencoded
//! Accept: application/json
//! X-Plex-*: <identity headers>
//!
//! login=<email-or-username>&password=<password>&rememberMe=true[&verificationCode=<code>]
//! ```
//!
//! ## 2FA semantics
//!
//! When the account has two-factor authentication enabled and the
//! caller did not supply `verificationCode`, plex.tv responds with
//! `HTTP 401 Unauthorized` and a JSON body of the form:
//!
//! ```json
//! { "errors": [
//!     { "code": 1029, "message": "Please enter the verification code", "status": 401 }
//! ]}
//! ```
//!
//! The library inspects the response body and surfaces this as
//! [`Error::TwoFactorRequired`] so callers can prompt the user, then
//! re-invoke [`MyPlexPasswordLogin::sign_in_with_code`] with the OTP.
//!
//! ## Security
//!
//! The supplied password is **never** logged. `MyPlexPasswordLogin`
//! does not retain the password across calls — it is consumed by
//! [`sign_in`](MyPlexPasswordLogin::sign_in) /
//! [`sign_in_with_code`](MyPlexPasswordLogin::sign_in_with_code) and
//! drops out of scope as soon as the form body is on the wire.

use serde::Deserialize;

use crate::client::HttpClient;
use crate::config::ClientConfig;
use crate::error::{Error, Result};
use crate::headers::PlexIdentity;
use crate::util::ids::{ClientIdentifier, PlexToken};

/// plex.tv sign-in endpoint.
const SIGNIN_URL: &str = "https://plex.tv/api/v2/users/signin";

/// Plex API error code returned in the JSON body of a `401` when 2FA
/// is required. Stable per Plex forum / SDK convention.
const ERR_CODE_OTP_REQUIRED: i64 = 1029;

// -----------------------------------------------------------------------------
// MyPlexPasswordLogin.
// -----------------------------------------------------------------------------

/// Password-based plex.tv sign-in.
///
/// Construct once with the calling application's identity, then invoke
/// [`sign_in`](Self::sign_in) (or
/// [`sign_in_with_code`](Self::sign_in_with_code) when the account has
/// 2FA enabled).
///
/// # Example
///
/// ```no_run
/// # use plex_rs::{ClientIdentifier, MyPlexPasswordLogin};
/// # use plex_rs::error::Error;
/// # async fn run() -> Result<(), Error> {
/// let login = MyPlexPasswordLogin::new(
///     ClientIdentifier::new("my-app-identifier")?,
///     None,
/// )?;
/// let token = match login.sign_in("alice@example.com", "hunter2").await {
///     Ok(t) => t,
///     Err(Error::TwoFactorRequired) => {
///         // Prompt the user for their authenticator code, then:
///         login.sign_in_with_code("alice@example.com", "hunter2", "123456").await?
///     }
///     Err(e) => return Err(e),
/// };
/// # let _ = token;
/// # Ok(()) }
/// ```
#[derive(Debug)]
pub struct MyPlexPasswordLogin {
    http: HttpClient,
    endpoint: String,
}

impl MyPlexPasswordLogin {
    /// Build a fresh [`MyPlexPasswordLogin`] using the default
    /// `plex-rs` identity and the supplied stable
    /// [`ClientIdentifier`].
    ///
    /// `identity` overrides the default `X-Plex-*` headers when
    /// `Some`.
    ///
    /// # Errors
    /// - [`Error::Config`] if the identifier is empty.
    /// - [`Error::InvalidHeader`] if `identity` contains a non-ASCII
    ///   header value.
    /// - [`Error::Transport`] if the TLS / DNS stack fails to
    ///   initialise.
    pub fn new(
        client_identifier: ClientIdentifier,
        identity: Option<PlexIdentity>,
    ) -> Result<Self> {
        let mut cfg_builder = ClientConfig::builder(client_identifier);
        if let Some(id) = identity {
            cfg_builder = cfg_builder.identity(id);
        }
        let http = HttpClient::new(cfg_builder.build()?)?;
        Ok(Self {
            http,
            endpoint: SIGNIN_URL.to_owned(),
        })
    }

    /// Build from a caller-supplied [`HttpClient`]. Useful when the
    /// application has already configured custom timeouts, retry
    /// policy, or identity headers.
    #[must_use]
    pub fn with_client(http: HttpClient) -> Self {
        Self {
            http,
            endpoint: SIGNIN_URL.to_owned(),
        }
    }

    /// Override the plex.tv sign-in endpoint. Intended for testing
    /// against a local mock server.
    #[doc(hidden)]
    #[must_use]
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    /// Sign in with `login` (email or username) and `password`.
    ///
    /// # Errors
    /// - [`Error::TwoFactorRequired`] if the account has 2FA enabled —
    ///   retry via [`sign_in_with_code`](Self::sign_in_with_code).
    /// - [`Error::Unauthorized`] for bad credentials.
    /// - Other [`Error`] variants for transport / parse failures.
    pub async fn sign_in(&self, login: &str, password: &str) -> Result<PlexToken> {
        self.post_signin(login, password, None, true).await
    }

    /// Sign in supplying a 2FA verification code alongside the
    /// credentials.
    ///
    /// `verification_code` is the OTP from the user's authenticator
    /// app (TOTP) or SMS.
    ///
    /// # Errors
    /// See [`sign_in`](Self::sign_in). [`Error::Unauthorized`] also
    /// covers a wrong OTP.
    pub async fn sign_in_with_code(
        &self,
        login: &str,
        password: &str,
        verification_code: &str,
    ) -> Result<PlexToken> {
        self.post_signin(login, password, Some(verification_code), true)
            .await
    }

    /// Drive the sign-in POST.
    ///
    /// The flow bypasses [`HttpClient`]'s standard JSON-with-retry
    /// envelope because we need to inspect the response body on
    /// `401` to distinguish a bad-credentials error from a 2FA gate.
    async fn post_signin(
        &self,
        login: &str,
        password: &str,
        verification_code: Option<&str>,
        remember: bool,
    ) -> Result<PlexToken> {
        let body = encode_form(login, password, verification_code, remember);
        let resp = self
            .http
            .inner()
            .post(&self.endpoint)
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
        let bytes = resp.bytes().await?;
        if status.is_success() {
            let dto: SigninDto = serde_json::from_slice(&bytes)?;
            let raw = dto.auth_token.filter(|s| !s.is_empty()).ok_or_else(|| {
                Error::Auth("plex.tv signin succeeded but returned no token".to_owned())
            })?;
            return PlexToken::new(raw);
        }
        // Failure path: 401 may indicate 2FA-required; everything else
        // routes through the standard status → Error mapping.
        let body_excerpt = String::from_utf8_lossy(&bytes);
        if status == http::StatusCode::UNAUTHORIZED && body_signals_otp(&body_excerpt) {
            return Err(Error::TwoFactorRequired);
        }
        Err(Error::from_status(status, &body_excerpt, &path))
    }
}

// -----------------------------------------------------------------------------
// Wire-format helpers.
// -----------------------------------------------------------------------------

/// Build an `application/x-www-form-urlencoded` body.
fn encode_form(
    login: &str,
    password: &str,
    verification_code: Option<&str>,
    remember: bool,
) -> Vec<u8> {
    let mut s = url::form_urlencoded::Serializer::new(String::new());
    s.append_pair("login", login);
    s.append_pair("password", password);
    s.append_pair("rememberMe", if remember { "true" } else { "false" });
    if let Some(code) = verification_code {
        s.append_pair("verificationCode", code);
    }
    s.finish().into_bytes()
}

/// Inspect a JSON error body for the well-known OTP-required signal.
///
/// Plex's error envelope is `{ "errors": [{ "code": 1029, … }] }`. We
/// look for `code: 1029` exactly, with a string-contains fallback for
/// the human-readable hint Plex has used in the past
/// (`"verification code"`, case-insensitive).
fn body_signals_otp(body: &str) -> bool {
    if let Ok(parsed) = serde_json::from_str::<SigninErrorEnvelope>(body)
        && parsed
            .errors
            .iter()
            .any(|e| e.code == Some(ERR_CODE_OTP_REQUIRED))
    {
        return true;
    }
    body.to_ascii_lowercase().contains("verification code")
}

// -----------------------------------------------------------------------------
// DTOs.
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SigninDto {
    #[serde(default)]
    auth_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SigninErrorEnvelope {
    #[serde(default)]
    errors: Vec<SigninErrorItem>,
}

#[derive(Debug, Deserialize)]
struct SigninErrorItem {
    #[serde(default)]
    code: Option<i64>,
}

// -----------------------------------------------------------------------------
// Tests.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_form_basic_pair_set() {
        let body = encode_form("alice@example.com", "hunter2", None, true);
        let s = String::from_utf8(body).unwrap();
        // form_urlencoded uses application/x-www-form-urlencoded
        // (`+` for space). Ordering is preserved by Serializer.
        assert!(s.starts_with("login=alice%40example.com"), "{s}");
        assert!(s.contains("&password=hunter2"));
        assert!(s.contains("&rememberMe=true"));
        assert!(!s.contains("verificationCode"));
    }

    #[test]
    fn encode_form_includes_verification_code_when_supplied() {
        let body = encode_form("u", "p", Some("123456"), false);
        let s = String::from_utf8(body).unwrap();
        assert!(s.contains("verificationCode=123456"), "{s}");
        assert!(s.contains("rememberMe=false"));
    }

    #[test]
    fn encode_form_escapes_special_characters() {
        let body = encode_form("alice+bob@plex.tv", "p&a=ss word", None, true);
        let s = String::from_utf8(body).unwrap();
        // `+` in email becomes `%2B`, `&` and `=` in password are
        // percent-encoded, space becomes `+`.
        assert!(s.contains("login=alice%2Bbob%40plex.tv"), "{s}");
        assert!(s.contains("password=p%26a%3Dss+word"), "{s}");
    }

    #[test]
    fn body_signals_otp_detects_code_1029() {
        let body = r#"{"errors":[{"code":1029,"message":"Please enter the verification code","status":401}]}"#;
        assert!(body_signals_otp(body));
    }

    #[test]
    fn body_signals_otp_ignores_other_codes() {
        let body =
            r#"{"errors":[{"code":1001,"message":"Invalid email or password","status":401}]}"#;
        assert!(!body_signals_otp(body));
    }

    #[test]
    fn body_signals_otp_falls_back_to_substring_match() {
        // Plex has historically returned a plain-text-ish hint without
        // the code field; the lowercase substring fallback catches it.
        let body = r#"{"error":"Please enter the verification code"}"#;
        assert!(body_signals_otp(body));
    }

    #[test]
    fn body_signals_otp_returns_false_for_unrelated_payloads() {
        assert!(!body_signals_otp(r#"{"errors":[]}"#));
        assert!(!body_signals_otp("not json"));
        assert!(!body_signals_otp(""));
    }

    #[test]
    fn signin_dto_parses_minimal_success_response() {
        let body = serde_json::json!({
            "authToken": "minted-token-here",
            "username": "alice",
            "email": "alice@example.com",
        });
        let dto: SigninDto = serde_json::from_value(body).unwrap();
        assert_eq!(dto.auth_token.as_deref(), Some("minted-token-here"));
    }

    #[test]
    fn signin_dto_treats_missing_token_as_none() {
        let body = serde_json::json!({"username": "alice"});
        let dto: SigninDto = serde_json::from_value(body).unwrap();
        assert!(dto.auth_token.is_none());
    }
}
