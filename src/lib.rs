//! # plex-rs
//!
//! Idiomatic, fully-async Rust 2024 client for the
//! [Plex Media Server](https://www.plex.tv/) HTTP API and `plex.tv` (`MyPlex`)
//! cloud services.
//!
//! See [`CLAUDE.md`](https://github.com/justdewey/plex-rs/blob/main/CLAUDE.md)
//! for the project charter and contributor guide, and the `analysis/`
//! directory for a deep dive on the Python reference implementation
//! ([`python-plexapi`](https://github.com/pkkid/python-plexapi)) that this
//! crate aims to reach parity with.
//!
//! ## Status
//!
//! This crate is **pre-1.0** and the public surface is unstable. The
//! parity checklist lives in `docs/parity.md` and the `analysis/` notes.
//!
//! ## Quick tour (planned)
//!
//! 1. Build a `ClientConfig` describing the client identity.
//! 2. Authenticate against `plex.tv` (token, password, or PIN/OAuth) to
//!    obtain a `MyPlexAccount`.
//! 3. Discover a `PlexServer` resource and connect to it.
//! 4. Browse `Library` sections, search, fetch media, control playback.
//!
//! Intra-doc links to the types above will be wired up as each module
//! lands. See `analysis/11-rust-mapping-recommendations.md` for the
//! milestone order.
//!
//! ## Cargo features
//!
//! | Feature        | Default | Purpose                                          |
//! | -------------- | :-----: | ------------------------------------------------ |
//! | `rustls`       |   yes   | Use `rustls` for TLS (default).                  |
//! | `native-tls`   |   no    | Use the platform-native TLS stack instead.       |
//! | `webhook-axum` |   no    | Provide an `axum` extractor for Plex webhooks.   |
//! | `discovery`    |   no    | GDM (raw UDP) local server discovery.            |
//! | `alerts`       |   no    | WebSocket-based real-time alert stream.          |

#![forbid(unsafe_code)]
#![deny(
    clippy::all,
    clippy::correctness,
    clippy::suspicious,
    clippy::perf,
    clippy::style,
    missing_docs,
    missing_debug_implementations,
    unreachable_pub,
    rust_2024_compatibility
)]
#![warn(clippy::pedantic, clippy::nursery, clippy::cargo)]
#![allow(
    // Common when re-exporting domain types matching Plex naming conventions.
    clippy::module_name_repetitions,
    // Errors are covered by the typed `Error` enum; do not require per-method docs.
    clippy::missing_errors_doc,
    // We document panic-freedom via tests, not prose.
    clippy::missing_panics_doc,
    // Crate-level metadata is in `Cargo.toml`; do not flag transitively.
    clippy::multiple_crate_versions
)]
#![cfg_attr(docsrs, feature(doc_cfg))]

// -----------------------------------------------------------------------------
// Module layout intentionally minimal at bootstrap; new modules land alongside
// the features that need them. See `CLAUDE.md` §3.
// -----------------------------------------------------------------------------

pub mod auth;
pub mod client;
pub mod config;
#[cfg(feature = "discovery")]
pub mod discover_gdm;
pub mod error;
pub mod headers;
pub mod library;
pub mod media;
pub mod pagination;
pub mod server;
pub mod traits;
pub mod uri;
pub mod util;
pub mod xml;

pub use crate::auth::MyPlexPinLogin;
pub use crate::client::HttpClient;
pub use crate::config::{ClientConfig, ClientConfigBuilder};
pub use crate::error::{Error, Result};
pub use crate::headers::PlexIdentity;
pub use crate::library::{
    FilterBuilder, FilterOp, Library, LibrarySection, LibrarySectionRef, SectionKind, SortDirection,
};
pub use crate::media::{
    Album, Artist, AudioStream, Chapter, Collection, Episode, LibraryItem, LyricStream, Marker,
    MarkerKind, Media, MediaPart, Movie, Photo, PhotoEntry, Photoalbum, Playlist, PlaylistKind,
    Season, Show, Stream, StreamCommon, SubtitleStream, Tag, TagKind, Track, UnknownStream,
    VideoStream,
};
pub use crate::pagination::PageRange;
pub use crate::server::{
    PlayState, PlayingSession, PlexServer, ServerIdentity, SessionPlayer, SessionUser,
    TranscodeSession,
};
pub use crate::traits::{
    EditContentRating, EditField, EditOriginalTitle, EditSortTitle, EditStudio, EditSummary,
    EditTagline, EditTags, EditTitle, EditYear, FieldValue, HasArtLock, HasArtUrl, HasCollections,
    HasCountries, HasDirectors, HasGenres, HasLabels, HasMoods, HasPosterLock, HasPosterUrl,
    HasProducers, HasRoles, HasStyles, HasThemeLock, HasThemeUrl, HasWriters, PlayedUnplayed,
    PlexObject, Ratable,
};
pub use crate::uri::PlexUri;
pub use crate::util::{
    ClientIdentifier, MachineIdentifier, PlayQueueId, PlexToken, RatingKey, SearchType,
};
pub use crate::xml::{MediaContainer, MediaContainerMeta};

// Public re-exports accumulate here as modules land:
//
// pub use crate::client::HttpClient;
// pub use crate::config::ClientConfig;
// pub use crate::myplex::MyPlexAccount;
// pub use crate::server::PlexServer;
// pub use crate::library::Library;

/// Crate version, exposed for the default `X-Plex-Version` header.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default product name used in the `X-Plex-Product` header.
///
/// Override via `ClientConfig` in production use — Plex relies on these
/// headers for client identification.
pub const DEFAULT_PRODUCT: &str = "plex-rs";

#[cfg(test)]
mod smoke_tests {
    use super::*;

    #[test]
    fn version_matches_cargo() {
        // Sanity check: the constant is populated by env! at compile time.
        assert!(!VERSION.is_empty());
        assert!(VERSION.chars().next().is_some_and(|c| c.is_ascii_digit()));
    }

    #[test]
    fn default_product_is_set() {
        assert_eq!(DEFAULT_PRODUCT, "plex-rs");
    }
}
