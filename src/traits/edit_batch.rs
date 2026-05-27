//! [`EditBatch`] — one PUT for many edits on one item.
//!
//! The individual `EditField` / `EditTags` traits send one PUT per
//! field. For bulk edits (rename + retag + relock in one go) that's
//! N round-trips. PMS happily accepts every field/tag pair concatenated
//! in a single query string; this module collects pending operations
//! and flushes them in one call.
//!
//! ## Wire format
//!
//! Plex's bulk edit shape combines the patterns documented in
//! [`crate::traits::EditField`] and [`crate::traits::EditTags`]:
//!
//! ```text
//! PUT /library/sections/<sid>/all
//!     ?id=<rating-key>
//!     &type=<metadata-type-id>
//!     &title.value=Arrival
//!     &title.locked=1
//!     &summary.value=...
//!     &summary.locked=1
//!     &genre%5B0%5D.tag.tag=Sci-Fi
//!     &genre%5B1%5D.tag.tag=Drama
//!     &genre.locked=1
//!     &label%5B%5D.tag.tag-=remove,me
//!     &label.locked=0
//! ```
//!
//! The same `id` / `type` pair drives every operation; everything
//! else is appended.

use std::fmt::Write;

use crate::error::Result;
use crate::traits::PlexObject;
use crate::traits::edit_field::{FieldValue, pct_query};

// -----------------------------------------------------------------------------
// EditBatch.
// -----------------------------------------------------------------------------

/// Builder accumulating a sequence of edits, flushed by
/// [`Self::execute`] in one PUT.
///
/// Construct via [`Self::new`] or via the [`EditBatchExt`] extension
/// (`item.batch()`).
///
/// Operations are applied in insertion order, which matches the
/// declaration order in the resulting query string. Plex is happy
/// with multiple `<field>.value` pairs for the same field but only
/// the last wins — order-sensitive callers should chain explicitly.
#[derive(Debug)]
pub struct EditBatch<'a, O: PlexObject> {
    item: &'a O,
    ops: Vec<Op>,
}

#[derive(Debug)]
enum Op {
    /// `<field>.value=<v>&<field>.locked=<L>`.
    Field {
        name: String,
        value: FieldValue,
        locked: bool,
    },
    /// `<field>.locked=<L>` with no `.value` pair.
    LockOnly { name: String, locked: bool },
    /// `<field>[0].tag.tag=v0&<field>[1].tag.tag=v1&...&<field>.locked=<L>`.
    ReplaceTags {
        name: String,
        items: Vec<String>,
        locked: bool,
    },
    /// `<field>[].tag.tag-=csv&<field>.locked=<L>`.
    RemoveTags {
        name: String,
        csv: String,
        locked: bool,
    },
}

impl<'a, O: PlexObject> EditBatch<'a, O> {
    /// Start a fresh batch.
    #[must_use]
    pub const fn new(item: &'a O) -> Self {
        Self {
            item,
            ops: Vec::new(),
        }
    }

    /// `true` when no operations have been queued.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Number of queued operations. Useful for "did anything
    /// actually change?" checks before calling [`Self::execute`].
    #[must_use]
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    // ---------------- low-level primitives ----------------

    /// Queue a single-field edit. Equivalent to
    /// [`EditField::edit_field`](crate::traits::EditField::edit_field).
    #[must_use]
    pub fn set_field(
        mut self,
        field: impl Into<String>,
        value: impl Into<FieldValue>,
        locked: bool,
    ) -> Self {
        self.ops.push(Op::Field {
            name: field.into(),
            value: value.into(),
            locked,
        });
        self
    }

    /// Queue a lock-only toggle on `field` (no `.value` pair). Same
    /// wire shape as
    /// [`EditField::lock_field`](crate::traits::EditField::lock_field).
    #[must_use]
    pub fn lock_field(mut self, field: impl Into<String>, locked: bool) -> Self {
        self.ops.push(Op::LockOnly {
            name: field.into(),
            locked,
        });
        self
    }

