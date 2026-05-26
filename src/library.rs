//! [`Library`] and [`LibrarySection`] — the section listing surface.
//!
//! Plex Media Server organises content into sections (Movies, TV
//! Shows, Music, Photos, …). [`Library::sections`] returns the full
//! list as typed [`LibrarySection`] values; each one carries a
//! [`LibrarySectionRef`] that future mutation traits can use to call
//! back into the right `/library/sections/<id>/...` endpoint.
//!
//! See [`analysis/05-library-and-search.md`](../analysis/05-library-and-search.md)
//! for the python-plexapi parity baseline.

use std::fmt;

use serde::Deserialize;
use url::Url;

use crate::client::HttpClient;
use crate::error::{Error, Result};
use crate::media::video::MetadataDto;
use crate::media::{Artist, Movie, Photoalbum, Show};
use crate::server::join_path;
use crate::xml::MediaContainer;

// -----------------------------------------------------------------------------
// SectionKind.
// -----------------------------------------------------------------------------

/// Discriminator for what a [`LibrarySection`] contains.
///
/// Plex's `type` attribute on `<Directory>` is one of `movie`, `show`,
/// `artist`, `photo`. Anything else lands in [`Other`](Self::Other)
/// so we don't crash on a future Plex addition.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SectionKind {
    /// `type=movie` — a Movies section.
    Movie,
    /// `type=show` — a TV Shows section.
    Show,
    /// `type=artist` — a Music section.
    Music,
    /// `type=photo` — a Photos section.
    Photo,
    /// Anything else — the raw `type` string is preserved verbatim.
    Other(String),
}

impl SectionKind {
    /// Map Plex's wire-format `type` string to a typed variant.
    #[must_use]
    pub fn from_wire(value: &str) -> Self {
        match value {
            "movie" => Self::Movie,
            "show" => Self::Show,
            "artist" => Self::Music,
            "photo" => Self::Photo,
            other => Self::Other(other.to_owned()),
        }
    }

    /// Reverse mapping: the canonical wire string.
    #[must_use]
    pub fn as_wire(&self) -> &str {
        match self {
            Self::Movie => "movie",
            Self::Show => "show",
            Self::Music => "artist",
            Self::Photo => "photo",
            Self::Other(s) => s,
        }
    }
}

impl fmt::Display for SectionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_wire())
    }
}

// -----------------------------------------------------------------------------
// LibrarySectionRef — the back-reference editable items need.
// -----------------------------------------------------------------------------

/// Back-reference an editable leaf needs to reach its parent section.
///
/// Per [`analysis/11-rust-mapping-recommendations.md`](../analysis/11-rust-mapping-recommendations.md)
/// §2.4, the PMS edit endpoint is
/// `PUT /library/sections/<sectionKey>/all?id=<ratingKey>…`, not the
/// intuitive `/library/metadata/<ratingKey>?…`. Every editable leaf
/// type therefore carries a `LibrarySectionRef` so it can construct
/// that URL without traversing through `PlexServer`.
#[derive(Clone)]
pub struct LibrarySectionRef {
    /// Numeric section ID (`key` attribute on the `<Directory>`).
    pub id: u32,
    /// Cheap-clone HTTP handle bound to this section's PMS.
    pub http: HttpClient,
    /// Base URL of the owning PMS.
    pub base_url: Url,
}

impl LibrarySectionRef {
    /// Construct a section-level URL of the form
    /// `<base>/library/sections/<id><suffix>`.
    ///
    /// # Errors
    /// Returns [`crate::Error::Url`] when `base_url` is malformed.
    pub fn url(&self, suffix: &str) -> Result<Url> {
        let path = format!("/library/sections/{}{}", self.id, suffix);
        join_path(&self.base_url, &path)
    }
}

impl fmt::Debug for LibrarySectionRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LibrarySectionRef")
            .field("id", &self.id)
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

// -----------------------------------------------------------------------------
// LibrarySection — one entry returned by /library/sections.
// -----------------------------------------------------------------------------

