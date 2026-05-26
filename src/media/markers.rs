//! Intro / credits markers and chapter navigation.
//!
//! Two complementary surfaces that Plex emits on playable video items
//! ([`crate::media::Movie`], [`crate::media::Episode`]):
//!
//! - **Markers** ([`Marker`]) — auto-detected ranges Plex uses to skip
//!   intros, credits, and commercials. JSON field name: `Marker`.
//! - **Chapters** ([`Chapter`]) — embedded chapter markers from the
//!   source file (DVD-style scene index). JSON field name: `Chapter`.
//!
//! Both come back as arrays of objects on the metadata element; this
//! module models them as `Vec<Marker>` and `Vec<Chapter>` attached to
//! the leaf type.

use serde::Deserialize;

// -----------------------------------------------------------------------------
// MarkerKind.
// -----------------------------------------------------------------------------

/// Type of a [`Marker`].
///
/// Plex emits the `type` attribute as one of `intro`, `credits`,
/// `commercial`. Unknown values are preserved in
/// [`MarkerKind::Other`] for forward compatibility.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MarkerKind {
    /// `type="intro"` — opening title sequence.
    Intro,
    /// `type="credits"` — closing credits sequence.
    Credits,
    /// `type="commercial"` — ad break (Live TV / DVR recordings).
    Commercial,
    /// Wire value Plex emits that this build does not recognise.
    Other(String),
}

impl MarkerKind {
    /// Map Plex's wire `type` string to a [`MarkerKind`].
    #[must_use]
    pub fn from_wire(value: &str) -> Self {
        match value {
            "intro" => Self::Intro,
            "credits" => Self::Credits,
            "commercial" => Self::Commercial,
            other => Self::Other(other.to_owned()),
        }
    }

    /// Canonical wire value.
    #[must_use]
    pub fn as_wire(&self) -> &str {
        match self {
            Self::Intro => "intro",
            Self::Credits => "credits",
            Self::Commercial => "commercial",
            Self::Other(s) => s,
        }
    }
}

// -----------------------------------------------------------------------------
// Marker.
// -----------------------------------------------------------------------------

/// One auto-detected marker (intro / credits / commercial range).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Marker {
    /// Numeric Plex marker identifier.
    pub id: u64,
    /// Marker kind.
    pub kind: MarkerKind,
    /// Range start in milliseconds (from the beginning of the item).
    pub start_ms: u64,
    /// Range end in milliseconds.
    pub end_ms: u64,
    /// Only meaningful for [`MarkerKind::Credits`] — whether this
    /// marker covers the *final* credits sequence (used by Plex's
    /// post-credits-scene skip logic).
    pub final_credits: bool,
}

impl Marker {
    /// Range length in milliseconds. Saturates at zero if `end_ms`
    /// precedes `start_ms` (which would be a Plex bug, but defensive).
    #[must_use]
    pub const fn duration_ms(&self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }

    /// `true` when `time_ms` falls inside `[start_ms, end_ms]`.
    #[must_use]
    pub const fn contains(&self, time_ms: u64) -> bool {
        time_ms >= self.start_ms && time_ms <= self.end_ms
    }
}

// -----------------------------------------------------------------------------
// Chapter.
// -----------------------------------------------------------------------------

/// One chapter from the embedded DVD-style scene index of a playable
/// item.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Chapter {
    /// Numeric Plex chapter identifier.
    pub id: u64,
    /// Chapter title (often `"Chapter N"` for files without named
    /// chapters, sometimes scene names for properly-tagged sources).
    pub title: Option<String>,
    /// Sequential position (1-based; Plex sometimes 0-bases).
    pub index: Option<i32>,
    /// Start offset in milliseconds.
    pub start_ms: u64,
    /// End offset in milliseconds — absent for the final chapter on
    /// some sources.
    pub end_ms: Option<u64>,
    /// Per-chapter thumbnail path on PMS.
    pub thumb: Option<String>,
}

// -----------------------------------------------------------------------------
// DTOs.
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MarkerDto {
    #[serde(default)]
    pub(crate) id: u64,
    #[serde(default, rename = "type")]
    pub(crate) marker_type: Option<String>,
    #[serde(default)]
    pub(crate) start_time_offset: u64,
    #[serde(default)]
    pub(crate) end_time_offset: u64,
    #[serde(default, rename = "final")]
    pub(crate) final_credits: Option<crate::server::PlexBoolField>,
}

