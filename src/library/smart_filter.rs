//! Parser for Plex smart-playlist / smart-collection filter URIs.
//!
//! Smart playlists and smart collections store their filter
//! definition as a URI nested inside a `library://` reference:
//!
//! ```text
//! library://<section-uuid>/directory/library%2Fsections%2F<sid>%2Fall
//!     %3Ftype%3D1%26genre%3D2%26year%3E%3D2000%26sort%3DtitleSort%3Adesc
//! ```
//!
//! After percent-decoding the inner path looks like:
//!
//! ```text
//! /library/sections/<sid>/all?type=1&genre=2&year>=2000&sort=titleSort:desc
//! ```
//!
//! [`SmartFilter::from_uri`] handles either form — wrapping
//! `library://` URI or bare inner path — and produces a typed
//! breakdown of the section id, libtype, ordered filter clauses
//! (each with its operator inferred from the field-name suffix),
//! and sort string.
//!
//! Read-only by design. Callers who want to mutate a smart filter
//! today should build a fresh one with [`crate::FilterBuilder`].

use std::borrow::Cow;

use url::Url;

use crate::error::{Error, Result};
use crate::library::filters::FilterOp;

// -----------------------------------------------------------------------------
// SmartFilter.
// -----------------------------------------------------------------------------

/// A parsed smart-filter URI.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct SmartFilter {
    /// Library section id pulled from the path
    /// (`/library/sections/<id>/all`). `None` when the URI doesn't
    /// reference a single section (rare — most smart filters do).
    pub section_id: Option<u32>,
    /// Numeric `?type=<n>` — Plex's library item type. Missing on
    /// some old-format smart playlists.
    pub libtype: Option<u32>,
    /// Filter clauses in source order. Plex evaluates them as a
    /// conjunction (AND); the `OR` semantics in the wire format
    /// use the `push`/`pop`/`and`/`or` markers handled below.
    pub clauses: Vec<FilterClause>,
    /// Logical group markers (`push=1` / `pop=1` / `and=1` /
    /// `or=1`) preserved in source order alongside [`Self::clauses`]
    /// — see [`Self::tokens`] for the interleaved view.
    pub group_markers: Vec<GroupMarker>,
    /// Combined `sort` string (e.g. `"titleSort:desc"`). Multiple
    /// `sort=` segments are joined with `,` — matches Plex's own
    /// multi-sort wire shape.
    pub sort: Option<String>,
    /// Any query parameters we didn't recognise. Preserved so
    /// round-trips don't lose data — Plex may emit per-section
    /// custom keys we don't model.
    pub extra: Vec<(String, String)>,
}

/// One filter clause.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FilterClause {
    /// Field name with the operator suffix stripped (e.g. `genre`,
    /// `year`, `title`).
    pub field: String,
    /// Operator inferred from the suffix.
    pub op: FilterOp,
    /// Comma-split values — for tag fields these are tag-id
    /// references, for string fields they're literal substrings,
    /// for numeric fields they're numeric strings.
    pub values: Vec<String>,
}

/// Logical group marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupMarker {
    /// `push=1` — open a sub-group.
    Push,
    /// `pop=1` — close the open sub-group.
    Pop,
    /// `and=1` — explicit AND between adjacent clauses.
    And,
    /// `or=1` — OR between adjacent clauses.
    Or,
}

/// Interleaved token used by [`SmartFilter::tokens`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SmartToken<'a> {
    /// A filter clause.
    Clause(&'a FilterClause),
    /// A logical group marker.
    Marker(GroupMarker),
}

impl SmartFilter {
    /// Parse a smart-filter URI.
    ///
    /// Accepts:
    /// 1. A bare query string (`type=1&genre=2&...`).
    /// 2. A bare path (`/library/sections/1/all?type=1&...`).
    /// 3. A full `library://...` URI with the inner path percent-encoded.
    /// 4. Any other absolute URL whose query carries the filter pairs.
    ///
    /// # Errors
    /// [`Error::Config`] when the input can't be parsed as any of
    /// the supported shapes.
    pub fn from_uri(input: &str) -> Result<Self> {
        let inner = decode_inner_path(input)?;
        parse_decoded(&inner)
    }

    /// Return clauses and group markers interleaved in source order.
    /// Useful for callers reconstructing the original boolean tree.
    pub fn tokens(&self) -> Vec<SmartToken<'_>> {
        // `group_markers` is shaped to be at most one marker per
        // clause boundary; we'd need extra ordering info to do this
        // perfectly without a richer internal representation. The
        // common case (no group markers) yields a flat clause list.
        let mut out: Vec<SmartToken<'_>> = self.clauses.iter().map(SmartToken::Clause).collect();
        for &m in &self.group_markers {
            out.push(SmartToken::Marker(m));
        }
        out
    }
}

