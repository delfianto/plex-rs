//! [`FilterBuilder`] — typed wrapper over Plex's filter / sort /
//! pagination query parameters.
//!
//! Plex's filter language lives in the URL query string of the
//! section-listing endpoints (`/library/sections/<id>/all`). Each
//! clause is `field<op>=value`. The set of operators is documented
//! in `python-plexapi/plexapi/library.py:1440-1460`:
//!
//! | Plex op   | Meaning (per field type)                          |
//! | --------- | ------------------------------------------------- |
//! | `=`       | int/tag/bool: is. str: contains.                  |
//! | `!=`      | int/tag/bool: is not. str: does not contain.      |
//! | `==`      | str: is (exact).                                  |
//! | `!==`     | str: is not (exact).                              |
//! | `<=`      | str: begins-with.                                 |
//! | `>=`      | str: ends-with.                                   |
//! | `>>=`     | int/datetime: greater-than / after.               |
//! | `<<=`     | int/datetime: less-than / before.                 |
//! | `&=`      | AND-combine multiple values.                      |
//!
//! [`FilterBuilder`] exposes these as named methods
//! (`equal`, `not_equal`, `exact`, `not_exact`, `starts_with`,
//! `ends_with`, `gt`, `lt`) that pick the right wire op for the
//! caller. It does **not** validate operators against the section's
//! filter schema — that requires a round-trip to
//! `/library/sections/<id>/filters` and lands in a future
//! iteration (`smart_filter` module).

use std::fmt;

// -----------------------------------------------------------------------------
// FilterOp — wire-level operator suffixes.
// -----------------------------------------------------------------------------

/// Wire-level filter operator suffix.
///
/// Public so callers wiring more exotic ops can request a specific
/// suffix via [`FilterBuilder::clause`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FilterOp {
    /// `field=value` — default. int/tag/bool: equal. str: contains.
    Default,
    /// `field!=value` — default negated.
    Not,
    /// `field==value` — string exact-equal.
    Exact,
    /// `field!==value` — string exact-not-equal.
    NotExact,
    /// `field<=value` — string starts-with.
    StartsWith,
    /// `field>=value` — string ends-with.
    EndsWith,
    /// `field>>=value` — numeric / datetime greater-than / after.
    GreaterThan,
    /// `field<<=value` — numeric / datetime less-than / before.
    LessThan,
    /// `field&=v1,v2,…` — AND-combine multiple values into a single clause.
    AndValues,
}

impl FilterOp {
    /// The literal suffix Plex expects between the field name and
    /// the `=value` portion.
    #[must_use]
    pub const fn wire_op(self) -> &'static str {
        match self {
            Self::Default => "=",
            Self::Not => "!=",
            Self::Exact => "==",
            Self::NotExact => "!==",
            Self::StartsWith => "<=",
            Self::EndsWith => ">=",
            Self::GreaterThan => ">>=",
            Self::LessThan => "<<=",
            Self::AndValues => "&=",
        }
    }
}

// -----------------------------------------------------------------------------
// SortDirection.
// -----------------------------------------------------------------------------

/// Sort direction passed to [`FilterBuilder::sort_by`] /
/// [`FilterBuilder::sort_by_desc`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SortDirection {
    /// Ascending — emitted as `sort=field:asc`.
    Asc,
    /// Descending — emitted as `sort=field:desc`.
    Desc,
}

impl SortDirection {
    /// The literal Plex sort-direction string.
    #[must_use]
    pub const fn as_wire(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }
}

// -----------------------------------------------------------------------------
// FilterBuilder.
// -----------------------------------------------------------------------------

/// Fluent builder over Plex's filter / sort / pagination query
/// parameters.
///
/// Construct with [`FilterBuilder::new`], chain `.equal()`, `.gt()`,
/// `.sort_by()`, `.limit()` etc., then pass to
/// [`crate::LibrarySection::filter`] for execution. The builder
/// itself is dependency-free and can be reused / inspected via
/// [`FilterBuilder::build_query`].
#[derive(Debug, Clone, Default)]
pub struct FilterBuilder {
    clauses: Vec<(String, FilterOp, String)>,
    sort: Option<(String, SortDirection)>,
    limit: Option<u32>,
    container_start: Option<u32>,
    container_size: Option<u32>,
    libtype: Option<u32>,
}

impl FilterBuilder {
    /// Construct an empty builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Restrict the listing to a specific Plex metadata type
    /// (`type=N` on the wire — see [`crate::SearchType`]).
    ///
    /// Most listing endpoints require this; section-level helpers
    /// (`movies()`, `shows()`, etc.) set it for you. When using
    /// [`crate::LibrarySection::filter`], set it explicitly with
    /// [`Self::libtype`].
    #[must_use]
    pub const fn libtype(mut self, type_id: u32) -> Self {
        self.libtype = Some(type_id);
        self
    }

    /// Append a default-operator clause (`field=value`).
    ///
    /// For tag / int / bool fields this is equality; for string fields
    /// this is substring contains.
    #[must_use]
    pub fn equal(mut self, field: impl Into<String>, value: impl fmt::Display) -> Self {
        self.clauses
            .push((field.into(), FilterOp::Default, value.to_string()));
        self
    }

