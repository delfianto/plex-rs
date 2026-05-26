//! Media / `MediaPart` / `Stream` chain.
//!
//! Every playable item ([`crate::media::Movie`], [`crate::media::Episode`],
//! [`crate::media::Track`], [`crate::media::Photo`]) carries one or more
//! [`Media`] versions describing the file(s) on disk. Each [`Media`] is
//! a re-encode (different quality / container) and contains one or
//! more [`MediaPart`]s (files — multi-part movies split into halves
//! are a common case). Each part contains zero or more [`Stream`]s
//! describing the audio / video / subtitle / lyric tracks inside.
//!
//! Wire shape (JSON, after `Accept: application/json`):
//!
//! ```text
//! { "Metadata": [ {
//!     ...,
//!     "Media": [ {
//!         "id": 1, "duration": 6963000, "bitrate": 12000, ...,
//!         "Part": [ {
//!             "id": 2, "key": "/library/parts/2/.../file.mkv", ...,
//!             "Stream": [
//!                 { "id": 1, "streamType": 1, "codec": "h264", ... },
//!                 { "id": 2, "streamType": 2, "codec": "ac3",  ... },
//!                 { "id": 3, "streamType": 3, "codec": "srt",  ... }
//!             ]
//!         } ]
//!     } ]
//! } ] }
//! ```
//!
//! `streamType` is `1` for video, `2` for audio, `3` for subtitle,
//! `4` for lyric. Anything else lands in [`Stream::Unknown`] to
//! keep forward compatibility.

use serde::Deserialize;

// -----------------------------------------------------------------------------
// Media — one encoding/version of a playable item.
// -----------------------------------------------------------------------------

/// One re-encode of a playable item (a quality / container variant).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Media {
    /// Numeric Plex media identifier.
    pub id: u64,
    /// Total duration in milliseconds.
    pub duration_ms: Option<u64>,
    /// Aggregate bitrate in kbps.
    pub bitrate: Option<u32>,
    /// Video width in pixels.
    pub width: Option<u32>,
    /// Video height in pixels.
    pub height: Option<u32>,
    /// Display aspect ratio (e.g. 1.78 for 16:9).
    pub aspect_ratio: Option<f32>,
    /// Audio channel count (e.g. 6 for 5.1).
    pub audio_channels: Option<u8>,
    /// Wire-format audio codec name (`aac`, `ac3`, `dts`, …).
    pub audio_codec: Option<String>,
    /// Wire-format video codec name (`h264`, `hevc`, `vc1`, …).
    pub video_codec: Option<String>,
    /// Container format (`mkv`, `mp4`, `mp3`, `flac`, …).
    pub container: Option<String>,
    /// Frame rate descriptor (`24p`, `PAL`, `60i`, …).
    pub video_frame_rate: Option<String>,
    /// Coarse resolution bucket (`sd`, `720`, `1080`, `4k`).
    pub video_resolution: Option<String>,
    /// Plex's optimised-for-streaming flag.
    pub optimized_for_streaming: bool,
    /// H.264 profile string (`main`, `high`, `main 10`).
    pub video_profile: Option<String>,
    /// Audio profile string.
    pub audio_profile: Option<String>,
    /// One or more file parts that make up this encoding.
    pub parts: Vec<MediaPart>,
}

// -----------------------------------------------------------------------------
// MediaPart — one file.
// -----------------------------------------------------------------------------

/// One file backing a [`Media`].
///
/// Movies split across multiple files (`CD1.mkv` + `CD2.mkv` style)
/// produce multiple [`MediaPart`]s per [`Media`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct MediaPart {
    /// Numeric Plex part identifier.
    pub id: u64,
    /// Relative download key — `/library/parts/<id>/<unix>/file.<ext>`.
    pub key: String,
    /// Absolute filesystem path on the PMS host (may be `None` for
    /// remote / cloud media).
    pub file: Option<String>,
    /// File size in bytes.
    pub size: Option<u64>,
    /// Container format.
    pub container: Option<String>,
    /// Duration in milliseconds.
    pub duration_ms: Option<u64>,
    /// Whether Plex has generated thumbnails for the part.
    pub has_thumbnail: bool,
    /// Plex's optimised-for-streaming flag at the part level.
    pub optimized_for_streaming: bool,
    /// Streams inside the file.
    pub streams: Vec<Stream>,
}

// -----------------------------------------------------------------------------
// Stream — sum type dispatched on Plex's streamType discriminator.
// -----------------------------------------------------------------------------

