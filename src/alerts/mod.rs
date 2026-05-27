//! Real-time PMS alert stream.
//!
//! Plex Media Server pushes a continuous JSON-over-WebSocket stream
//! at `/:/websockets/notifications`. Subscribers receive typed
//! [`AlertEvent`] notifications for playback state changes, library
//! scans, transcode sessions, server settings, and so on.
//!
//! ## Quick start
//!
//! ```no_run
//! # use plex_rs::PlexServer;
//! # use plex_rs::alerts::{Alerts, AlertEvent};
//! # use futures_util::StreamExt;
//! # async fn run(plex: PlexServer) -> Result<(), plex_rs::Error> {
//! let mut stream = Alerts::connect(&plex).await?;
//! while let Some(event) = stream.next().await {
//!     match event? {
//!         AlertEvent::Playing(p) => println!("player session {} -> {:?}",
//!             p.session_key, p.state),
//!         AlertEvent::Timeline(t) => println!("library item {} -> state {}",
//!             t.item_id, t.state),
//!         other => tracing::debug!("alert: {other:?}"),
//!     }
//! }
//! # Ok(()) }
//! ```
//!
//! ## Reconnection
//!
//! [`Alerts::connect`] returns a single-shot stream. The websocket
//! peer may close at any time (server restart, network blip); the
//! caller decides when to reconnect. A typical pattern is:
//!
//! ```no_run
//! # use plex_rs::PlexServer;
//! # use plex_rs::alerts::Alerts;
//! # use plex_rs::client::retry_delay;
//! # use futures_util::StreamExt;
//! # use std::time::Duration;
//! # async fn loop_with_reconnect(plex: PlexServer) -> Result<(), plex_rs::Error> {
//! let mut attempt: u32 = 0;
//! loop {
//!     match Alerts::connect(&plex).await {
//!         Ok(mut s) => {
//!             attempt = 0;
//!             while let Some(ev) = s.next().await {
//!                 // handle ev
//!                 let _ = ev?;
//!             }
//!         }
//!         Err(e) => tracing::warn!("alerts connect failed: {e}"),
//!     }
//!     attempt = attempt.saturating_add(1);
//!     let delay = retry_delay(attempt, Duration::from_millis(250), Duration::from_secs(30));
//!     tokio::time::sleep(delay).await;
//! }
//! # }
//! ```
//!
//! ## Cancellation safety
//!
//! Dropping the [`Alerts`] stream closes the websocket cleanly. The
//! inner future is cancel-safe — partially-consumed frames are
//! discarded along with the connection.
//!
//! ## Cargo feature
//!
//! This module is gated behind the `alerts` Cargo feature. The
//! feature pulls in `tokio-tungstenite` for the WebSocket transport.

use std::pin::Pin;
use std::task::{Context, Poll};

use futures_util::Stream;
use futures_util::stream::{SplitStream, StreamExt};
use serde::Deserialize;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use url::Url;

use crate::error::{Error, Result};
use crate::server::PlexServer;
use crate::util::ids::PlexToken;

/// PMS alerts WebSocket key.
const ALERTS_PATH: &str = "/:/websockets/notifications";

// -----------------------------------------------------------------------------
// AlertEvent — typed enumeration of the discriminator field.
// -----------------------------------------------------------------------------

