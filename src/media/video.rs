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
use crate::traits::{
    EditField, EditSummary, EditTags, EditTitle, HasArtLock, HasArtUrl, HasCollections, HasGenres,
    HasPosterLock, HasPosterUrl, HasThemeLock, HasThemeUrl, PlayedUnplayed, PlexObject, Ratable,
};
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
    /// File / part / stream chain. Empty when not populated by the
    /// originating endpoint (most listing endpoints omit `Media[]`).
    pub media: Vec<super::streams::Media>,
    /// Genre / Director / Writer / Role / Collection / etc. tags
    /// aggregated into one flat list. Empty when not emitted.
    pub tags: Vec<super::tags::Tag>,
    /// Auto-detected intro / credits / commercial markers.
    pub markers: Vec<super::markers::Marker>,
    /// Embedded DVD-style chapter index.
    pub chapters: Vec<super::markers::Chapter>,
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

    /// Whether this movie has ever been played.
    ///
    /// Equivalent to [`crate::PlayedUnplayed::is_played`] but
    /// available without importing the trait, matching the parity
    /// expectation set by python-plexapi.
    #[must_use]
    pub const fn is_played(&self) -> bool {
        self.view_count > 0
    }
}

// -----------------------------------------------------------------------------
// Show — a TV series at the top of the season → episode hierarchy.
// -----------------------------------------------------------------------------

/// A Plex TV show — one entry under a `SectionKind::Show` library
/// section. Contains seasons (fetched via [`Show::seasons`]).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Show {
    /// Show's primary metadata identifier.
    pub rating_key: RatingKey,
    /// Relative key — `/library/metadata/<rating_key>`.
    pub key: String,
    /// Title.
    pub title: String,
    /// Sort title.
    pub title_sort: Option<String>,
    /// Plot summary.
    pub summary: Option<String>,
    /// Network / studio.
    pub studio: Option<String>,
    /// Content rating (`TV-MA`, `TV-PG`, …).
    pub content_rating: Option<String>,
    /// First-air year.
    pub year: Option<u16>,
    /// Plex's user rating, 0..=10.
    pub rating: Option<f32>,
    /// Audience score, 0..=10.
    pub audience_rating: Option<f32>,
    /// Episode duration in milliseconds (typical, not guaranteed).
    pub duration_ms: Option<u64>,
    /// `YYYY-MM-DD` first-air date of the pilot.
    pub originally_available_at: Option<String>,
    /// Number of seasons.
    pub child_count: Option<u32>,
    /// Total number of episodes in the show.
    pub leaf_count: Option<u32>,
    /// Number of episodes the user has played.
    pub viewed_leaf_count: Option<u32>,
    /// View count at the show level (rarely populated).
    pub view_count: u32,
    /// Last playback timestamp (epoch seconds).
    pub last_viewed_at: Option<i64>,
    /// Add timestamp (epoch seconds).
    pub added_at: Option<i64>,
    /// Metadata update timestamp (epoch seconds).
    pub updated_at: Option<i64>,
    /// Poster path.
    pub thumb: Option<String>,
    /// Background-art path.
    pub art: Option<String>,
    /// Theme-song path.
    pub theme: Option<String>,
    /// Primary GUID.
    pub guid: Option<String>,
    /// Genre / Country / Role / Collection / etc. tags.
    pub tags: Vec<super::tags::Tag>,
    /// Back-reference for M3 edits.
    pub section_ref: LibrarySectionRef,
}

impl Show {
    /// List the seasons of this show.
    ///
    /// Calls `GET /library/metadata/<rk>/children` and parses the
    /// response into [`Season`] values.
    ///
    /// # Errors
    /// Any [`Error`] variant; see [`crate::HttpClient`].
    pub async fn seasons(&self) -> Result<Vec<Season>> {
        list_children::<Season, _>(&self.section_ref, self.rating_key, |dto, sref| {
            dto.into_season(sref)
        })
        .await
    }

