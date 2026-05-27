//! Current-playback sessions surface.
//!
//! Plex exposes the "now playing" list at `GET /status/sessions`.
//! Each session entry is a metadata element (`<Video>` / `<Track>` /
//! `<Photo>`) augmented with the session key, view offset, plus
//! nested `<User>`, `<Player>`, and optional `<TranscodeSession>`
//! children describing who is watching, on what device, and whether
//! the server is currently transcoding for them.
//!
//! Endpoint surface (M4.3 implements *italicised*):
//!
//! - `GET /status/sessions` — *list*
//! - `DELETE /status/sessions/terminate?sessionId=…&reason=…` — *stop*
//! - `GET /transcode/sessions` — defer (transcode-only listing)
//! - `GET /status/sessions/history/all` — defer (paginated history)

use serde::Deserialize;
use url::Url;

use crate::client::HttpClient;
use crate::error::{Error, Result};
use crate::library::LibrarySectionRef;
use crate::media::LibraryItem;
use crate::media::video::MetadataDto;
use crate::xml::MediaContainer;

// -----------------------------------------------------------------------------
// Domain types.
// -----------------------------------------------------------------------------

/// The state a session's player reports.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PlayState {
    /// Actively decoding and rendering frames.
    Playing,
    /// User paused playback.
    Paused,
    /// Buffering (network or decoder).
    Buffering,
    /// Player stopped (rare on `/status/sessions` — usually drops off
    /// the list when stopped).
    Stopped,
    /// Forward-compat for wire values Plex adds later.
    Other(String),
}

impl PlayState {
    /// Map Plex's wire `state` attribute to a typed value.
    #[must_use]
    pub fn from_wire(s: &str) -> Self {
        match s {
            "playing" => Self::Playing,
            "paused" => Self::Paused,
            "buffering" => Self::Buffering,
            "stopped" => Self::Stopped,
            other => Self::Other(other.to_owned()),
        }
    }
}

/// The user a session is associated with.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SessionUser {
    /// plex.tv account identifier (`0` for the local owner).
    pub id: u64,
    /// Display name.
    pub title: Option<String>,
    /// Avatar URL.
    pub thumb: Option<String>,
}

/// The player a session is being delivered to.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SessionPlayer {
    /// LAN IP address.
    pub address: Option<String>,
    /// Public IP address (relay / remote-access sessions).
    pub remote_public_address: Option<String>,
    /// User-friendly device name.
    pub device: Option<String>,
    /// Stable client identifier.
    pub machine_identifier: Option<String>,
    /// Hardware model (`AppleTV5,3`, `K1`, …).
    pub model: Option<String>,
    /// Plex client product name (`Plex for Roku`, …).
    pub product: Option<String>,
    /// Playback state.
    pub state: PlayState,
    /// Title the player advertises (e.g. user-set name).
    pub title: Option<String>,
    /// Player platform (`Roku`, `iOS`, …).
    pub platform: Option<String>,
    /// Platform version string.
    pub platform_version: Option<String>,
    /// Client product version.
    pub version: Option<String>,
    /// Whether the player is on the local network.
    pub local: bool,
    /// Whether the player is reachable from the server (some
    /// clients accept reverse-direction control, some don't).
    pub controllable: bool,
}

/// One transcode session running on the server.
///
/// M4.3 captures the most-used scalar fields; the full
/// `TranscodeSession` element has ~30 attributes and lands in a
/// follow-up alongside the dedicated transcode surface.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct TranscodeSession {
    /// Transcode session key (matches the parent session key).
    pub key: String,
    /// Whether throttled — server is keeping pace with the client.
    pub throttled: bool,
    /// Progress percentage (0..=100).
    pub progress: Option<f32>,
    /// Duration in milliseconds of the source media.
    pub duration_ms: Option<u64>,
    /// Estimated remaining transcode time in milliseconds.
    pub remaining_ms: Option<u64>,
    /// Speed factor (1.0 = real-time).
    pub speed: Option<f32>,
    /// Source video codec.
    pub source_video_codec: Option<String>,
    /// Source audio codec.
    pub source_audio_codec: Option<String>,
    /// Target video codec.
    pub video_codec: Option<String>,
    /// Target audio codec.
    pub audio_codec: Option<String>,
    /// Target container format.
    pub container: Option<String>,
    /// Hardware-accelerated decode/encode flag.
    pub transcode_hw_requested: bool,
}

