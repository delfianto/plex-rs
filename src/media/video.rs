//! Video-domain leaves.
//!
//! Currently models [`Movie`]. Show / Season / Episode / Clip / Extra
//! land in M2.2 — they'll share the common-video field set that
//! `python-plexapi/plexapi/video.py:Video._loadData` populates on every
//! video class (analysis/06 §B).

use serde::Deserialize;
use url::Url;

use crate::error::{Error, Result};
use crate::library::LibrarySectionRef;
use crate::util::ids::RatingKey;

// -----------------------------------------------------------------------------
// Movie.
// -----------------------------------------------------------------------------

/// A Plex movie — one entry under a `SectionKind::Movie` library
/// section.
///
/// Carries the scalar metadata Plex returns on the `<Video>` element
/// for movies (`type="movie"`). The associated `<Media>`/`<Part>`/
/// `<Stream>` chain plus `<Genre>`/`<Director>`/etc. tag list are
/// deferred to M2.5 and M2.6 respectively. Editing operations land
/// in M3 (the trait suite drives off the `section_ref` back-link).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Movie {
    /// Plex's primary metadata identifier (`ratingKey`).
    pub rating_key: RatingKey,
    /// Relative key — `/library/metadata/<rating_key>`.
    pub key: String,
    /// User-visible title.
    pub title: String,
    /// Sort title (often the title with leading articles dropped).
    pub title_sort: Option<String>,
    /// Original title (e.g. the title in the production language).
    pub original_title: Option<String>,
    /// Studio that produced the movie.
    pub studio: Option<String>,
    /// Plot summary.
    pub summary: Option<String>,
    /// Marketing tagline ("In space, no one can hear you scream").
    pub tagline: Option<String>,
    /// Content rating (`PG-13`, `R`, `TV-MA`, …).
    pub content_rating: Option<String>,
    /// Release year.
    pub year: Option<u16>,
    /// Plex's normalised user rating, 0.0..=10.0.
    pub rating: Option<f32>,
    /// Audience score, 0.0..=10.0.
    pub audience_rating: Option<f32>,
    /// Aggregated critic score, 0.0..=10.0.
    pub critic_rating: Option<f32>,
    /// Duration in milliseconds.
    pub duration_ms: Option<u64>,
    /// View count (how many times this user has played it).
    pub view_count: u32,
    /// Most recent playback timestamp (epoch seconds).
    pub last_viewed_at: Option<i64>,
    /// Last partial-play view offset in milliseconds (resume point).
    pub view_offset_ms: Option<u64>,
    /// Date originally released (`YYYY-MM-DD`).
    pub originally_available_at: Option<String>,
    /// Date the file was added to the library (epoch seconds).
    pub added_at: Option<i64>,
    /// Date the metadata last changed (epoch seconds).
    pub updated_at: Option<i64>,
    /// Path to the poster image.
    pub thumb: Option<String>,
    /// Path to the background art image.
    pub art: Option<String>,
    /// Primary GUID (`plex://movie/...` or legacy `com.plexapp.agents.imdb://tt...`).
    pub guid: Option<String>,
    /// Back-reference to the owning library section, used for M3 edits.
    pub section_ref: LibrarySectionRef,
}

impl Movie {
    /// Convenience accessor that returns the canonical key path.
    #[must_use]
    pub fn key_path(&self) -> &str {
        &self.key
    }

    /// Absolute URL of the poster image, resolved against the server's
    /// base URL. Returns [`None`] when [`Self::thumb`] is absent.
    ///
    /// # Errors
    /// Returns [`Error::Url`] if the thumb path can't be joined to the
    /// base URL.
    pub fn thumb_url(&self) -> Result<Option<Url>> {
        match &self.thumb {
            None => Ok(None),
            Some(p) => Ok(Some(self.section_ref.base_url.join(p)?)),
        }
    }

    /// Has this movie ever been played?
    #[must_use]
    pub const fn is_played(&self) -> bool {
        self.view_count > 0
    }
}

// -----------------------------------------------------------------------------
// DTO (JSON metadata element).
// -----------------------------------------------------------------------------

/// Common video-metadata DTO. Captures the union of fields Movie /
/// Show / Episode / Clip carry; per-class fields are added when those
/// types land.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MetadataDto {
    pub(crate) rating_key: String,
    pub(crate) key: String,
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) title_sort: Option<String>,
    #[serde(default)]
    pub(crate) original_title: Option<String>,
    #[serde(default)]
    pub(crate) studio: Option<String>,
    #[serde(default)]
    pub(crate) summary: Option<String>,
    #[serde(default)]
    pub(crate) tagline: Option<String>,
    #[serde(default)]
    pub(crate) content_rating: Option<String>,
    #[serde(default)]
    pub(crate) year: Option<u16>,
    #[serde(default)]
    pub(crate) rating: Option<f32>,
    #[serde(default)]
    pub(crate) audience_rating: Option<f32>,
    #[serde(default)]
    pub(crate) rating_critic: Option<f32>,
    // Note: Plex emits the critic field as `ratingCritic`, not
    // `criticRating`, in the JSON-on-the-wire payload despite the
    // schema-doc name. See analysis/08 §3 wart list.
    #[serde(default)]
    pub(crate) duration: Option<u64>,
    #[serde(default)]
    pub(crate) view_count: Option<u32>,
    #[serde(default)]
    pub(crate) last_viewed_at: Option<i64>,
    #[serde(default)]
    pub(crate) view_offset: Option<u64>,
    #[serde(default)]
    pub(crate) originally_available_at: Option<String>,
    #[serde(default)]
    pub(crate) added_at: Option<i64>,
    #[serde(default)]
    pub(crate) updated_at: Option<i64>,
    #[serde(default)]
    pub(crate) thumb: Option<String>,
    #[serde(default)]
    pub(crate) art: Option<String>,
    #[serde(default)]
    pub(crate) guid: Option<String>,
}