    /// Watched-fraction convenience: `viewed_leaf_count / leaf_count`
    /// (returns `None` when either count is missing or zero).
    #[must_use]
    pub fn watch_progress(&self) -> Option<f32> {
        match (self.leaf_count, self.viewed_leaf_count) {
            (Some(total), Some(viewed)) if total > 0 =>
            {
                #[allow(clippy::cast_precision_loss)]
                Some(viewed as f32 / total as f32)
            }
            _ => None,
        }
    }
}

// -----------------------------------------------------------------------------
// Season — under a Show, contains episodes.
// -----------------------------------------------------------------------------

/// A Plex season (one container under a [`Show`]).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Season {
    /// Season's metadata identifier.
    pub rating_key: RatingKey,
    /// Relative key — `/library/metadata/<rating_key>`.
    pub key: String,
    /// Season title (often `"Season N"`).
    pub title: String,
    /// Season number (`index` on the wire). `None` for "All Episodes"
    /// or specials placeholder seasons.
    pub index: Option<i32>,
    /// Show this season belongs to.
    pub parent_rating_key: RatingKey,
    /// Show key.
    pub parent_key: Option<String>,
    /// Show title.
    pub parent_title: Option<String>,
    /// Show poster (sometimes shown on the season list).
    pub parent_thumb: Option<String>,
    /// Number of episodes in this season.
    pub leaf_count: Option<u32>,
    /// Number of episodes the user has played.
    pub viewed_leaf_count: Option<u32>,
    /// Number of children (typically equals `leaf_count`).
    pub child_count: Option<u32>,
    /// Summary (often empty for seasons).
    pub summary: Option<String>,
    /// Season-poster path.
    pub thumb: Option<String>,
    /// Season-art path.
    pub art: Option<String>,
    /// Add timestamp (epoch seconds).
    pub added_at: Option<i64>,
    /// Metadata update timestamp (epoch seconds).
    pub updated_at: Option<i64>,
    /// Last playback timestamp (epoch seconds).
    pub last_viewed_at: Option<i64>,
    /// View count.
    pub view_count: u32,
    /// Primary GUID.
    pub guid: Option<String>,
    /// Back-reference for M3 edits.
    pub section_ref: LibrarySectionRef,
}

impl Season {
    /// List the episodes of this season.
    ///
    /// Calls `GET /library/metadata/<rk>/children`.
    ///
    /// # Errors
    /// Any [`Error`] variant.
    pub async fn episodes(&self) -> Result<Vec<Episode>> {
        list_children::<Episode, _>(&self.section_ref, self.rating_key, |dto, sref| {
            dto.into_episode(sref)
        })
        .await
    }
}

// -----------------------------------------------------------------------------
// Episode — leaf playable.
// -----------------------------------------------------------------------------

