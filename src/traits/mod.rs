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

pub mod played_unplayed;
pub mod ratable;

pub use played_unplayed::PlayedUnplayed;
pub use ratable::Ratable;

use url::Url;

use crate::HttpClient;
use crate::RatingKey;

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
    /// Borrow the HTTP client this object's section is bound to.
    fn http(&self) -> &HttpClient;

    /// Borrow the base URL of the owning PMS.
    fn base_url(&self) -> &Url;

    /// This object's primary [`RatingKey`].
    fn rating_key(&self) -> RatingKey;
}
