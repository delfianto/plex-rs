//! Plex `type=` search-type discriminator.
//!
//! Many PMS endpoints accept (and several require) a `type=N` query
//! parameter that selects the metadata kind to operate on. The
//! integer values are defined in
//! `python-plexapi/plexapi/utils.py:35` and reproduced verbatim here.
//!
//! The enum is non-exhaustive: Plex has added values over time (e.g.
//! `optimizedVersion=42`) and may add more. Callers can read an
//! unknown numeric value via [`SearchType::from_u32`] returning
//! [`SearchType::Unknown`] for forward-compatibility.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::Error;

/// Plex metadata-type discriminator.
///
/// Discriminants match the integers Plex sends and expects on the wire.
/// [`SearchType::Unknown`] captures any value Plex adds later so that
/// parsing a new response does not break older callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "u32", from = "u32")]
#[allow(missing_docs, clippy::enum_variant_names)] // Variants are self-describing.
pub enum SearchType {
    // Wire-integer mapping lives in [`SearchType::as_u32`] /
    // [`SearchType::from_u32`]. The match arms there are the source
    // of truth; do not add `= N` discriminants here (Rust disallows
    // explicit discriminants when one variant carries data, and
    // duplicating the table invites drift).
    Movie,
    Show,
    Season,
    Episode,
    Trailer,
    Comic,
    Person,
    Artist,
    Album,
    Track,
    Picture,
    Clip,
    Photo,
    Photoalbum,
    Playlist,
    PlaylistFolder,
    Collection,
    OptimizedVersion,
    UserPlaylistItem,
    /// Forward-compatibility escape hatch: any numeric value Plex
    /// emits that this build does not yet recognise. Wire value is
    /// the contained `u32`.
    Unknown(u32),
}

impl SearchType {
    /// Map a numeric value into a [`SearchType`]. Unknown values land
    /// in [`SearchType::Unknown`] rather than failing — Plex
    /// occasionally adds new metadata kinds.
    #[must_use]
    pub const fn from_u32(value: u32) -> Self {
        match value {
            1 => Self::Movie,
            2 => Self::Show,
            3 => Self::Season,
            4 => Self::Episode,
            5 => Self::Trailer,
            6 => Self::Comic,
            7 => Self::Person,
            8 => Self::Artist,
            9 => Self::Album,
            10 => Self::Track,
            11 => Self::Picture,
            12 => Self::Clip,
            13 => Self::Photo,
            14 => Self::Photoalbum,
            15 => Self::Playlist,
            16 => Self::PlaylistFolder,
            18 => Self::Collection,
            42 => Self::OptimizedVersion,
            1001 => Self::UserPlaylistItem,
            other => Self::Unknown(other),
        }
    }

    /// Reverse of [`SearchType::from_u32`]: render this type as the
    /// integer Plex expects on the wire.
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        match self {
            Self::Movie => 1,
            Self::Show => 2,
            Self::Season => 3,
            Self::Episode => 4,
            Self::Trailer => 5,
            Self::Comic => 6,
            Self::Person => 7,
            Self::Artist => 8,
            Self::Album => 9,
            Self::Track => 10,
            Self::Picture => 11,
            Self::Clip => 12,
            Self::Photo => 13,
            Self::Photoalbum => 14,
            Self::Playlist => 15,
            Self::PlaylistFolder => 16,
            Self::Collection => 18,
            Self::OptimizedVersion => 42,
            Self::UserPlaylistItem => 1001,
            Self::Unknown(n) => n,
        }
    }

    /// Canonical name the Plex API uses for this kind (`"movie"`,
    /// `"show"`, …). Returns [`None`] for [`SearchType::Unknown`].
    #[must_use]
    pub const fn as_name(self) -> Option<&'static str> {
        Some(match self {
            Self::Movie => "movie",
            Self::Show => "show",
            Self::Season => "season",
            Self::Episode => "episode",
            Self::Trailer => "trailer",
            Self::Comic => "comic",
            Self::Person => "person",
            Self::Artist => "artist",
            Self::Album => "album",
            Self::Track => "track",
            Self::Picture => "picture",
            Self::Clip => "clip",
            Self::Photo => "photo",
            Self::Photoalbum => "photoalbum",
            Self::Playlist => "playlist",
            Self::PlaylistFolder => "playlistFolder",
            Self::Collection => "collection",
            Self::OptimizedVersion => "optimizedVersion",
            Self::UserPlaylistItem => "userPlaylistItem",
            Self::Unknown(_) => return None,
        })
    }
}

impl From<SearchType> for u32 {
    fn from(t: SearchType) -> Self {
        t.as_u32()
    }
}

impl From<u32> for SearchType {
    fn from(value: u32) -> Self {
        Self::from_u32(value)
    }
}

impl fmt::Display for SearchType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.as_name() {
            Some(name) => f.write_str(name),
            None => write!(f, "type={}", self.as_u32()),
        }
    }
}