/// A Plex episode — a single playable leaf under a [`Season`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Episode {
    /// Episode's metadata identifier.
    pub rating_key: RatingKey,
    /// Relative key — `/library/metadata/<rating_key>`.
    pub key: String,
    /// Episode title.
    pub title: String,
    /// Episode-number-within-season (`index`).
    pub index: Option<i32>,
    /// Sort title.
    pub title_sort: Option<String>,
    /// Plot summary.
    pub summary: Option<String>,
    /// Content rating.
    pub content_rating: Option<String>,
    /// Air year.
    pub year: Option<u16>,
    /// User rating, 0..=10.
    pub rating: Option<f32>,
    /// Audience rating, 0..=10.
    pub audience_rating: Option<f32>,
    /// Duration in milliseconds.
    pub duration_ms: Option<u64>,
    /// Air date `YYYY-MM-DD`.
    pub originally_available_at: Option<String>,

    /// Season (parent) rating key.
    pub parent_rating_key: RatingKey,
    /// Season key.
    pub parent_key: Option<String>,
    /// Season title.
    pub parent_title: Option<String>,
    /// Season number (`parentIndex`).
    pub parent_index: Option<i32>,
    /// Season poster path.
    pub parent_thumb: Option<String>,

    /// Show (grandparent) rating key.
    pub grandparent_rating_key: RatingKey,
    /// Show key.
    pub grandparent_key: Option<String>,
    /// Show title.
    pub grandparent_title: Option<String>,
    /// Show poster path.
    pub grandparent_thumb: Option<String>,
    /// Show background-art path.
    pub grandparent_art: Option<String>,
    /// Show theme path.
    pub grandparent_theme: Option<String>,

    /// File / part / stream chain. Empty when not populated by the
    /// originating endpoint.
    pub media: Vec<super::streams::Media>,
    /// Director / Writer / etc. tags. Empty when not emitted.
    pub tags: Vec<super::tags::Tag>,
    /// Auto-detected intro / credits / commercial markers.
    pub markers: Vec<super::markers::Marker>,
    /// Embedded chapter index.
    pub chapters: Vec<super::markers::Chapter>,

    /// Episode poster path.
    pub thumb: Option<String>,
    /// Episode background art (rare).
    pub art: Option<String>,
    /// View count.
    pub view_count: u32,
    /// Last playback timestamp (epoch seconds).
    pub last_viewed_at: Option<i64>,
    /// Resume position in milliseconds.
    pub view_offset_ms: Option<u64>,
    /// Add timestamp (epoch seconds).
    pub added_at: Option<i64>,
    /// Metadata update timestamp (epoch seconds).
    pub updated_at: Option<i64>,
    /// Primary GUID.
    pub guid: Option<String>,
    /// Back-reference for M3 edits.
    pub section_ref: LibrarySectionRef,
}

impl Episode {
    /// Whether this episode has ever been played.
    ///
    /// Equivalent to [`crate::PlayedUnplayed::is_played`] but
    /// available without importing the trait.
    #[must_use]
    pub const fn is_played(&self) -> bool {
        self.view_count > 0
    }

    /// `Sn × Ee` short label (e.g. `"S02E07"`). Returns
    /// [`None`] when either index is missing.
    #[must_use]
    pub fn season_episode_label(&self) -> Option<String> {
        match (self.parent_index, self.index) {
            (Some(s), Some(e)) => Some(format!("S{s:02}E{e:02}")),
            _ => None,
        }
    }
}

// Shared helper: fetch /library/metadata/<rk>/children and convert
// each item with the caller-provided closure. Used by both the video
// and audio hierarchies (Show::seasons / Season::episodes /
// Artist::albums / Album::tracks).
pub(crate) async fn list_children<T, F>(
    section_ref: &LibrarySectionRef,
    rating_key: RatingKey,
    convert: F,
) -> Result<Vec<T>>
where
    F: Fn(MetadataDto, LibrarySectionRef) -> Result<T>,
{
    let path = format!("/library/metadata/{rating_key}/children");
    let url = section_ref.base_url.join(&path)?;
    let body = section_ref.http.get_bytes(url.as_str()).await?;
    let body_str = std::str::from_utf8(&body)
        .map_err(|e| Error::Config(format!("/children body not utf-8: {e}")))?;
    let mc: crate::xml::MediaContainer<MetadataDto> =
        crate::xml::MediaContainer::from_json(body_str, "Metadata")?;
    mc.items
        .into_iter()
        .map(|dto| convert(dto, section_ref.clone()))
        .collect()
}

