//! plex.tv Watchlist.
//!
//! The user-level "to watch" list lives on `discover.provider.plex.tv`
//! (the cloud catalogue) — distinct from any single PMS. Items refer
//! to Plex's global catalogue entries, identified by a hex
//! `ratingKey` extracted from a `plex://movie/...` GUID.
//!
//! ## Wire endpoints
//!
//! | Method | Path                                                | Purpose            |
//! | ------ | --------------------------------------------------- | ------------------ |
//! | GET    | `/library/sections/watchlist/<filter>`              | List              |
//! | PUT    | `/actions/addToWatchlist?ratingKey=<rk>`            | Add one item       |
//! | PUT    | `/actions/removeFromWatchlist?ratingKey=<rk>`       | Remove one item    |
//!
//! All against the Discover base
//! (`https://discover.provider.plex.tv` by default). Override via
//! [`MyPlexClient::with_discover_base`] for tests.

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::myplex::MyPlexClient;
use crate::util::search_type::SearchType;

// -----------------------------------------------------------------------------
// WatchlistFilter / WatchlistKind / WatchlistOptions.
// -----------------------------------------------------------------------------

/// Server-side filter on the watchlist listing.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum WatchlistFilter {
    /// Every watchlist entry.
    #[default]
    All,
    /// Only entries that Plex has confirmed are streamable now.
    Available,
    /// Only entries that have already released theatrically /
    /// on streaming.
    Released,
}

impl WatchlistFilter {
    /// URL path segment used by Plex.
    #[must_use]
    pub const fn as_path(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Available => "available",
            Self::Released => "released",
        }
    }
}

/// Type filter on the watchlist listing.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WatchlistKind {
    /// `type=1` — movies only.
    Movie,
    /// `type=2` — shows only.
    Show,
}

impl WatchlistKind {
    /// Numeric wire value of `?type=...`.
    #[must_use]
    pub const fn as_wire(self) -> u32 {
        match self {
            Self::Movie => SearchType::Movie.as_u32(),
            Self::Show => SearchType::Show.as_u32(),
        }
    }
}

/// Options for [`MyPlexClient::watchlist_with`].
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct WatchlistOptions {
    /// Availability filter. Default [`WatchlistFilter::All`].
    pub filter: WatchlistFilter,
    /// Type filter. Default `None` returns both movies and shows.
    pub kind: Option<WatchlistKind>,
    /// Sort string in `field:dir` shape, e.g.
    /// `"watchlistedAt:desc"`, `"titleSort:asc"`,
    /// `"originallyAvailableAt:desc"`, `"rating:desc"`.
    pub sort: Option<String>,
    /// Caller-side cap on total results.
    pub max_results: Option<usize>,
}

impl WatchlistOptions {
    /// Set the availability filter (builder style).
    #[must_use]
    pub const fn with_filter(mut self, filter: WatchlistFilter) -> Self {
        self.filter = filter;
        self
    }

    /// Restrict to movies or shows.
    #[must_use]
    pub const fn with_kind(mut self, kind: WatchlistKind) -> Self {
        self.kind = Some(kind);
        self
    }

    /// Set the sort string verbatim.
    #[must_use]
    pub fn with_sort(mut self, sort: impl Into<String>) -> Self {
        self.sort = Some(sort.into());
        self
    }

    /// Cap result count.
    #[must_use]
    pub const fn with_max_results(mut self, n: usize) -> Self {
        self.max_results = Some(n);
        self
    }
}

// -----------------------------------------------------------------------------
// WatchlistItem.
// -----------------------------------------------------------------------------

/// One entry in a Plex Discover watchlist.
///
/// Distinct from [`crate::LibraryItem`] — watchlist items refer to
/// the global Plex cloud catalogue, not to a specific PMS library,
/// so the section-attached trait machinery doesn't apply. To act on
/// the corresponding library item, search a PMS for the item's
/// [`Self::guid`] / [`Self::rating_key`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct WatchlistItem {
    /// Full GUID (`plex://movie/<hex>` / `plex://show/<hex>`).
    pub guid: String,
    /// Hex rating key extracted from [`Self::guid`]. Used by
    /// [`MyPlexClient::add_to_watchlist`] /
    /// [`remove_from_watchlist`](MyPlexClient::remove_from_watchlist).
    pub rating_key: String,
    /// Wire `type` (`movie` or `show`).
    pub kind: String,
    /// Title.
    pub title: String,
    /// Release year, when known.
    pub year: Option<u16>,
    /// Synopsis.
    pub summary: Option<String>,
    /// Poster path on Plex's CDN.
    pub thumb: Option<String>,
    /// Backdrop path.
    pub art: Option<String>,
    /// Original release date string (`YYYY-MM-DD`).
    pub originally_available_at: Option<String>,
    /// Critic rating (0..=10).
    pub rating: Option<f32>,
    /// Audience rating (0..=10).
    pub audience_rating: Option<f32>,
    /// When the user added this item to their watchlist (epoch
    /// seconds).
    pub watchlisted_at: Option<i64>,
    /// Full raw JSON payload — use for fields beyond the
    /// projection above (e.g. genre list, cast).
    pub raw: serde_json::Value,
}