impl FromStr for SearchType {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let m = match s {
            "movie" => Self::Movie,
            "show" => Self::Show,
            "season" => Self::Season,
            "episode" => Self::Episode,
            "trailer" => Self::Trailer,
            "comic" => Self::Comic,
            "person" => Self::Person,
            "artist" => Self::Artist,
            "album" => Self::Album,
            "track" => Self::Track,
            "picture" => Self::Picture,
            "clip" => Self::Clip,
            "photo" => Self::Photo,
            "photoalbum" => Self::Photoalbum,
            "playlist" => Self::Playlist,
            "playlistFolder" => Self::PlaylistFolder,
            "collection" => Self::Collection,
            "optimizedVersion" => Self::OptimizedVersion,
            "userPlaylistItem" => Self::UserPlaylistItem,
            other => {
                return Err(Error::Config(format!("unknown SearchType name {other:?}")));
            }
        };
        Ok(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_known_values() {
        for st in [
            SearchType::Movie,
            SearchType::Show,
            SearchType::Episode,
            SearchType::Track,
            SearchType::Collection,
            SearchType::OptimizedVersion,
            SearchType::UserPlaylistItem,
        ] {
            assert_eq!(SearchType::from_u32(st.as_u32()), st);
        }
    }

    #[test]
    fn unknown_value_round_trips_through_u32() {
        let st = SearchType::from_u32(99);
        assert_eq!(st, SearchType::Unknown(99));
        assert_eq!(st.as_u32(), 99);
    }

    #[test]
    fn from_str_for_known_names() {
        assert_eq!("movie".parse::<SearchType>().unwrap(), SearchType::Movie);
        assert_eq!("show".parse::<SearchType>().unwrap(), SearchType::Show);
        assert_eq!(
            "playlistFolder".parse::<SearchType>().unwrap(),
            SearchType::PlaylistFolder
        );
    }

    #[test]
    fn from_str_rejects_unknown_name() {
        let err = "wat".parse::<SearchType>().unwrap_err();
        assert!(matches!(err, Error::Config(_)));
    }

    #[test]
    fn display_uses_canonical_names() {
        assert_eq!(SearchType::Movie.to_string(), "movie");
        assert_eq!(SearchType::Collection.to_string(), "collection");
        assert_eq!(SearchType::Unknown(7777).to_string(), "type=7777");
    }

    #[test]
    fn serde_round_trips_through_u32() {
        let st = SearchType::Album;
        let j = serde_json::to_string(&st).unwrap();
        assert_eq!(j, "9");
        let back: SearchType = serde_json::from_str(&j).unwrap();
        assert_eq!(back, st);
    }

    #[test]
    fn serde_unknown_int_round_trips() {
        let raw = 5555u32;
        let st: SearchType = serde_json::from_str(&raw.to_string()).unwrap();
        assert_eq!(st, SearchType::Unknown(raw));
        assert_eq!(serde_json::to_string(&st).unwrap(), "5555");
    }

    #[test]
    fn as_name_for_unknown_is_none() {
        assert!(SearchType::Unknown(0).as_name().is_none());
    }

    #[test]
    fn discriminants_match_python_plexapi_table() {
        // Spot-check the eight values most likely to be wrong-by-typo
        // against utils.py:35.
        assert_eq!(SearchType::Movie.as_u32(), 1);
        assert_eq!(SearchType::Show.as_u32(), 2);
        assert_eq!(SearchType::Track.as_u32(), 10);
        assert_eq!(SearchType::Collection.as_u32(), 18); // 17 is intentionally skipped.
        assert_eq!(SearchType::OptimizedVersion.as_u32(), 42);
        assert_eq!(SearchType::UserPlaylistItem.as_u32(), 1001);
    }

    /// The full (variant, wire-int, wire-name) table — the source of
    /// truth for the three mappings. Exercising every row hits every
    /// match arm in `from_u32`, `as_u32`, `as_name`, and `from_str`.
    const TABLE: &[(SearchType, u32, &str)] = &[
        (SearchType::Movie, 1, "movie"),
        (SearchType::Show, 2, "show"),
        (SearchType::Season, 3, "season"),
        (SearchType::Episode, 4, "episode"),
        (SearchType::Trailer, 5, "trailer"),
        (SearchType::Comic, 6, "comic"),
        (SearchType::Person, 7, "person"),
        (SearchType::Artist, 8, "artist"),
        (SearchType::Album, 9, "album"),
        (SearchType::Track, 10, "track"),
        (SearchType::Picture, 11, "picture"),
        (SearchType::Clip, 12, "clip"),
        (SearchType::Photo, 13, "photo"),
        (SearchType::Photoalbum, 14, "photoalbum"),
        (SearchType::Playlist, 15, "playlist"),
        (SearchType::PlaylistFolder, 16, "playlistFolder"),
        (SearchType::Collection, 18, "collection"),
        (SearchType::OptimizedVersion, 42, "optimizedVersion"),
        (SearchType::UserPlaylistItem, 1001, "userPlaylistItem"),
    ];

    #[test]
    fn every_variant_round_trips_through_u32() {
        for &(variant, n, _) in TABLE {
            assert_eq!(variant.as_u32(), n, "as_u32 wrong for {variant:?}");
            assert_eq!(
                SearchType::from_u32(n),
                variant,
                "from_u32({n}) should yield {variant:?}"
            );
            // The blanket From impls delegate to the inherent methods.
            assert_eq!(u32::from(variant), n);
            assert_eq!(SearchType::from(n), variant);
        }
    }

    #[test]
    fn every_variant_maps_to_canonical_name() {
        for &(variant, _, name) in TABLE {
            assert_eq!(
                variant.as_name(),
                Some(name),
                "as_name wrong for {variant:?}"
            );
            // Display of a named variant is exactly its canonical name.
            assert_eq!(variant.to_string(), name);
        }
    }

    #[test]
    fn every_canonical_name_parses_back() {
        for &(variant, _, name) in TABLE {
            assert_eq!(
                name.parse::<SearchType>().unwrap(),
                variant,
                "from_str({name:?}) should yield {variant:?}"
            );
        }
    }
}