/// One stream inside a [`MediaPart`].
///
/// Dispatched on `streamType` (`1`=video, `2`=audio, `3`=subtitle,
/// `4`=lyric). Unknown values are preserved in [`Stream::Unknown`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Stream {
    /// A video stream (`streamType=1`).
    Video(VideoStream),
    /// An audio stream (`streamType=2`).
    Audio(AudioStream),
    /// A subtitle stream (`streamType=3`).
    Subtitle(SubtitleStream),
    /// A lyric stream (`streamType=4`).
    Lyric(LyricStream),
    /// A stream type Plex emits that this build doesn't yet model.
    Unknown(UnknownStream),
}

/// Common identifier and stream-type fields shared by every variant.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct StreamCommon {
    /// Plex stream identifier.
    pub id: u64,
    /// Wire-format `streamType` integer (1/2/3/4/…).
    pub stream_type: u32,
}

/// A video stream.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct VideoStream {
    /// Shared id / type fields.
    pub common: StreamCommon,
    /// Codec (`h264`, `hevc`, …).
    pub codec: Option<String>,
    /// Language identifier (`eng`, `jpn`, …).
    pub language: Option<String>,
    /// Width in pixels.
    pub width: Option<u32>,
    /// Height in pixels.
    pub height: Option<u32>,
    /// Bitrate in kbps.
    pub bitrate: Option<u32>,
    /// Frame rate.
    pub frame_rate: Option<f32>,
    /// Bit depth (`8` / `10`).
    pub bit_depth: Option<u8>,
    /// Whether this is the player's default video stream.
    pub default: bool,
    /// Human-readable stream label.
    pub display_title: Option<String>,
}

/// An audio stream.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct AudioStream {
    /// Shared id / type fields.
    pub common: StreamCommon,
    /// Codec.
    pub codec: Option<String>,
    /// Language.
    pub language: Option<String>,
    /// Channel count.
    pub channels: Option<u8>,
    /// Channel layout descriptor (`5.1(side)`, `7.1`, …).
    pub audio_channel_layout: Option<String>,
    /// Bitrate in kbps.
    pub bitrate: Option<u32>,
    /// Sample rate in Hz.
    pub sampling_rate: Option<u32>,
    /// Default-track flag.
    pub default: bool,
    /// Currently-selected flag (the track the player is sending).
    pub selected: bool,
    /// Display title.
    pub display_title: Option<String>,
}

/// A subtitle stream.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SubtitleStream {
    /// Shared id / type fields.
    pub common: StreamCommon,
    /// Codec (`srt`, `ass`, `pgs`, …).
    pub codec: Option<String>,
    /// Language.
    pub language: Option<String>,
    /// ISO 639-3 language code.
    pub language_code: Option<String>,
    /// Default-track flag.
    pub default: bool,
    /// Selected-track flag.
    pub selected: bool,
    /// Forced-subtitles flag.
    pub forced: bool,
    /// Display title (e.g. "English (SDH)").
    pub display_title: Option<String>,
    /// External-subtitle URL when the track lives outside the
    /// container (`/library/streams/<id>`).
    pub key: Option<String>,
}

/// A lyric stream (audio playback time-aligned text).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LyricStream {
    /// Shared id / type fields.
    pub common: StreamCommon,
    /// Codec (`lrc`, `txt`).
    pub codec: Option<String>,
    /// Provider that supplied the lyric (`com.plexapp.agents.lyricfind`).
    pub provider: Option<String>,
    /// External key when the lyric is hosted separately.
    pub key: Option<String>,
}

/// Catch-all for stream types Plex adds that this build does not
/// recognise. Preserves the raw `streamType` integer so callers can
/// pattern-match.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct UnknownStream {
    /// Shared id / type fields.
    pub common: StreamCommon,
    /// Whatever codec Plex reported, if any.
    pub codec: Option<String>,
}

