//! Discover catalogue search.
//!
//! Plex's Discover endpoint runs full-text search against the
//! cloud catalogue rather than a single PMS library. Results
//! come back as global catalogue entries (same shape as
//! [`crate::WatchlistItem`]) identified by `plex://kind/<hex>`
//! GUIDs.
//!
//! ## Wire endpoint
//!
//! ```text
//! GET https://discover.provider.plex.tv/library/search
//!     ?query=<text>
//!     &limit=<n>
//!     &searchTypes=movies,tv
//!     &searchProviders=discover
//!     &includeMetadata=1
//! ```
//!
//! The response wraps results in `SearchResults[]`, each carrying
//! a `SearchResult[]` keyed by `id` (`"external"` is the catalogue
//! itself, others surface in-library matches when the user has a
//! PMS attached). We flatten and return every catalogue match.

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::myplex::MyPlexClient;

// -----------------------------------------------------------------------------
// DiscoverKind / DiscoverOptions.
// -----------------------------------------------------------------------------

/// Type filter on a Discover search.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DiscoverKind {
    /// `searchTypes=movies` — movies only.
    Movie,
    /// `searchTypes=tv` — TV shows only.
    Show,
}

impl DiscoverKind {
    /// Wire spelling of the `searchTypes` parameter.
    #[must_use]
    pub const fn as_wire(self) -> &'static str {
        match self {
            Self::Movie => "movies",
            Self::Show => "tv",
        }
    }
}

/// Options for [`MyPlexClient::discover_search`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DiscoverOptions {
    /// Hard cap on results. Default 30 (matches python-plexapi).
    pub limit: u32,
    /// Type filter. `None` searches both movies and shows.
    pub kind: Option<DiscoverKind>,
    /// Which catalogue providers to query. Default `"discover"`.
    /// Set to e.g. `"discover,PLEXAVOD,PLEXTVOD"` to also include
    /// Plex's free / rental services.
    pub providers: String,
}

impl Default for DiscoverOptions {
    fn default() -> Self {
        Self {
            limit: 30,
            kind: None,
            providers: "discover".to_owned(),
        }
    }
}

impl DiscoverOptions {
    /// Set [`Self::limit`] (builder style).
    #[must_use]
    pub const fn with_limit(mut self, n: u32) -> Self {
        self.limit = n;
        self
    }

    /// Restrict to movies or shows.
    #[must_use]
    pub const fn with_kind(mut self, kind: DiscoverKind) -> Self {
        self.kind = Some(kind);
        self
    }

    /// Override the search-providers list.
    #[must_use]
    pub fn with_providers(mut self, p: impl Into<String>) -> Self {
        self.providers = p.into();
        self
    }
}

// -----------------------------------------------------------------------------
// DiscoverItem.
// -----------------------------------------------------------------------------

/// One result from a Discover search.
///
/// Same shape as [`crate::WatchlistItem`] (cloud catalogue
/// entries, no PMS section reference). Use [`Self::rating_key`]
/// for the watchlist add/remove and scrobble paths.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DiscoverItem {
    /// Full GUID (`plex://movie/<hex>` / `plex://show/<hex>`).
    pub guid: String,
    /// Hex rating key extracted from [`Self::guid`].
    pub rating_key: String,
    /// Wire `type` (`movie` or `show`).
    pub kind: String,
    /// Title.
    pub title: String,
    /// Release year, when known.
    pub year: Option<u16>,
    /// Synopsis.
    pub summary: Option<String>,
    /// Original release date (`YYYY-MM-DD`).
    pub originally_available_at: Option<String>,
    /// Critic rating (0..=10).
    pub rating: Option<f32>,
    /// Audience rating (0..=10).
    pub audience_rating: Option<f32>,
    /// Content rating (e.g. `PG-13`).
    pub content_rating: Option<String>,
    /// Search-result score (0.0..=1.0). Higher is better.
    pub score: Option<f32>,
    /// Full raw payload — use for fields beyond the projection.
    pub raw: serde_json::Value,
}