    /// Append a negation clause (`field!=value`).
    #[must_use]
    pub fn not_equal(mut self, field: impl Into<String>, value: impl fmt::Display) -> Self {
        self.clauses
            .push((field.into(), FilterOp::Not, value.to_string()));
        self
    }

    /// Append a string-exact-equal clause (`field==value`).
    #[must_use]
    pub fn exact(mut self, field: impl Into<String>, value: impl fmt::Display) -> Self {
        self.clauses
            .push((field.into(), FilterOp::Exact, value.to_string()));
        self
    }

    /// Append a string-exact-not-equal clause (`field!==value`).
    #[must_use]
    pub fn not_exact(mut self, field: impl Into<String>, value: impl fmt::Display) -> Self {
        self.clauses
            .push((field.into(), FilterOp::NotExact, value.to_string()));
        self
    }

    /// Append a string starts-with clause (`field<=value`).
    #[must_use]
    pub fn starts_with(mut self, field: impl Into<String>, value: impl fmt::Display) -> Self {
        self.clauses
            .push((field.into(), FilterOp::StartsWith, value.to_string()));
        self
    }

    /// Append a string ends-with clause (`field>=value`).
    #[must_use]
    pub fn ends_with(mut self, field: impl Into<String>, value: impl fmt::Display) -> Self {
        self.clauses
            .push((field.into(), FilterOp::EndsWith, value.to_string()));
        self
    }

    /// Append a numeric / datetime greater-than clause (`field>>=value`).
    #[must_use]
    pub fn gt(mut self, field: impl Into<String>, value: impl fmt::Display) -> Self {
        self.clauses
            .push((field.into(), FilterOp::GreaterThan, value.to_string()));
        self
    }

    /// Append a numeric / datetime less-than clause (`field<<=value`).
    #[must_use]
    pub fn lt(mut self, field: impl Into<String>, value: impl fmt::Display) -> Self {
        self.clauses
            .push((field.into(), FilterOp::LessThan, value.to_string()));
        self
    }

    /// Append an AND-combine clause (`field&=v1,v2,…`).
    ///
    /// Plex applies all listed values with AND semantics — e.g.
    /// `genre&=Action,Sci-Fi` returns items tagged with **both**.
    #[must_use]
    pub fn and_values<I, V>(mut self, field: impl Into<String>, values: I) -> Self
    where
        I: IntoIterator<Item = V>,
        V: fmt::Display,
    {
        let joined = values
            .into_iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",");
        self.clauses
            .push((field.into(), FilterOp::AndValues, joined));
        self
    }

    /// Append a clause with an explicit [`FilterOp`]. Use this when
    /// you need an operator the named convenience methods don't
    /// cover.
    #[must_use]
    pub fn clause(
        mut self,
        field: impl Into<String>,
        op: FilterOp,
        value: impl fmt::Display,
    ) -> Self {
        self.clauses.push((field.into(), op, value.to_string()));
        self
    }

    /// Sort the result by `field` ascending (`sort=field:asc`).
    #[must_use]
    pub fn sort_by(mut self, field: impl Into<String>) -> Self {
        self.sort = Some((field.into(), SortDirection::Asc));
        self
    }

    /// Sort the result by `field` descending (`sort=field:desc`).
    #[must_use]
    pub fn sort_by_desc(mut self, field: impl Into<String>) -> Self {
        self.sort = Some((field.into(), SortDirection::Desc));
        self
    }

    /// Cap the response size (`X-Plex-Container-Size` header).
    ///
    /// Plex paginates listings; the header pair is the official
    /// pagination mechanism (see
    /// [`crate::PageRange`]). For most callers `.limit()` is enough.
    #[must_use]
    pub const fn limit(mut self, n: u32) -> Self {
        self.limit = Some(n);
        self
    }

    /// Set the offset for paginated listings.
    #[must_use]
    pub const fn offset(mut self, n: u32) -> Self {
        self.container_start = Some(n);
        self
    }

    /// Set the page size for paginated listings (alias of
    /// [`Self::limit`] when the caller is thinking in terms of
    /// pagination instead of result-count caps).
    #[must_use]
    pub const fn page_size(mut self, n: u32) -> Self {
        self.container_size = Some(n);
        self
    }

    /// Render the configured filter as a URL query string suffix
    /// (excluding the leading `?`). Idempotent — repeated calls return
    /// the same string. Empty when no clauses / sort / pagination is
    /// configured.
    #[must_use]
    pub fn build_query(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(t) = self.libtype {
            parts.push(format!("type={t}"));
        }
        for (field, op, value) in &self.clauses {
            parts.push(format!(
                "{field}{op}{value}",
                field = pct_encode(field),
                op = op.wire_op(),
                value = pct_encode(value),
            ));
        }
        if let Some((field, dir)) = &self.sort {
            parts.push(format!("sort={}%3A{}", pct_encode(field), dir.as_wire()));
        }
        if let Some(n) = self.limit {
            parts.push(format!("limit={n}"));
        }
        if let Some(n) = self.container_start {
            parts.push(format!("X-Plex-Container-Start={n}"));
        }
        if let Some(n) = self.container_size {
            parts.push(format!("X-Plex-Container-Size={n}"));
        }
        parts.join("&")
    }

    /// Returns `true` when the builder would emit an empty query
    /// string (no clauses, no sort, no pagination).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.clauses.is_empty()
            && self.sort.is_none()
            && self.limit.is_none()
            && self.container_start.is_none()
            && self.container_size.is_none()
            && self.libtype.is_none()
    }
}