// -----------------------------------------------------------------------------
// MyPlexClient impl.
// -----------------------------------------------------------------------------

impl MyPlexClient {
    /// Fetch the user's watchlist with default options
    /// ([`WatchlistFilter::All`], no type filter, no sort).
    ///
    /// # Errors
    /// Any transport / parse [`Error`] variant.
    pub async fn watchlist(&self) -> Result<Vec<WatchlistItem>> {
        self.watchlist_with(&WatchlistOptions::default()).await
    }

    /// Fetch the user's watchlist with caller-tuned options.
    ///
    /// # Errors
    /// Any transport / parse [`Error`] variant.
    pub async fn watchlist_with(&self, opts: &WatchlistOptions) -> Result<Vec<WatchlistItem>> {
        let url = build_watchlist_url(self.discover_base(), opts)?;
        let bytes = self.http().get_bytes(url.as_str()).await?;
        let body = std::str::from_utf8(&bytes)
            .map_err(|e| Error::Config(format!("watchlist body not utf-8: {e}")))?;
        let env: WatchlistEnvelope = serde_json::from_str(body)?;
        let cap = opts.max_results.unwrap_or(usize::MAX);
        let mut out: Vec<WatchlistItem> = env
            .container
            .metadata
            .into_iter()
            .map(WatchlistItemDto::into_domain)
            .collect();
        out.truncate(cap);
        Ok(out)
    }

    /// Add a single item to the watchlist by its hex rating key.
    ///
    /// `rating_key` is the trailing hex segment of the item's GUID
    /// (e.g. `5d776b59ad5437001f796d8b` for
    /// `plex://movie/5d776b59ad5437001f796d8b`). For an item already
    /// in hand, use [`WatchlistItem::rating_key`] directly.
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn add_to_watchlist(&self, rating_key: &str) -> Result<()> {
        let url = format!(
            "{}/actions/addToWatchlist?ratingKey={}",
            self.discover_base(),
            urlencode_segment(rating_key),
        );
        self.http().put_no_body(&url).await
    }

    /// Remove a single item from the watchlist by its hex rating
    /// key.
    ///
    /// # Errors
    /// Any transport [`Error`] variant.
    pub async fn remove_from_watchlist(&self, rating_key: &str) -> Result<()> {
        let url = format!(
            "{}/actions/removeFromWatchlist?ratingKey={}",
            self.discover_base(),
            urlencode_segment(rating_key),
        );
        self.http().put_no_body(&url).await
    }
}

// -----------------------------------------------------------------------------
// URL construction.
// -----------------------------------------------------------------------------

fn build_watchlist_url(base: &str, opts: &WatchlistOptions) -> Result<url::Url> {
    let raw = format!(
        "{}/library/sections/watchlist/{}",
        base,
        opts.filter.as_path()
    );
    let mut url = url::Url::parse(&raw)?;
    {
        let mut qp = url.query_pairs_mut();
        // Python-plexapi always includes these; mirror that.
        qp.append_pair("includeCollections", "1");
        qp.append_pair("includeExternalMedia", "1");
        if let Some(kind) = opts.kind {
            qp.append_pair("type", &kind.as_wire().to_string());
        }
        if let Some(sort) = &opts.sort {
            qp.append_pair("sort", sort);
        }
    }
    Ok(url)
}

const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// Hex-rating-key URL-safe encoder. Plex's keys are already
/// `[0-9a-f]+` so this is just a defensive escape — anything funky
/// gets percent-encoded.
fn urlencode_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        if matches!(byte, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0xF) as usize] as char);
        }
    }
    out
}

/// Extract the trailing path segment from a `plex://...` GUID.
fn rating_key_from_guid(guid: &str) -> String {
    guid.rsplit('/').next().unwrap_or("").to_owned()
}

// -----------------------------------------------------------------------------
// DTOs.
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct WatchlistEnvelope {
    #[serde(rename = "MediaContainer")]
    container: WatchlistContainer,
}