impl MarkerDto {
    pub(crate) fn into_domain(self) -> Marker {
        Marker {
            id: self.id,
            kind: MarkerKind::from_wire(self.marker_type.as_deref().unwrap_or("")),
            start_ms: self.start_time_offset,
            end_ms: self.end_time_offset,
            final_credits: self.final_credits.is_some_and(|b| b.to_bool()),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChapterDto {
    #[serde(default)]
    pub(crate) id: u64,
    #[serde(default)]
    pub(crate) tag: Option<String>,
    #[serde(default)]
    pub(crate) index: Option<i32>,
    #[serde(default)]
    pub(crate) start_time_offset: u64,
    #[serde(default)]
    pub(crate) end_time_offset: Option<u64>,
    #[serde(default)]
    pub(crate) thumb: Option<String>,
}

impl ChapterDto {
    pub(crate) fn into_domain(self) -> Chapter {
        Chapter {
            id: self.id,
            title: self.tag,
            index: self.index,
            start_ms: self.start_time_offset,
            end_ms: self.end_time_offset,
            thumb: self.thumb,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_kind_round_trips() {
        for k in [
            MarkerKind::Intro,
            MarkerKind::Credits,
            MarkerKind::Commercial,
        ] {
            assert_eq!(MarkerKind::from_wire(k.as_wire()), k);
        }
    }

    #[test]
    fn unknown_marker_type_preserved_as_other() {
        let m = MarkerKind::from_wire("recap");
        assert_eq!(m, MarkerKind::Other("recap".to_owned()));
        assert_eq!(m.as_wire(), "recap");
    }

    #[test]
    fn marker_dto_parses_intro() {
        let dto: MarkerDto = serde_json::from_value(serde_json::json!({
            "id": 1, "type": "intro",
            "startTimeOffset": 30_000, "endTimeOffset": 90_000
        }))
        .unwrap();
        let m = dto.into_domain();
        assert_eq!(m.id, 1);
        assert_eq!(m.kind, MarkerKind::Intro);
        assert_eq!(m.start_ms, 30_000);
        assert_eq!(m.end_ms, 90_000);
        assert_eq!(m.duration_ms(), 60_000);
        assert!(!m.final_credits);
        assert!(m.contains(60_000));
        assert!(!m.contains(15_000));
        assert!(m.contains(30_000));
        assert!(m.contains(90_000));
    }

    #[test]
    fn marker_dto_parses_final_credits_flag() {
        let dto: MarkerDto = serde_json::from_value(serde_json::json!({
            "id": 2, "type": "credits",
            "startTimeOffset": 5_000_000, "endTimeOffset": 5_800_000,
            "final": "1"
        }))
        .unwrap();
        let m = dto.into_domain();
        assert!(m.final_credits);
        assert_eq!(m.kind, MarkerKind::Credits);
    }

    #[test]
    fn marker_duration_saturates_when_end_before_start() {
        let m = Marker {
            id: 1,
            kind: MarkerKind::Intro,
            start_ms: 1000,
            end_ms: 500,
            final_credits: false,
        };
        assert_eq!(m.duration_ms(), 0);
    }

    #[test]
    fn chapter_dto_parses_full() {
        let dto: ChapterDto = serde_json::from_value(serde_json::json!({
            "id": 1,
            "tag": "Opening Scene",
            "index": 1,
            "startTimeOffset": 0,
            "endTimeOffset": 600_000,
            "thumb": "/library/metadata/100/chapter/1"
        }))
        .unwrap();
        let c = dto.into_domain();
        assert_eq!(c.id, 1);
        assert_eq!(c.title.as_deref(), Some("Opening Scene"));
        assert_eq!(c.index, Some(1));
        assert_eq!(c.start_ms, 0);
        assert_eq!(c.end_ms, Some(600_000));
        assert!(c.thumb.is_some());
    }

    #[test]
    fn chapter_dto_handles_missing_end() {
        let dto: ChapterDto = serde_json::from_value(serde_json::json!({
            "id": 5, "startTimeOffset": 0
        }))
        .unwrap();
        let c = dto.into_domain();
        assert_eq!(c.id, 5);
        assert!(c.end_ms.is_none());
        assert!(c.title.is_none());
    }
}
