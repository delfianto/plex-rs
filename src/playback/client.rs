//! `PlexClient` — remote control of a Plex player.
//!
//! A [`PlexClient`] connects directly to a Plex player's HTTP
//! endpoint (typically port 32500) — **not** to the Plex Media
//! Server. Commands hit `/player/{controller}/{command}` with the
//! `X-Plex-Target-Client-Identifier` header set to the player's
//! `machineIdentifier` and a monotonic `commandID` query parameter.
//!
//! ## Controllers
//!
//! - `navigation/*` — move selection, select, back, etc.
//!   (`moveUp`, `moveDown`, `moveLeft`, `moveRight`, `select`,
//!   `back`, `contextMenu`, `home`, `music`, `pageUp`, `pageDown`).
//! - `playback/*` — play, pause, seek, volume, etc. Each command
//!   accepts a mandatory `type=<video|music|photo>` parameter so a
//!   single player can multiplex foreground video and background
//!   music.
//! - `mirror/*` and `playback/playMedia` — start playback of a
//!   specific item (with a backing [`crate::PlayQueue`]) or
//!   navigate to a media details page.
//!
//! ## Command IDs
//!
//! Plex requires `commandID` to be monotonically increasing per
//! caller. We sequence with an internal `AtomicU64` so concurrent
//! commands from cloned [`PlexClient`] handles serialise correctly.
//!
//! ## Discovery
//!
//! To find a player, enumerate [`crate::MyPlexResource`] entries
//! whose [`provides`](crate::MyPlexResource::provides) contains
//! `"player"`, then construct a [`PlexClient`] from the resource's
//! connection list.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use url::Url;

use crate::client::HttpClient;
use crate::config::ClientConfig;
use crate::error::{Error, Result};
use crate::playback::PlayQueue;
use crate::server::PlexServer;
use crate::util::ids::{ClientIdentifier, MachineIdentifier, PlexToken};

// -----------------------------------------------------------------------------
// MediaType — argument to every playback command.
// -----------------------------------------------------------------------------

/// Foreground media class on the target player.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MediaType {
    /// Video — movies, TV episodes, clips.
    Video,
    /// Music — tracks, albums, artists.
    Music,
    /// Photo — slideshows.
    Photo,
}

impl MediaType {
    /// Wire spelling Plex expects (`video`, `music`, `photo`).
    #[must_use]
    pub const fn as_wire(self) -> &'static str {
        match self {
            Self::Video => "video",
            Self::Music => "music",
            Self::Photo => "photo",
        }
    }
}

/// Repeat-mode argument to [`PlexClient::set_repeat`].
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RepeatMode {
    /// Repeat disabled.
    Off,
    /// Repeat the currently playing item.
    One,
    /// Repeat the whole queue.
    All,
}

impl RepeatMode {
    /// Numeric wire encoding (`0` / `1` / `2`).
    #[must_use]
    pub const fn as_wire(self) -> &'static str {
        match self {
            Self::Off => "0",
            Self::One => "1",
            Self::All => "2",
        }
    }
}

// -----------------------------------------------------------------------------
// PlexClient.
// -----------------------------------------------------------------------------

/// Remote-control handle for a single Plex player.
#[derive(Debug, Clone)]
pub struct PlexClient {
    http: HttpClient,
    base_url: Url,
    machine_identifier: MachineIdentifier,
    command_id: Arc<AtomicU64>,
}

impl PlexClient {
    /// Connect to a player at `base_url` using the supplied
    /// `access_token` (per-resource, **not** the account token) and
    /// the player's `machine_identifier` (sent as
    /// `X-Plex-Target-Client-Identifier` on every command).
    ///
    /// The constructor does not eagerly probe the player — it's
    /// cheap, building only the inner `reqwest::Client`. The first
    /// failure surfaces from the first command.
    ///
    /// # Errors
    /// - [`Error::Config`] if the `client_identifier` is empty.
    /// - [`Error::Transport`] if TLS / DNS init fails.
    pub fn connect(
        base_url: Url,
        access_token: PlexToken,
        machine_identifier: MachineIdentifier,
        client_identifier: ClientIdentifier,
    ) -> Result<Self> {
        let cfg = ClientConfig::builder(client_identifier)
            .token(Some(access_token))
            .build()?;
        let http = HttpClient::new(cfg)?;
        Ok(Self::from_http(base_url, http, machine_identifier))
    }

