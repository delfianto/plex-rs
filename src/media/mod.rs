//! Plex media domain types.
//!
//! Hierarchically organised after the four upstream content classes:
//! [`video`] (Movie / Show / Season / Episode / Clip),
//! `audio` (Artist / Album / Track — lands in M2.3), and
//! `photo` (Photoalbum / Photo — lands in M2.4). Each leaf type is a plain
//! `#[non_exhaustive]` `struct` carrying the parsed scalar attributes
//! plus a `LibrarySectionRef` back-link so M3's edit traits can
//! construct mutation URLs without re-traversing through `PlexServer`
//! (see [`analysis/11-rust-mapping-recommendations.md`](../../analysis/11-rust-mapping-recommendations.md)
//! §2.4).
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