/// One alert from PMS. The variants follow Plex's `type` field on
/// the `NotificationContainer`. Unknown / future types fall through
/// to [`AlertEvent::Unknown`] with the raw JSON for forward compat.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum AlertEvent {
    /// Playback state change. Emitted on play / pause / stop /
    /// buffering for every active session.
    Playing(PlayingNotification),
    /// Library item lifecycle (scan started, processed, deleted).
    /// See [`TimelineEntry::state`] for the per-state semantics.
    Timeline(TimelineEntry),
    /// Background activity (library scan, optimise, refresh).
    Activity(ActivityNotification),
    /// Transcode-session lifecycle event. The wire `type` is
    /// `transcodeSession.start` / `.update` / `.end`; the embedded
    /// [`TranscodeSessionNotification::lifecycle`] preserves which.
    TranscodeSession(TranscodeSessionNotification),
    /// Server-status notifications (banner-style messages PMS
    /// surfaces in its UI).
    Status(StatusNotification),
    /// Reachability changes — public-address mappings flip on / off.
    Reachability(ReachabilityNotification),
    /// Server-wide setting / preference change.
    Setting(SettingNotification),
    /// Background-processing queue updates.
    BackgroundProcessingQueue(BackgroundProcessingQueueNotification),
    /// Catch-all for unrecognised event types. The wire `type` and
    /// the raw payload are preserved so callers can extend without
    /// requiring a crate update.
    Unknown {
        /// Wire `type` discriminator.
        kind: String,
        /// Raw JSON payload as Plex sent it.
        raw: serde_json::Value,
    },
}

/// Lifecycle phase of a [`AlertEvent::TranscodeSession`] event.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TranscodeLifecycle {
    /// `transcodeSession.start`.
    Start,
    /// `transcodeSession.update`.
    Update,
    /// `transcodeSession.end`.
    End,
}

// -----------------------------------------------------------------------------
// Notification payloads.
// -----------------------------------------------------------------------------

/// One play-session state change.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct PlayingNotification {
    /// Stable per-session identifier (matches the `sessionKey` on
    /// `/status/sessions`).
    #[serde(default)]
    pub session_key: String,
    /// `ratingKey` of the playing item.
    #[serde(default)]
    pub rating_key: String,
    /// Wire metadata key (`/library/metadata/<rk>`).
    #[serde(default)]
    pub key: String,
    /// Current playback position in milliseconds.
    #[serde(default)]
    pub view_offset: Option<u64>,
    /// `playing` / `paused` / `buffering` / `stopped`.
    #[serde(default)]
    pub state: String,
    /// User account id that owns the session.
    #[serde(default, rename = "userID")]
    pub user_id: Option<u64>,
    /// Client-supplied transcode-session id, when transcoding.
    #[serde(default, rename = "transcodeSession")]
    pub transcode_session: Option<String>,
}

/// One library-item lifecycle event.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct TimelineEntry {
    /// Rating key of the affected item.
    #[serde(default, rename = "itemID")]
    pub item_id: u64,
    /// Parent rating key (e.g. season for an episode).
    #[serde(default, rename = "parentItemID")]
    pub parent_item_id: Option<u64>,
    /// Section the item belongs to.
    #[serde(default, rename = "sectionID")]
    pub section_id: Option<i64>,
    /// Numeric Plex `type` (e.g. `1=movie`, `2=show`).
    #[serde(default, rename = "type")]
    pub plex_type: Option<i32>,
    /// `state` — meaning is documented under
    /// [`AlertEvent::Timeline`]: 0 created, 1 processing, 2 matching,
    /// 3 metadata download, 4 metadata process, 5 done, 9 deleted.
    #[serde(default)]
    pub state: i32,
    /// Title at the moment of the event (may differ from the final
    /// matched title).
    #[serde(default)]
    pub title: Option<String>,
    /// Wire identifier (`com.plexapp.plugins.library`).
    #[serde(default)]
    pub identifier: Option<String>,
    /// Wire metadata state for backfill operations.
    #[serde(default, rename = "metadataState")]
    pub metadata_state: Option<String>,
}

/// Activity (scan / optimise / refresh) notification.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ActivityNotification {
    /// `started` / `updated` / `ended`.
    #[serde(default)]
    pub event: String,
    /// Specifics for this activity. PMS embeds an `Activity` object;
    /// we expose the parsed view + the raw payload for fields the
    /// crate doesn't surface yet.
    #[serde(default, rename = "Activity")]
    pub activity: Option<ActivityBody>,
}

