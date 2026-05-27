//! Capability traits exposed by domain leaf types.
//!
//! Following the architecture decision in
//! `analysis/11-rust-mapping-recommendations.md` §5 — explicit
//! extension traits per capability (Option B) rather than a single
//! monolithic interface. Each leaf type (`Movie`, `Episode`, …)
//! implements the subset of traits that apply to it; pattern-match
//! on a [`crate::LibraryItem`] to recover the concrete type and use
//! the trait methods directly.
//!
//! Traits land here as the corresponding mutating endpoints are
//! implemented:
//!
//! - [`PlexObject`] / [`played_unplayed::PlayedUnplayed`] — M3.1.
//! - `Ratable`, `Reload`, `EditField`, `EditTags`, image traits —
//!   subsequent M3 sub-items.

pub mod edit_batch;
pub mod edit_field;
pub mod edit_tags;
pub mod images;
pub mod playable;
pub mod played_unplayed;
pub mod ratable;
pub mod reload;

pub use edit_batch::{EditBatch, EditBatchExt};
pub use edit_field::{
    EditContentRating, EditField, EditOriginalTitle, EditSortTitle, EditStudio, EditSummary,
    EditTagline, EditTitle, EditYear, FieldValue,
};
pub use edit_tags::{
    EditTags, HasCollections, HasCountries, HasDirectors, HasGenres, HasLabels, HasMoods,
    HasProducers, HasRoles, HasStyles, HasWriters,
};
pub use images::{HasArtLock, HasArtUrl, HasPosterLock, HasPosterUrl, HasThemeLock, HasThemeUrl};
pub use playable::Playable;
pub use played_unplayed::PlayedUnplayed;
pub use ratable::Ratable;
pub use reload::Reload;

use url::Url;

use crate::HttpClient;
use crate::RatingKey;
use crate::library::LibrarySectionRef;

// -----------------------------------------------------------------------------
// PlexObject — the supertrait every capability trait builds on.
// -----------------------------------------------------------------------------

/// Common surface every Plex domain object exposes.
///
/// Capability traits (`PlayedUnplayed`, `Ratable`, `EditField`, …)
/// build on this so they can issue HTTP calls without each leaf
/// type having to re-implement the same boilerplate.
///
/// Implementors are the concrete leaf structs (`Movie`, `Episode`,
/// `Track`, etc.) — each holds a [`crate::LibrarySectionRef`] from
/// which the HTTP client and base URL are borrowed.
pub trait PlexObject: Send + Sync {
    /// Borrow the owning library-section back-reference. Edit traits
    /// use this to construct
    /// `PUT /library/sections/<section_id>/all?...` URLs — Plex
    /// dispatches edits through the section, not the item itself
    /// (analysis/11 §2.4).
    fn section_ref(&self) -> &LibrarySectionRef;

    /// This object's primary [`RatingKey`].
    fn rating_key(&self) -> RatingKey;

    /// Plex's wire-format metadata-type discriminator
    /// (1 = movie, 2 = show, 4 = episode, 9 = album, 10 = track, …).
    /// Used as `?type=<N>` on edit and search endpoints.
    fn metadata_type_id(&self) -> u32;

    /// Borrow the HTTP client. Default-derived from
    /// [`Self::section_ref`].
    fn http(&self) -> &HttpClient {
        &self.section_ref().http
    }

    /// Borrow the base URL. Default-derived from
    /// [`Self::section_ref`].
    fn base_url(&self) -> &Url {
        &self.section_ref().base_url
    }
}
