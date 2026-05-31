//! `<MediaContainer>` envelope shared by every PMS response.
//!
//! Plex Media Server wraps every response (XML or JSON) in a
//! `MediaContainer` root carrying pagination, identity, and library-
//! section metadata, plus a child list whose XML element / JSON field
//! name varies by endpoint (`Video`, `Directory`, `Metadata`, `Hub`,
//! `Setting`, `Provider`, …).
//!
//! The 12 `mediaContainerWith*` schemas in the official
//! `OpenAPI` spec collapse to **one** generic envelope in Rust. This module
//! provides that generic plus helpers to parse the JSON body Plex
//! returns when `Accept: application/json` is sent.
//!
//! The XML path (used by the small minority of endpoints that don't
//! speak JSON) is **not** modelled here — it lives in `xml::dto::*`
//! per surface area, since `quick_xml::de` cannot generically
//! dispatch on dynamically-named child elements without a custom
//! `Deserialize` impl.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};

/// Deserialize an optional `u32` that Plex may serialise as either a JSON
/// number (`1`) or a JSON string (`"1"`).
///
/// Plex is inconsistent across endpoints: library listings emit
/// `librarySectionID` as a number, but `/status/sessions/history` emits the
/// very same field as a string. This accepts both forms (and `null` /
/// absence as `None`) and rejects anything non-numeric.
pub(crate) fn de_opt_u32_flex<'de, D>(de: D) -> std::result::Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrStr {
        Num(u32),
        Str(String),
    }

    Ok(match Option::<NumOrStr>::deserialize(de)? {
        None => None,
        Some(NumOrStr::Num(n)) => Some(n),
        Some(NumOrStr::Str(s)) => Some(s.trim().parse().map_err(serde::de::Error::custom)?),
    })
}

/// Deserialize a field Plex may emit as a JSON string, boolean, or number
/// into a `String`.
///
/// `/:/prefs` is the worst offender: PMS 1.43+ emits the `value` / `default`
/// of `bool` settings as bare JSON booleans and of `int` / `double` settings
/// as bare numbers, while older servers quote everything. Normalising to a
/// `String` lets the downstream typed parse (per the setting's declared kind)
/// stay unchanged across both wire forms. `null` / absence becomes `""`.
pub(crate) fn de_string_flex<'de, D>(de: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrPrimitive {
        Str(String),
        Bool(bool),
        // `i64` / `f64` (not `serde_json::Number`) so the variants round-trip
        // through serde's untagged content buffer; `Str` is tried first so a
        // quoted numeric like `"60"` stays verbatim rather than being reparsed.
        Int(i64),
        Float(f64),
    }

    Ok(match Option::<StringOrPrimitive>::deserialize(de)? {
        None => String::new(),
        Some(StringOrPrimitive::Str(s)) => s,
        Some(StringOrPrimitive::Bool(b)) => b.to_string(),
        Some(StringOrPrimitive::Int(n)) => n.to_string(),
        Some(StringOrPrimitive::Float(f)) => f.to_string(),
    })
}

// -----------------------------------------------------------------------------
// MediaContainerMeta — the well-known scalar fields every container carries.
// -----------------------------------------------------------------------------