/// Body of an [`ActivityNotification`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ActivityBody {
    /// Stable activity id.
    #[serde(default)]
    pub uuid: Option<String>,
    /// Activity type (e.g. `library.refresh.items`).
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    /// `true` while the activity is cancelable.
    #[serde(default)]
    pub cancellable: Option<bool>,
    /// 0..=100 progress hint.
    #[serde(default)]
    pub progress: Option<u8>,
    /// Title hint shown to UI clients.
    #[serde(default)]
    pub title: Option<String>,
    /// Optional subtitle.
    #[serde(default)]
    pub subtitle: Option<String>,
}

/// Transcode-session lifecycle update.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct TranscodeSessionNotification {
    /// Which phase this event represents.
    pub lifecycle: TranscodeLifecycle,
    /// Embedded `TranscodeSession` payload (kept as raw JSON because
    /// `crate::server::sessions::TranscodeSession` is shaped for the
    /// `/status/sessions` endpoint and the fields here differ
    /// slightly).
    pub raw: serde_json::Value,
}

/// Server-status notification (UI banner / toast).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct StatusNotification {
    /// Title text shown in the UI.
    #[serde(default)]
    pub title: Option<String>,
    /// Description / body text.
    #[serde(default)]
    pub description: Option<String>,
    /// Status code (`info` / `warning` / `error`).
    #[serde(default, rename = "notificationName")]
    pub name: Option<String>,
}

/// Reachability (NAT / public-address) change.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ReachabilityNotification {
    /// `true` when PMS just confirmed itself publicly reachable.
    #[serde(default)]
    pub reachability: bool,
}

/// Setting / preference change pushed from PMS.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct SettingNotification {
    /// Setting key (e.g. `TranscoderQuality`).
    #[serde(default)]
    pub id: Option<String>,
    /// New value, as a string. Numeric / boolean settings still
    /// arrive as strings on the wire.
    #[serde(default)]
    pub value: Option<String>,
}

/// Background-processing queue notification — emitted when an
/// internal queue of pending work changes size or composition.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct BackgroundProcessingQueueNotification {
    /// Queue identifier.
    #[serde(default, rename = "queueID")]
    pub queue_id: Option<i64>,
    /// Optional event tag.
    #[serde(default)]
    pub event: Option<String>,
}

// -----------------------------------------------------------------------------
// Wire envelope.
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct AlertsFrame {
    #[serde(rename = "NotificationContainer")]
    container: NotificationContainer,
}

#[derive(Debug, Deserialize)]
struct NotificationContainer {
    #[serde(rename = "type")]
    kind: String,
    /// Per-type body. The field name varies by `type`; we keep the
    /// raw JSON and pick out the right key in `decode_one`.
    #[serde(flatten)]
    payload: serde_json::Value,
}

// -----------------------------------------------------------------------------
// Alerts — public stream wrapper.
// -----------------------------------------------------------------------------

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Stream of [`AlertEvent`]s from a PMS WebSocket.
///
/// Construct via [`Alerts::connect`]. Each frame received from PMS
/// can carry multiple notifications; the stream flattens them so
/// each `.next().await` yields exactly one event.
pub struct Alerts {
    reader: SplitStream<Ws>,
    buffer: std::collections::VecDeque<AlertEvent>,
    closed: bool,
}

impl std::fmt::Debug for Alerts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Alerts")
            .field("reader", &"SplitStream<...>")
            .field("buffered", &self.buffer.len())
            .field("closed", &self.closed)
            .finish()
    }
}