/// One library section.
///
/// Carries the descriptive fields PMS emits on each `<Directory>` plus
/// a [`LibrarySectionRef`] that callers can pass to mutation
/// operations once those land in M3.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct LibrarySection {
    /// Discriminator (`type` attribute).
    pub kind: SectionKind,
    /// User-visible title (e.g. "Movies", "Documentaries").
    pub title: String,
    /// Stable section UUID; used to construct `library://` URIs.
    pub uuid: String,
    /// Agent identifier (`com.plexapp.agents.imdb`, etc.).
    pub agent: Option<String>,
    /// Scanner identifier (`Plex Movie Scanner`, etc.).
    pub scanner: Option<String>,
    /// Section language (`en`, `de`, …).
    pub language: Option<String>,
    /// When the section was first created (epoch seconds).
    pub created_at: Option<i64>,
    /// When the section was last updated (epoch seconds).
    pub updated_at: Option<i64>,
    /// Whether the section is enabled for sync.
    pub allow_sync: bool,
    /// Per-item refresh granularity (Plex's `refreshing` flag).
    pub refreshing: bool,
    /// Back-ref for edit operations.
    pub section_ref: LibrarySectionRef,
}

impl LibrarySection {
    /// Convenience accessor for the numeric section ID.
    #[must_use]
    pub const fn id(&self) -> u32 {
        self.section_ref.id
    }

    /// List every movie in this section.
    ///
    /// Equivalent to python-plexapi's `MovieSection.all()`. Calls
    /// `GET /library/sections/<id>/all` with `?type=1` (Plex's movie
    /// search-type discriminator) and parses the
    /// `MediaContainer.Metadata[]` payload.
    ///
    /// # Errors
    /// - [`Error::Config`] if this section's [`kind`](Self::kind) is
    ///   not [`SectionKind::Movie`] — calling `movies()` on a
    ///   non-movie section is a programmer error.
    /// - Any [`Error`] variant from the underlying transport.
    pub async fn movies(&self) -> Result<Vec<Movie>> {
        self.list_typed(SectionKind::Movie, "1", "movie", MetadataDto::into_movie)
            .await
    }

    /// List every show in this section.
    ///
    /// Equivalent to python-plexapi's `ShowSection.all()`. Calls
    /// `GET /library/sections/<id>/all?type=2` and parses each
    /// `<Video type="show">` into a [`Show`].
    ///
    /// # Errors
    /// - [`Error::Config`] if this section is not [`SectionKind::Show`].
    /// - Any transport [`Error`] from the underlying [`HttpClient`].
    pub async fn shows(&self) -> Result<Vec<Show>> {
        self.list_typed(SectionKind::Show, "2", "show", MetadataDto::into_show)
            .await
    }

    /// List every artist in this section.
    ///
    /// Calls `GET /library/sections/<id>/all?type=8` and parses each
    /// `<Directory type="artist">` into an [`Artist`].
    ///
    /// # Errors
    /// - [`Error::Config`] if this section is not [`SectionKind::Music`].
    /// - Any transport [`Error`].
    pub async fn artists(&self) -> Result<Vec<Artist>> {
        self.list_typed(SectionKind::Music, "8", "artist", MetadataDto::into_artist)
            .await
    }

    /// List every top-level photo album in this section.
    ///
    /// Calls `GET /library/sections/<id>/all?type=14`. Sub-albums and
    /// photos are reached via [`Photoalbum::children`] /
    /// [`Photoalbum::sub_albums`] / [`Photoalbum::photos`].
    ///
    /// # Errors
    /// - [`Error::Config`] if this section is not [`SectionKind::Photo`].
    /// - Any transport [`Error`].
    pub async fn photoalbums(&self) -> Result<Vec<Photoalbum>> {
        self.list_typed(
            SectionKind::Photo,
            "14",
            "photo",
            MetadataDto::into_photoalbum,
        )
        .await
    }

