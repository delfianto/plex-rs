//! Collections — section-attached named groupings of metadata items.
//!
//! A Plex collection lives under a single library section and groups
//! items of that section's kind (Movies / Shows / Albums / …). On the
//! wire its details look like a metadata element with `type=18`
//! ([`crate::SearchType::Collection`]); but the listing endpoint is
//! `/library/sections/<id>/collections`, distinct from the
//! item-listing endpoints in `LibrarySection`.
//!
//! Endpoint surface (M4.2 implements *italicised*):
//!
//! - `GET /library/sections/<id>/collections` — *list*
//! - `GET /library/collections/<rk>/children` — *items*
//! - `DELETE /library/collections/<rk>` — *delete*
//! - `PUT /library/collections/<rk>/items?uri=` — defer (add items)
//! - `DELETE /library/collections/<rk>/items/<itemID>` — defer (remove)
//! - `PUT /library/metadata/<rk>/prefs?...` — defer (mode/sort)
//!
//! Edit traits ([`crate::EditTitle`], [`crate::EditSummary`], tag
//! traits, image traits) compose naturally because a Collection
//! carries a [`crate::LibrarySectionRef`].

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::library::LibrarySectionRef;
use crate::media::LibraryItem;
use crate::media::video::MetadataDto;
use crate::server::join_path;
use crate::traits::{
    EditField, EditSummary, EditTags, EditTitle, HasArtLock, HasArtUrl, HasCollections, HasGenres,
    HasLabels, HasPosterLock, HasPosterUrl, PlexObject, Ratable,
};
use crate::util::ids::RatingKey;
use crate::xml::MediaContainer;

// -----------------------------------------------------------------------------
// Collection.
// -----------------------------------------------------------------------------

/// A Plex collection — a section-scoped named grouping.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Collection {
    /// Plex metadata identifier.
    pub rating_key: RatingKey,
    /// Relative key — `/library/collections/<rating_key>`.
    pub key: String,
    /// Collection title.
    pub title: String,
    /// User-provided summary.
    pub summary: Option<String>,
    /// Underlying content kind (`movie`, `show`, `album`, …).
    pub subtype: Option<String>,
    /// Whether the collection is rule-driven.
    pub smart: bool,
    /// Number of items.
    pub child_count: Option<u32>,
    /// Number of items (`leafCount` — often equals `child_count`).
    pub leaf_count: Option<u32>,
    /// Number of items the user has played through.
    pub viewed_leaf_count: Option<u32>,
    /// Plex's collection display mode — `default` / `hide`.
    pub collection_mode: Option<String>,
    /// Plex's collection sort — `default` / `alpha` / `custom`.
    pub collection_sort: Option<String>,
    /// Composite (collage) thumbnail path.
    pub composite: Option<String>,
    /// Poster path (`thumb` on the wire).
    pub thumb: Option<String>,
    /// Background art path.
    pub art: Option<String>,
    /// Add timestamp (epoch seconds).
    pub added_at: Option<i64>,
    /// Update timestamp (epoch seconds).
    pub updated_at: Option<i64>,
    /// Primary GUID.
    pub guid: Option<String>,
    /// Back-reference to the owning library section. Used by every
    /// edit trait Collection implements.
    pub section_ref: LibrarySectionRef,
}

impl Collection {
    /// List the items in this collection.
    ///
    /// Calls `GET /library/collections/<rk>/children` and parses
    /// the response into mixed [`LibraryItem`] variants.
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn items(&self) -> Result<Vec<LibraryItem>> {
        let url = join_path(
            &self.section_ref.base_url,
            &format!("/library/collections/{}/children", self.rating_key),
        )?;
        let body = self.section_ref.http.get_bytes(url.as_str()).await?;
        let body_str = std::str::from_utf8(&body)
            .map_err(|e| Error::Config(format!("collection items body not utf-8: {e}")))?;
        let mc: MediaContainer<MetadataDto> = MediaContainer::from_json(body_str, "Metadata")?;
        mc.items
            .into_iter()
            .map(|dto| dto.into_library_item(self.section_ref.clone()))
            .collect()
    }

    /// Delete this collection server-side.
    ///
    /// Calls `DELETE /library/collections/<rk>`. Items are not
    /// deleted — only the grouping.
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn delete(self) -> Result<()> {
        let url = join_path(
            &self.section_ref.base_url,
            &format!("/library/collections/{}", self.rating_key),
        )?;
        self.section_ref.http.delete(url.as_str()).await
    }
}

// -----------------------------------------------------------------------------
// Trait impls — Collection is section-attached so it composes with
// every M3 edit trait that doesn't require a metadata `type` other
// than 18 (Collection).
// -----------------------------------------------------------------------------

impl PlexObject for Collection {
    fn section_ref(&self) -> &LibrarySectionRef {
        &self.section_ref
    }
    fn rating_key(&self) -> RatingKey {
        self.rating_key
    }
    fn metadata_type_id(&self) -> u32 {
        18
    }
}

impl Ratable for Collection {}
impl EditField for Collection {}
impl EditTitle for Collection {}
impl EditSummary for Collection {}
impl EditTags for Collection {}
impl HasGenres for Collection {}
impl HasCollections for Collection {}
impl HasLabels for Collection {}

impl HasArtUrl for Collection {
    fn art_path(&self) -> Option<&str> {
        self.art.as_deref()
    }
}
impl HasArtLock for Collection {}
impl HasPosterUrl for Collection {
    fn thumb_path(&self) -> Option<&str> {
        self.thumb.as_deref()
    }
}
impl HasPosterLock for Collection {}

// -----------------------------------------------------------------------------
// DTO.
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CollectionDto {
    pub(crate) rating_key: String,
    pub(crate) key: String,
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) summary: Option<String>,
    #[serde(default)]
    pub(crate) subtype: Option<String>,
    #[serde(default)]
    pub(crate) smart: Option<crate::server::PlexBoolField>,
    #[serde(default)]
    pub(crate) child_count: Option<u32>,
    #[serde(default)]
    pub(crate) leaf_count: Option<u32>,
    #[serde(default)]
    pub(crate) viewed_leaf_count: Option<u32>,
    #[serde(default)]
    pub(crate) collection_mode: Option<String>,
    #[serde(default)]
    pub(crate) collection_sort: Option<String>,
    #[serde(default)]
    pub(crate) composite: Option<String>,
    #[serde(default)]
    pub(crate) thumb: Option<String>,
    #[serde(default)]
    pub(crate) art: Option<String>,
    #[serde(default)]
    pub(crate) added_at: Option<i64>,
    #[serde(default)]
    pub(crate) updated_at: Option<i64>,
    #[serde(default)]
    pub(crate) guid: Option<String>,
}

impl CollectionDto {
    pub(crate) fn into_domain(self, section_ref: LibrarySectionRef) -> Result<Collection> {
        let rating_key: RatingKey = self
            .rating_key
            .parse()
            .map_err(|e: Error| Error::Config(format!("collection.ratingKey: {e}")))?;
        Ok(Collection {
            rating_key,
            key: self.key,
            title: self.title,
            summary: self.summary,
            subtype: self.subtype,
            smart: self.smart.is_some_and(|b| b.to_bool()),
            child_count: self.child_count,
            leaf_count: self.leaf_count,
            viewed_leaf_count: self.viewed_leaf_count,
            collection_mode: self.collection_mode,
            collection_sort: self.collection_sort,
            composite: self.composite,
            thumb: self.thumb,
            art: self.art,
            added_at: self.added_at,
            updated_at: self.updated_at,
            guid: self.guid,
            section_ref,
        })
    }
}