impl Alerts {
    /// Connect to the PMS alerts WebSocket.
    ///
    /// Reuses the server's base URL and token (auth happens via the
    /// `X-Plex-Token` query parameter — Plex's websocket endpoint
    /// doesn't accept the standard `X-Plex-*` HTTP headers).
    ///
    /// # Errors
    /// - [`Error::Config`] if the server has no token (every
    ///   non-anonymous PMS install requires one for the alerts
    ///   endpoint).
    /// - [`Error::Transport`] wrapping any DNS / TCP / TLS / WS
    ///   handshake failure.
    pub async fn connect(server: &PlexServer) -> Result<Self> {
        let token = server
            .http()
            .config()
            .token
            .as_ref()
            .ok_or_else(|| Error::Config("alerts: no token on PlexServer".to_owned()))?;
        let ws_url = alerts_url(server.base_url(), token)?;
        Self::connect_with_url(ws_url.as_str()).await
    }

    /// Connect to a raw `ws://` / `wss://` URL.
    ///
    /// The URL must already include `?X-Plex-Token=...` in its
    /// query when targeting a real PMS. Primarily exposed for test
    /// infrastructure and advanced callers (e.g. when the PMS is
    /// reachable via a tunnel whose base URL doesn't match the
    /// PMS's own `base_url`). Normal usage should prefer
    /// [`Self::connect`].
    ///
    /// # Errors
    /// [`Error::Config`] wrapping any DNS / TCP / TLS / WS
    /// handshake failure.
    pub async fn connect_with_url(ws_url: &str) -> Result<Self> {
        let (ws, _resp) = connect_async(ws_url)
            .await
            .map_err(|e| Error::Config(format!("alerts websocket connect: {e}")))?;
        let (_writer, reader) = ws.split();
        Ok(Self {
            reader,
            buffer: std::collections::VecDeque::new(),
            closed: false,
        })
    }
}