    /// Internal helper: ensure the section's kind matches, fetch
    /// `/library/sections/<id>/all?type=<n>`, and convert each
    /// `MetadataDto` with the caller-supplied closure.
    async fn list_typed<T, F>(
        &self,
        expected: SectionKind,
        type_param: &str,
        type_label: &str,
        convert: F,
    ) -> Result<Vec<T>>
    where
        F: Fn(MetadataDto, LibrarySectionRef) -> Result<T>,
    {
        if self.kind != expected {
            return Err(Error::Config(format!(
                "section {:?} is {} not {type_label}",
                self.title, self.kind,
            )));
        }
        let url = self.section_ref.url(&format!("/all?type={type_param}"))?;
        let body = self.section_ref.http.get_bytes(url.as_str()).await?;
        let body_str = std::str::from_utf8(&body).map_err(|e| {
            Error::Config(format!(
                "sections/{}/all body not utf-8: {e}",
                self.section_ref.id
            ))
        })?;
        let mc: MediaContainer<MetadataDto> = MediaContainer::from_json(body_str, "Metadata")?;
        mc.items
            .into_iter()
            .map(|dto| convert(dto, self.section_ref.clone()))
            .collect()
    }
}

// -----------------------------------------------------------------------------
// DTO + conversion.
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectoryDto {
    /// Numeric section id; Plex emits it as a string in JSON.
    key: String,
    #[serde(rename = "type")]
    kind: String,
    title: String,
    #[serde(default)]
    uuid: Option<String>,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    scanner: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    created_at: Option<i64>,
    #[serde(default)]
    updated_at: Option<i64>,
    #[serde(default)]
    allow_sync: Option<crate::server::PlexBoolField>,
    #[serde(default)]
    refreshing: Option<crate::server::PlexBoolField>,
}

impl DirectoryDto {
    fn into_section(self, http: HttpClient, base_url: Url) -> Result<LibrarySection> {
        let id: u32 = self.key.parse().map_err(|e| {
            crate::Error::Config(format!(
                "library section key {:?} not numeric: {e}",
                self.key
            ))
        })?;
        Ok(LibrarySection {
            kind: SectionKind::from_wire(&self.kind),
            title: self.title,
            uuid: self.uuid.unwrap_or_default(),
            agent: self.agent,
            scanner: self.scanner,
            language: self.language,
            created_at: self.created_at,
            updated_at: self.updated_at,
            allow_sync: self.allow_sync.is_some_and(|b| b.to_bool()),
            refreshing: self.refreshing.is_some_and(|b| b.to_bool()),
            section_ref: LibrarySectionRef { id, http, base_url },
        })
    }
}

// -----------------------------------------------------------------------------
// Library — the listing surface.
// -----------------------------------------------------------------------------

/// Library handle bound to a specific PMS.
///
/// Cheap to construct from [`crate::PlexServer::library`]; holds a
/// cloned [`HttpClient`].
#[derive(Clone)]
pub struct Library {
    http: HttpClient,
    base_url: Url,
}

impl Library {
    /// Construct directly. Most callers should go through
    /// [`crate::PlexServer::library`] instead.
    #[must_use]
    pub const fn new(http: HttpClient, base_url: Url) -> Self {
        Self { http, base_url }
    }

    /// List every library section on the server.
    ///
    /// Calls `GET /library/sections` and parses the
    /// `MediaContainer.Directory[]` payload.
    ///
    /// # Errors
    /// Any [`crate::Error`] variant; in particular,
    /// [`crate::Error::Unauthorized`] when the bound token is invalid
    /// or expired.
    pub async fn sections(&self) -> Result<Vec<LibrarySection>> {
        let url = join_path(&self.base_url, "/library/sections")?;
        let body = self.http.get_bytes(url.as_str()).await?;
        let body_str = std::str::from_utf8(&body)
            .map_err(|e| crate::Error::Config(format!("library/sections body not utf-8: {e}")))?;
        let mc: MediaContainer<DirectoryDto> = MediaContainer::from_json(body_str, "Directory")?;
        mc.items
            .into_iter()
            .map(|dto| dto.into_section(self.http.clone(), self.base_url.clone()))
            .collect()
    }
}

impl fmt::Debug for Library {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Library")
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_kind_round_trips_known_values() {
        for k in [
            SectionKind::Movie,
            SectionKind::Show,
            SectionKind::Music,
            SectionKind::Photo,
        ] {
            assert_eq!(SectionKind::from_wire(k.as_wire()), k);
        }
    }