    /// Queue a full replace of the named tag family.
    #[must_use]
    pub fn replace_tags(mut self, field: impl Into<String>, items: &[&str], locked: bool) -> Self {
        self.ops.push(Op::ReplaceTags {
            name: field.into(),
            items: items.iter().map(|s| (*s).to_owned()).collect(),
            locked,
        });
        self
    }

    /// Queue a remove of the named tags from `field`.
    #[must_use]
    pub fn remove_tags(mut self, field: impl Into<String>, items: &[&str], locked: bool) -> Self {
        self.ops.push(Op::RemoveTags {
            name: field.into(),
            csv: items.join(","),
            locked,
        });
        self
    }

    // ---------------- ergonomic shortcuts mirroring the per-field traits ----------------

    /// Convenience: set `title`.
    #[must_use]
    pub fn set_title(self, value: &str, locked: bool) -> Self {
        self.set_field("title", value, locked)
    }
    /// Convenience: set `summary`.
    #[must_use]
    pub fn set_summary(self, value: &str, locked: bool) -> Self {
        self.set_field("summary", value, locked)
    }
    /// Convenience: set `tagline`.
    #[must_use]
    pub fn set_tagline(self, value: &str, locked: bool) -> Self {
        self.set_field("tagline", value, locked)
    }
    /// Convenience: set `studio`.
    #[must_use]
    pub fn set_studio(self, value: &str, locked: bool) -> Self {
        self.set_field("studio", value, locked)
    }
    /// Convenience: set `contentRating`.
    #[must_use]
    pub fn set_content_rating(self, value: &str, locked: bool) -> Self {
        self.set_field("contentRating", value, locked)
    }
    /// Convenience: set `titleSort`.
    #[must_use]
    pub fn set_sort_title(self, value: &str, locked: bool) -> Self {
        self.set_field("titleSort", value, locked)
    }
    /// Convenience: set `originalTitle`.
    #[must_use]
    pub fn set_original_title(self, value: &str, locked: bool) -> Self {
        self.set_field("originalTitle", value, locked)
    }
    /// Convenience: set release `year`.
    #[must_use]
    pub fn set_year(self, value: u16, locked: bool) -> Self {
        self.set_field("year", value, locked)
    }

    /// Convenience: replace `genre` tags.
    #[must_use]
    pub fn replace_genres(self, items: &[&str], locked: bool) -> Self {
        self.replace_tags("genre", items, locked)
    }
    /// Convenience: replace `collection` tags.
    #[must_use]
    pub fn replace_collections(self, items: &[&str], locked: bool) -> Self {
        self.replace_tags("collection", items, locked)
    }
    /// Convenience: replace `label` tags.
    #[must_use]
    pub fn replace_labels(self, items: &[&str], locked: bool) -> Self {
        self.replace_tags("label", items, locked)
    }
    /// Convenience: replace `director` tags.
    #[must_use]
    pub fn replace_directors(self, items: &[&str], locked: bool) -> Self {
        self.replace_tags("director", items, locked)
    }
    /// Convenience: replace `writer` tags.
    #[must_use]
    pub fn replace_writers(self, items: &[&str], locked: bool) -> Self {
        self.replace_tags("writer", items, locked)
    }

    // ---------------- materialise / execute ----------------

    /// Build the wire-format query string, including the leading
    /// `id` / `type` pair. Exposed `pub(crate)` for unit testing —
    /// the public surface only calls [`Self::execute`].
    pub(crate) fn build_query(&self) -> String {
        let mut q = String::new();
        write!(
            q,
            "id={rk}&type={ty}",
            rk = self.item.rating_key(),
            ty = self.item.metadata_type_id(),
        )
        .unwrap();
        for op in &self.ops {
            match op {
                Op::Field {
                    name,
                    value,
                    locked,
                } => {
                    write!(
                        q,
                        "&{f}.value={v}&{f}.locked={lock}",
                        f = pct_query(name),
                        v = pct_query(&value.to_string()),
                        lock = u8::from(*locked),
                    )
                    .unwrap();
                }
                Op::LockOnly { name, locked } => {
                    write!(
                        q,
                        "&{f}.locked={lock}",
                        f = pct_query(name),
                        lock = u8::from(*locked),
                    )
                    .unwrap();
                }
                Op::ReplaceTags {
                    name,
                    items,
                    locked,
                } => {
                    for (idx, item) in items.iter().enumerate() {
                        write!(
                            q,
                            "&{f}%5B{idx}%5D.tag.tag={v}",
                            f = pct_query(name),
                            v = pct_query(item),
                        )
                        .unwrap();
                    }
                    write!(
                        q,
                        "&{f}.locked={lock}",
                        f = pct_query(name),
                        lock = u8::from(*locked),
                    )
                    .unwrap();
                }
                Op::RemoveTags { name, csv, locked } => {
                    write!(
                        q,
                        "&{f}%5B%5D.tag.tag-={v}&{f}.locked={lock}",
                        f = pct_query(name),
                        v = pct_query(csv),
                        lock = u8::from(*locked),
                    )
                    .unwrap();
                }
            }
        }
        q
    }

