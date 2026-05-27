//! Transcoded streaming URL construction.
//!
//! Plex's universal transcoder serves DASH (`.mpd`) and HLS (`.m3u8`)
//! manifests from a single endpoint:
//!
//! ```text
//! /<streamtype>/:/transcode/universal/start.<container>?<params>
//! ```
//!
//! where `<streamtype>` is `video` or `audio`, `<container>` is
//! `m3u8` for HLS / `mpd` for DASH, and `<params>` carry the source
//! path, quality caps, and audio/subtitle stream selection.
//!
//! This module is the transcoded complement to
//! [`crate::Playable::direct_play_url`]. Use direct-play when the
//! source media is compatible with the player; reach for the
//! transcoder when you need:
//!
//! - bandwidth-capped streaming (set [`TranscodeOptions::max_video_bitrate`])
//! - resolution downscaling for slow devices
//! - format conversion for legacy players that can't decode the
//!   source codec
//! - subtitle burn-in / forced subtitle selection
//!
//! ## Quick example
//!
//! ```no_run
//! # use plex_rs::{PlexServer, LibraryItem, Movie};
//! use plex_rs::playback::{TranscodeOptions, TranscodeProtocol};
//!
//! # async fn run(server: PlexServer, movie: Movie) -> Result<(), plex_rs::Error> {
//! let url = TranscodeOptions::new()
//!     .protocol(TranscodeProtocol::Hls)
//!     .max_video_bitrate(8_000)         // kbps
//!     .video_resolution("1920x1080")
//!     .build_for(&server, &movie.key)?;
//! // hand `url` to ffmpeg / mpv / VLC
//! # let _ = url; Ok(()) }
//! ```

use std::fmt::Write;

use url::Url;

use crate::error::{Error, Result};
use crate::server::PlexServer;

// -----------------------------------------------------------------------------
// TranscodeProtocol.
// -----------------------------------------------------------------------------

/// Streaming protocol the transcoder produces.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum TranscodeProtocol {
    /// Apple HTTP Live Streaming. Output is `start.m3u8`.
    #[default]
    Hls,
    /// MPEG-DASH. Output is `start.mpd`.
    Dash,
}

impl TranscodeProtocol {
    /// Container extension used in the URL path (`m3u8` / `mpd`).
    #[must_use]
    pub const fn container_ext(self) -> &'static str {
        match self {
            Self::Hls => "m3u8",
            Self::Dash => "mpd",
        }
    }

    /// Wire spelling of the `?protocol=...` query parameter.
    #[must_use]
    pub const fn as_wire(self) -> &'static str {
        match self {
            Self::Hls => "hls",
            Self::Dash => "dash",
        }
    }
}

// -----------------------------------------------------------------------------
// StreamKind.
// -----------------------------------------------------------------------------

/// Stream class — drives the leading path segment.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum StreamKind {
    /// `/video/:/transcode/...` — movies, episodes, clips.
    #[default]
    Video,
    /// `/audio/:/transcode/...` — tracks, albums.
    Audio,
}

impl StreamKind {
    /// URL path segment (`video` / `audio`).
    #[must_use]
    pub const fn as_path(self) -> &'static str {
        match self {
            Self::Video => "video",
            Self::Audio => "audio",
        }
    }
}

// -----------------------------------------------------------------------------
// LocationHint.
// -----------------------------------------------------------------------------

/// Network-location hint to the transcoder. Affects bitrate
/// selection: `Lan` permits higher bitrates than `Wan`.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum LocationHint {
    /// `?location=lan`.
    Lan,
    /// `?location=wan`.
    Wan,
}

impl LocationHint {
    /// Wire spelling.
    #[must_use]
    pub const fn as_wire(self) -> &'static str {
        match self {
            Self::Lan => "lan",
            Self::Wan => "wan",
        }
    }
}

// -----------------------------------------------------------------------------
// TranscodeOptions.
// -----------------------------------------------------------------------------