    #[test]
    fn section_kind_other_preserves_string() {
        let k = SectionKind::from_wire("podcast");
        assert_eq!(k.as_wire(), "podcast");
        assert_eq!(k.to_string(), "podcast");
    }

    #[test]
    fn library_section_ref_url_builds_correctly() {
        // Construct a section-ref without going through the network.
        let cfg = crate::ClientConfig::builder(crate::ClientIdentifier::new("t").unwrap())
            .build()
            .unwrap();
        let http = HttpClient::new(cfg).unwrap();
        let base = Url::parse("http://plex.local:32400/").unwrap();
        let r = LibrarySectionRef {
            id: 5,
            http,
            base_url: base,
        };
        let url = r.url("/all?type=1&year=2024").unwrap();
        assert_eq!(url.path(), "/library/sections/5/all");
        assert_eq!(url.query(), Some("type=1&year=2024"));
    }

    #[test]
    fn directory_dto_parses_movie_section() {
        let body = r#"{
            "MediaContainer": {
                "size": 1,
                "Directory": [{
                    "key": "1",
                    "type": "movie",
                    "title": "Movies",
                    "uuid": "abc-uuid",
                    "agent": "com.plexapp.agents.imdb",
                    "scanner": "Plex Movie Scanner",
                    "language": "en",
                    "createdAt": 1700000000,
                    "updatedAt": 1700000999,
                    "allowSync": "1",
                    "refreshing": false
                }]
            }
        }"#;
        let mc: MediaContainer<DirectoryDto> =
            MediaContainer::from_json(body, "Directory").unwrap();
        assert_eq!(mc.items.len(), 1);
        let cfg = crate::ClientConfig::builder(crate::ClientIdentifier::new("t").unwrap())
            .build()
            .unwrap();
        let http = HttpClient::new(cfg).unwrap();
        let base = Url::parse("http://plex.local:32400/").unwrap();
        let section = mc
            .items
            .into_iter()
            .next()
            .unwrap()
            .into_section(http, base)
            .unwrap();
        assert_eq!(section.kind, SectionKind::Movie);
        assert_eq!(section.title, "Movies");
        assert_eq!(section.uuid, "abc-uuid");
        assert_eq!(section.id(), 1);
        assert_eq!(section.agent.as_deref(), Some("com.plexapp.agents.imdb"));
        assert!(section.allow_sync);
        assert!(!section.refreshing);
    }

    #[test]
    fn directory_dto_rejects_non_numeric_key() {
        let body = r#"{
            "MediaContainer": {
                "size": 1,
                "Directory": [{"key": "not-a-number", "type": "movie", "title": "X"}]
            }
        }"#;
        let mc: MediaContainer<DirectoryDto> =
            MediaContainer::from_json(body, "Directory").unwrap();
        let cfg = crate::ClientConfig::builder(crate::ClientIdentifier::new("t").unwrap())
            .build()
            .unwrap();
        let http = HttpClient::new(cfg).unwrap();
        let base = Url::parse("http://plex.local:32400/").unwrap();
        let err = mc
            .items
            .into_iter()
            .next()
            .unwrap()
            .into_section(http, base)
            .unwrap_err();
        assert!(matches!(err, crate::Error::Config(_)));
    }

    #[test]
    fn unknown_kind_lands_in_other_variant() {
        let body = r#"{
            "MediaContainer": {
                "size": 1,
                "Directory": [{"key": "9", "type": "podcast", "title": "Podcasts"}]
            }
        }"#;
        let mc: MediaContainer<DirectoryDto> =
            MediaContainer::from_json(body, "Directory").unwrap();
        let cfg = crate::ClientConfig::builder(crate::ClientIdentifier::new("t").unwrap())
            .build()
            .unwrap();
        let http = HttpClient::new(cfg).unwrap();
        let base = Url::parse("http://plex.local:32400/").unwrap();
        let section = mc
            .items
            .into_iter()
            .next()
            .unwrap()
            .into_section(http, base)
            .unwrap();
        assert_eq!(section.kind, SectionKind::Other("podcast".to_owned()));
    }
}