/// Scalar metadata fields shared by every `<MediaContainer>` response.
///
/// Every field is optional because individual endpoints populate only
/// the subset they care about (e.g. `/identity` populates
/// `machine_identifier`+`version` but no pagination fields; library
/// listings populate `size`/`total_size` but no `identifier`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaContainerMeta {
    /// Number of items in *this* response page. Always present in
    /// practice; defaults to zero if Plex omits it.
    #[serde(default)]
    pub size: u32,

    /// Total number of items matching the request, across all pages.
    /// Present on listing endpoints.
    #[serde(default)]
    pub total_size: Option<u32>,

    /// Pagination offset of this page (echoes `X-Plex-Container-Start`).
    #[serde(default)]
    pub offset: Option<u32>,

    /// PMS / plex.tv content provider identifier
    /// (e.g. `com.plexapp.plugins.library`).
    #[serde(default)]
    pub identifier: Option<String>,

    /// URL prefix Plex uses for image / thumbnail asset URLs.
    #[serde(default)]
    pub media_tag_prefix: Option<String>,

    /// Version stamp on Plex's media tag set.
    #[serde(default)]
    pub media_tag_version: Option<i64>,

    /// Whether the requested resource is cloud-sync-eligible.
    #[serde(default)]
    pub allow_sync: Option<bool>,

    /// Single-line title for the page (e.g. section name).
    #[serde(default)]
    pub title: Option<String>,

    /// Section breadcrumb segment 1 (typically the library name).
    #[serde(default)]
    pub title1: Option<String>,

    /// Section breadcrumb segment 2 (typically the category name).
    #[serde(default)]
    pub title2: Option<String>,

    /// Library section integer key for child items, when applicable.
    #[serde(default, deserialize_with = "crate::xml::de_opt_u32_flex")]
    pub library_section_id: Option<u32>,

    /// Library section title.
    #[serde(default)]
    pub library_section_title: Option<String>,

    /// Library section UUID (for `library://` URI construction).
    #[serde(default)]
    pub library_section_uuid: Option<String>,

    /// Whether the response page contains the last item in the set
    /// (paginated listings).
    #[serde(default)]
    pub more: Option<bool>,
}

// -----------------------------------------------------------------------------
// MediaContainer<T> — meta + a typed Vec of items.
// -----------------------------------------------------------------------------

/// The fully-parsed envelope: metadata + a typed list of items.
///
/// Construct via [`MediaContainer::from_json`], which takes the JSON
/// field name that holds the child list (Plex uses different names
/// per endpoint — `Metadata`, `Directory`, `Hub`, `Setting`, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaContainer<T> {
    /// Common scalar fields.
    pub meta: MediaContainerMeta,
    /// Typed children, deserialised from the JSON field whose name was
    /// passed to [`MediaContainer::from_json`].
    pub items: Vec<T>,
}

impl<T> MediaContainer<T> {
    /// Length of the [`items`](Self::items) list.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether [`items`](Self::items) is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl<T> Default for MediaContainer<T> {
    fn default() -> Self {
        Self {
            meta: MediaContainerMeta::default(),
            items: Vec::new(),
        }
    }
}

// -----------------------------------------------------------------------------
// JSON parsing.
// -----------------------------------------------------------------------------

/// Outer wrapper carrying the JSON object: `{"MediaContainer": {...}}`.
#[derive(Debug, Deserialize)]
struct JsonEnvelope {
    /// Inner container object. The body field name is fixed by Plex.
    #[serde(rename = "MediaContainer")]
    container: Value,
}

impl<T> MediaContainer<T>
where
    T: for<'de> Deserialize<'de>,
{
    /// Parse a JSON `{"MediaContainer": {...}}` body, extracting the
    /// items list from the field named `items_key`.
    ///
    /// Plex uses different child field names per endpoint:
    /// - `Metadata` — movies, episodes, tracks, photos, playlists
    /// - `Directory` — library sections, navigation
    /// - `Hub` — universal search / continueWatching
    /// - `Setting` — server preferences
    /// - `Provider` — agents
    /// - `Server` — `/myplex/account` listing
    ///
    /// The caller passes the appropriate key for the endpoint being
    /// parsed.
    ///
    /// # Errors
    /// Returns [`Error::Json`] if the body isn't valid JSON or doesn't
    /// have the expected `{"MediaContainer": {...}}` shape, and
    /// [`Error::Internal`] when the items field exists but isn't a JSON
    /// array.
    pub fn from_json(body: &str, items_key: &str) -> Result<Self> {
        let env: JsonEnvelope = serde_json::from_str(body)?;
        let inner = env.container;
        Self::from_inner_value(inner, items_key)
    }

    /// Parse from an already-extracted `MediaContainer` JSON value
    /// (useful when an outer endpoint embeds a MediaContainer-shaped
    /// inner object).
    ///
    /// # Errors
    /// See [`from_json`](Self::from_json).
    pub fn from_inner_value(inner: Value, items_key: &str) -> Result<Self> {
        let mut inner_obj = match inner {
            Value::Object(map) => map,
            other => {
                return Err(Error::Internal(
                    "MediaContainer payload was not a JSON object",
                ))
                .map_err(|_e: Error| {
                    Error::Json(serde::de::Error::custom(format!(
                        "MediaContainer payload was {} not object",
                        type_name(&other),
                    )))
                });
            }
        };
        // Extract the items array first so it doesn't try to deserialise as part of meta.
        let items_value = inner_obj.remove(items_key);
        let meta_value = Value::Object(inner_obj);
        let meta: MediaContainerMeta = serde_json::from_value(meta_value)?;
        let items: Vec<T> = match items_value {
            Some(Value::Array(arr)) => arr
                .into_iter()
                .map(serde_json::from_value::<T>)
                .collect::<std::result::Result<Vec<_>, _>>()?,
            // Field absent or null → empty page.
            None | Some(Value::Null) => Vec::new(),
            Some(other) => {
                return Err(Error::Json(serde::de::Error::custom(format!(
                    "MediaContainer.{items_key} was {} not array",
                    type_name(&other),
                ))));
            }
        };
        Ok(Self { meta, items })
    }
}

const fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
    struct StubItem {
        #[serde(rename = "ratingKey")]
        rating_key: String,
        title: String,
    }

    // ---------- happy paths ----------

    #[test]
    fn parses_pagination_metadata() {
        let body = r#"{
            "MediaContainer": {
                "size": 2,
                "totalSize": 42,
                "offset": 0,
                "identifier": "com.plexapp.plugins.library",
                "title1": "Movies",
                "Metadata": [
                    {"ratingKey": "1", "title": "First"},
                    {"ratingKey": "2", "title": "Second"}
                ]
            }
        }"#;
        let c: MediaContainer<StubItem> = MediaContainer::from_json(body, "Metadata").unwrap();
        assert_eq!(c.meta.size, 2);
        assert_eq!(c.meta.total_size, Some(42));
        assert_eq!(c.meta.offset, Some(0));
        assert_eq!(
            c.meta.identifier.as_deref(),
            Some("com.plexapp.plugins.library")
        );
        assert_eq!(c.meta.title1.as_deref(), Some("Movies"));
        assert_eq!(c.len(), 2);
        assert_eq!(c.items[0].title, "First");
    }

    #[test]
    fn parses_empty_container() {
        let body = r#"{
            "MediaContainer": {
                "size": 0
            }
        }"#;
        let c: MediaContainer<StubItem> = MediaContainer::from_json(body, "Metadata").unwrap();
        assert_eq!(c.meta.size, 0);
        assert!(c.is_empty());
    }

    #[test]
    fn supports_different_item_field_names() {
        let body = r#"{
            "MediaContainer": {
                "size": 1,
                "Directory": [{"ratingKey": "9", "title": "Movies"}]
            }
        }"#;
        let c: MediaContainer<StubItem> = MediaContainer::from_json(body, "Directory").unwrap();
        assert_eq!(c.items[0].rating_key, "9");
    }

    #[test]
    fn missing_items_field_yields_empty_vec() {
        let body = r#"{"MediaContainer": {"size": 0, "title1": "Empty"}}"#;
        let c: MediaContainer<StubItem> = MediaContainer::from_json(body, "Metadata").unwrap();
        assert_eq!(c.meta.title1.as_deref(), Some("Empty"));
        assert!(c.items.is_empty());
    }

    #[test]
    fn null_items_field_yields_empty_vec() {
        let body = r#"{"MediaContainer": {"size": 0, "Metadata": null}}"#;
        let c: MediaContainer<StubItem> = MediaContainer::from_json(body, "Metadata").unwrap();
        assert!(c.items.is_empty());
    }

    #[test]
    fn unknown_meta_fields_are_ignored() {
        let body = r#"{
            "MediaContainer": {
                "size": 0,
                "future_field_plex_adds_in_v1_99": "ok"
            }
        }"#;
        let c: MediaContainer<StubItem> = MediaContainer::from_json(body, "Metadata").unwrap();
        assert_eq!(c.meta.size, 0);
    }

    // ---------- error paths ----------

    #[test]
    fn rejects_garbage_json() {
        let err = MediaContainer::<StubItem>::from_json("{", "Metadata").unwrap_err();
        assert!(matches!(err, Error::Json(_)));
    }

    #[test]
    fn rejects_missing_outer_wrapper() {
        let err = MediaContainer::<StubItem>::from_json(r#"{"size": 0}"#, "Metadata").unwrap_err();
        assert!(matches!(err, Error::Json(_)));
    }

    #[test]
    fn rejects_non_array_items_field() {
        let body = r#"{"MediaContainer": {"size": 0, "Metadata": "not-an-array"}}"#;
        let err = MediaContainer::<StubItem>::from_json(body, "Metadata").unwrap_err();
        assert!(matches!(err, Error::Json(_)));
    }

    #[test]
    fn rejects_inner_value_not_object() {
        let err =
            MediaContainer::<StubItem>::from_inner_value(Value::Null, "Metadata").unwrap_err();
        assert!(matches!(err, Error::Json(_)));
    }

    #[test]
    fn item_parse_error_propagates() {
        // ratingKey is required as a string here; passing a number should fail.
        let body =
            r#"{"MediaContainer": {"size": 1, "Metadata": [{"ratingKey": 1, "title": "T"}]}}"#;
        let err = MediaContainer::<StubItem>::from_json(body, "Metadata").unwrap_err();
        assert!(matches!(err, Error::Json(_)));
    }

    // ---------- convenience ----------

    #[test]
    fn default_container_is_empty() {
        let c: MediaContainer<StubItem> = MediaContainer::default();
        assert_eq!(c.len(), 0);
        assert!(c.is_empty());
        assert_eq!(c.meta, MediaContainerMeta::default());
    }

    // ---------- de_string_flex ----------

    #[derive(Debug, Deserialize)]
    struct FlexStr {
        #[serde(default, deserialize_with = "de_string_flex")]
        value: String,
    }

    fn flex_str(v: serde_json::Value) -> String {
        let mut map = serde_json::Map::new();
        map.insert("value".to_owned(), v);
        let parsed: FlexStr = serde_json::from_value(serde_json::Value::Object(map)).unwrap();
        parsed.value
    }

    #[test]
    fn de_string_flex_accepts_string() {
        assert_eq!(flex_str(serde_json::json!("hi")), "hi");
    }

    #[test]
    fn de_string_flex_accepts_bool() {
        assert_eq!(flex_str(serde_json::json!(true)), "true");
        assert_eq!(flex_str(serde_json::json!(false)), "false");
    }

    #[test]
    fn de_string_flex_accepts_integer() {
        assert_eq!(flex_str(serde_json::json!(60)), "60");
    }

    #[test]
    fn de_string_flex_accepts_float() {
        assert_eq!(flex_str(serde_json::json!(1.5)), "1.5");
    }

    #[test]
    fn de_string_flex_null_and_absent_become_empty() {
        assert_eq!(flex_str(serde_json::json!(null)), "");
        // Field entirely absent (relies on #[serde(default)]).
        let parsed: FlexStr = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(parsed.value, "");
    }

    // ---------- de_opt_u32_flex ----------

    #[derive(Debug, Deserialize)]
    struct FlexOptU32 {
        #[serde(default, deserialize_with = "de_opt_u32_flex")]
        value: Option<u32>,
    }

    fn flex_opt_u32(v: serde_json::Value) -> std::result::Result<Option<u32>, serde_json::Error> {
        let mut map = serde_json::Map::new();
        map.insert("value".to_owned(), v);
        serde_json::from_value::<FlexOptU32>(serde_json::Value::Object(map)).map(|p| p.value)
    }

    #[test]
    fn de_opt_u32_flex_accepts_number() {
        assert_eq!(flex_opt_u32(serde_json::json!(5)).unwrap(), Some(5));
    }

    #[test]
    fn de_opt_u32_flex_accepts_numeric_string() {
        assert_eq!(flex_opt_u32(serde_json::json!("5")).unwrap(), Some(5));
    }

    #[test]
    fn de_opt_u32_flex_trims_whitespace_in_string() {
        assert_eq!(flex_opt_u32(serde_json::json!(" 7 ")).unwrap(), Some(7));
    }

    #[test]
    fn de_opt_u32_flex_null_and_absent_become_none() {
        assert_eq!(flex_opt_u32(serde_json::json!(null)).unwrap(), None);
        let parsed: FlexOptU32 = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(parsed.value, None);
    }

    #[test]
    fn de_opt_u32_flex_rejects_non_numeric_string() {
        assert!(flex_opt_u32(serde_json::json!("abc")).is_err());
    }
}