/// Audio-side alias so `media::audio` doesn't have to import the
/// long path. Same function — both hierarchies use the same wire shape.
pub(crate) use self::list_children as list_children_audio;

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
    /// Plex's `type` discriminator on `<Video>` / `<Track>` /
    /// `<Photo>` / `<Directory>` (`movie`/`show`/`season`/
    /// `episode`/`clip`/`artist`/`album`/`track`/`photoalbum`/
    /// `photo`). Needed for mixed-content listings (e.g.
    /// `Photoalbum::children()`).
    #[serde(rename = "type", default)]
    pub(crate) metadata_type: Option<String>,
    // TV-hierarchy fields (Show / Season / Episode).
    #[serde(default)]
    pub(crate) index: Option<i32>,
    #[serde(default)]
    pub(crate) child_count: Option<u32>,
    #[serde(default)]
    pub(crate) leaf_count: Option<u32>,
    #[serde(default)]
    pub(crate) viewed_leaf_count: Option<u32>,
    #[serde(default)]
    pub(crate) theme: Option<String>,
    #[serde(default)]
    pub(crate) parent_rating_key: Option<String>,
    #[serde(default)]
    pub(crate) parent_key: Option<String>,
    #[serde(default)]
    pub(crate) parent_title: Option<String>,
    #[serde(default)]
    pub(crate) parent_index: Option<i32>,
    #[serde(default)]
    pub(crate) parent_thumb: Option<String>,
    #[serde(default)]
    pub(crate) grandparent_rating_key: Option<String>,
    #[serde(default)]
    pub(crate) grandparent_key: Option<String>,
    #[serde(default)]
    pub(crate) grandparent_title: Option<String>,
    #[serde(default)]
    pub(crate) grandparent_thumb: Option<String>,
    #[serde(default)]
    pub(crate) grandparent_art: Option<String>,
    #[serde(default)]
    pub(crate) grandparent_theme: Option<String>,
    /// Per-encoding Media chain (file / part / stream). Empty when
    /// the endpoint is a listing that doesn't include media metadata.
    #[serde(default, rename = "Media")]
    pub(crate) media: Vec<super::streams::MediaDto>,

    // Tag families — collected into a single Vec<Tag> by the
    // conversion methods. All optional / default-empty.
    #[serde(default, rename = "Genre")]
    pub(crate) genres: Vec<super::tags::TagDto>,
    #[serde(default, rename = "Director")]
    pub(crate) directors: Vec<super::tags::TagDto>,
    #[serde(default, rename = "Writer")]
    pub(crate) writers: Vec<super::tags::TagDto>,
    #[serde(default, rename = "Country")]
    pub(crate) countries: Vec<super::tags::TagDto>,
    #[serde(default, rename = "Producer")]
    pub(crate) producers: Vec<super::tags::TagDto>,
    #[serde(default, rename = "Role")]
    pub(crate) roles: Vec<super::tags::TagDto>,
    #[serde(default, rename = "Collection")]
    pub(crate) collections: Vec<super::tags::TagDto>,
    #[serde(default, rename = "Label")]
    pub(crate) labels: Vec<super::tags::TagDto>,
    #[serde(default, rename = "Mood")]
    pub(crate) moods: Vec<super::tags::TagDto>,
    #[serde(default, rename = "Style")]
    pub(crate) styles: Vec<super::tags::TagDto>,
    /// Intro / credits / commercial markers.
    #[serde(default, rename = "Marker")]
    pub(crate) markers: Vec<super::markers::MarkerDto>,
    /// Embedded chapter index.
    #[serde(default, rename = "Chapter")]
    pub(crate) chapters: Vec<super::markers::ChapterDto>,
}

impl MetadataDto {
    /// Collect every tag-family vector into a single `Vec<Tag>`.
    pub(crate) fn collect_tags(&mut self) -> Vec<super::tags::Tag> {
        super::tags::collect(super::tags::TagFamilies {
            genres: std::mem::take(&mut self.genres),
            directors: std::mem::take(&mut self.directors),
            writers: std::mem::take(&mut self.writers),
            countries: std::mem::take(&mut self.countries),
            producers: std::mem::take(&mut self.producers),
            roles: std::mem::take(&mut self.roles),
            collections: std::mem::take(&mut self.collections),
            labels: std::mem::take(&mut self.labels),
            moods: std::mem::take(&mut self.moods),
            styles: std::mem::take(&mut self.styles),
        })
    }
}

/// Parse a stringified rating key into a [`RatingKey`].
fn parse_rating_key(s: &str, field: &str) -> Result<RatingKey> {
    s.parse::<RatingKey>()
        .map_err(|e| Error::Config(format!("metadata.{field} not numeric: {e}")))
}