/// RFC 3986 percent-encoder for query strings.
fn pct_encode(input: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_builder_emits_empty_string() {
        let b = FilterBuilder::new();
        assert!(b.is_empty());
        assert_eq!(b.build_query(), "");
    }

    #[test]
    fn filter_op_wire_suffixes_match_plex_docs() {
        // Per python-plexapi/library.py:1442-1460
        assert_eq!(FilterOp::Default.wire_op(), "=");
        assert_eq!(FilterOp::Not.wire_op(), "!=");
        assert_eq!(FilterOp::Exact.wire_op(), "==");
        assert_eq!(FilterOp::NotExact.wire_op(), "!==");
        assert_eq!(FilterOp::StartsWith.wire_op(), "<=");
        assert_eq!(FilterOp::EndsWith.wire_op(), ">=");
        assert_eq!(FilterOp::GreaterThan.wire_op(), ">>=");
        assert_eq!(FilterOp::LessThan.wire_op(), "<<=");
        assert_eq!(FilterOp::AndValues.wire_op(), "&=");
    }

    #[test]
    fn equal_clause_emits_default_op() {
        let q = FilterBuilder::new().equal("genre", "Action").build_query();
        assert_eq!(q, "genre=Action");
    }

    #[test]
    fn not_equal_clause_emits_negation() {
        let q = FilterBuilder::new()
            .not_equal("genre", "Comedy")
            .build_query();
        assert_eq!(q, "genre!=Comedy");
    }

    #[test]
    fn exact_clauses_for_strings() {
        let q = FilterBuilder::new().exact("title", "Arrival").build_query();
        assert_eq!(q, "title==Arrival");
    }

    #[test]
    fn starts_with_uses_str_begin_op() {
        let q = FilterBuilder::new()
            .starts_with("title", "Blade")
            .build_query();
        assert_eq!(q, "title<=Blade");
    }

    #[test]
    fn ends_with_uses_str_end_op() {
        let q = FilterBuilder::new()
            .ends_with("title", "Runner")
            .build_query();
        assert_eq!(q, "title>=Runner");
    }

    #[test]
    fn gt_lt_use_double_arrow_for_int_datetime() {
        let q = FilterBuilder::new()
            .gt("year", 2020)
            .lt("year", 2025)
            .build_query();
        assert_eq!(q, "year>>=2020&year<<=2025");
    }

    #[test]
    fn and_values_joins_with_commas() {
        let q = FilterBuilder::new()
            .and_values("genre", ["Action", "Sci-Fi"])
            .build_query();
        assert_eq!(q, "genre&=Action%2CSci-Fi");
    }

    #[test]
    fn sort_by_uses_colon_with_pct_encoded_direction_separator() {
        let q = FilterBuilder::new().sort_by("titleSort").build_query();
        assert_eq!(q, "sort=titleSort%3Aasc");
        let q = FilterBuilder::new().sort_by_desc("addedAt").build_query();
        assert_eq!(q, "sort=addedAt%3Adesc");
    }

    #[test]
    fn limit_offset_size_each_render() {
        let q = FilterBuilder::new()
            .limit(50)
            .offset(100)
            .page_size(25)
            .build_query();
        assert_eq!(
            q,
            "limit=50&X-Plex-Container-Start=100&X-Plex-Container-Size=25"
        );
    }

    #[test]
    fn libtype_emits_type_param() {
        let q = FilterBuilder::new().libtype(1).build_query();
        assert_eq!(q, "type=1");
    }

    #[test]
    fn full_chain_round_trips_in_insertion_order() {
        let q = FilterBuilder::new()
            .libtype(1)
            .equal("genre", "Action")
            .gt("year", 2010)
            .sort_by_desc("rating")
            .limit(20)
            .build_query();
        assert_eq!(
            q,
            "type=1&genre=Action&year>>=2010&sort=rating%3Adesc&limit=20"
        );
    }

    #[test]
    fn pct_encoding_handles_spaces_and_punctuation() {
        let q = FilterBuilder::new()
            .equal("director", "Denis Villeneuve")
            .build_query();
        assert_eq!(q, "director=Denis%20Villeneuve");
    }

    #[test]
    fn explicit_clause_with_custom_op() {
        let q = FilterBuilder::new()
            .clause("custom", FilterOp::AndValues, "a,b,c")
            .build_query();
        assert_eq!(q, "custom&=a%2Cb%2Cc");
    }
}
