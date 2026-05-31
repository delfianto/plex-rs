//! Plex media domain types.
//!
//! Hierarchically organised after the four upstream content classes:
//! [`video`] (Movie / Show / Season / Episode / Clip),
//! `audio` (Artist / Album / Track — lands in M2.3), and
//! `photo` (Photoalbum / Photo — lands in M2.4). Each leaf type is a plain
//! `#[non_exhaustive]` `struct` carrying the parsed scalar attributes
//! plus a `LibrarySectionRef` back-link so M3's edit traits can
//! construct mutation URLs without re-traversing through `PlexServer`.
//!
//! At M2.1 only [`video::Movie`] is implemented; Show / Season /
//! Episode follow in M2.2, music in M2.3, photos in M2.4.

pub mod audio;
pub mod collection;
pub mod markers;
pub mod photo;
pub mod playlist;
pub mod streams;
pub mod tags;
pub mod video;

pub use audio::{Album, Artist, Track};
pub use collection::Collection;
pub use markers::{Chapter, Marker, MarkerKind};
pub use photo::{Photo, PhotoEntry, Photoalbum};
pub use playlist::{Playlist, PlaylistKind};
pub use streams::{
    AudioStream, LyricStream, Media, MediaPart, Stream, StreamCommon, SubtitleStream,
    UnknownStream, VideoStream,
};
pub use tags::{Tag, TagKind};
pub use video::{Episode, Movie, Season, Show};

use crate::error::{Error, Result};
use crate::library::LibrarySectionRef;
use crate::media::video::MetadataDto;

// -----------------------------------------------------------------------------
// LibraryItem — sum type for endpoints that return mixed content.
// -----------------------------------------------------------------------------

/// A single metadata item, discriminated by Plex's wire `type` field.
///
/// Endpoints that return mixed-content listings (search,
/// recentlyAdded, onDeck, hub search) yield a `Vec<LibraryItem>`.
/// Pattern-match to recover the concrete leaf type.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum LibraryItem {
    /// A movie (`type="movie"`).
    Movie(Movie),
    /// A TV show (`type="show"`).
    Show(Show),
    /// A TV season (`type="season"`).
    Season(Season),
    /// A TV episode (`type="episode"`).
    Episode(Episode),
    /// A musical artist (`type="artist"`).
    Artist(Artist),
    /// A music album (`type="album"`).
    Album(Album),
    /// A music track (`type="track"`).
    Track(Track),
    /// A photo album (`type="photoalbum"`).
    Photoalbum(Photoalbum),
    /// A photo or photo-library clip (`type="photo"` / `"clip"`).
    Photo(Photo),
}

impl LibraryItem {
    /// Borrow the item's title regardless of variant.
    #[must_use]
    pub fn title(&self) -> &str {
        match self {
            Self::Movie(m) => &m.title,
            Self::Show(s) => &s.title,
            Self::Season(s) => &s.title,
            Self::Episode(e) => &e.title,
            Self::Artist(a) => &a.title,
            Self::Album(a) => &a.title,
            Self::Track(t) => &t.title,
            Self::Photoalbum(p) => &p.title,
            Self::Photo(p) => &p.title,
        }
    }

    /// Borrow the item's rating key regardless of variant.
    #[must_use]
    pub const fn rating_key(&self) -> crate::RatingKey {
        match self {
            Self::Movie(m) => m.rating_key,
            Self::Show(s) => s.rating_key,
            Self::Season(s) => s.rating_key,
            Self::Episode(e) => e.rating_key,
            Self::Artist(a) => a.rating_key,
            Self::Album(a) => a.rating_key,
            Self::Track(t) => t.rating_key,
            Self::Photoalbum(p) => p.rating_key,
            Self::Photo(p) => p.rating_key,
        }
    }

    /// Borrow the item's wire key (`/library/metadata/<rating-key>`).
    #[must_use]
    pub fn key(&self) -> &str {
        match self {
            Self::Movie(m) => &m.key,
            Self::Show(s) => &s.key,
            Self::Season(s) => &s.key,
            Self::Episode(e) => &e.key,
            Self::Artist(a) => &a.key,
            Self::Album(a) => &a.key,
            Self::Track(t) => &t.key,
            Self::Photoalbum(p) => &p.key,
            Self::Photo(p) => &p.key,
        }
    }

    /// The Plex "list type" used as the `type=` query parameter when
    /// creating a `PlayQueue`. `"video"` for movies / TV; `"audio"`
    /// for music; `"photo"` for photos.
    #[must_use]
    pub const fn list_type(&self) -> &'static str {
        match self {
            Self::Movie(_) | Self::Show(_) | Self::Season(_) | Self::Episode(_) => "video",
            Self::Artist(_) | Self::Album(_) | Self::Track(_) => "audio",
            Self::Photoalbum(_) | Self::Photo(_) => "photo",
        }
    }
}