impl MetadataDto {
    pub(crate) fn into_show(mut self, section_ref: LibrarySectionRef) -> Result<Show> {
        let rating_key = parse_rating_key(&self.rating_key, "ratingKey")?;
        let tags = self.collect_tags();
        Ok(Show {
            rating_key,
            key: self.key,
            title: self.title,
            title_sort: self.title_sort,
            summary: self.summary,
            studio: self.studio,
            content_rating: self.content_rating,
            year: self.year,
            rating: self.rating,
            audience_rating: self.audience_rating,
            duration_ms: self.duration,
            originally_available_at: self.originally_available_at,
            child_count: self.child_count,
            leaf_count: self.leaf_count,
            viewed_leaf_count: self.viewed_leaf_count,
            view_count: self.view_count.unwrap_or(0),
            last_viewed_at: self.last_viewed_at,
            added_at: self.added_at,
            updated_at: self.updated_at,
            thumb: self.thumb,
            art: self.art,
            theme: self.theme,
            guid: self.guid,
            tags,
            section_ref,
        })
    }

    pub(crate) fn into_season(self, section_ref: LibrarySectionRef) -> Result<Season> {
        let rating_key = parse_rating_key(&self.rating_key, "ratingKey")?;
        let parent_rating_key = self
            .parent_rating_key
            .as_deref()
            .map(|s| parse_rating_key(s, "parentRatingKey"))
            .transpose()?
            .ok_or_else(|| Error::Config("season missing parentRatingKey".to_owned()))?;
        Ok(Season {
            rating_key,
            key: self.key,
            title: self.title,
            index: self.index,
            parent_rating_key,
            parent_key: self.parent_key,
            parent_title: self.parent_title,
            parent_thumb: self.parent_thumb,
            leaf_count: self.leaf_count,
            viewed_leaf_count: self.viewed_leaf_count,
            child_count: self.child_count,
            summary: self.summary,
            thumb: self.thumb,
            art: self.art,
            added_at: self.added_at,
            updated_at: self.updated_at,
            last_viewed_at: self.last_viewed_at,
            view_count: self.view_count.unwrap_or(0),
            guid: self.guid,
            section_ref,
        })
    }

    pub(crate) fn into_episode(mut self, section_ref: LibrarySectionRef) -> Result<Episode> {
        let rating_key = parse_rating_key(&self.rating_key, "ratingKey")?;
        let tags = self.collect_tags();
        let markers = self
            .markers
            .into_iter()
            .map(super::markers::MarkerDto::into_domain)
            .collect();
        let chapters = self
            .chapters
            .into_iter()
            .map(super::markers::ChapterDto::into_domain)
            .collect();
        let parent_rating_key = self
            .parent_rating_key
            .as_deref()
            .map(|s| parse_rating_key(s, "parentRatingKey"))
            .transpose()?
            .ok_or_else(|| Error::Config("episode missing parentRatingKey".to_owned()))?;
        let grandparent_rating_key = self
            .grandparent_rating_key
            .as_deref()
            .map(|s| parse_rating_key(s, "grandparentRatingKey"))
            .transpose()?
            .ok_or_else(|| Error::Config("episode missing grandparentRatingKey".to_owned()))?;
        let media = self
            .media
            .into_iter()
            .map(super::streams::MediaDto::into_domain)
            .collect();
        Ok(Episode {
            rating_key,
            key: self.key,
            title: self.title,
            index: self.index,
            title_sort: self.title_sort,
            summary: self.summary,
            content_rating: self.content_rating,
            year: self.year,
            rating: self.rating,
            audience_rating: self.audience_rating,
            duration_ms: self.duration,
            originally_available_at: self.originally_available_at,
            parent_rating_key,
            parent_key: self.parent_key,
            parent_title: self.parent_title,
            parent_index: self.parent_index,
            parent_thumb: self.parent_thumb,
            grandparent_rating_key,
            grandparent_key: self.grandparent_key,
            grandparent_title: self.grandparent_title,
            grandparent_thumb: self.grandparent_thumb,
            grandparent_art: self.grandparent_art,
            grandparent_theme: self.grandparent_theme,
            media,
            tags,
            markers,
            chapters,
            thumb: self.thumb,
            art: self.art,
            view_count: self.view_count.unwrap_or(0),
            last_viewed_at: self.last_viewed_at,
            view_offset_ms: self.view_offset,
            added_at: self.added_at,
            updated_at: self.updated_at,
            guid: self.guid,
            section_ref,
        })
    }