// -----------------------------------------------------------------------------
// MyPlexClient::discover_search.
// -----------------------------------------------------------------------------

impl MyPlexClient {
    /// Search the Plex Discover catalogue.
    ///
    /// Default options return up to 30 catalogue matches across
    /// movies and shows.
    ///
    /// # Errors
    /// Any transport / parse [`Error`] variant.
    pub async fn discover_search(
        &self,
        query: &str,
        opts: &DiscoverOptions,
    ) -> Result<Vec<DiscoverItem>> {
        let search_types = opts.kind.map_or("movies,tv", DiscoverKind::as_wire);
        let mut url = url::Url::parse(&format!("{}/library/search", self.discover_base()))?;
        {
            let mut qp = url.query_pairs_mut();
            qp.append_pair("query", query);
            qp.append_pair("limit", &opts.limit.to_string());
            qp.append_pair("searchTypes", search_types);
            qp.append_pair("searchProviders", &opts.providers);
            qp.append_pair("includeMetadata", "1");
        }
        let bytes = self.http().get_bytes(url.as_str()).await?;
        let body = std::str::from_utf8(&bytes)
            .map_err(|e| Error::Config(format!("discover body not utf-8: {e}")))?;
        let env: SearchEnvelope = serde_json::from_str(body)?;
        Ok(flatten(&env))
    }
}

// -----------------------------------------------------------------------------
// Parser.
// -----------------------------------------------------------------------------

fn flatten(env: &SearchEnvelope) -> Vec<DiscoverItem> {
    let mut out = Vec::new();
    for bucket in &env.container.search_results {
        for hit in &bucket.search_result {
            let raw = hit.metadata.clone();
            let guid = raw
                .get("guid")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            let rating_key = guid.rsplit('/').next().unwrap_or("").to_owned();
            let kind = raw
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            let title = raw
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            let year = raw
                .get("year")
                .and_then(serde_json::Value::as_u64)
                .and_then(|n| u16::try_from(n).ok());
            let summary = raw
                .get("summary")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            let originally_available_at = raw
                .get("originallyAvailableAt")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            let rating = raw.get("rating").and_then(serde_json::Value::as_f64).map(
                #[allow(clippy::cast_possible_truncation)]
                |f| f as f32,
            );
            let audience_rating = raw
                .get("audienceRating")
                .and_then(serde_json::Value::as_f64)
                .map(
                    #[allow(clippy::cast_possible_truncation)]
                    |f| f as f32,
                );
            let content_rating = raw
                .get("contentRating")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            out.push(DiscoverItem {
                guid,
                rating_key,
                kind,
                title,
                year,
                summary,
                originally_available_at,
                rating,
                audience_rating,
                content_rating,
                score: hit.score,
                raw,
            });
        }
    }
    out
}

#[derive(Debug, Deserialize)]
struct SearchEnvelope {
    #[serde(rename = "MediaContainer")]
    container: SearchContainer,
}

#[derive(Debug, Deserialize, Default)]
struct SearchContainer {
    #[serde(rename = "SearchResults", default)]
    search_results: Vec<SearchBucket>,
}

#[derive(Debug, Deserialize)]
struct SearchBucket {
    #[serde(rename = "SearchResult", default)]
    search_result: Vec<SearchHit>,
}

#[derive(Debug, Deserialize)]
struct SearchHit {
    #[serde(default)]
    score: Option<f32>,
    #[serde(rename = "Metadata", default)]
    metadata: serde_json::Value,
}