impl MetadataDto {
    /// Dispatch on the wire `type` to construct the right
    /// [`LibraryItem`] variant. Used by every mixed-content listing
    /// (search, recentlyAdded, onDeck, hub search).
    pub(crate) fn into_library_item(self, section_ref: LibrarySectionRef) -> Result<LibraryItem> {
        let ty = self.metadata_type.as_deref().unwrap_or_default().to_owned();
        match ty.as_str() {
            "movie" => Ok(LibraryItem::Movie(self.into_movie(section_ref)?)),
            "show" => Ok(LibraryItem::Show(self.into_show(section_ref)?)),
            "season" => Ok(LibraryItem::Season(self.into_season(section_ref)?)),
            "episode" => Ok(LibraryItem::Episode(self.into_episode(section_ref)?)),
            "artist" => Ok(LibraryItem::Artist(self.into_artist(section_ref)?)),
            "album" => Ok(LibraryItem::Album(self.into_album(section_ref)?)),
            "track" => Ok(LibraryItem::Track(self.into_track(section_ref)?)),
            "photoalbum" => Ok(LibraryItem::Photoalbum(self.into_photoalbum(section_ref)?)),
            "photo" | "clip" => Ok(LibraryItem::Photo(self.into_photo(section_ref)?)),
            "" => Err(Error::Config(
                "metadata element missing wire `type` discriminator".to_owned(),
            )),
            _ => Err(Error::Config(format!("unknown metadata type {ty:?}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HttpClient;
    use url::Url;

    fn fixture_ref() -> LibrarySectionRef {
        let cfg = crate::ClientConfig::builder(crate::ClientIdentifier::new("t").unwrap())
            .build()
            .unwrap();
        let http = HttpClient::new(cfg).unwrap();
        LibrarySectionRef {
            id: 1,
            http,
            base_url: Url::parse("http://plex.local:32400/").unwrap(),
        }
    }

    /// Build a `LibraryItem` of the given wire `type` from a minimal
    /// metadata DTO, supplying the parent/grandparent keys the TV /
    /// music hierarchies require.
    fn item(ty: &str) -> LibraryItem {
        let body = serde_json::json!({
            "ratingKey": "42",
            "key": "/library/metadata/42",
            "title": "Item Title",
            "type": ty,
            "parentRatingKey": "7",
            "grandparentRatingKey": "3",
        });
        let dto: MetadataDto = serde_json::from_value(body).unwrap();
        dto.into_library_item(fixture_ref()).unwrap()
    }

    #[test]
    fn accessors_agree_across_every_variant() {
        for ty in [
            "movie",
            "show",
            "season",
            "episode",
            "artist",
            "album",
            "track",
            "photoalbum",
            "photo",
            "clip",
        ] {
            let it = item(ty);
            assert_eq!(it.title(), "Item Title", "title wrong for {ty}");
            assert_eq!(it.key(), "/library/metadata/42", "key wrong for {ty}");
            assert_eq!(it.rating_key().get(), 42, "rating_key wrong for {ty}");
        }
    }

    #[test]
    fn variant_dispatch_picks_the_right_leaf() {
        assert!(matches!(item("movie"), LibraryItem::Movie(_)));
        assert!(matches!(item("show"), LibraryItem::Show(_)));
        assert!(matches!(item("season"), LibraryItem::Season(_)));
        assert!(matches!(item("episode"), LibraryItem::Episode(_)));
        assert!(matches!(item("artist"), LibraryItem::Artist(_)));
        assert!(matches!(item("album"), LibraryItem::Album(_)));
        assert!(matches!(item("track"), LibraryItem::Track(_)));
        assert!(matches!(item("photoalbum"), LibraryItem::Photoalbum(_)));
        // Both "photo" and "clip" map to the Photo variant.
        assert!(matches!(item("photo"), LibraryItem::Photo(_)));
        assert!(matches!(item("clip"), LibraryItem::Photo(_)));
    }

    #[test]
    fn list_type_groups_by_content_family() {
        assert_eq!(item("movie").list_type(), "video");
        assert_eq!(item("show").list_type(), "video");
        assert_eq!(item("season").list_type(), "video");
        assert_eq!(item("episode").list_type(), "video");
        assert_eq!(item("artist").list_type(), "audio");
        assert_eq!(item("album").list_type(), "audio");
        assert_eq!(item("track").list_type(), "audio");
        assert_eq!(item("photoalbum").list_type(), "photo");
        assert_eq!(item("photo").list_type(), "photo");
    }

    #[test]
    fn into_library_item_rejects_missing_type() {
        let body = serde_json::json!({
            "ratingKey": "1",
            "key": "/library/metadata/1",
            "title": "No Type",
        });
        let dto: MetadataDto = serde_json::from_value(body).unwrap();
        let err = dto.into_library_item(fixture_ref()).unwrap_err();
        assert!(matches!(err, Error::Config(_)));
    }

    #[test]
    fn into_library_item_rejects_unknown_type() {
        let body = serde_json::json!({
            "ratingKey": "1",
            "key": "/library/metadata/1",
            "title": "Mystery",
            "type": "hologram",
        });
        let dto: MetadataDto = serde_json::from_value(body).unwrap();
        let err = dto.into_library_item(fixture_ref()).unwrap_err();
        assert!(matches!(err, Error::Config(_)));
    }
}