impl MetadataDto {
    pub(crate) fn into_movie(self, section_ref: LibrarySectionRef) -> Result<Movie> {
        let rating_key: RatingKey = self
            .rating_key
            .parse()
            .map_err(|e: Error| Error::Config(format!("metadata.ratingKey not numeric: {e}")))?;
        Ok(Movie {
            rating_key,
            key: self.key,
            title: self.title,
            title_sort: self.title_sort,
            original_title: self.original_title,
            studio: self.studio,
            summary: self.summary,
            tagline: self.tagline,
            content_rating: self.content_rating,
            year: self.year,
            rating: self.rating,
            audience_rating: self.audience_rating,
            critic_rating: self.rating_critic,
            duration_ms: self.duration,
            view_count: self.view_count.unwrap_or(0),
            last_viewed_at: self.last_viewed_at,
            view_offset_ms: self.view_offset,
            originally_available_at: self.originally_available_at,
            added_at: self.added_at,
            updated_at: self.updated_at,
            thumb: self.thumb,
            art: self.art,
            guid: self.guid,
            section_ref,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HttpClient;

    fn fixture_ref() -> LibrarySectionRef {
        let cfg = crate::ClientConfig::builder(crate::ClientIdentifier::new("t").unwrap())
            .build()
            .unwrap();
        let http = HttpClient::new(cfg).unwrap();
        let base = Url::parse("http://plex.local:32400/").unwrap();
        LibrarySectionRef {
            id: 1,
            http,
            base_url: base,
        }
    }

    #[test]
    fn movie_dto_parses_minimal_fields() {
        let body = serde_json::json!({
            "ratingKey": "100",
            "key": "/library/metadata/100",
            "title": "Blade Runner"
        });
        let dto: MetadataDto = serde_json::from_value(body).unwrap();
        let movie = dto.into_movie(fixture_ref()).unwrap();
        assert_eq!(movie.rating_key.get(), 100);
        assert_eq!(movie.title, "Blade Runner");
        assert_eq!(movie.view_count, 0);
        assert!(!movie.is_played());
    }

    #[test]
    fn movie_dto_parses_full_metadata() {
        let body = serde_json::json!({
            "ratingKey": "12345",
            "key": "/library/metadata/12345",
            "title": "Arrival",
            "titleSort": "Arrival",
            "studio": "Paramount Pictures",
            "summary": "A linguist works with the military to communicate with aliens.",
            "tagline": "Why are they here?",
            "contentRating": "PG-13",
            "year": 2016,
            "rating": 7.9,
            "audienceRating": 8.2,
            "ratingCritic": 9.4,
            "duration": 6_963_000,
            "viewCount": 3,
            "lastViewedAt": 1_700_000_000,
            "viewOffset": 0,
            "originallyAvailableAt": "2016-11-11",
            "addedAt": 1_612_345_000,
            "updatedAt": 1_700_000_099,
            "thumb": "/library/metadata/12345/thumb/1700000099",
            "art": "/library/metadata/12345/art/1700000099",
            "guid": "plex://movie/5d776829151a60001f2436f1"
        });
        let dto: MetadataDto = serde_json::from_value(body).unwrap();
        let m = dto.into_movie(fixture_ref()).unwrap();
        assert_eq!(m.title, "Arrival");
        assert_eq!(m.year, Some(2016));
        assert_eq!(m.duration_ms, Some(6_963_000));
        assert_eq!(m.rating, Some(7.9));
        assert_eq!(m.audience_rating, Some(8.2));
        assert_eq!(m.critic_rating, Some(9.4));
        assert_eq!(m.view_count, 3);
        assert!(m.is_played());
        assert_eq!(m.originally_available_at.as_deref(), Some("2016-11-11"));
        assert_eq!(m.added_at, Some(1_612_345_000));
        assert!(m.thumb.is_some());
        assert!(m.guid.unwrap().starts_with("plex://movie/"));
    }

    #[test]
    fn movie_dto_rejects_non_numeric_rating_key() {
        let body = serde_json::json!({
            "ratingKey": "abc",
            "key": "/library/metadata/abc",
            "title": "Test"
        });
        let dto: MetadataDto = serde_json::from_value(body).unwrap();
        let err = dto.into_movie(fixture_ref()).unwrap_err();
        assert!(matches!(err, Error::Config(_)));
    }

    #[test]
    fn thumb_url_resolves_against_base() {
        let body = serde_json::json!({
            "ratingKey": "1",
            "key": "/library/metadata/1",
            "title": "X",
            "thumb": "/library/metadata/1/thumb/100"
        });
        let dto: MetadataDto = serde_json::from_value(body).unwrap();
        let m = dto.into_movie(fixture_ref()).unwrap();
        let url = m.thumb_url().unwrap().unwrap();
        assert_eq!(url.path(), "/library/metadata/1/thumb/100");
        assert_eq!(url.host_str(), Some("plex.local"));
    }

    #[test]
    fn thumb_url_returns_none_when_thumb_absent() {
        let body = serde_json::json!({
            "ratingKey": "1",
            "key": "/library/metadata/1",
            "title": "X"
        });
        let dto: MetadataDto = serde_json::from_value(body).unwrap();
        let m = dto.into_movie(fixture_ref()).unwrap();
        assert!(m.thumb_url().unwrap().is_none());
    }
}