    pub(crate) fn into_movie(mut self, section_ref: LibrarySectionRef) -> Result<Movie> {
        let rating_key = parse_rating_key(&self.rating_key, "ratingKey")?;
        let tags = self.collect_tags();
        let media = self
            .media
            .into_iter()
            .map(super::streams::MediaDto::into_domain)
            .collect();
        let markers = self
            .markers
            .into_iter()
            .map(super::markers::MarkerDto::into_domain)
            .collect();
        let chapters = self
            .chapters
            .into_iter()
            .map(super::markers::ChapterDto::into_domain)
            .collect();
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
            media,
            tags,
            markers,
            chapters,
            section_ref,
        })
    }
}

// -----------------------------------------------------------------------------
// Trait impls — PlexObject + PlayedUnplayed for the playable leaves.
// -----------------------------------------------------------------------------

macro_rules! impl_plex_object {
    ($ty:ty, $type_id:expr) => {
        impl PlexObject for $ty {
            fn section_ref(&self) -> &LibrarySectionRef {
                &self.section_ref
            }
            fn rating_key(&self) -> RatingKey {
                self.rating_key
            }
            fn metadata_type_id(&self) -> u32 {
                $type_id
            }
        }
    };
}

impl_plex_object!(Movie, 1);
impl_plex_object!(Show, 2);
impl_plex_object!(Season, 3);
impl_plex_object!(Episode, 4);

impl PlayedUnplayed for Movie {
    fn view_count(&self) -> u32 {
        self.view_count
    }
}
impl PlayedUnplayed for Episode {
    fn view_count(&self) -> u32 {
        self.view_count
    }
}
impl PlayedUnplayed for Show {
    fn view_count(&self) -> u32 {
        self.view_count
    }
}
impl PlayedUnplayed for Season {
    fn view_count(&self) -> u32 {
        self.view_count
    }
}

impl Ratable for Movie {}
impl Ratable for Show {}
impl Ratable for Episode {}
// Season ratings exist on the wire but are rarely user-set; leaving
// the impl off for now and adding when we find a real use case.

impl EditField for Movie {}
impl EditField for Show {}
impl EditField for Season {}
impl EditField for Episode {}

impl EditTitle for Movie {}
impl EditTitle for Show {}
impl EditTitle for Season {}
impl EditTitle for Episode {}

impl EditSummary for Movie {}
impl EditSummary for Show {}
impl EditSummary for Season {}
impl EditSummary for Episode {}

impl EditTags for Movie {}
impl EditTags for Show {}
impl EditTags for Episode {}

impl HasGenres for Movie {}
impl HasGenres for Show {}
impl HasGenres for Episode {}

impl HasCollections for Movie {}
impl HasCollections for Show {}
impl HasCollections for Episode {}

macro_rules! impl_has_art {
    ($ty:ty) => {
        impl HasArtUrl for $ty {
            fn art_path(&self) -> Option<&str> {
                self.art.as_deref()
            }
        }
        impl HasArtLock for $ty {}
        impl HasPosterUrl for $ty {
            fn thumb_path(&self) -> Option<&str> {
                self.thumb.as_deref()
            }
        }
        impl HasPosterLock for $ty {}
    };
}

impl_has_art!(Movie);
impl_has_art!(Show);
impl_has_art!(Season);
impl_has_art!(Episode);

impl HasThemeUrl for Show {
    fn theme_path(&self) -> Option<&str> {
        self.theme.as_deref()
    }
}
impl HasThemeLock for Show {}

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
