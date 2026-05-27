//! M5.2 — `MyPlexPasswordLogin` integration tests.
//!
//! Drives a wiremock-backed plex.tv replica through the three branches
//! of the password sign-in flow:
//!
//! 1. happy path (200 + `authToken` minted)
//! 2. 2FA required (401 + `code: 1029` envelope)
//! 3. plain bad credentials (401 without the OTP marker)

use plex_rs::{ClientIdentifier, MyPlexPasswordLogin, error::Error};
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn login() -> MyPlexPasswordLogin {
    let cid = ClientIdentifier::new("password-login-test").unwrap();
    MyPlexPasswordLogin::new(cid, None).unwrap()
}

#[tokio::test]
async fn sign_in_returns_minted_token_on_success() {
    let server = MockServer::start().await;
    let response = serde_json::json!({
        "authToken": "the-minted-token",
        "username": "alice",
        "email": "alice@example.com",
    });
    Mock::given(method("POST"))
        .and(path("/api/v2/users/signin"))
        .and(header("content-type", "application/x-www-form-urlencoded"))
        .and(body_string_contains("login=alice%40example.com"))
        .and(body_string_contains("password=hunter2"))
        .and(body_string_contains("rememberMe=true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .expect(1)
        .mount(&server)
        .await;

    let token = login()
        .with_endpoint(format!("{}/api/v2/users/signin", server.uri()))
        .sign_in("alice@example.com", "hunter2")
        .await
        .unwrap();
    assert_eq!(token.expose(), "the-minted-token");
}

#[tokio::test]
async fn sign_in_surfaces_two_factor_required_when_otp_envelope_returned() {
    let server = MockServer::start().await;
    let envelope = serde_json::json!({
        "errors": [{
            "code": 1029,
            "message": "Please enter the verification code",
            "status": 401,
        }]
    });
    Mock::given(method("POST"))
        .and(path("/api/v2/users/signin"))
        .respond_with(ResponseTemplate::new(401).set_body_json(envelope))
        .expect(1)
        .mount(&server)
        .await;

    let err = login()
        .with_endpoint(format!("{}/api/v2/users/signin", server.uri()))
        .sign_in("alice@example.com", "hunter2")
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::TwoFactorRequired),
        "expected TwoFactorRequired, got {err:?}"
    );
}

#[tokio::test]
async fn sign_in_with_code_returns_token_when_otp_correct() {
    let server = MockServer::start().await;
    let response = serde_json::json!({"authToken": "token-after-otp"});
    Mock::given(method("POST"))
        .and(path("/api/v2/users/signin"))
        .and(body_string_contains("verificationCode=123456"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .expect(1)
        .mount(&server)
        .await;

    let token = login()
        .with_endpoint(format!("{}/api/v2/users/signin", server.uri()))
        .sign_in_with_code("alice@example.com", "hunter2", "123456")
        .await
        .unwrap();
    assert_eq!(token.expose(), "token-after-otp");
}

#[tokio::test]
async fn sign_in_returns_unauthorized_for_bad_credentials() {
    let server = MockServer::start().await;
    let envelope = serde_json::json!({
        "errors": [{
            "code": 1001,
            "message": "Invalid email, username, or password.",
            "status": 401,
        }]
    });
    Mock::given(method("POST"))
        .and(path("/api/v2/users/signin"))
        .respond_with(ResponseTemplate::new(401).set_body_json(envelope))
        .expect(1)
        .mount(&server)
        .await;

    let err = login()
        .with_endpoint(format!("{}/api/v2/users/signin", server.uri()))
        .sign_in("alice@example.com", "wrong-password")
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Unauthorized),
        "expected Unauthorized, got {err:?}"
    );
}
