//! [`EditField`] and field-specific traits.
//!
//! Plex's metadata edit endpoint is the somewhat-surprising
//! `PUT /library/sections/<section_id>/all?id=<rating_key>&type=<N>&<field>.value=<v>&<field>.locked=<0|1>`.
//! One request emits *one* `<field>.value` /
//! `<field>.locked` pair — multiple-field edits chain pairs in the
//! same query string. The `LibrarySectionRef` carried on every leaf
//! object provides the section-id back-link the URL needs.
//!
//! [`EditField`] is the low-level primitive: emit one
//! `field.value` / `field.locked` pair for any field. Field-specific
//! traits ([`EditTitle`], [`EditSummary`], …) layer on top with
//! `edit_title`/`edit_summary` named methods that pick the right
//! `field` string so callers don't have to remember Plex's
//! wire-format names.

use std::fmt;

use crate::error::Result;
use crate::traits::PlexObject;

// -----------------------------------------------------------------------------
// FieldValue — typed wrapper around the wire-format string value.
// -----------------------------------------------------------------------------

/// A typed value passed to [`EditField::edit_field`].
///
/// Plex serialises everything as a query-string value on the wire;
/// this enum exists so the call site reads more clearly than passing
/// a bare [`String`] for everything. Convert via [`From`] —
/// `&str` / `String` / `i64` / `u32` / `f32` / `bool` are all
/// supported.
#[derive(Debug, Clone)]
pub enum FieldValue {
    /// String — emitted verbatim (after percent-encoding).
    Str(String),
    /// Integer — formatted as decimal.
    Int(i64),
    /// Float — formatted with `{value}` (no exponent).
    Float(f32),
    /// Boolean — emitted as `1` / `0`.
    Bool(bool),
}

impl fmt::Display for FieldValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Str(s) => f.write_str(s),
            Self::Int(i) => i.fmt(f),
            Self::Float(v) => v.fmt(f),
            Self::Bool(b) => f.write_str(if *b { "1" } else { "0" }),
        }
    }
}

impl From<&str> for FieldValue {
    fn from(s: &str) -> Self {
        Self::Str(s.to_owned())
    }
}
impl From<String> for FieldValue {
    fn from(s: String) -> Self {
        Self::Str(s)
    }
}
impl From<i64> for FieldValue {
    fn from(v: i64) -> Self {
        Self::Int(v)
    }
}
impl From<i32> for FieldValue {
    fn from(v: i32) -> Self {
        Self::Int(v.into())
    }
}
impl From<u32> for FieldValue {
    fn from(v: u32) -> Self {
        Self::Int(v.into())
    }
}
impl From<u16> for FieldValue {
    fn from(v: u16) -> Self {
        Self::Int(v.into())
    }
}
impl From<f32> for FieldValue {
    fn from(v: f32) -> Self {
        Self::Float(v)
    }
}
impl From<bool> for FieldValue {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}

// -----------------------------------------------------------------------------
// EditField — the universal single-field edit primitive.
// -----------------------------------------------------------------------------

/// Emit a single-field edit request against PMS.
///
/// Implementors are leaf types whose metadata lives under a library
/// section. The default-method body constructs the wire URL —
/// implementors don't override it.
pub trait EditField: PlexObject {
    /// Edit `field` to `value`, optionally setting the lock flag.
    ///
    /// Plex's wire shape:
    /// `PUT /library/sections/<section_id>/all?id=<rating_key>&type=<N>&<field>.value=<v>&<field>.locked=<0|1>`
    ///
    /// `field` is the wire-format field name (e.g. `"title"`,
    /// `"summary"`, `"contentRating"`); field-specific traits like
    /// [`EditTitle`] supply the right string so call sites stay
    /// readable.
    ///
    /// `locked = true` tells PMS not to overwrite this field during
    /// the next metadata refresh.
    ///
    /// # Errors
    /// Any transport [`crate::Error`] variant.
    fn edit_field(
        &self,
        field: &str,
        value: impl Into<FieldValue> + Send,
        locked: bool,
    ) -> impl std::future::Future<Output = Result<()>> + Send
    where
        Self: Sync,
    {
        let value_owned: FieldValue = value.into();
        async move {
            let path = format!(
                "/library/sections/{section}/all?id={rk}&type={ty}&{f}.value={v}&{f}.locked={lock}",
                section = self.section_ref().id,
                rk = self.rating_key(),
                ty = self.metadata_type_id(),
                f = pct(field),
                v = pct(&value_owned.to_string()),
                lock = u8::from(locked),
            );
            let url = self.base_url().join(&path)?;
            self.http().put_no_body(url.as_str()).await
        }
    }

    /// Toggle just the lock flag on `field` without setting its
    /// value. Wire form:
    /// `PUT /library/sections/<sid>/all?id=<rk>&type=<n>&<field>.locked=<0|1>`.
    ///
    /// Used by the image lock traits (`HasArtLock::lock_art`,
    /// `HasPosterLock::lock_poster`, …) — Plex's lock-only path
    /// omits the `.value` pair entirely.
    ///
    /// # Errors
    /// Any transport [`crate::Error`].
    fn lock_field(
        &self,
        field: &str,
        locked: bool,
    ) -> impl std::future::Future<Output = Result<()>> + Send
    where
        Self: Sync,
    {
        let field_owned = field.to_owned();
        async move {
            let path = format!(
                "/library/sections/{section}/all?id={rk}&type={ty}&{f}.locked={lock}",
                section = self.section_ref().id,
                rk = self.rating_key(),
                ty = self.metadata_type_id(),
                f = pct(&field_owned),
                lock = u8::from(locked),
            );
            let url = self.base_url().join(&path)?;
            self.http().put_no_body(url.as_str()).await
        }
    }
}