// -----------------------------------------------------------------------------
// DTOs.
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MediaDto {
    pub(crate) id: u64,
    #[serde(default)]
    pub(crate) duration: Option<u64>,
    #[serde(default)]
    pub(crate) bitrate: Option<u32>,
    #[serde(default)]
    pub(crate) width: Option<u32>,
    #[serde(default)]
    pub(crate) height: Option<u32>,
    #[serde(default)]
    pub(crate) aspect_ratio: Option<f32>,
    #[serde(default)]
    pub(crate) audio_channels: Option<u8>,
    #[serde(default)]
    pub(crate) audio_codec: Option<String>,
    #[serde(default)]
    pub(crate) video_codec: Option<String>,
    #[serde(default)]
    pub(crate) container: Option<String>,
    #[serde(default)]
    pub(crate) video_frame_rate: Option<String>,
    #[serde(default)]
    pub(crate) video_resolution: Option<String>,
    #[serde(default)]
    pub(crate) optimized_for_streaming: Option<crate::server::PlexBoolField>,
    #[serde(default)]
    pub(crate) video_profile: Option<String>,
    #[serde(default)]
    pub(crate) audio_profile: Option<String>,
    #[serde(default, rename = "Part")]
    pub(crate) parts: Vec<MediaPartDto>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MediaPartDto {
    pub(crate) id: u64,
    pub(crate) key: String,
    #[serde(default)]
    pub(crate) file: Option<String>,
    #[serde(default)]
    pub(crate) size: Option<u64>,
    #[serde(default)]
    pub(crate) container: Option<String>,
    #[serde(default)]
    pub(crate) duration: Option<u64>,
    #[serde(default)]
    pub(crate) has_thumbnail: Option<crate::server::PlexBoolField>,
    #[serde(default)]
    pub(crate) optimized_for_streaming: Option<crate::server::PlexBoolField>,
    #[serde(default, rename = "Stream")]
    pub(crate) streams: Vec<StreamDto>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StreamDto {
    pub(crate) id: u64,
    #[serde(default)]
    pub(crate) stream_type: u32,
    #[serde(default)]
    pub(crate) codec: Option<String>,
    #[serde(default)]
    pub(crate) language: Option<String>,
    #[serde(default)]
    pub(crate) language_code: Option<String>,
    #[serde(default)]
    pub(crate) width: Option<u32>,
    #[serde(default)]
    pub(crate) height: Option<u32>,
    #[serde(default)]
    pub(crate) bitrate: Option<u32>,
    #[serde(default)]
    pub(crate) frame_rate: Option<f32>,
    #[serde(default)]
    pub(crate) bit_depth: Option<u8>,
    #[serde(default)]
    pub(crate) channels: Option<u8>,
    #[serde(default)]
    pub(crate) audio_channel_layout: Option<String>,
    #[serde(default)]
    pub(crate) sampling_rate: Option<u32>,
    #[serde(default)]
    pub(crate) default: Option<crate::server::PlexBoolField>,
    #[serde(default)]
    pub(crate) selected: Option<crate::server::PlexBoolField>,
    #[serde(default)]
    pub(crate) forced: Option<crate::server::PlexBoolField>,
    #[serde(default)]
    pub(crate) display_title: Option<String>,
    #[serde(default)]
    pub(crate) key: Option<String>,
    #[serde(default)]
    pub(crate) provider: Option<String>,
}

// -----------------------------------------------------------------------------
// DTO → domain.
// -----------------------------------------------------------------------------

impl MediaDto {
    pub(crate) fn into_domain(self) -> Media {
        Media {
            id: self.id,
            duration_ms: self.duration,
            bitrate: self.bitrate,
            width: self.width,
            height: self.height,
            aspect_ratio: self.aspect_ratio,
            audio_channels: self.audio_channels,
            audio_codec: self.audio_codec,
            video_codec: self.video_codec,
            container: self.container,
            video_frame_rate: self.video_frame_rate,
            video_resolution: self.video_resolution,
            optimized_for_streaming: self.optimized_for_streaming.is_some_and(|b| b.to_bool()),
            video_profile: self.video_profile,
            audio_profile: self.audio_profile,
            parts: self
                .parts
                .into_iter()
                .map(MediaPartDto::into_domain)
                .collect(),
        }
    }
}

impl MediaPartDto {
    fn into_domain(self) -> MediaPart {
        MediaPart {
            id: self.id,
            key: self.key,
            file: self.file,
            size: self.size,
            container: self.container,
            duration_ms: self.duration,
            has_thumbnail: self.has_thumbnail.is_some_and(|b| b.to_bool()),
            optimized_for_streaming: self.optimized_for_streaming.is_some_and(|b| b.to_bool()),
            streams: self
                .streams
                .into_iter()
                .map(StreamDto::into_domain)
                .collect(),
        }
    }
}

impl StreamDto {
    const fn common(&self) -> StreamCommon {
        StreamCommon {
            id: self.id,
            stream_type: self.stream_type,
        }
    }

    fn into_domain(self) -> Stream {
        match self.stream_type {
            1 => Stream::Video(VideoStream {
                common: self.common(),
                codec: self.codec,
                language: self.language,
                width: self.width,
                height: self.height,
                bitrate: self.bitrate,
                frame_rate: self.frame_rate,
                bit_depth: self.bit_depth,
                default: self.default.is_some_and(|b| b.to_bool()),
                display_title: self.display_title,
            }),
            2 => Stream::Audio(AudioStream {
                common: self.common(),
                codec: self.codec,
                language: self.language,
                channels: self.channels,
                audio_channel_layout: self.audio_channel_layout,
                bitrate: self.bitrate,
                sampling_rate: self.sampling_rate,
                default: self.default.is_some_and(|b| b.to_bool()),
                selected: self.selected.is_some_and(|b| b.to_bool()),
                display_title: self.display_title,
            }),
            3 => Stream::Subtitle(SubtitleStream {
                common: self.common(),
                codec: self.codec,
                language: self.language,
                language_code: self.language_code,
                default: self.default.is_some_and(|b| b.to_bool()),
                selected: self.selected.is_some_and(|b| b.to_bool()),
                forced: self.forced.is_some_and(|b| b.to_bool()),
                display_title: self.display_title,
                key: self.key,
            }),
            4 => Stream::Lyric(LyricStream {
                common: self.common(),
                codec: self.codec,
                provider: self.provider,
                key: self.key,
            }),
            _ => Stream::Unknown(UnknownStream {
                common: self.common(),
                codec: self.codec,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_media_chain() {
        let json = serde_json::json!({
            "id": 100,
            "duration": 6_963_000,
            "bitrate": 12000,
            "width": 1920,
            "height": 1080,
            "aspectRatio": 1.78,
            "audioChannels": 6,
            "audioCodec": "ac3",
            "videoCodec": "h264",
            "container": "mkv",
            "videoFrameRate": "24p",
            "videoResolution": "1080",
            "optimizedForStreaming": "1",
            "videoProfile": "high",
            "Part": [{
                "id": 200,
                "key": "/library/parts/200/abc/file.mkv",
                "file": "/data/movies/foo.mkv",
                "size": 8_000_000_000_u64,
                "duration": 6_963_000,
                "container": "mkv",
                "hasThumbnail": "1",
                "Stream": [
                    {"id": 1, "streamType": 1, "codec": "h264",
                     "width": 1920, "height": 1080, "frameRate": 23.976,
                     "default": "1", "displayTitle": "1080p H.264"},
                    {"id": 2, "streamType": 2, "codec": "ac3",
                     "channels": 6, "audioChannelLayout": "5.1(side)",
                     "default": "1", "selected": "1",
                     "displayTitle": "English (AC3 5.1)"},
                    {"id": 3, "streamType": 3, "codec": "srt",
                     "language": "English", "languageCode": "eng",
                     "default": false, "selected": false, "forced": "0",
                     "displayTitle": "English (SRT)",
                     "key": "/library/streams/3"},
                    {"id": 4, "streamType": 4, "codec": "lrc",
                     "provider": "lyricfind"},
                    {"id": 5, "streamType": 99, "codec": "future"}
                ]
            }]
        });
        let dto: MediaDto = serde_json::from_value(json).unwrap();
        let m = dto.into_domain();
        assert_eq!(m.id, 100);
        assert_eq!(m.bitrate, Some(12000));
        assert_eq!(m.video_codec.as_deref(), Some("h264"));
        assert!(m.optimized_for_streaming);
        assert_eq!(m.parts.len(), 1);

        let p = &m.parts[0];
        assert_eq!(p.id, 200);
        assert_eq!(p.size, Some(8_000_000_000));
        assert!(p.has_thumbnail);
        assert_eq!(p.streams.len(), 5);

        match &p.streams[0] {
            Stream::Video(v) => {
                assert_eq!(v.common.id, 1);
                assert_eq!(v.codec.as_deref(), Some("h264"));
                assert_eq!(v.width, Some(1920));
                assert_eq!(v.height, Some(1080));
                assert!(v.default);
            }
            other => panic!("expected Video, got {other:?}"),
        }
        match &p.streams[1] {
            Stream::Audio(a) => {
                assert_eq!(a.channels, Some(6));
                assert!(a.default);
                assert!(a.selected);
                assert_eq!(a.audio_channel_layout.as_deref(), Some("5.1(side)"));
            }
            other => panic!("expected Audio, got {other:?}"),
        }
        match &p.streams[2] {
            Stream::Subtitle(s) => {
                assert_eq!(s.language_code.as_deref(), Some("eng"));
                assert!(!s.forced);
                assert_eq!(s.key.as_deref(), Some("/library/streams/3"));
            }
            other => panic!("expected Subtitle, got {other:?}"),
        }
        match &p.streams[3] {
            Stream::Lyric(l) => {
                assert_eq!(l.provider.as_deref(), Some("lyricfind"));
            }
            other => panic!("expected Lyric, got {other:?}"),
        }
        match &p.streams[4] {
            Stream::Unknown(u) => {
                assert_eq!(u.common.stream_type, 99);
                assert_eq!(u.codec.as_deref(), Some("future"));
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn empty_media_dto_yields_empty_chain() {
        let dto: MediaDto = serde_json::from_value(serde_json::json!({"id": 1})).unwrap();
        let m = dto.into_domain();
        assert_eq!(m.id, 1);
        assert!(m.parts.is_empty());
        assert!(!m.optimized_for_streaming);
    }
}