/// One currently-playing session.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PlayingSession {
    /// Unique session identifier — passed to `stop()`.
    pub session_key: String,
    /// The media item being played.
    pub item: LibraryItem,
    /// View offset in milliseconds (current playback position).
    pub view_offset_ms: Option<u64>,
    /// User context.
    pub user: SessionUser,
    /// Player context.
    pub player: SessionPlayer,
    /// Transcode context (only present when the server is
    /// transcoding for this session).
    pub transcode: Option<TranscodeSession>,
    http: HttpClient,
    base_url: Url,
}

impl PlayingSession {
    /// Terminate the session server-side.
    ///
    /// Issues `GET /status/sessions/terminate?sessionId=<key>&reason=<text>`.
    /// `reason` is shown to the client as the disconnect message.
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn stop(&self, reason: Option<&str>) -> Result<()> {
        let reason_q = reason.map_or(String::new(), |r| format!("&reason={}", pct_query(r)));
        let path = format!(
            "/status/sessions/terminate?sessionId={}{}",
            self.session_key, reason_q,
        );
        let url = self.base_url.join(&path)?;
        let _ = self.http.get_bytes(url.as_str()).await?;
        Ok(())
    }
}

/// Minimal RFC 3986 percent-encoder for the `reason` query value.
fn pct_query(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                out.push('%');
                let hi = byte >> 4;
                let lo = byte & 0x0F;
                out.push(if hi < 10 {
                    (b'0' + hi) as char
                } else {
                    (b'A' + hi - 10) as char
                });
                out.push(if lo < 10 {
                    (b'0' + lo) as char
                } else {
                    (b'A' + lo - 10) as char
                });
            }
        }
    }
    out
}

// -----------------------------------------------------------------------------
// DTO.
// -----------------------------------------------------------------------------

/// Top-level `<Video>` / `<Track>` / `<Photo>` element augmented with
/// session-only fields, plus the nested `<User>` / `<Player>` /
/// `<TranscodeSession>` children.
///
/// We deserialise via `serde_json::Value` so the same DTO can extract
/// both the standard metadata shape (handed to [`MetadataDto`]) and
/// the session-specific sub-elements.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionItemDto {
    #[serde(rename = "sessionKey")]
    pub(crate) session_key: Option<String>,
    #[serde(rename = "viewOffset", default)]
    pub(crate) view_offset: Option<u64>,
    #[serde(rename = "User", default)]
    pub(crate) user: Option<UserDto>,
    #[serde(rename = "Player", default)]
    pub(crate) player: Option<PlayerDto>,
    #[serde(rename = "TranscodeSession", default)]
    pub(crate) transcode: Option<TranscodeDto>,
    /// All other fields flatten into the standard metadata DTO.
    #[serde(flatten)]
    pub(crate) metadata: MetadataDto,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UserDto {
    #[serde(default)]
    pub(crate) id: Option<serde_json::Value>,
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) thumb: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlayerDto {
    #[serde(default)]
    pub(crate) address: Option<String>,
    #[serde(default)]
    pub(crate) remote_public_address: Option<String>,
    #[serde(default)]
    pub(crate) device: Option<String>,
    #[serde(default)]
    pub(crate) machine_identifier: Option<String>,
    #[serde(default)]
    pub(crate) model: Option<String>,
    #[serde(default)]
    pub(crate) product: Option<String>,
    #[serde(default)]
    pub(crate) state: Option<String>,
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) platform: Option<String>,
    #[serde(default)]
    pub(crate) platform_version: Option<String>,
    #[serde(default)]
    pub(crate) version: Option<String>,
    #[serde(default)]
    pub(crate) local: Option<crate::server::PlexBoolField>,
    #[serde(default)]
    pub(crate) controllable: Option<crate::server::PlexBoolField>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranscodeDto {
    pub(crate) key: String,
    #[serde(default)]
    pub(crate) throttled: Option<crate::server::PlexBoolField>,
    #[serde(default)]
    pub(crate) progress: Option<f32>,
    #[serde(default)]
    pub(crate) duration: Option<u64>,
    #[serde(default)]
    pub(crate) remaining: Option<u64>,
    #[serde(default)]
    pub(crate) speed: Option<f32>,
    #[serde(default)]
    pub(crate) source_video_codec: Option<String>,
    #[serde(default)]
    pub(crate) source_audio_codec: Option<String>,
    #[serde(default)]
    pub(crate) video_codec: Option<String>,
    #[serde(default)]
    pub(crate) audio_codec: Option<String>,
    #[serde(default)]
    pub(crate) container: Option<String>,
    #[serde(default)]
    pub(crate) transcode_hw_requested: Option<crate::server::PlexBoolField>,
}