#[derive(Debug, Deserialize, Default)]
struct WatchlistContainer {
    #[serde(rename = "Metadata", default)]
    metadata: Vec<WatchlistItemDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WatchlistItemDto {
    #[serde(default)]
    guid: String,
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    year: Option<u16>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    thumb: Option<String>,
    #[serde(default)]
    art: Option<String>,
    #[serde(default)]
    originally_available_at: Option<String>,
    #[serde(default)]
    rating: Option<f32>,
    #[serde(default)]
    audience_rating: Option<f32>,
    #[serde(default)]
    watchlisted_at: Option<i64>,
    /// Full raw payload preserved for fields beyond the projection.
    #[serde(flatten)]
    raw: serde_json::Value,
}

impl WatchlistItemDto {
    fn into_domain(self) -> WatchlistItem {
        let rating_key = rating_key_from_guid(&self.guid);
        WatchlistItem {
            rating_key,
            guid: self.guid,
            kind: self.kind,
            title: self.title,
            year: self.year,
            summary: self.summary,
            thumb: self.thumb,
            art: self.art,
            originally_available_at: self.originally_available_at,
            rating: self.rating,
            audience_rating: self.audience_rating,
            watchlisted_at: self.watchlisted_at,
            raw: self.raw,
        }
    }
}

// -----------------------------------------------------------------------------
// Tests.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rating_key_from_guid_strips_prefix() {
        assert_eq!(
            rating_key_from_guid("plex://movie/5d776b59ad5437001f796d8b"),
            "5d776b59ad5437001f796d8b",
        );
        assert_eq!(rating_key_from_guid("plex://show/abc"), "abc");
        assert_eq!(rating_key_from_guid("nopath"), "nopath");
    }

    #[test]
    fn build_watchlist_url_default_filter_is_all() {
        let url = build_watchlist_url("https://d.example", &WatchlistOptions::default()).unwrap();
        assert_eq!(url.path(), "/library/sections/watchlist/all");
        let q = url.query().unwrap();
        assert!(q.contains("includeCollections=1"));
        assert!(q.contains("includeExternalMedia=1"));
    }

    #[test]
    fn build_watchlist_url_filter_released() {
        let url = build_watchlist_url(
            "https://d.example",
            &WatchlistOptions::default().with_filter(WatchlistFilter::Released),
        )
        .unwrap();
        assert_eq!(url.path(), "/library/sections/watchlist/released");
    }

    #[test]
    fn build_watchlist_url_kind_filter_is_numeric() {
        let url = build_watchlist_url(
            "https://d.example",
            &WatchlistOptions::default().with_kind(WatchlistKind::Movie),
        )
        .unwrap();
        let q = url.query().unwrap();
        assert!(q.contains("type=1"), "{q}");
        let url = build_watchlist_url(
            "https://d.example",
            &WatchlistOptions::default().with_kind(WatchlistKind::Show),
        )
        .unwrap();
        let q = url.query().unwrap();
        assert!(q.contains("type=2"), "{q}");
    }

    #[test]
    fn build_watchlist_url_sort_appended() {
        let url = build_watchlist_url(
            "https://d.example",
            &WatchlistOptions::default().with_sort("titleSort:desc"),
        )
        .unwrap();
        let q = url.query().unwrap();
        assert!(q.contains("sort=titleSort%3Adesc"), "{q}");
    }

    #[test]
    fn parse_watchlist_envelope_extracts_items_with_rating_keys() {
        let body = serde_json::json!({
            "MediaContainer": {
                "size": 2,
                "Metadata": [
                    {
                        "guid": "plex://movie/aaaa",
                        "type": "movie",
                        "title": "Arrival",
                        "year": 2016,
                        "rating": 8.5,
                        "watchlistedAt": 1_700_000_000
                    },
                    {
                        "guid": "plex://show/bbbb",
                        "type": "show",
                        "title": "Severance",
                        "year": 2022
                    }
                ]
            }
        });
        let env: WatchlistEnvelope = serde_json::from_value(body).unwrap();
        let items: Vec<WatchlistItem> = env
            .container
            .metadata
            .into_iter()
            .map(WatchlistItemDto::into_domain)
            .collect();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].rating_key, "aaaa");
        assert_eq!(items[0].title, "Arrival");
        assert_eq!(items[0].rating, Some(8.5));
        assert_eq!(items[0].watchlisted_at, Some(1_700_000_000));
        assert_eq!(items[1].rating_key, "bbbb");
        assert_eq!(items[1].kind, "show");
    }

    #[test]
    fn parse_watchlist_preserves_unknown_fields_in_raw() {
        let body = serde_json::json!({
            "MediaContainer": {
                "Metadata": [{
                    "guid": "plex://movie/x",
                    "type": "movie",
                    "title": "X",
                    "customExtra": "kept"
                }]
            }
        });
        let env: WatchlistEnvelope = serde_json::from_value(body).unwrap();
        let item = env
            .container
            .metadata
            .into_iter()
            .next()
            .unwrap()
            .into_domain();
        assert_eq!(
            item.raw.get("customExtra").and_then(|v| v.as_str()),
            Some("kept"),
        );
    }

    #[test]
    fn urlencode_segment_escapes_special_chars() {
        assert_eq!(urlencode_segment("plain"), "plain");
        assert_eq!(urlencode_segment("a/b"), "a%2Fb");
        assert_eq!(urlencode_segment("a b"), "a%20b");
    }

    #[test]
    fn options_builder_chain() {
        let opts = WatchlistOptions::default()
            .with_filter(WatchlistFilter::Released)
            .with_kind(WatchlistKind::Show)
            .with_sort("rating:desc")
            .with_max_results(10);
        assert_eq!(opts.filter, WatchlistFilter::Released);
        assert_eq!(opts.kind, Some(WatchlistKind::Show));
        assert_eq!(opts.sort.as_deref(), Some("rating:desc"));
        assert_eq!(opts.max_results, Some(10));
    }
}
