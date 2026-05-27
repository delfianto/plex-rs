//! [`Reload`] — re-fetch a partial metadata leaf with full detail.
//!
//! Plex's listing endpoints (`/library/sections/<id>/all`, search,
//! recentlyAdded, …) return *partial* metadata: each item carries
//! the scalar fields but typically omits `Media[]`, the tag
//! families, markers, and chapters. To get the full picture you
//! re-fetch the item via `GET /library/metadata/<rk>`.
//!
//! [`Reload`] is the trait that wraps that re-fetch. Each leaf
//! type's `Full` associated type points back at itself (after a
//! reload you have the same struct, just with the previously-empty
//! collection fields populated).

use crate::error::{Error, Result};
use crate::media::video::MetadataDto;
use crate::traits::PlexObject;
use crate::xml::MediaContainer;

/// Re-fetch this metadata leaf with the full detail set Plex emits
/// on a direct `GET /library/metadata/<rk>` call.
///
/// Listing endpoints return partial items (no `Media[]`, no tags,
/// no markers); `reload()` upgrades to the full record.
pub trait Reload: PlexObject + Sized {
    /// The fully-loaded version of this type. For every leaf in
    /// `plex-rs` this is `Self` — there is no separate partial /
    /// full type, only a populated or empty `Vec<…>` of subordinate
    /// data on the same struct.
    type Full;

    /// Re-fetch `self` from PMS and return the full-detail value.
    /// Consumes `self` because the caller will typically replace
    /// their previously-held partial value with the result.
    ///
    /// # Errors
    /// Any transport [`Error`] variant. [`Error::NotFound`] when
    /// PMS no longer recognises the rating key (item was deleted
    /// between list and reload).
    fn reload(self) -> impl std::future::Future<Output = Result<Self::Full>> + Send
    where
        Self: Sync;
}

/// Crate-private helper: fetch `/library/metadata/<rating_key>` and
/// return the single child `MetadataDto`. Used by every leaf's
/// `Reload` impl.
pub(crate) async fn fetch_metadata<T: PlexObject + Sync>(object: &T) -> Result<MetadataDto> {
    let url = object
        .base_url()
        .join(&format!("/library/metadata/{}", object.rating_key()))?;
    let body = object.http().get_bytes(url.as_str()).await?;
    let body_str = std::str::from_utf8(&body)
        .map_err(|e| Error::Config(format!("metadata body not utf-8: {e}")))?;
    let mc: MediaContainer<MetadataDto> = MediaContainer::from_json(body_str, "Metadata")?;
    mc.items.into_iter().next().ok_or_else(|| Error::NotFound {
        resource: format!("/library/metadata/{}", object.rating_key()),
    })
}