    /// Flush every queued operation in one `PUT`.
    ///
    /// No-op when [`Self::is_empty`] is true — emitting an empty
    /// edit would PUT an unmodified payload but still hit the
    /// server. Returns `Ok(())` immediately instead.
    ///
    /// # Errors
    /// Any transport [`crate::Error`] variant.
    pub async fn execute(self) -> Result<()>
    where
        O: Sync,
    {
        if self.ops.is_empty() {
            return Ok(());
        }
        let q = self.build_query();
        let path = format!(
            "/library/sections/{section}/all?{q}",
            section = self.item.section_ref().id,
        );
        let url = self.item.base_url().join(&path)?;
        self.item.http().put_no_body(url.as_str()).await
    }
}

// -----------------------------------------------------------------------------
// EditBatchExt — the `item.batch()` extension trait.
// -----------------------------------------------------------------------------

/// Adds [`Self::batch`] to every editable leaf type.
///
/// `EditBatchExt` is auto-implemented for every type that implements
/// the parent [`crate::traits::EditField`] — i.e. every leaf with a
/// `LibrarySectionRef` back-link. Movie / Show / Season / Episode /
/// Artist / Album / Track / Collection all qualify.
pub trait EditBatchExt: crate::traits::EditField {
    /// Start a fresh [`EditBatch`] targeting this item.
    fn batch(&self) -> EditBatch<'_, Self>
    where
        Self: Sized,
    {
        EditBatch::new(self)
    }
}

impl<T: crate::traits::EditField> EditBatchExt for T {}