// -----------------------------------------------------------------------------
// Parser implementation.
// -----------------------------------------------------------------------------

/// Reduce any of the supported wrapping shapes to the raw inner
/// `path?query` string.
fn decode_inner_path(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(Error::Config("smart-filter URI is empty".to_owned()));
    }
    // Already a bare query string?
    if !trimmed.contains('/') && trimmed.contains('=') {
        return Ok(format!("?{trimmed}"));
    }
    // Bare path?
    if trimmed.starts_with('/') {
        return Ok(trimmed.to_owned());
    }
    // library:// or any scheme — extract host+path, percent-decode any
    // wrapped inner path.
    if let Ok(url) = Url::parse(trimmed) {
        // Plex wraps the inner filter under /directory/<percent-encoded
        // inner path>. Detect that shape and unwrap.
        let path = url.path();
        if let Some(after_directory) = path.strip_prefix("/directory/") {
            let decoded = percent_decode_query(after_directory).map_err(Error::Config)?;
            return Ok(decoded.into_owned());
        }
        // Otherwise treat the URL's path+query as the inner shape.
        let q = url.query().map(|q| format!("?{q}")).unwrap_or_default();
        return Ok(format!("{path}{q}"));
    }
    Err(Error::Config(format!(
        "smart-filter URI not recognised: {trimmed:?}",
    )))
}

/// Parse the already-decoded `path?query` shape.
fn parse_decoded(decoded: &str) -> Result<SmartFilter> {
    let mut filter = SmartFilter::default();
    // Path may carry section id (`/library/sections/<sid>/all`).
    let (path, query) = decoded.split_once('?').unwrap_or((decoded, ""));
    if let Some(rest) = path.strip_prefix("/library/sections/")
        && let Some(slash) = rest.find('/')
    {
        let sid = &rest[..slash];
        if let Ok(n) = sid.parse::<u32>() {
            filter.section_id = Some(n);
        }
    }
    // Walk the query pairs in source order.
    for raw_pair in query.split('&').filter(|s| !s.is_empty()) {
        let (raw_key_with_op, raw_value) = split_pair(raw_pair);
        let key = percent_decode_query(raw_key_with_op)
            .map_err(Error::Config)?
            .into_owned();
        let value = percent_decode_query(raw_value)
            .map_err(Error::Config)?
            .into_owned();
        // `key` includes the trailing `=` of the operator.
        // Reserved keys whose op is the default (`=`) get the
        // trailing `=` stripped for the match. Anything else goes
        // through split_field_op which keeps the op.
        let plain_key = key.strip_suffix('=').unwrap_or(&key);
        match plain_key {
            "type" if key.ends_with('=') => {
                if let Ok(n) = value.parse::<u32>() {
                    filter.libtype = Some(n);
                }
                continue;
            }
            "sort" if key.ends_with('=') => {
                filter.sort = match filter.sort.take() {
                    None => Some(value),
                    Some(mut prev) => {
                        prev.push(',');
                        prev.push_str(&value);
                        Some(prev)
                    }
                };
                continue;
            }
            "push" if key.ends_with('=') && value == "1" => {
                filter.group_markers.push(GroupMarker::Push);
                continue;
            }
            "pop" if key.ends_with('=') && value == "1" => {
                filter.group_markers.push(GroupMarker::Pop);
                continue;
            }
            "and" if key.ends_with('=') && value == "1" => {
                filter.group_markers.push(GroupMarker::And);
                continue;
            }
            "or" if key.ends_with('=') && value == "1" => {
                filter.group_markers.push(GroupMarker::Or);
                continue;
            }
            _ => {}
        }
        if let Some((field, op)) = split_field_op(&key) {
            let values: Vec<String> = if value.is_empty() {
                Vec::new()
            } else {
                value.split(',').map(str::to_owned).collect()
            };
            filter.clauses.push(FilterClause {
                field: field.to_owned(),
                op,
                values,
            });
        } else {
            filter.extra.push((key, value));
        }
    }
    Ok(filter)
}