/// Builder for a transcoder URL.
///
/// Defaults match python-plexapi's `getStreamURL`:
/// - protocol = HLS
/// - stream kind = Video
/// - fast seek = on
/// - copy timestamps = on
/// - offset = 0
///
/// Override any field via the builder setters, then call
/// [`Self::build_for`] with a [`PlexServer`] and the item's wire
/// `key` (`/library/metadata/<rk>`).
#[derive(Debug, Clone)]
#[non_exhaustive]
// Plex's transcoder accepts ~10 independent toggles; collapsing
// them into a bitflag would be an abstraction not driven by the
// wire shape.
#[allow(clippy::struct_excessive_bools)]
pub struct TranscodeOptions {
    /// Output protocol.
    pub protocol: TranscodeProtocol,
    /// Stream class.
    pub stream_kind: StreamKind,
    /// Variant index into the source's `Media[]` array. Default 0.
    pub media_index: u32,
    /// Part index into the variant's `Part[]` array. Default 0.
    pub part_index: u32,
    /// Seek offset in milliseconds. Resume-from-here.
    pub offset_ms: u64,
    /// Enable fast seek (`fastSeek=1`). Default `true`.
    pub fast_seek: bool,
    /// Copy timestamps (`copyts=1`). Default `true`.
    pub copy_ts: bool,
    /// Force the transcoder to re-encode rather than just remuxing
    /// (`directStream=0`). Default `false`.
    pub force_re_encode: bool,
    /// Force transcoding even when direct-play is possible
    /// (`directPlay=0`). Default `false`.
    pub force_transcode: bool,
    /// Cap on encoded video bitrate in kilobits per second.
    pub max_video_bitrate: Option<u32>,
    /// Target resolution as `<W>x<H>` (e.g. `"1280x720"`). The
    /// transcoder will downscale to fit within this box.
    pub video_resolution: Option<String>,
    /// Quality preset, 0..=100. Higher is better.
    pub video_quality: Option<u8>,
    /// Subtitle scale, 0..=200 (100 = native).
    pub subtitle_size: Option<u8>,
    /// Audio boost, 0..=200 (100 = native).
    pub audio_boost: Option<u8>,
    /// Network-location hint.
    pub location: Option<LocationHint>,
    /// Buffer size in kilobits — controls how aggressively the
    /// transcoder works ahead.
    pub media_buffer_size: Option<u32>,
    /// Override the `X-Plex-Platform` (defaults to `"Chrome"`,
    /// matching python-plexapi). Some players want
    /// `"iOS"` / `"Android"` / etc. to get the right transcoder
    /// behaviour.
    pub platform: Option<String>,
    /// Override the `X-Plex-Session-Identifier`. When `None`, no
    /// session id is emitted — the transcoder generates one.
    pub session_id: Option<String>,
}

impl Default for TranscodeOptions {
    fn default() -> Self {
        Self {
            protocol: TranscodeProtocol::default(),
            stream_kind: StreamKind::default(),
            media_index: 0,
            part_index: 0,
            offset_ms: 0,
            fast_seek: true,
            copy_ts: true,
            force_re_encode: false,
            force_transcode: false,
            max_video_bitrate: None,
            video_resolution: None,
            video_quality: None,
            subtitle_size: None,
            audio_boost: None,
            location: None,
            media_buffer_size: None,
            platform: None,
            session_id: None,
        }
    }
}

impl TranscodeOptions {
    /// Build a fresh options set with the documented defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the output protocol.
    #[must_use]
    pub const fn protocol(mut self, protocol: TranscodeProtocol) -> Self {
        self.protocol = protocol;
        self
    }

    /// Set the stream class (`video` / `audio`).
    #[must_use]
    pub const fn stream_kind(mut self, kind: StreamKind) -> Self {
        self.stream_kind = kind;
        self
    }

    /// Set the seek offset (in milliseconds).
    #[must_use]
    pub const fn offset_ms(mut self, ms: u64) -> Self {
        self.offset_ms = ms;
        self
    }

    /// Cap the encoded video bitrate (kbps). Values below 64 are
    /// clamped to 64 — matches python-plexapi.
    #[must_use]
    pub const fn max_video_bitrate(mut self, kbps: u32) -> Self {
        self.max_video_bitrate = Some(if kbps < 64 { 64 } else { kbps });
        self
    }

    /// Set target resolution. Format must be `<W>x<H>`.
    #[must_use]
    pub fn video_resolution(mut self, res: impl Into<String>) -> Self {
        self.video_resolution = Some(res.into());
        self
    }

    /// Set quality preset (0..=100, higher better). Values above 100
    /// are clamped to 100.
    #[must_use]
    pub const fn video_quality(mut self, q: u8) -> Self {
        self.video_quality = Some(if q > 100 { 100 } else { q });
        self
    }

    /// Set subtitle scale (0..=200). Values above 200 are clamped.
    #[must_use]
    pub const fn subtitle_size(mut self, n: u8) -> Self {
        self.subtitle_size = Some(if n > 200 { 200 } else { n });
        self
    }

