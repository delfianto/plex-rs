//! Inbound Plex webhook handling.
//!
//! Plex sends `multipart/form-data` POSTs to user-configured webhook
//! URLs whenever a tracked event happens — playback state changes,
//! library additions, database events, and so on. The body has one
//! mandatory `payload` form field carrying a JSON document, and an
//! optional `thumb` field with a poster image.
//!
//! Usage with [`axum`]:
//!
//! ```no_run
//! # use axum::{Router, routing::post};
//! # use plex_rs::webhook::{WebhookPayload, WebhookEvent};
//! async fn on_plex_event(payload: WebhookPayload) {
//!     match payload.event {
//!         WebhookEvent::MediaPlay => println!("now playing: {:?}",
//!             payload.metadata.as_ref().and_then(|m| m.title.as_deref())),
//!         WebhookEvent::MediaStop => println!("stopped"),
//!         other => tracing::debug!("plex event: {other:?}"),
//!     }
//! }
//!
//! # fn main() {
//! let app: Router = Router::new().route("/plex", post(on_plex_event));
//! # let _ = app;
//! # }
//! ```
//!
//! ## Off-axum usage
//!
//! [`WebhookPayload::from_json`] decodes a raw JSON string (the
//! `payload` form field's value) into a typed payload. Use this when
//! the receiver is a non-axum framework — the multipart parsing is
//! framework's responsibility; only the JSON shape lives here.
//!
//! ## Cargo feature
//!
//! This module is gated behind the `webhook-axum` Cargo feature. The
//! feature pulls in `axum` with the `multipart` feature.

use axum::{
    body::Bytes,
    extract::{FromRequest, Multipart, Request},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use crate::error::Error;

// -----------------------------------------------------------------------------
// WebhookEvent — discriminator.
// -----------------------------------------------------------------------------

/// Plex's `event` field on the webhook JSON payload.
///
/// The list mirrors the values Plex documents at
/// <https://support.plex.tv/articles/115002267687-webhooks/>.
/// Unknown / future events fall through to [`WebhookEvent::Unknown`]
/// for forward compatibility.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum WebhookEvent {
    /// `media.play` — playback started.
    MediaPlay,
    /// `media.pause` — playback paused.
    MediaPause,
    /// `media.resume` — playback resumed.
    MediaResume,
    /// `media.stop` — playback stopped.
    MediaStop,
    /// `media.scrobble` — playback reached the scrobble threshold
    /// (~90% by default).
    MediaScrobble,
    /// `media.rate` — the user rated the item.
    MediaRate,
    /// `library.on.deck` — added to "On Deck".
    LibraryOnDeck,
    /// `library.new` — added to a library.
    LibraryNew,
    /// `admin.database.backup` — database backup completed.
    AdminDatabaseBackup,
    /// `admin.database.corrupted` — database corruption detected.
    AdminDatabaseCorrupted,
    /// `device.new` — new device claimed the account.
    DeviceNew,
    /// `playback.started` — playback session began (distinct from
    /// `media.play`).
    PlaybackStarted,
    /// Any other event string Plex might emit.
    Unknown(String),
}

impl WebhookEvent {
    fn from_wire(raw: &str) -> Self {
        match raw {
            "media.play" => Self::MediaPlay,
            "media.pause" => Self::MediaPause,
            "media.resume" => Self::MediaResume,
            "media.stop" => Self::MediaStop,
            "media.scrobble" => Self::MediaScrobble,
            "media.rate" => Self::MediaRate,
            "library.on.deck" => Self::LibraryOnDeck,
            "library.new" => Self::LibraryNew,
            "admin.database.backup" => Self::AdminDatabaseBackup,
            "admin.database.corrupted" => Self::AdminDatabaseCorrupted,
            "device.new" => Self::DeviceNew,
            "playback.started" => Self::PlaybackStarted,
            other => Self::Unknown(other.to_owned()),
        }
    }

    /// Wire spelling, matching what Plex sends.
    #[must_use]
    pub fn as_wire(&self) -> &str {
        match self {
            Self::MediaPlay => "media.play",
            Self::MediaPause => "media.pause",
            Self::MediaResume => "media.resume",
            Self::MediaStop => "media.stop",
            Self::MediaScrobble => "media.scrobble",
            Self::MediaRate => "media.rate",
            Self::LibraryOnDeck => "library.on.deck",
            Self::LibraryNew => "library.new",
            Self::AdminDatabaseBackup => "admin.database.backup",
            Self::AdminDatabaseCorrupted => "admin.database.corrupted",
            Self::DeviceNew => "device.new",
            Self::PlaybackStarted => "playback.started",
            Self::Unknown(s) => s.as_str(),
        }
    }
}