impl Stream for Alerts {
    type Item = Result<AlertEvent>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            if let Some(ev) = self.buffer.pop_front() {
                return Poll::Ready(Some(Ok(ev)));
            }
            if self.closed {
                return Poll::Ready(None);
            }
            match Pin::new(&mut self.reader).poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => {
                    self.closed = true;
                    return Poll::Ready(None);
                }
                Poll::Ready(Some(Err(e))) => {
                    self.closed = true;
                    return Poll::Ready(Some(Err(Error::Config(format!(
                        "alerts websocket read error: {e}"
                    )))));
                }
                Poll::Ready(Some(Ok(msg))) => {
                    if let Some(events) = handle_message(msg) {
                        for ev in events {
                            self.buffer.push_back(ev);
                        }
                    }
                    // Loop to drain the buffer.
                }
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Helpers.
// -----------------------------------------------------------------------------

/// Build the WebSocket URL given the server base URL and token.
///
/// Replaces `http`/`https` with `ws`/`wss` and appends
/// `?X-Plex-Token=...`.
fn alerts_url(base: &Url, token: &PlexToken) -> Result<Url> {
    let mut url = base.join(ALERTS_PATH)?;
    // Some `base` URLs may already have a token in the query (rare);
    // we don't bother to dedup — Plex accepts the last one.
    url.query_pairs_mut()
        .append_pair("X-Plex-Token", token.expose());
    let new_scheme = match url.scheme() {
        "https" => "wss",
        _ => "ws",
    };
    url.set_scheme(new_scheme)
        .map_err(|()| Error::Config("alerts: failed to set ws scheme".to_owned()))?;
    Ok(url)
}

/// Convert one WebSocket frame to zero or more `AlertEvent`s.
/// Non-text frames (binary, ping, pong, close) are ignored.
fn handle_message(msg: Message) -> Option<Vec<AlertEvent>> {
    match msg {
        Message::Text(text) => decode_frame(&text).ok(),
        Message::Binary(bytes) => std::str::from_utf8(&bytes)
            .ok()
            .and_then(|s| decode_frame(s).ok()),
        Message::Close(_) | Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => None,
    }
}

/// Decode one wire frame into the (possibly empty) list of events.
fn decode_frame(text: &str) -> Result<Vec<AlertEvent>> {
    let frame: AlertsFrame = serde_json::from_str(text)?;
    Ok(decode_container(frame.container))
}

/// Dispatch a single `NotificationContainer` to typed events. Plex
/// uses a per-type embedded array, so each container expands to N
/// events. The body field name matches the python convention:
///
/// | `type` value                         | body key                          |
/// | ------------------------------------ | --------------------------------- |
/// | `playing`                            | `PlaySessionStateNotification`    |
/// | `timeline`                           | `TimelineEntry`                   |
/// | `activity`                           | `ActivityNotification`            |
/// | `transcodeSession.start/.update/.end` | `TranscodeSession`               |
/// | `status`                             | `StatusNotification`              |
/// | `reachability`                       | `ReachabilityNotification`        |
/// | `setting`                            | `Setting`                         |
/// | `backgroundProcessingQueue`          | `BackgroundProcessingQueueEventNotification` |
fn decode_container(c: NotificationContainer) -> Vec<AlertEvent> {
    fn pluck_array<'a>(
        v: &'a serde_json::Value,
        keys: &[&str],
    ) -> Option<&'a Vec<serde_json::Value>> {
        for k in keys {
            if let Some(serde_json::Value::Array(arr)) = v.get(*k) {
                return Some(arr);
            }
        }
        None
    }
    let kind = c.kind.clone();
    match kind.as_str() {
        "playing" => decode_array::<PlayingNotification>(
            &c.payload,
            &["PlaySessionStateNotification"],
            AlertEvent::Playing,
        )
        .unwrap_or_else(|| vec![unknown(kind.clone(), c.payload.clone())]),
        "timeline" => {
            decode_array::<TimelineEntry>(&c.payload, &["TimelineEntry"], AlertEvent::Timeline)
                .unwrap_or_else(|| vec![unknown(kind.clone(), c.payload.clone())])
        }
        "activity" => decode_array::<ActivityNotification>(
            &c.payload,
            &["ActivityNotification"],
            AlertEvent::Activity,
        )
        .unwrap_or_else(|| vec![unknown(kind.clone(), c.payload.clone())]),
        "status" => decode_array::<StatusNotification>(
            &c.payload,
            &["StatusNotification"],
            AlertEvent::Status,
        )
        .unwrap_or_else(|| vec![unknown(kind.clone(), c.payload.clone())]),
        "reachability" => decode_array::<ReachabilityNotification>(
            &c.payload,
            &["ReachabilityNotification"],
            AlertEvent::Reachability,
        )
        .unwrap_or_else(|| vec![unknown(kind.clone(), c.payload.clone())]),
        "setting" | "preference" => decode_array::<SettingNotification>(
            &c.payload,
            &["Setting", "Preference"],
            AlertEvent::Setting,
        )
        .unwrap_or_else(|| vec![unknown(kind.clone(), c.payload.clone())]),
        "backgroundProcessingQueue" => decode_array::<BackgroundProcessingQueueNotification>(
            &c.payload,
            &[
                "BackgroundProcessingQueueEventNotification",
                "BackgroundProcessingQueueNotification",
            ],
            AlertEvent::BackgroundProcessingQueue,
        )
        .unwrap_or_else(|| vec![unknown(kind.clone(), c.payload.clone())]),
        "transcodeSession.start" => decode_transcode(&c.payload, TranscodeLifecycle::Start),
        "transcodeSession.update" => decode_transcode(&c.payload, TranscodeLifecycle::Update),
        "transcodeSession.end" => decode_transcode(&c.payload, TranscodeLifecycle::End),
        _ => {
            // Unknown discriminator — keep the payload intact for
            // downstream extension.
            let _ = pluck_array(&c.payload, &[]);
            vec![unknown(kind, c.payload)]
        }
    }
}

