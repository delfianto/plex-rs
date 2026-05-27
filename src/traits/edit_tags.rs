//! [`EditTags`] and per-family tag-edit traits.
//!
//! Plex's `<Genre>`/`<Director>`/`<Collection>`/`<Label>`/… families
//! all share the same edit wire format:
//!
//! - **Replace** the entire list:
//!   `PUT /library/sections/<sid>/all?id=<rk>&type=<n>&<field>[0].tag.tag=v1&<field>[1].tag.tag=v2&<field>.locked=<0|1>`.
//! - **Remove** a subset by comma-joined name:
//!   `PUT /library/sections/<sid>/all?id=<rk>&type=<n>&<field>[].tag.tag-=v1,v2&<field>.locked=<0|1>`
//!   (note the trailing `-` on `tag-=`).
//!
//! See analysis/08 §3.4 for the canonical sigil. The "add" semantic
//! is **not** a single wire op — python-plexapi reads the current tag
//! list, prepends, and emits a full replace. We expose `replace_tags`
//! and `remove_tags` as primitives; per-family traits
//! ([`HasGenres`], [`HasCollections`]) layer ergonomic
//! `replace_*` / `remove_*` aliases on top with the right field
//! string baked in.

use crate::error::Result;
use crate::traits::PlexObject;
use crate::traits::edit_field::pct_query;

/// Replace / remove tag-family values on a metadata item.
///
/// Implementors are leaf types whose metadata has tag children
/// (Movie / Show / Episode / Album / Track / Artist — every leaf the
/// `MetadataDto::collect_tags()` helper populates).
pub trait EditTags: PlexObject {
    /// Replace this item's `<field>` tag list with `items`.
    ///
    /// Wire form: `PUT /library/sections/<sid>/all?id=<rk>&type=<n>
    /// &<field>[0].tag.tag=v0&<field>[1].tag.tag=v1&…&<field>.locked=<0|1>`.
    ///
    /// `locked = true` prevents PMS from overwriting these tags
    /// during the next metadata refresh.
    ///
    /// # Errors
    /// Any transport [`crate::Error`] variant.
    fn replace_tags(
        &self,
        field: &str,
        items: &[&str],
        locked: bool,
    ) -> impl std::future::Future<Output = Result<()>> + Send
    where
        Self: Sync,
    {
        let field_owned = field.to_owned();
        let items_owned: Vec<String> = items.iter().map(|s| (*s).to_owned()).collect();
        async move {
            use std::fmt::Write;
            let mut q = String::new();
            write!(
                q,
                "id={rk}&type={ty}",
                rk = self.rating_key(),
                ty = self.metadata_type_id(),
            )
            .unwrap();
            for (idx, item) in items_owned.iter().enumerate() {
                write!(
                    q,
                    "&{f}%5B{idx}%5D.tag.tag={v}",
                    f = pct_query(&field_owned),
                    v = pct_query(item),
                )
                .unwrap();
            }
            write!(
                q,
                "&{f}.locked={lock}",
                f = pct_query(&field_owned),
                lock = u8::from(locked),
            )
            .unwrap();
            let path = format!(
                "/library/sections/{section}/all?{q}",
                section = self.section_ref().id,
            );
            let url = self.base_url().join(&path)?;
            self.http().put_no_body(url.as_str()).await
        }
    }

    /// Remove the named tags from this item's `<field>` list.
    ///
    /// Wire form: `…?id=<rk>&type=<n>&<field>[].tag.tag-=v1,v2&<field>.locked=<0|1>`
    /// (the trailing `-` on `tag-=` is the remove sigil — analysis/08 §3.4).
    ///
    /// # Errors
    /// Any transport [`crate::Error`] variant.
    fn remove_tags(
        &self,
        field: &str,
        items: &[&str],
        locked: bool,
    ) -> impl std::future::Future<Output = Result<()>> + Send
    where
        Self: Sync,
    {
        let field_owned = field.to_owned();
        let csv = items.join(",");
        async move {
            let q = format!(
                "id={rk}&type={ty}&{f}%5B%5D.tag.tag-={v}&{f}.locked={lock}",
                rk = self.rating_key(),
                ty = self.metadata_type_id(),
                f = pct_query(&field_owned),
                v = pct_query(&csv),
                lock = u8::from(locked),
            );
            let path = format!(
                "/library/sections/{section}/all?{q}",
                section = self.section_ref().id,
            );
            let url = self.base_url().join(&path)?;
            self.http().put_no_body(url.as_str()).await
        }
    }
}

// -----------------------------------------------------------------------------
// Per-family ergonomic traits.
// -----------------------------------------------------------------------------

/// Replace / remove `<Genre>` tags on an item.
pub trait HasGenres: EditTags {
    /// Replace the genre list with `items`.
    ///
    /// # Errors
    /// Any transport [`crate::Error`] variant.
    fn replace_genres(
        &self,
        items: &[&str],
        locked: bool,
    ) -> impl std::future::Future<Output = Result<()>> + Send
    where
        Self: Sync,
    {
        self.replace_tags("genre", items, locked)
    }

    /// Remove the named genres.
    ///
    /// # Errors
    /// Any transport [`crate::Error`] variant.
    fn remove_genres(
        &self,
        items: &[&str],
        locked: bool,
    ) -> impl std::future::Future<Output = Result<()>> + Send
    where
        Self: Sync,
    {
        self.remove_tags("genre", items, locked)
    }
}

/// Replace / remove `<Collection>` tags on an item.
pub trait HasCollections: EditTags {
    /// Replace the collection list with `items`.
    ///
    /// # Errors
    /// Any transport [`crate::Error`] variant.
    fn replace_collections(
        &self,
        items: &[&str],
        locked: bool,
    ) -> impl std::future::Future<Output = Result<()>> + Send
    where
        Self: Sync,
    {
        self.replace_tags("collection", items, locked)
    }

    /// Remove the named collections.
    ///
    /// # Errors
    /// Any transport [`crate::Error`] variant.
    fn remove_collections(
        &self,
        items: &[&str],
        locked: bool,
    ) -> impl std::future::Future<Output = Result<()>> + Send
    where
        Self: Sync,
    {
        self.remove_tags("collection", items, locked)
    }
}