// -----------------------------------------------------------------------------
// Sub-payloads.
// -----------------------------------------------------------------------------

/// The Plex account that triggered the event.
#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct WebhookAccount {
    /// plex.tv account id.
    #[serde(default)]
    pub id: Option<u64>,
    /// Account avatar URL.
    #[serde(default)]
    pub thumb: Option<String>,
    /// Display name.
    #[serde(default)]
    pub title: Option<String>,
}

/// The Plex server that hosts the affected media.
#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct WebhookServer {
    /// Server friendly name.
    #[serde(default)]
    pub title: Option<String>,
    /// Server `machineIdentifier`.
    #[serde(default)]
    pub uuid: Option<String>,
}

/// The player device involved in the event.
#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct WebhookPlayer {
    /// `true` if the player is on the local network.
    #[serde(default)]
    pub local: Option<bool>,
    /// Public IP of the player.
    #[serde(default, rename = "publicAddress")]
    pub public_address: Option<String>,
    /// Player friendly name.
    #[serde(default)]
    pub title: Option<String>,
    /// Player `clientIdentifier`.
    #[serde(default)]
    pub uuid: Option<String>,
}

/// Minimal projection of the Plex `Metadata` element shipped with
/// the webhook. The full element is preserved in
/// [`Self::raw`] for callers that need fields beyond the projection.
#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct WebhookMetadata {
    /// `ratingKey` of the affected item (as a string — matches the wire).
    #[serde(default, rename = "ratingKey")]
    pub rating_key: Option<String>,
    /// Wire key (`/library/metadata/<rk>`).
    #[serde(default)]
    pub key: Option<String>,
    /// `guid` of the item.
    #[serde(default)]
    pub guid: Option<String>,
    /// Wire `type` (`movie`, `episode`, `track`, …).
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    /// Title (e.g. the movie / episode title).
    #[serde(default)]
    pub title: Option<String>,
    /// Show title for episodes.
    #[serde(default, rename = "grandparentTitle")]
    pub grandparent_title: Option<String>,
    /// Season title for episodes.
    #[serde(default, rename = "parentTitle")]
    pub parent_title: Option<String>,
    /// Library section id.
    #[serde(default, rename = "librarySectionID")]
    pub library_section_id: Option<i64>,
    /// Library section title.
    #[serde(default, rename = "librarySectionTitle")]
    pub library_section_title: Option<String>,
    /// Library section type (`movie`, `show`, `artist`, `photo`).
    #[serde(default, rename = "librarySectionType")]
    pub library_section_type: Option<String>,
    /// Full raw payload — use for fields not projected above.
    #[serde(flatten)]
    pub raw: serde_json::Value,
}

// -----------------------------------------------------------------------------
// WebhookPayload — the parsed JSON document.
// -----------------------------------------------------------------------------

/// One incoming Plex webhook event.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct WebhookPayload {
    /// What happened.
    pub event: WebhookEvent,
    /// `true` if a real user triggered the event (vs an
    /// admin-initiated background activity).
    pub user: bool,
    /// `true` if the account owns the affected server.
    pub owner: bool,
    /// User context.
    pub account: Option<WebhookAccount>,
    /// Server context.
    pub server: Option<WebhookServer>,
    /// Player context (absent for `library.*` / `admin.*` events).
    pub player: Option<WebhookPlayer>,
    /// Affected library item (absent for `admin.*` / `device.new`).
    pub metadata: Option<WebhookMetadata>,
    /// `media.rate` rating value (0..=10 scale).
    pub rating: Option<f32>,
    /// Raw JSON payload as Plex sent it. Useful when the typed
    /// projection drops a field a caller needs.
    pub raw: serde_json::Value,
    /// Bytes of the `thumb` form field if Plex included one.
    /// Always empty when the payload was constructed via
    /// [`Self::from_json`] (which has no multipart context).
    pub thumb_bytes: Option<Bytes>,
}

impl WebhookPayload {
    /// Decode a raw JSON payload string into a typed event.
    ///
    /// # Errors
    /// Returns [`Error::Json`] when the input is not valid JSON or
    /// the `event` field is missing.
    pub fn from_json(json: &str) -> Result<Self, Error> {
        let dto: WebhookPayloadDto = serde_json::from_str(json)?;
        Ok(Self::from_dto(dto, None))
    }