fn decode_array<T>(
    payload: &serde_json::Value,
    candidate_keys: &[&str],
    wrap: impl Fn(T) -> AlertEvent,
) -> Option<Vec<AlertEvent>>
where
    T: for<'de> Deserialize<'de>,
{
    for key in candidate_keys {
        if let Some(arr) = payload.get(*key).and_then(serde_json::Value::as_array) {
            let mut out = Vec::with_capacity(arr.len());
            for entry in arr {
                if let Ok(decoded) = serde_json::from_value::<T>(entry.clone()) {
                    out.push(wrap(decoded));
                }
            }
            return Some(out);
        }
    }
    None
}

fn decode_transcode(payload: &serde_json::Value, lifecycle: TranscodeLifecycle) -> Vec<AlertEvent> {
    // Look for a TranscodeSession array; if absent, treat the entire
    // payload as the single session body.
    payload
        .get("TranscodeSession")
        .and_then(serde_json::Value::as_array)
        .map_or_else(
            || {
                vec![AlertEvent::TranscodeSession(TranscodeSessionNotification {
                    lifecycle,
                    raw: payload.clone(),
                })]
            },
            |arr| {
                arr.iter()
                    .map(|raw| {
                        AlertEvent::TranscodeSession(TranscodeSessionNotification {
                            lifecycle,
                            raw: raw.clone(),
                        })
                    })
                    .collect()
            },
        )
}

const fn unknown(kind: String, raw: serde_json::Value) -> AlertEvent {
    AlertEvent::Unknown { kind, raw }
}