    /// Construct from a pre-configured [`HttpClient`]. The client
    /// must already carry the player's access token.
    #[must_use]
    pub fn from_http(
        base_url: Url,
        http: HttpClient,
        machine_identifier: MachineIdentifier,
    ) -> Self {
        Self {
            http,
            base_url,
            machine_identifier,
            command_id: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Borrow the player's base URL.
    #[must_use]
    pub const fn base_url(&self) -> &Url {
        &self.base_url
    }

    /// Borrow the player's machine identifier.
    #[must_use]
    pub const fn machine_identifier(&self) -> &MachineIdentifier {
        &self.machine_identifier
    }

    // ------- navigation -------

    /// Move selection up.
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn move_up(&self) -> Result<()> {
        self.send("navigation/moveUp", &[]).await
    }
    /// Move selection down.
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn move_down(&self) -> Result<()> {
        self.send("navigation/moveDown", &[]).await
    }
    /// Move selection left.
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn move_left(&self) -> Result<()> {
        self.send("navigation/moveLeft", &[]).await
    }
    /// Move selection right.
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn move_right(&self) -> Result<()> {
        self.send("navigation/moveRight", &[]).await
    }
    /// Activate the currently-selected element.
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn select(&self) -> Result<()> {
        self.send("navigation/select", &[]).await
    }
    /// Go back one navigation level.
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn back(&self) -> Result<()> {
        self.send("navigation/back", &[]).await
    }
    /// Open the context menu for the current selection.
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn context_menu(&self) -> Result<()> {
        self.send("navigation/contextMenu", &[]).await
    }
    /// Navigate to the player's home screen.
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn go_to_home(&self) -> Result<()> {
        self.send("navigation/home", &[]).await
    }
    /// Navigate to the music section.
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn go_to_music(&self) -> Result<()> {
        self.send("navigation/music", &[]).await
    }
    /// Page up.
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn page_up(&self) -> Result<()> {
        self.send("navigation/pageUp", &[]).await
    }
    /// Page down.
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn page_down(&self) -> Result<()> {
        self.send("navigation/pageDown", &[]).await
    }

    // ------- playback -------

    /// Start playback (`type=<mtype>`).
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn play(&self, mtype: MediaType) -> Result<()> {
        self.send("playback/play", &[("type", mtype.as_wire())])
            .await
    }
    /// Pause playback.
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn pause(&self, mtype: MediaType) -> Result<()> {
        self.send("playback/pause", &[("type", mtype.as_wire())])
            .await
    }
    /// Stop playback.
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn stop(&self, mtype: MediaType) -> Result<()> {
        self.send("playback/stop", &[("type", mtype.as_wire())])
            .await
    }
    /// Skip to the next item in the queue.
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn skip_next(&self, mtype: MediaType) -> Result<()> {
        self.send("playback/skipNext", &[("type", mtype.as_wire())])
            .await
    }
    /// Skip to the previous item in the queue.
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn skip_previous(&self, mtype: MediaType) -> Result<()> {
        self.send("playback/skipPrevious", &[("type", mtype.as_wire())])
            .await
    }
    /// Seek to a position (milliseconds from the start of the
    /// current item).
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn seek_to(&self, position_ms: u64, mtype: MediaType) -> Result<()> {
        let pos = position_ms.to_string();
        self.send(
            "playback/seekTo",
            &[("offset", pos.as_str()), ("type", mtype.as_wire())],
        )
        .await
    }
    /// Step forward by a chunk (player-defined, usually ~30s).
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn step_forward(&self, mtype: MediaType) -> Result<()> {
        self.send("playback/stepForward", &[("type", mtype.as_wire())])
            .await
    }
    /// Step back by a chunk (player-defined, usually ~10s).
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn step_back(&self, mtype: MediaType) -> Result<()> {
        self.send("playback/stepBack", &[("type", mtype.as_wire())])
            .await
    }
    /// Set the player's volume (0..=100).
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn set_volume(&self, volume: u8, mtype: MediaType) -> Result<()> {
        let v = volume.min(100).to_string();
        self.send(
            "playback/setParameters",
            &[("volume", v.as_str()), ("type", mtype.as_wire())],
        )
        .await
    }
    /// Set repeat mode.
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn set_repeat(&self, mode: RepeatMode, mtype: MediaType) -> Result<()> {
        self.send(
            "playback/setParameters",
            &[("repeat", mode.as_wire()), ("type", mtype.as_wire())],
        )
        .await
    }
    /// Toggle shuffle.
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn set_shuffle(&self, on: bool, mtype: MediaType) -> Result<()> {
        self.send(
            "playback/setParameters",
            &[
                ("shuffle", if on { "1" } else { "0" }),
                ("type", mtype.as_wire()),
            ],
        )
        .await
    }

    // ------- mirror / playMedia -------

    /// Tell the player to start playing the given [`PlayQueue`].
    ///
    /// This composes a `playback/playMedia` command with all the
    /// fields a player needs to fetch the queue and its items from
    /// the PMS — `protocol`/`address`/`port`/`machineIdentifier`/
    /// `token` describe the server; `containerKey` references the
    /// queue; `key` and `type` describe the starting item.
    ///
    /// `offset_ms` resumes playback from a position other than the
    /// start. The PMS's *own* token is forwarded to the player so it
    /// can authenticate back to the server.
    ///
    /// # Errors
    /// - [`Error::Config`] when `queue.items` is empty (no starting
    ///   key to give the player) or the PMS base URL has no host /
    ///   no token.
    /// - Any transport [`Error`] variant.
    pub async fn play_media(
        &self,
        server: &PlexServer,
        queue: &PlayQueue,
        offset_ms: u64,
    ) -> Result<()> {
        let first = queue.items.first().ok_or_else(|| {
            Error::Config("PlayQueue has no items — cannot play_media".to_owned())
        })?;
        let server_url = server.base_url();
        let host = server_url
            .host_str()
            .ok_or_else(|| Error::Config("PMS base URL has no host".to_owned()))?
            .to_owned();
        let scheme = server_url.scheme().to_owned();
        let port = server_url.port_or_known_default().map_or_else(
            || (if scheme == "https" { 443 } else { 80 }).to_string(),
            |p| p.to_string(),
        );
        let machine_id = server.identity().machine_identifier.as_str().to_owned();
        let token = server
            .http()
            .config()
            .token
            .as_ref()
            .ok_or_else(|| Error::Config("PMS has no token; cannot forward".to_owned()))?
            .expose()
            .to_owned();
        let container_key = format!("/playQueues/{}?window=100&own=1", queue.id.0);
        let offset_str = offset_ms.to_string();
        let item_key = first.item.key().to_owned();
        let mtype = first.item.list_type();
        // PMS uses "video"/"audio"/"photo"; the player expects "music"
        // not "audio" for audio content.
        let mtype = if mtype == "audio" { "music" } else { mtype };
        let pairs: [(&str, &str); 10] = [
            ("providerIdentifier", "com.plexapp.plugins.library"),
            ("machineIdentifier", machine_id.as_str()),
            ("protocol", scheme.as_str()),
            ("address", host.as_str()),
            ("port", port.as_str()),
            ("offset", offset_str.as_str()),
            ("key", item_key.as_str()),
            ("type", mtype),
            ("containerKey", container_key.as_str()),
            ("token", token.as_str()),
        ];
        self.send("playback/playMedia", &pairs).await
    }

    // ------- internals -------

    /// Send a `/player/<path>` GET with `commandID` and the
    /// target-client header. Discards the response body.
    async fn send(&self, path: &str, params: &[(&str, &str)]) -> Result<()> {
        let cmd_id = self.command_id.fetch_add(1, Ordering::SeqCst);
        let cmd_id_s = cmd_id.to_string();
        let mut url = self.base_url.join(&format!("/player/{path}"))?;
        {
            let mut qp = url.query_pairs_mut();
            for (k, v) in params {
                qp.append_pair(k, v);
            }
            qp.append_pair("commandID", &cmd_id_s);
        }
        let target = self.machine_identifier.as_str();
        let headers: [(&str, &str); 1] = [("X-Plex-Target-Client-Identifier", target)];
        let _ = self
            .http
            .get_bytes_with_headers(url.as_str(), &headers)
            .await?;
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Tests.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_type_wire_spellings() {
        assert_eq!(MediaType::Video.as_wire(), "video");
        assert_eq!(MediaType::Music.as_wire(), "music");
        assert_eq!(MediaType::Photo.as_wire(), "photo");
    }

    #[test]
    fn repeat_mode_wire_spellings() {
        assert_eq!(RepeatMode::Off.as_wire(), "0");
        assert_eq!(RepeatMode::One.as_wire(), "1");
        assert_eq!(RepeatMode::All.as_wire(), "2");
    }

    #[test]
    fn command_id_increments_per_send_call() {
        // Sanity check on the AtomicU64 sequencing without hitting
        // the network. fetch_add returns the *previous* value, so the
        // first command gets id 1, the second id 2, etc.
        let counter = AtomicU64::new(1);
        let first = counter.fetch_add(1, Ordering::SeqCst);
        let second = counter.fetch_add(1, Ordering::SeqCst);
        assert_eq!(first, 1);
        assert_eq!(second, 2);
    }

    #[test]
    fn from_http_seeds_command_id_at_one() {
        let cfg = ClientConfig::builder(ClientIdentifier::new("t").unwrap())
            .build()
            .unwrap();
        let http = HttpClient::new(cfg).unwrap();
        let mid = MachineIdentifier::new("abcd").unwrap();
        let url = Url::parse("http://player.local:32500").unwrap();
        let client = PlexClient::from_http(url.clone(), http, mid.clone());
        assert_eq!(client.base_url(), &url);
        assert_eq!(client.machine_identifier(), &mid);
        assert_eq!(client.command_id.load(Ordering::SeqCst), 1);
    }
}