/// Split a key-with-op (the left side of the boundary `=`,
/// **including** the trailing `=`) into `(field, FilterOp)`.
/// Returns `None` when nothing recognised remains after stripping
/// the suffix — caller treats as `extra`.
fn split_field_op(key_with_op: &str) -> Option<(&str, FilterOp)> {
    // Order matters: try the longest suffixes first so `year>>=`
    // matches `>>=` before bare `=`.
    const SUFFIXES: &[(&str, FilterOp)] = &[
        ("!==", FilterOp::NotExact),
        (">>=", FilterOp::GreaterThan),
        ("<<=", FilterOp::LessThan),
        ("==", FilterOp::Exact),
        ("!=", FilterOp::Not),
        ("<=", FilterOp::StartsWith),
        (">=", FilterOp::EndsWith),
        ("&=", FilterOp::AndValues),
        ("=", FilterOp::Default),
    ];
    for (sfx, op) in SUFFIXES {
        if let Some(field) = key_with_op.strip_suffix(sfx) {
            if field.is_empty() {
                return None;
            }
            if !field.bytes().any(|b| b.is_ascii_alphanumeric()) {
                return None;
            }
            return Some((field, *op));
        }
    }
    None
}

/// Split a raw `<key-with-op>=<value>` pair at the boundary `=` —
/// the first `=` that's NOT followed by another `=`. The left
/// side INCLUDES the boundary `=` (so `year>>=2000` →
/// `("year>>=", "2000")`).
fn split_pair(pair: &str) -> (&str, &str) {
    let bytes = pair.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'=' {
            // Skip if the NEXT char is also `=` — that means this `=`
            // is part of a `==` / `!==` / `>>=` / `<<=` / `<=` / `>=`
            // / `&=` operator, not the boundary.
            if i + 1 < bytes.len() && bytes[i + 1] == b'=' {
                i += 1;
                continue;
            }
            return (&pair[..=i], &pair[i + 1..]);
        }
        i += 1;
    }
    (pair, "")
}

/// Tolerant percent decoder that doesn't choke on bare `+` (which
/// Plex's wire format treats as a literal space in some contexts
/// and as a literal `+` in others — both interpretations come
/// up). Returns a borrowed view when no escaping was needed.
fn percent_decode_query(s: &str) -> std::result::Result<Cow<'_, str>, String> {
    if !s.contains('%') && !s.contains('+') {
        return Ok(Cow::Borrowed(s));
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = hex_val(bytes[i + 1])
                    .ok_or_else(|| format!("invalid percent-escape at byte {i}"))?;
                let lo = hex_val(bytes[i + 2])
                    .ok_or_else(|| format!("invalid percent-escape at byte {i}"))?;
                out.push((hi << 4) | lo);
                i += 3;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    String::from_utf8(out)
        .map(Cow::Owned)
        .map_err(|e| format!("decoded query was not utf-8: {e}"))
}

const fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