    /// Set audio boost (0..=200). Values above 200 are clamped.
    #[must_use]
    pub const fn audio_boost(mut self, n: u8) -> Self {
        self.audio_boost = Some(if n > 200 { 200 } else { n });
        self
    }

    /// Set network-location hint.
    #[must_use]
    pub const fn location(mut self, loc: LocationHint) -> Self {
        self.location = Some(loc);
        self
    }

    /// Override the `X-Plex-Platform` value.
    #[must_use]
    pub fn platform(mut self, p: impl Into<String>) -> Self {
        self.platform = Some(p.into());
        self
    }

    /// Override the session id.
    #[must_use]
    pub fn session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = Some(id.into());
        self
    }

    /// Force transcoding (skip direct-play decision).
    #[must_use]
    pub const fn force_transcode(mut self, on: bool) -> Self {
        self.force_transcode = on;
        self
    }

    /// Force re-encoding (skip direct-stream / remux path).
    #[must_use]
    pub const fn force_re_encode(mut self, on: bool) -> Self {
        self.force_re_encode = on;
        self
    }

    /// Select a non-zero media variant.
    #[must_use]
    pub const fn media_index(mut self, idx: u32) -> Self {
        self.media_index = idx;
        self
    }

    /// Select a non-zero part within the chosen media variant.
    #[must_use]
    pub const fn part_index(mut self, idx: u32) -> Self {
        self.part_index = idx;
        self
    }

    /// Build the transcoder URL pointing at the supplied item's
    /// `key` (e.g. `/library/metadata/42`).
    ///
    /// The PMS auth token is appended as `X-Plex-Token`. The
    /// resulting URL is safe to hand to an external player.
    ///
    /// # Errors
    /// - [`Error::Config`] if `video_resolution` is set to an
    ///   invalid format (anything other than `WxH`).
    /// - [`Error::Config`] when the PMS has no token (the
    ///   transcoder rejects anonymous requests).
    /// - [`Error::Url`] on join failure.
    pub fn build_for(&self, server: &PlexServer, item_key: &str) -> Result<Url> {
        // Validate video_resolution shape early so callers see a
        // meaningful error rather than the transcoder's 500.
        if let Some(res) = &self.video_resolution {
            if !is_valid_resolution(res) {
                return Err(Error::Config(format!(
                    "video_resolution must be WxH, got {res:?}",
                )));
            }
        }
        let token = server
            .http()
            .config()
            .token
            .as_ref()
            .ok_or_else(|| {
                Error::Config("transcode: PMS has no token; cannot build URL".to_owned())
            })?
            .expose()
            .to_owned();
        let path = format!(
            "/{stream}/:/transcode/universal/start.{ext}",
            stream = self.stream_kind.as_path(),
            ext = self.protocol.container_ext(),
        );
        let mut url = server.base_url().join(&path)?;
        {
            let mut qp = url.query_pairs_mut();
            qp.append_pair("path", item_key);
            qp.append_pair("protocol", self.protocol.as_wire());
            qp.append_pair("mediaIndex", &self.media_index.to_string());
            qp.append_pair("partIndex", &self.part_index.to_string());
            qp.append_pair("offset", &self.offset_ms.to_string());
            qp.append_pair("fastSeek", bool_wire(self.fast_seek));
            qp.append_pair("copyts", bool_wire(self.copy_ts));
            if self.force_transcode {
                qp.append_pair("directPlay", "0");
            }
            if self.force_re_encode {
                qp.append_pair("directStream", "0");
            }
            if let Some(bitrate) = self.max_video_bitrate {
                qp.append_pair("maxVideoBitrate", &bitrate.to_string());
            }
            if let Some(res) = &self.video_resolution {
                qp.append_pair("videoResolution", res);
            }
            if let Some(q) = self.video_quality {
                qp.append_pair("videoQuality", &q.to_string());
            }
            if let Some(s) = self.subtitle_size {
                qp.append_pair("subtitleSize", &s.to_string());
            }
            if let Some(a) = self.audio_boost {
                qp.append_pair("audioBoost", &a.to_string());
            }
            if let Some(loc) = self.location {
                qp.append_pair("location", loc.as_wire());
            }
            if let Some(buf) = self.media_buffer_size {
                qp.append_pair("mediaBufferSize", &buf.to_string());
            }
            qp.append_pair(
                "X-Plex-Platform",
                self.platform.as_deref().unwrap_or("Chrome"),
            );
            if let Some(sid) = &self.session_id {
                qp.append_pair("X-Plex-Session-Identifier", sid);
            }
            qp.append_pair("X-Plex-Token", &token);
        }
        Ok(url)
    }
}