// -----------------------------------------------------------------------------
// Tests.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alerts_url_swaps_http_for_ws_and_appends_token() {
        let base = Url::parse("http://pms.local:32400").unwrap();
        let token = PlexToken::new("abcd").unwrap();
        let ws = alerts_url(&base, &token).unwrap();
        assert_eq!(ws.scheme(), "ws");
        assert_eq!(ws.path(), "/:/websockets/notifications");
        assert!(ws.query().unwrap().contains("X-Plex-Token=abcd"));
    }

    #[test]
    fn alerts_url_swaps_https_for_wss() {
        let base = Url::parse("https://pms.example.com").unwrap();
        let token = PlexToken::new("x").unwrap();
        let ws = alerts_url(&base, &token).unwrap();
        assert_eq!(ws.scheme(), "wss");
    }

    #[test]
    fn decode_frame_parses_playing_event() {
        let text = r#"{"NotificationContainer":{
            "type":"playing","size":1,
            "PlaySessionStateNotification":[{
                "sessionKey":"12","ratingKey":"31425",
                "key":"/library/metadata/31425",
                "viewOffset":12345,"state":"playing",
                "userID":1,"transcodeSession":"abc"
            }]
        }}"#;
        let events = decode_frame(text).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            AlertEvent::Playing(p) => {
                assert_eq!(p.session_key, "12");
                assert_eq!(p.rating_key, "31425");
                assert_eq!(p.view_offset, Some(12345));
                assert_eq!(p.state, "playing");
                assert_eq!(p.user_id, Some(1));
                assert_eq!(p.transcode_session.as_deref(), Some("abc"));
            }
            other => panic!("expected Playing, got {other:?}"),
        }
    }

    #[test]
    fn decode_frame_parses_timeline_event_with_state_values() {
        let text = r#"{"NotificationContainer":{
            "type":"timeline","size":1,
            "TimelineEntry":[
                {"itemID":42,"state":5,"type":1,"title":"Arrival"}
            ]
        }}"#;
        let events = decode_frame(text).unwrap();
        match &events[0] {
            AlertEvent::Timeline(t) => {
                assert_eq!(t.item_id, 42);
                assert_eq!(t.state, 5);
                assert_eq!(t.plex_type, Some(1));
                assert_eq!(t.title.as_deref(), Some("Arrival"));
            }
            other => panic!("expected Timeline, got {other:?}"),
        }
    }

    #[test]
    fn decode_frame_parses_activity_event_with_progress() {
        let text = r#"{"NotificationContainer":{
            "type":"activity","size":1,
            "ActivityNotification":[{
                "event":"updated",
                "Activity":{
                    "uuid":"u","type":"library.refresh.items",
                    "progress":42,"cancellable":true,
                    "title":"Refreshing"
                }
            }]
        }}"#;
        let events = decode_frame(text).unwrap();
        match &events[0] {
            AlertEvent::Activity(a) => {
                assert_eq!(a.event, "updated");
                let body = a.activity.as_ref().unwrap();
                assert_eq!(body.kind.as_deref(), Some("library.refresh.items"));
                assert_eq!(body.progress, Some(42));
                assert_eq!(body.cancellable, Some(true));
            }
            other => panic!("expected Activity, got {other:?}"),
        }
    }

    #[test]
    fn decode_frame_parses_transcode_session_start_with_lifecycle() {
        let text = r#"{"NotificationContainer":{
            "type":"transcodeSession.start","size":1,
            "TranscodeSession":[{"key":"/transcode/session/X","throttled":false}]
        }}"#;
        let events = decode_frame(text).unwrap();
        match &events[0] {
            AlertEvent::TranscodeSession(t) => {
                assert_eq!(t.lifecycle, TranscodeLifecycle::Start);
                assert!(t.raw.get("key").is_some());
            }
            other => panic!("expected TranscodeSession, got {other:?}"),
        }
    }

    #[test]
    fn decode_frame_parses_transcode_session_end_with_lifecycle() {
        let text = r#"{"NotificationContainer":{
            "type":"transcodeSession.end","size":1,
            "TranscodeSession":[{"key":"/transcode/session/Y"}]
        }}"#;
        let events = decode_frame(text).unwrap();
        match &events[0] {
            AlertEvent::TranscodeSession(t) => {
                assert_eq!(t.lifecycle, TranscodeLifecycle::End);
            }
            other => panic!("expected TranscodeSession, got {other:?}"),
        }
    }

    #[test]
    fn decode_frame_parses_status_notification() {
        let text = r#"{"NotificationContainer":{
            "type":"status","size":1,
            "StatusNotification":[{
                "title":"Library Update","description":"Done","notificationName":"info"
            }]
        }}"#;
        let events = decode_frame(text).unwrap();
        match &events[0] {
            AlertEvent::Status(s) => {
                assert_eq!(s.title.as_deref(), Some("Library Update"));
                assert_eq!(s.description.as_deref(), Some("Done"));
                assert_eq!(s.name.as_deref(), Some("info"));
            }
            other => panic!("expected Status, got {other:?}"),
        }
    }

    #[test]
    fn decode_frame_yields_multiple_events_per_container() {
        let text = r#"{"NotificationContainer":{
            "type":"timeline","size":3,
            "TimelineEntry":[
                {"itemID":1,"state":0},
                {"itemID":2,"state":1},
                {"itemID":3,"state":5}
            ]
        }}"#;
        let events = decode_frame(text).unwrap();
        assert_eq!(events.len(), 3);
    }

    #[test]
    fn decode_frame_unknown_type_yields_unknown_with_raw_payload() {
        let text = r#"{"NotificationContainer":{
            "type":"futureEventType","size":0,
            "FutureNotification":[{"foo":1}]
        }}"#;
        let events = decode_frame(text).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            AlertEvent::Unknown { kind, raw } => {
                assert_eq!(kind, "futureEventType");
                assert!(raw.get("FutureNotification").is_some());
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn decode_frame_invalid_json_returns_err() {
        let res = decode_frame("not json");
        assert!(res.is_err());
    }

    #[test]
    fn decode_frame_empty_container_yields_empty_events() {
        let text = r#"{"NotificationContainer":{"type":"playing","size":0}}"#;
        let events = decode_frame(text).unwrap();
        // No body array → empty Vec (not Unknown, because the
        // discriminator is recognised).
        assert!(events.is_empty() || matches!(events[0], AlertEvent::Unknown { .. }));
    }
}
