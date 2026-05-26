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
pub mod video;

pub use audio::{Album, Artist, Track};
pub use video::{Episode, Movie, Season, Show};