/// RFC 3986 percent-encoder for query-string values. Crate-private
/// alias re-exported as [`pct_query`] for sibling trait modules
/// (`edit_tags`).
pub(crate) fn pct_query(input: &str) -> String {
    pct(input)
}

/// RFC 3986 percent-encoder for query-string values.
fn pct(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                out.push('%');
                out.push(hex_upper(byte >> 4));
                out.push(hex_upper(byte & 0x0F));
            }
        }
    }
    out
}

const fn hex_upper(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'A' + (n - 10)) as char,
        _ => '?',
    }
}

// -----------------------------------------------------------------------------
// EditTitle — the canonical example of a field-specific edit trait.
// -----------------------------------------------------------------------------

/// Edit the item's `title`.
///
/// Default body calls [`EditField::edit_field`] with `"title"` as
/// the wire field name. Implementors typically just `impl EditTitle
/// for $type {}` — the trait carries no required methods.
pub trait EditTitle: EditField {
    /// Set the item's title to `value`. `locked = true` prevents PMS
    /// from overwriting it on the next metadata refresh.
    ///
    /// # Errors
    /// Any [`crate::Error`] variant.
    fn edit_title(
        &self,
        value: &str,
        locked: bool,
    ) -> impl std::future::Future<Output = Result<()>> + Send
    where
        Self: Sync,
    {
        self.edit_field("title", value, locked)
    }
}

/// Edit the item's `summary`.
pub trait EditSummary: EditField {
    /// Set the item's summary.
    ///
    /// # Errors
    /// Any [`crate::Error`] variant.
    fn edit_summary(
        &self,
        value: &str,
        locked: bool,
    ) -> impl std::future::Future<Output = Result<()>> + Send
    where
        Self: Sync,
    {
        self.edit_field("summary", value, locked)
    }
}

/// Declare a string-valued field-specific edit trait.
///
/// Usage: `declare_edit_field_trait!(EditTagline, edit_tagline, "tagline");`
/// emits `pub trait EditTagline: EditField { fn edit_tagline(...) }`
/// that internally calls `edit_field("tagline", ...)`.
///
/// The wire field name (third argument) may differ from the Rust
/// method name when Plex's schema is inconsistent (e.g.
/// `EditSortTitle` edits `titleSort`, not `sortTitle`).
#[macro_export]
macro_rules! declare_edit_field_trait {
    ($trait_name:ident, $method_name:ident, $wire_field:expr) => {
        #[doc = concat!("Edit the item's `", $wire_field, "` field.")]
        pub trait $trait_name: $crate::traits::EditField {
            #[doc = concat!("Set the wire-format `", $wire_field, "` value.")]
            #[doc = ""]
            #[doc = "# Errors"]
            #[doc = "Any [`crate::Error`] variant."]
            fn $method_name(
                &self,
                value: &str,
                locked: bool,
            ) -> impl ::std::future::Future<Output = $crate::error::Result<()>> + Send
            where
                Self: Sync,
            {
                self.edit_field($wire_field, value, locked)
            }
        }
    };
}

declare_edit_field_trait!(EditTagline, edit_tagline, "tagline");
declare_edit_field_trait!(EditStudio, edit_studio, "studio");
declare_edit_field_trait!(EditContentRating, edit_content_rating, "contentRating");
// `titleSort` and `originalTitle` are intentional wire-format choices
// — Plex's edit endpoint uses these names regardless of how the field
// is exposed on the read side.
declare_edit_field_trait!(EditSortTitle, edit_sort_title, "titleSort");
declare_edit_field_trait!(EditOriginalTitle, edit_original_title, "originalTitle");

/// Edit the item's release `year`. Wire form is integer.
///
/// Provided as a hand-written trait rather than via
/// [`declare_edit_field_trait`] because the value is numeric and the
/// macro is string-oriented.
pub trait EditYear: EditField {
    /// Set the year.
    ///
    /// # Errors
    /// Any [`crate::Error`] variant.
    fn edit_year(
        &self,
        value: u16,
        locked: bool,
    ) -> impl std::future::Future<Output = Result<()>> + Send
    where
        Self: Sync,
    {
        self.edit_field("year", value, locked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_value_display_serialises_correctly() {
        assert_eq!(FieldValue::Str("Foo".into()).to_string(), "Foo");
        assert_eq!(FieldValue::Int(2024).to_string(), "2024");
        assert_eq!(FieldValue::Bool(true).to_string(), "1");
        assert_eq!(FieldValue::Bool(false).to_string(), "0");
    }

    #[test]
    fn field_value_converts_from_primitives() {
        let s: FieldValue = "Hello".into();
        assert!(matches!(s, FieldValue::Str(_)));
        let i: FieldValue = 5_i64.into();
        assert!(matches!(i, FieldValue::Int(5)));
        let u: FieldValue = 7_u32.into();
        assert!(matches!(u, FieldValue::Int(7)));
        let b: FieldValue = true.into();
        assert!(matches!(b, FieldValue::Bool(true)));
    }

    #[test]
    fn pct_encodes_reserved_chars() {
        assert_eq!(pct("Hello World"), "Hello%20World");
        assert_eq!(pct("a&b=c"), "a%26b%3Dc");
        assert_eq!(pct("safe-.~_"), "safe-.~_");
    }
}