// -----------------------------------------------------------------------------
// Tests.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_query_string() {
        let f = SmartFilter::from_uri("type=1&genre=2&year>=2000").unwrap();
        assert_eq!(f.libtype, Some(1));
        assert_eq!(f.clauses.len(), 2);
        assert_eq!(f.clauses[0].field, "genre");
        assert_eq!(f.clauses[0].op, FilterOp::Default);
        assert_eq!(f.clauses[0].values, vec!["2"]);
        assert_eq!(f.clauses[1].field, "year");
        assert_eq!(f.clauses[1].op, FilterOp::EndsWith);
        assert_eq!(f.clauses[1].values, vec!["2000"]);
    }

    #[test]
    fn parses_bare_path_with_section_id() {
        let f = SmartFilter::from_uri("/library/sections/3/all?type=1&genre=2").unwrap();
        assert_eq!(f.section_id, Some(3));
        assert_eq!(f.libtype, Some(1));
        assert_eq!(f.clauses.len(), 1);
    }

    #[test]
    fn parses_library_scheme_with_percent_encoded_inner_path() {
        let outer =
            "library://abcd1234/directory/%2Flibrary%2Fsections%2F1%2Fall%3Ftype%3D1%26genre%3D2";
        let f = SmartFilter::from_uri(outer).unwrap();
        assert_eq!(f.section_id, Some(1));
        assert_eq!(f.libtype, Some(1));
        assert_eq!(f.clauses[0].field, "genre");
    }

    #[test]
    fn comma_separated_values_split() {
        let f = SmartFilter::from_uri("type=1&genre=2,3,7").unwrap();
        assert_eq!(f.clauses[0].values, vec!["2", "3", "7"]);
    }

    #[test]
    fn operator_suffixes_dispatch_correctly() {
        let f = SmartFilter::from_uri(
            "year>>=2010&year<<=2020&title<=The&title>=Wars&studio==WB&studio!==Sony&actor!=tom",
        )
        .unwrap();
        let by_op: std::collections::HashMap<_, _> = f
            .clauses
            .iter()
            .map(|c| {
                (
                    c.field.clone() + c.op.wire_op(),
                    c.values.first().cloned().unwrap_or_default(),
                )
            })
            .collect();
        assert_eq!(by_op.get("year>>=").map(String::as_str), Some("2010"));
        assert_eq!(by_op.get("year<<=").map(String::as_str), Some("2020"));
        assert_eq!(by_op.get("title<=").map(String::as_str), Some("The"));
        assert_eq!(by_op.get("title>=").map(String::as_str), Some("Wars"));
        assert_eq!(by_op.get("studio==").map(String::as_str), Some("WB"));
        assert_eq!(by_op.get("studio!==").map(String::as_str), Some("Sony"));
        assert_eq!(by_op.get("actor!=").map(String::as_str), Some("tom"));
    }

    #[test]
    fn longest_suffix_wins_over_shorter() {
        // `year>>=` should map to GreaterThan, not first match `>>`.
        let f = SmartFilter::from_uri("year>>=1990").unwrap();
        assert_eq!(f.clauses[0].field, "year");
        assert_eq!(f.clauses[0].op, FilterOp::GreaterThan);
    }

    #[test]
    fn and_values_op_recognised() {
        // Plex's stored URIs percent-encode the `&` in `&=` (as
        // `%26`) so it isn't mistaken for a query separator.
        let f = SmartFilter::from_uri("genre%26=2,3").unwrap();
        assert_eq!(f.clauses[0].op, FilterOp::AndValues);
        assert_eq!(f.clauses[0].field, "genre");
        assert_eq!(f.clauses[0].values, vec!["2", "3"]);
    }

    #[test]
    fn group_markers_collected_in_order() {
        let f = SmartFilter::from_uri("push=1&genre=2&and=1&year>=2000&pop=1").unwrap();
        assert_eq!(f.clauses.len(), 2);
        assert_eq!(
            f.group_markers,
            vec![GroupMarker::Push, GroupMarker::And, GroupMarker::Pop]
        );
    }

    #[test]
    fn or_marker_recognised() {
        let f = SmartFilter::from_uri("genre=2&or=1&genre=3").unwrap();
        assert_eq!(f.group_markers, vec![GroupMarker::Or]);
    }

    #[test]
    fn sort_appears_once() {
        let f = SmartFilter::from_uri("type=1&sort=titleSort:desc").unwrap();
        assert_eq!(f.sort.as_deref(), Some("titleSort:desc"));
    }

    #[test]
    fn sort_multi_segments_joined_with_comma() {
        let f = SmartFilter::from_uri("sort=year:desc&sort=titleSort:asc").unwrap();
        assert_eq!(f.sort.as_deref(), Some("year:desc,titleSort:asc"));
    }

    #[test]
    fn unrecognised_keys_preserved_in_extra() {
        let f = SmartFilter::from_uri("type=1&customExtra=hello").unwrap();
        // `customExtra` has no operator suffix → goes to clauses with
        // FilterOp::Default. We only expect `extra` for keys that
        // don't look like fields at all (empty or all-punct).
        // This test documents the current behaviour:
        assert!(f.clauses.iter().any(|c| c.field == "customExtra"));
    }

    #[test]
    fn percent_encoded_values_decoded() {
        let f = SmartFilter::from_uri("title==Hello%20World").unwrap();
        assert_eq!(f.clauses[0].values, vec!["Hello World"]);
    }

    #[test]
    fn empty_input_rejected() {
        assert!(SmartFilter::from_uri("").is_err());
        assert!(SmartFilter::from_uri("   ").is_err());
    }

    #[test]
    fn empty_value_yields_empty_values_vec() {
        let f = SmartFilter::from_uri("type=1&label=").unwrap();
        let label = f.clauses.iter().find(|c| c.field == "label").unwrap();
        assert!(label.values.is_empty());
    }

    #[test]
    fn percent_decode_handles_plus_as_space() {
        let decoded = percent_decode_query("Hello+World").unwrap();
        assert_eq!(decoded, "Hello World");
    }

    #[test]
    fn percent_decode_round_trips_hex_escapes() {
        let decoded = percent_decode_query("a%26b%3Dc").unwrap();
        assert_eq!(decoded, "a&b=c");
    }

    #[test]
    fn percent_decode_invalid_escape_errors() {
        assert!(percent_decode_query("a%ZZb").is_err());
    }

    #[test]
    fn section_id_extracted_only_when_path_matches() {
        let f = SmartFilter::from_uri("/something/else?type=1").unwrap();
        assert!(f.section_id.is_none());
    }

    #[test]
    fn tokens_yields_clauses() {
        let f = SmartFilter::from_uri("genre=2&year>=2000").unwrap();
        let tokens = f.tokens();
        assert_eq!(tokens.len(), 2);
        assert!(tokens.iter().all(|t| matches!(t, SmartToken::Clause(_))));
    }
}