impl SessionItemDto {
    pub(crate) fn into_domain(self, http: HttpClient, base_url: Url) -> Result<PlayingSession> {
        let session_key = self
            .session_key
            .ok_or_else(|| Error::Config("session entry missing sessionKey".to_owned()))?;
        let view_offset_ms = self.view_offset;

        // Build a synthetic section-ref using the embedded
        // librarySectionID (Plex includes it on session entries).
        let section_id = self.metadata.library_section_id_for_playlist().unwrap_or(0);
        let section_ref = LibrarySectionRef {
            id: section_id,
            http: http.clone(),
            base_url: base_url.clone(),
        };
        let item = self.metadata.into_library_item(section_ref)?;

        let user = self.user.map_or(
            SessionUser {
                id: 0,
                title: None,
                thumb: None,
            },
            |u| SessionUser {
                id: parse_user_id(u.id.as_ref()),
                title: u.title,
                thumb: u.thumb,
            },
        );

        let player = self.player.map_or(
            SessionPlayer {
                address: None,
                remote_public_address: None,
                device: None,
                machine_identifier: None,
                model: None,
                product: None,
                state: PlayState::Other(String::new()),
                title: None,
                platform: None,
                platform_version: None,
                version: None,
                local: false,
                controllable: false,
            },
            |p| SessionPlayer {
                state: PlayState::from_wire(p.state.as_deref().unwrap_or("")),
                address: p.address,
                remote_public_address: p.remote_public_address,
                device: p.device,
                machine_identifier: p.machine_identifier,
                model: p.model,
                product: p.product,
                title: p.title,
                platform: p.platform,
                platform_version: p.platform_version,
                version: p.version,
                local: p.local.is_some_and(|b| b.to_bool()),
                controllable: p.controllable.is_some_and(|b| b.to_bool()),
            },
        );

        let transcode = self.transcode.map(|t| TranscodeSession {
            key: t.key,
            throttled: t.throttled.is_some_and(|b| b.to_bool()),
            progress: t.progress,
            duration_ms: t.duration,
            remaining_ms: t.remaining,
            speed: t.speed,
            source_video_codec: t.source_video_codec,
            source_audio_codec: t.source_audio_codec,
            video_codec: t.video_codec,
            audio_codec: t.audio_codec,
            container: t.container,
            transcode_hw_requested: t.transcode_hw_requested.is_some_and(|b| b.to_bool()),
        });

        Ok(PlayingSession {
            session_key,
            item,
            view_offset_ms,
            user,
            player,
            transcode,
            http,
            base_url,
        })
    }
}

fn parse_user_id(v: Option<&serde_json::Value>) -> u64 {
    match v {
        Some(serde_json::Value::Number(n)) => n.as_u64().unwrap_or(0),
        Some(serde_json::Value::String(s)) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

// -----------------------------------------------------------------------------
// PlexServer integration.
// -----------------------------------------------------------------------------

impl crate::server::PlexServer {
    /// List currently-playing sessions on this server.
    ///
    /// Calls `GET /status/sessions`. Returns one [`PlayingSession`]
    /// per active session — each carries the played item, user,
    /// player, and optional transcode info.
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn sessions(&self) -> Result<Vec<PlayingSession>> {
        let url = self.base_url().join("/status/sessions")?;
        let body = self.http().get_bytes(url.as_str()).await?;
        let body_str = std::str::from_utf8(&body)
            .map_err(|e| Error::Config(format!("/status/sessions body not utf-8: {e}")))?;
        let mc: MediaContainer<SessionItemDto> = MediaContainer::from_json(body_str, "Metadata")?;
        mc.items
            .into_iter()
            .map(|dto| dto.into_domain(self.http().clone(), self.base_url().clone()))
            .collect()
    }
}