    fn from_dto(dto: WebhookPayloadDto, thumb_bytes: Option<Bytes>) -> Self {
        Self {
            event: WebhookEvent::from_wire(&dto.event),
            user: dto.user.unwrap_or(false),
            owner: dto.owner.unwrap_or(false),
            account: dto.account,
            server: dto.server,
            player: dto.player,
            metadata: dto.metadata,
            rating: dto.rating,
            raw: dto.raw,
            thumb_bytes,
        }
    }
}

#[derive(Debug, Deserialize)]
struct WebhookPayloadDto {
    event: String,
    #[serde(default)]
    user: Option<bool>,
    #[serde(default)]
    owner: Option<bool>,
    #[serde(default, rename = "Account")]
    account: Option<WebhookAccount>,
    #[serde(default, rename = "Server")]
    server: Option<WebhookServer>,
    #[serde(default, rename = "Player")]
    player: Option<WebhookPlayer>,
    #[serde(default, rename = "Metadata")]
    metadata: Option<WebhookMetadata>,
    #[serde(default)]
    rating: Option<f32>,
    /// Preserve the full payload for callers that need more fields.
    #[serde(flatten)]
    raw: serde_json::Value,
}

// -----------------------------------------------------------------------------
// Axum extractor.
// -----------------------------------------------------------------------------

/// Rejection returned by the [`WebhookPayload`] extractor when the
/// inbound request doesn't conform to Plex's webhook shape.
#[derive(Debug)]
#[non_exhaustive]
pub enum WebhookRejection {
    /// Request body wasn't a `multipart/form-data` POST.
    NotMultipart(String),
    /// The `payload` field was missing.
    MissingPayload,
    /// The `payload` field wasn't valid JSON in the expected shape.
    InvalidPayload(String),
    /// I/O error draining the multipart body.
    Io(String),
}

impl std::fmt::Display for WebhookRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotMultipart(m) => write!(f, "expected multipart/form-data: {m}"),
            Self::MissingPayload => write!(f, "missing `payload` form field"),
            Self::InvalidPayload(m) => write!(f, "invalid `payload` JSON: {m}"),
            Self::Io(m) => write!(f, "multipart body read error: {m}"),
        }
    }
}

impl std::error::Error for WebhookRejection {}

impl IntoResponse for WebhookRejection {
    fn into_response(self) -> Response {
        (StatusCode::BAD_REQUEST, self.to_string()).into_response()
    }
}

#[axum::async_trait]
impl<S: Send + Sync> FromRequest<S> for WebhookPayload {
    type Rejection = WebhookRejection;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let mut multipart = Multipart::from_request(req, state)
            .await
            .map_err(|e| WebhookRejection::NotMultipart(e.to_string()))?;
        let mut payload_json: Option<String> = None;
        let mut thumb_bytes: Option<Bytes> = None;
        while let Some(field) = multipart
            .next_field()
            .await
            .map_err(|e| WebhookRejection::Io(e.to_string()))?
        {
            match field.name().unwrap_or_default() {
                "payload" => {
                    let text = field
                        .text()
                        .await
                        .map_err(|e| WebhookRejection::Io(e.to_string()))?;
                    payload_json = Some(text);
                }
                "thumb" => {
                    let bytes = field
                        .bytes()
                        .await
                        .map_err(|e| WebhookRejection::Io(e.to_string()))?;
                    thumb_bytes = Some(bytes);
                }
                _ => {
                    // Drain unknown fields so the parser advances.
                    let _ = field
                        .bytes()
                        .await
                        .map_err(|e| WebhookRejection::Io(e.to_string()))?;
                }
            }
        }
        let json = payload_json.ok_or(WebhookRejection::MissingPayload)?;
        let dto: WebhookPayloadDto = serde_json::from_str(&json)
            .map_err(|e| WebhookRejection::InvalidPayload(e.to_string()))?;
        Ok(Self::from_dto(dto, thumb_bytes))
    }
}