/// Build a transcode URL via the verbose long form, retained for
/// callers that want a single call site without the builder. Most
/// callers should use [`TranscodeOptions::build_for`].
///
/// # Errors
/// See [`TranscodeOptions::build_for`].
pub fn transcode_url(server: &PlexServer, item_key: &str, opts: &TranscodeOptions) -> Result<Url> {
    opts.build_for(server, item_key)
}

const fn bool_wire(b: bool) -> &'static str {
    if b { "1" } else { "0" }
}

/// Validate the `WxH` shape without pulling in regex.
fn is_valid_resolution(s: &str) -> bool {
    let mut split = s.split('x');
    let Some(w) = split.next() else { return false };
    let Some(h) = split.next() else { return false };
    if split.next().is_some() {
        return false;
    }
    !w.is_empty()
        && !h.is_empty()
        && w.bytes().all(|b| b.is_ascii_digit())
        && h.bytes().all(|b| b.is_ascii_digit())
}

// Use `Write` to silence unused-import lint when no debug
// formatting is needed inline.
const _: fn(&mut String) = |s| {
    let _ = write!(s, "");
};

// -----------------------------------------------------------------------------
// Tests.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::HttpClient;
    use crate::config::ClientConfig;
    use crate::server::{PlexServer, ServerIdentity};
    use crate::util::ids::{ClientIdentifier, MachineIdentifier, PlexToken};

    fn stub_server(token: &str) -> PlexServer {
        let cfg = ClientConfig::builder(ClientIdentifier::new("t").unwrap())
            .token(Some(PlexToken::new(token).unwrap()))
            .build()
            .unwrap();
        let http = HttpClient::new(cfg).unwrap();
        let base = Url::parse("http://pms.local:32400").unwrap();
        let identity = ServerIdentity {
            machine_identifier: MachineIdentifier::new("m").unwrap(),
            version: "v".into(),
            friendly_name: None,
            platform: None,
            platform_version: None,
            my_plex: false,
            my_plex_username: None,
            my_plex_signin_state: None,
            my_plex_subscription: false,
            allow_media_deletion: false,
            allow_sharing: false,
            livetv: None,
            updated_at: None,
        };
        PlexServer::__test_new(base, http, identity)
    }

    #[test]
    fn default_protocol_is_hls_with_m3u8_extension() {
        assert_eq!(TranscodeProtocol::Hls.container_ext(), "m3u8");
        assert_eq!(TranscodeProtocol::Hls.as_wire(), "hls");
    }

    #[test]
    fn dash_protocol_uses_mpd_extension() {
        assert_eq!(TranscodeProtocol::Dash.container_ext(), "mpd");
        assert_eq!(TranscodeProtocol::Dash.as_wire(), "dash");
    }

    #[test]
    fn build_for_emits_universal_path_and_token() {
        let server = stub_server("secret");
        let url = TranscodeOptions::new()
            .build_for(&server, "/library/metadata/42")
            .unwrap();
        assert_eq!(url.path(), "/video/:/transcode/universal/start.m3u8");
        let q = url.query().unwrap();
        assert!(q.contains("path=%2Flibrary%2Fmetadata%2F42"), "{q}");
        assert!(q.contains("X-Plex-Token=secret"), "{q}");
        assert!(q.contains("protocol=hls"));
        assert!(q.contains("fastSeek=1"));
        assert!(q.contains("copyts=1"));
        assert!(q.contains("offset=0"));
        // Defaults that should NOT appear (force-* flags off):
        assert!(!q.contains("directPlay=0"));
        assert!(!q.contains("directStream=0"));
    }

    #[test]
    fn dash_swaps_path_extension_and_protocol() {
        let server = stub_server("t");
        let url = TranscodeOptions::new()
            .protocol(TranscodeProtocol::Dash)
            .build_for(&server, "/library/metadata/1")
            .unwrap();
        assert_eq!(url.path(), "/video/:/transcode/universal/start.mpd");
        assert!(url.query().unwrap().contains("protocol=dash"));
    }

    #[test]
    fn audio_stream_kind_changes_path_segment() {
        let server = stub_server("t");
        let url = TranscodeOptions::new()
            .stream_kind(StreamKind::Audio)
            .build_for(&server, "/library/metadata/1")
            .unwrap();
        assert!(url.path().starts_with("/audio/:/transcode/universal/"));
    }

    #[test]
    fn force_transcode_and_re_encode_emit_zero_pairs() {
        let server = stub_server("t");
        let url = TranscodeOptions::new()
            .force_transcode(true)
            .force_re_encode(true)
            .build_for(&server, "/library/metadata/1")
            .unwrap();
        let q = url.query().unwrap();
        assert!(q.contains("directPlay=0"), "{q}");
        assert!(q.contains("directStream=0"), "{q}");
    }

    #[test]
    fn max_video_bitrate_clamps_below_64() {
        let opts = TranscodeOptions::new().max_video_bitrate(30);
        assert_eq!(opts.max_video_bitrate, Some(64));
        let opts = TranscodeOptions::new().max_video_bitrate(8000);
        assert_eq!(opts.max_video_bitrate, Some(8000));
    }

    #[test]
    fn video_resolution_invalid_format_rejected() {
        let server = stub_server("t");
        let err = TranscodeOptions::new()
            .video_resolution("1080p") // not WxH
            .build_for(&server, "/library/metadata/1")
            .unwrap_err();
        assert!(matches!(err, Error::Config(ref m) if m.contains("WxH")));
    }

    #[test]
    fn video_resolution_valid_format_accepted() {
        let server = stub_server("t");
        let url = TranscodeOptions::new()
            .video_resolution("1920x1080")
            .build_for(&server, "/library/metadata/1")
            .unwrap();
        assert!(url.query().unwrap().contains("videoResolution=1920x1080"));
    }

    #[test]
    fn quality_subtitle_audio_clamp_to_100_or_200() {
        let opts = TranscodeOptions::new()
            .video_quality(255)
            .subtitle_size(255)
            .audio_boost(255);
        assert_eq!(opts.video_quality, Some(100));
        assert_eq!(opts.subtitle_size, Some(200));
        assert_eq!(opts.audio_boost, Some(200));
    }

    #[test]
    fn location_hint_appears_as_lan_or_wan() {
        let server = stub_server("t");
        let url = TranscodeOptions::new()
            .location(LocationHint::Lan)
            .build_for(&server, "/library/metadata/1")
            .unwrap();
        assert!(url.query().unwrap().contains("location=lan"));
    }

    #[test]
    fn build_for_returns_config_error_when_token_missing() {
        // Construct a tokenless PlexServer.
        let cfg = ClientConfig::builder(ClientIdentifier::new("t").unwrap())
            .build()
            .unwrap();
        let http = HttpClient::new(cfg).unwrap();
        let base = Url::parse("http://pms.local:32400").unwrap();
        let identity = ServerIdentity {
            machine_identifier: MachineIdentifier::new("m").unwrap(),
            version: "v".into(),
            friendly_name: None,
            platform: None,
            platform_version: None,
            my_plex: false,
            my_plex_username: None,
            my_plex_signin_state: None,
            my_plex_subscription: false,
            allow_media_deletion: false,
            allow_sharing: false,
            livetv: None,
            updated_at: None,
        };
        let server = PlexServer::__test_new(base, http, identity);
        let err = TranscodeOptions::new()
            .build_for(&server, "/library/metadata/1")
            .unwrap_err();
        assert!(matches!(err, Error::Config(ref m) if m.contains("token")));
    }

    #[test]
    fn platform_override_appears_in_query() {
        let server = stub_server("t");
        let url = TranscodeOptions::new()
            .platform("iOS")
            .build_for(&server, "/library/metadata/1")
            .unwrap();
        assert!(url.query().unwrap().contains("X-Plex-Platform=iOS"));
    }

    #[test]
    fn session_id_appears_only_when_set() {
        let server = stub_server("t");
        let with = TranscodeOptions::new()
            .session_id("abc-123")
            .build_for(&server, "/library/metadata/1")
            .unwrap();
        assert!(
            with.query()
                .unwrap()
                .contains("X-Plex-Session-Identifier=abc-123")
        );
        let without = TranscodeOptions::new()
            .build_for(&server, "/library/metadata/1")
            .unwrap();
        assert!(!without.query().unwrap().contains("Session-Identifier"));
    }

    #[test]
    fn is_valid_resolution_accepts_common_shapes() {
        assert!(is_valid_resolution("1920x1080"));
        assert!(is_valid_resolution("1280x720"));
        assert!(is_valid_resolution("3840x2160"));
        assert!(!is_valid_resolution("1080"));
        assert!(!is_valid_resolution("1080p"));
        assert!(!is_valid_resolution("1280x"));
        assert!(!is_valid_resolution("x720"));
        assert!(!is_valid_resolution("1920x1080x"));
    }
}
