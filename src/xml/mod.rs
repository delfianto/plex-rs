//! `<MediaContainer>` envelope shared by every PMS response.
//!
//! Plex Media Server wraps every response (XML or JSON) in a
//! `MediaContainer` root carrying pagination, identity, and library-
//! section metadata, plus a child list whose XML element / JSON field
//! name varies by endpoint (`Video`, `Directory`, `Metadata`, `Hub`,
//! `Setting`, `Provider`, …).
//!
//! From [`analysis/01-openapi-overview.md`](../../analysis/01-openapi-overview.md)
//! §4.6 the 12 `mediaContainerWith*` schemas in the official
//! `OpenAPI` spec collapse to **one** generic envelope in Rust. This module
//! provides that generic plus helpers to parse the JSON body Plex
//! returns when `Accept: application/json` is sent.
//!
//! The XML path (used by the small minority of endpoints that don't
//! speak JSON) is **not** modelled here — it lives in `xml::dto::*`
//! per surface area, since `quick_xml::de` cannot generically
//! dispatch on dynamically-named child elements without a custom
//! `Deserialize` impl. The JSON-first design follows the
//! recommendation in `analysis/01` §1.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};

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
    #[serde(default)]
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
}