// -----------------------------------------------------------------------------
// Tests.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_kind_wire_spellings() {
        assert_eq!(DiscoverKind::Movie.as_wire(), "movies");
        assert_eq!(DiscoverKind::Show.as_wire(), "tv");
    }

    #[test]
    fn options_defaults() {
        let o = DiscoverOptions::default();
        assert_eq!(o.limit, 30);
        assert!(o.kind.is_none());
        assert_eq!(o.providers, "discover");
    }

    #[test]
    fn options_builder_chain() {
        let o = DiscoverOptions::default()
            .with_limit(50)
            .with_kind(DiscoverKind::Show)
            .with_providers("discover,PLEXAVOD");
        assert_eq!(o.limit, 50);
        assert_eq!(o.kind, Some(DiscoverKind::Show));
        assert_eq!(o.providers, "discover,PLEXAVOD");
    }

    #[test]
    fn flatten_picks_metadata_across_all_buckets() {
        let env: SearchEnvelope = serde_json::from_value(serde_json::json!({
            "MediaContainer": {
                "size": 1,
                "SearchResults": [
                    {
                        "id": "external",
                        "title": "Discover",
                        "SearchResult": [
                            {
                                "score": 0.95,
                                "Metadata": {
                                    "guid": "plex://movie/aaaa",
                                    "type": "movie",
                                    "title": "Arrival",
                                    "year": 2016,
                                    "rating": 8.0,
                                    "audienceRating": 7.5,
                                    "contentRating": "PG-13",
                                    "summary": "Aliens arrive"
                                }
                            },
                            {
                                "score": 0.55,
                                "Metadata": {
                                    "guid": "plex://show/bbbb",
                                    "type": "show",
                                    "title": "Severance",
                                    "year": 2022
                                }
                            }
                        ]
                    }
                ]
            }
        }))
        .unwrap();
        let items = flatten(&env);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].rating_key, "aaaa");
        assert_eq!(items[0].title, "Arrival");
        assert_eq!(items[0].year, Some(2016));
        assert_eq!(items[0].rating, Some(8.0));
        assert_eq!(items[0].audience_rating, Some(7.5));
        assert_eq!(items[0].content_rating.as_deref(), Some("PG-13"));
        assert_eq!(items[0].score, Some(0.95));
        assert_eq!(items[1].kind, "show");
        assert_eq!(items[1].rating_key, "bbbb");
    }

    #[test]
    fn flatten_handles_empty_search_results() {
        let env: SearchEnvelope = serde_json::from_value(serde_json::json!({
            "MediaContainer": {"SearchResults": []}
        }))
        .unwrap();
        assert!(flatten(&env).is_empty());
    }

    #[test]
    fn flatten_handles_missing_search_results() {
        let env: SearchEnvelope =
            serde_json::from_value(serde_json::json!({"MediaContainer": {}})).unwrap();
        assert!(flatten(&env).is_empty());
    }

    #[test]
    fn flatten_preserves_raw_for_unprojected_fields() {
        let env: SearchEnvelope = serde_json::from_value(serde_json::json!({
            "MediaContainer": {
                "SearchResults": [{
                    "SearchResult": [{
                        "Metadata": {
                            "guid": "plex://movie/x",
                            "type": "movie",
                            "title": "X",
                            "customExtra": "kept"
                        }
                    }]
                }]
            }
        }))
        .unwrap();
        let items = flatten(&env);
        assert_eq!(
            items[0].raw.get("customExtra").and_then(|v| v.as_str()),
            Some("kept"),
        );
    }

    #[test]
    fn flatten_combines_multiple_buckets() {
        let env: SearchEnvelope = serde_json::from_value(serde_json::json!({
            "MediaContainer": {
                "SearchResults": [
                    {"id": "external", "SearchResult": [
                        {"Metadata": {"guid": "plex://movie/a", "type": "movie", "title": "A"}}
                    ]},
                    {"id": "library", "SearchResult": [
                        {"Metadata": {"guid": "plex://movie/b", "type": "movie", "title": "B"}}
                    ]}
                ]
            }
        }))
        .unwrap();
        let items = flatten(&env);
        assert_eq!(items.len(), 2);
    }
}