// -----------------------------------------------------------------------------
// Tests.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_payload(event: &str) -> serde_json::Value {
        serde_json::json!({
            "event": event,
            "user": true,
            "owner": true,
            "Account": {"id": 1, "thumb": "http://t", "title": "alice"},
            "Server": {"title": "Living Room", "uuid": "pms-machine-id"},
            "Player": {
                "local": true,
                "publicAddress": "1.2.3.4",
                "title": "Apple TV",
                "uuid": "player-id"
            },
            "Metadata": {
                "ratingKey": "42",
                "key": "/library/metadata/42",
                "type": "movie",
                "title": "Arrival",
                "librarySectionID": 1,
                "librarySectionTitle": "Movies",
                "librarySectionType": "movie"
            }
        })
    }

    #[test]
    fn webhook_event_from_wire_handles_all_documented_kinds() {
        let cases = [
            ("media.play", WebhookEvent::MediaPlay),
            ("media.pause", WebhookEvent::MediaPause),
            ("media.resume", WebhookEvent::MediaResume),
            ("media.stop", WebhookEvent::MediaStop),
            ("media.scrobble", WebhookEvent::MediaScrobble),
            ("media.rate", WebhookEvent::MediaRate),
            ("library.on.deck", WebhookEvent::LibraryOnDeck),
            ("library.new", WebhookEvent::LibraryNew),
            ("admin.database.backup", WebhookEvent::AdminDatabaseBackup),
            (
                "admin.database.corrupted",
                WebhookEvent::AdminDatabaseCorrupted,
            ),
            ("device.new", WebhookEvent::DeviceNew),
            ("playback.started", WebhookEvent::PlaybackStarted),
        ];
        for (wire, expected) in cases {
            let got = WebhookEvent::from_wire(wire);
            assert_eq!(got, expected, "wire={wire}");
            assert_eq!(got.as_wire(), wire);
        }
    }

    #[test]
    fn webhook_event_unknown_passthrough_round_trip() {
        let ev = WebhookEvent::from_wire("future.event");
        match &ev {
            WebhookEvent::Unknown(s) => assert_eq!(s, "future.event"),
            other => panic!("expected Unknown, got {other:?}"),
        }
        assert_eq!(ev.as_wire(), "future.event");
    }

    #[test]
    fn from_json_parses_media_play() {
        let body = sample_payload("media.play").to_string();
        let p = WebhookPayload::from_json(&body).unwrap();
        assert_eq!(p.event, WebhookEvent::MediaPlay);
        assert!(p.user);
        assert!(p.owner);
        assert_eq!(p.account.unwrap().title.as_deref(), Some("alice"));
        assert_eq!(p.server.unwrap().uuid.as_deref(), Some("pms-machine-id"));
        assert_eq!(p.player.unwrap().title.as_deref(), Some("Apple TV"));
        let m = p.metadata.unwrap();
        assert_eq!(m.rating_key.as_deref(), Some("42"));
        assert_eq!(m.title.as_deref(), Some("Arrival"));
        assert_eq!(m.library_section_type.as_deref(), Some("movie"));
        assert!(p.thumb_bytes.is_none());
    }

    #[test]
    fn from_json_parses_media_rate_with_rating() {
        let mut body = sample_payload("media.rate");
        body["rating"] = serde_json::json!(8.5);
        let p = WebhookPayload::from_json(&body.to_string()).unwrap();
        assert_eq!(p.event, WebhookEvent::MediaRate);
        assert_eq!(p.rating, Some(8.5));
    }

    #[test]
    fn from_json_parses_admin_event_without_metadata() {
        let body = serde_json::json!({
            "event": "admin.database.backup",
            "user": false,
            "owner": true,
            "Server": {"title": "PMS", "uuid": "abc"}
        });
        let p = WebhookPayload::from_json(&body.to_string()).unwrap();
        assert_eq!(p.event, WebhookEvent::AdminDatabaseBackup);
        assert!(!p.user);
        assert!(p.owner);
        assert!(p.metadata.is_none());
        assert!(p.player.is_none());
    }

    #[test]
    fn from_json_preserves_raw_payload() {
        let body = serde_json::json!({
            "event": "media.play",
            "customExtraField": "preserved"
        });
        let p = WebhookPayload::from_json(&body.to_string()).unwrap();
        assert_eq!(
            p.raw.get("customExtraField").and_then(|v| v.as_str()),
            Some("preserved"),
        );
    }

    #[test]
    fn from_json_rejects_missing_event() {
        let body = r#"{"user":true}"#;
        let err = WebhookPayload::from_json(body).unwrap_err();
        assert!(matches!(err, Error::Json(_)));
    }

    #[test]
    fn from_json_rejects_invalid_json() {
        let err = WebhookPayload::from_json("not json").unwrap_err();
        assert!(matches!(err, Error::Json(_)));
    }

    #[test]
    fn from_json_defaults_user_owner_to_false_when_absent() {
        let body = serde_json::json!({"event": "media.play"});
        let p = WebhookPayload::from_json(&body.to_string()).unwrap();
        assert!(!p.user);
        assert!(!p.owner);
    }
}