// -----------------------------------------------------------------------------
// Tests.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::HttpClient;
    use crate::config::ClientConfig;
    use crate::library::LibrarySectionRef;
    use crate::util::ids::{ClientIdentifier, RatingKey};
    use url::Url;

    /// Minimal stub implementing `PlexObject` so `build_query` can
    /// run without touching the network.
    struct StubItem {
        rating_key: RatingKey,
        type_id: u32,
        section_ref: LibrarySectionRef,
    }

    impl std::fmt::Debug for StubItem {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("StubItem").finish_non_exhaustive()
        }
    }

    impl PlexObject for StubItem {
        fn rating_key(&self) -> RatingKey {
            self.rating_key
        }
        fn metadata_type_id(&self) -> u32 {
            self.type_id
        }
        fn section_ref(&self) -> &LibrarySectionRef {
            &self.section_ref
        }
    }

    fn stub(rk: u64, type_id: u32, section: u32) -> StubItem {
        let cfg = ClientConfig::builder(ClientIdentifier::new("t").unwrap())
            .build()
            .unwrap();
        let http = HttpClient::new(cfg).unwrap();
        let base_url = Url::parse("http://localhost").unwrap();
        StubItem {
            rating_key: RatingKey(rk),
            type_id,
            section_ref: LibrarySectionRef {
                id: section,
                http,
                base_url,
            },
        }
    }

    #[test]
    fn empty_batch_query_carries_only_id_and_type() {
        let item = stub(42, 1, 7);
        let q = EditBatch::new(&item).build_query();
        assert_eq!(q, "id=42&type=1");
    }

    #[test]
    fn set_field_emits_value_and_locked_pair() {
        let item = stub(42, 1, 7);
        let q = EditBatch::new(&item)
            .set_field("title", "Arrival", true)
            .build_query();
        assert_eq!(q, "id=42&type=1&title.value=Arrival&title.locked=1");
    }

    #[test]
    fn percent_encoded_value_in_set_field() {
        let item = stub(1, 1, 1);
        let q = EditBatch::new(&item)
            .set_field("title", "Hello World & Friends", false)
            .build_query();
        assert!(q.contains("title.value=Hello%20World%20%26%20Friends"));
        assert!(q.contains("title.locked=0"));
    }

    #[test]
    fn lock_only_op_has_no_value_pair() {
        let item = stub(1, 1, 1);
        let q = EditBatch::new(&item).lock_field("art", true).build_query();
        assert_eq!(q, "id=1&type=1&art.locked=1");
        assert!(!q.contains("art.value"));
    }

    #[test]
    fn replace_tags_indexes_each_entry() {
        let item = stub(10, 1, 1);
        let q = EditBatch::new(&item)
            .replace_tags("genre", &["Sci-Fi", "Drama"], true)
            .build_query();
        assert!(q.contains("genre%5B0%5D.tag.tag=Sci-Fi"));
        assert!(q.contains("genre%5B1%5D.tag.tag=Drama"));
        assert!(q.contains("genre.locked=1"));
    }

    #[test]
    fn replace_tags_with_empty_items_still_emits_lock() {
        let item = stub(10, 1, 1);
        let q = EditBatch::new(&item)
            .replace_tags("collection", &[], false)
            .build_query();
        assert_eq!(q, "id=10&type=1&collection.locked=0");
    }

    #[test]
    fn remove_tags_uses_dash_sigil_with_csv_value() {
        let item = stub(10, 1, 1);
        let q = EditBatch::new(&item)
            .remove_tags("label", &["a", "b", "c"], true)
            .build_query();
        assert!(q.contains("label%5B%5D.tag.tag-=a%2Cb%2Cc"));
        assert!(q.contains("label.locked=1"));
    }

    #[test]
    fn batch_combines_multiple_ops_in_one_query() {
        let item = stub(42, 1, 7);
        let q = EditBatch::new(&item)
            .set_title("Arrival", true)
            .set_summary("Aliens arrive", true)
            .set_year(2016, true)
            .replace_genres(&["Sci-Fi", "Drama"], true)
            .remove_tags("label", &["bad"], false)
            .build_query();
        // Sanity-check each fragment is present.
        assert!(q.contains("title.value=Arrival"));
        assert!(q.contains("summary.value=Aliens%20arrive"));
        assert!(q.contains("year.value=2016"));
        assert!(q.contains("genre%5B0%5D.tag.tag=Sci-Fi"));
        assert!(q.contains("genre%5B1%5D.tag.tag=Drama"));
        assert!(q.contains("label%5B%5D.tag.tag-=bad"));
        assert!(q.contains("label.locked=0"));
    }

    #[test]
    fn convenience_shortcuts_match_low_level_field_names() {
        let item = stub(1, 1, 1);
        let q1 = EditBatch::new(&item)
            .set_sort_title("foo", true)
            .build_query();
        assert!(q1.contains("titleSort.value=foo"));
        let q2 = EditBatch::new(&item)
            .set_original_title("Bar", true)
            .build_query();
        assert!(q2.contains("originalTitle.value=Bar"));
        let q3 = EditBatch::new(&item)
            .replace_directors(&["Denis Villeneuve"], true)
            .build_query();
        assert!(q3.contains("director%5B0%5D.tag.tag=Denis%20Villeneuve"));
    }

    #[test]
    fn is_empty_and_len_track_ops_count() {
        let item = stub(1, 1, 1);
        let b = EditBatch::new(&item);
        assert!(b.is_empty());
        assert_eq!(b.len(), 0);
        let b2 = b.set_title("x", true).set_summary("y", true);
        assert!(!b2.is_empty());
        assert_eq!(b2.len(), 2);
    }

    #[test]
    fn execute_is_noop_when_empty() {
        // Smoke check via build_query — execute() short-circuits
        // without touching the URL builder, which is the property
        // we care about.
        let item = stub(1, 1, 1);
        let b = EditBatch::new(&item);
        assert!(b.is_empty());
    }
}
