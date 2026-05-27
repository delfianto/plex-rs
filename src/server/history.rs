//! Playback history.
//!
//! [`PlexServer::history`] returns a [`HistoryQuery`] builder that
//! applies the filters python-plexapi supports and then collects (or
//! streams) the resulting `HistoryEntry` list.
//!
//! Wire endpoint: `GET /status/sessions/history/all`. Filters are
//! query parameters; pagination is the standard
//! `X-Plex-Container-Start` / `-Size` header pair (see
//! [`crate::PageRange`]).
//!
//! ## Filters
//!
//! | Builder method            | Wire parameter            | Notes |
//! | ------------------------- | ------------------------- | ----- |
//! | [`HistoryQuery::account`] | `accountID=<u64>`         | filter by user |
//! | [`HistoryQuery::library_section`] | `librarySectionID=<u32>` | filter by section |
//! | [`HistoryQuery::rating_key`] | `metadataItemID=<u64>` | filter to one item |
//! | [`HistoryQuery::mindate`]  | `viewedAt>=<epoch_secs>`  | inclusive lower bound |
//! | [`HistoryQuery::max_results`] | (none — client cap)    | stop early |
//! | [`HistoryQuery::page_size`] | `X-Plex-Container-Size`  | tune page chunk |
//!
//! Default sort is `viewedAt:desc` (most-recent first), matching
//! python-plexapi.
//!
//! ## Streaming vs collect
//!
//! Both [`HistoryQuery::collect`] and [`HistoryQuery::stream`] are
//! provided. `collect` is convenient when results fit in memory;
//! `stream` is the right choice for large libraries where the caller
//! wants to early-terminate or apply additional filters lazily.

use std::pin::Pin;
use std::task::{Context, Poll};

use chrono::{DateTime, Utc};
use futures_util::Stream;
use serde::Deserialize;
use url::Url;

use crate::client::HttpClient;
use crate::error::{Error, Result};
use crate::library::LibrarySectionRef;
use crate::media::LibraryItem;
use crate::media::video::MetadataDto;
use crate::pagination::{HEADER_CONTAINER_SIZE, HEADER_CONTAINER_START, PageRange};
use crate::server::PlexServer;
use crate::util::ids::RatingKey;
use crate::xml::MediaContainer;

// -----------------------------------------------------------------------------
// HistoryEntry — one row of /status/sessions/history/all.
// -----------------------------------------------------------------------------

/// A single playback-history entry.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct HistoryEntry {
    /// History row identifier (used by [`Self::delete`]). Plex emits
    /// the path here (e.g. `/status/sessions/history/123`); we keep
    /// it as-is so it can be passed back to the server verbatim.
    pub history_key: Option<String>,
    /// User account that performed the playback.
    pub account_id: u64,
    /// Device that performed the playback.
    pub device_id: u64,
    /// When the playback occurred. `None` only when Plex omits the
    /// field (rare; covered by the `default` serde annotation).
    pub viewed_at: Option<DateTime<Utc>>,
    /// The played item — full metadata, exactly as if it had been
    /// fetched from the library. Lets callers reuse all the existing
    /// trait machinery (Reload, Playable, etc.).
    pub item: LibraryItem,
}

impl HistoryEntry {
    /// Delete this history row from the server.
    ///
    /// Plex deletes a row via `DELETE` on its `historyKey`. When the
    /// row has no key (older PMS versions, edge cases) this is a
    /// no-op returning [`Error::Config`].
    ///
    /// # Errors
    /// - [`Error::Config`] when this entry has no `history_key`.
    /// - Any transport [`Error`] variant.
    pub async fn delete(&self, http: &HttpClient, base_url: &Url) -> Result<()> {
        let key = self
            .history_key
            .as_deref()
            .ok_or_else(|| Error::Config("history entry missing historyKey".to_owned()))?;
        let url = base_url.join(key)?;
        http.delete(url.as_str()).await
    }
}

// -----------------------------------------------------------------------------
// HistoryQuery — builder + executor.
// -----------------------------------------------------------------------------

/// Builder for a single history listing call.
///
/// Construct via [`PlexServer::history`]. All filter setters return
/// `self`. Terminate with [`Self::collect`] (eager) or
/// [`Self::stream`] (lazy).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct HistoryQuery {
    http: HttpClient,
    base_url: Url,
    account_id: Option<u64>,
    library_section_id: Option<u32>,
    rating_key: Option<u64>,
    mindate_epoch_secs: Option<i64>,
    max_results: Option<usize>,
    page_size: u32,
}

impl HistoryQuery {
    pub(crate) const fn new(http: HttpClient, base_url: Url) -> Self {
        Self {
            http,
            base_url,
            account_id: None,
            library_section_id: None,
            rating_key: None,
            mindate_epoch_secs: None,
            max_results: None,
            page_size: 50,
        }
    }

    /// Filter to a specific user account.
    #[must_use]
    pub const fn account(mut self, account_id: u64) -> Self {
        self.account_id = Some(account_id);
        self
    }

    /// Filter to a specific library section.
    #[must_use]
    pub const fn library_section(mut self, section_id: u32) -> Self {
        self.library_section_id = Some(section_id);
        self
    }

    /// Filter to a specific media item.
    #[must_use]
    pub const fn rating_key(mut self, rk: RatingKey) -> Self {
        self.rating_key = Some(rk.0);
        self
    }

    /// Inclusive lower bound on `viewedAt` — only entries played at
    /// or after this instant are returned.
    #[must_use]
    pub const fn mindate(mut self, t: DateTime<Utc>) -> Self {
        self.mindate_epoch_secs = Some(t.timestamp());
        self
    }

    /// Set the inclusive `viewedAt` lower bound from a raw Unix
    /// epoch seconds value.
    #[must_use]
    pub const fn mindate_epoch_secs(mut self, secs: i64) -> Self {
        self.mindate_epoch_secs = Some(secs);
        self
    }

    /// Cap the total number of entries returned (across all pages).
    #[must_use]
    pub const fn max_results(mut self, n: usize) -> Self {
        self.max_results = Some(n);
        self
    }

    /// Tune the page chunk size (defaults to 50). Larger pages mean
    /// fewer HTTP round-trips at the cost of more memory per chunk.
    ///
    /// `n == 0` is silently clamped to 1.
    #[must_use]
    pub const fn page_size(mut self, n: u32) -> Self {
        self.page_size = if n == 0 { 1 } else { n };
        self
    }

    /// Build the request URL for a given page (sans pagination
    /// headers, which are passed separately as a side-channel).
    ///
    /// Visible to the test suite via `pub(crate)`.
    pub(crate) fn build_url(&self) -> Result<Url> {
        let mut url = self.base_url.join("/status/sessions/history/all")?;
        {
            let mut qp = url.query_pairs_mut();
            // Default sort: most-recent first.
            qp.append_pair("sort", "viewedAt:desc");
            if let Some(account) = self.account_id {
                qp.append_pair("accountID", &account.to_string());
            }
            if let Some(section) = self.library_section_id {
                qp.append_pair("librarySectionID", &section.to_string());
            }
            if let Some(rk) = self.rating_key {
                qp.append_pair("metadataItemID", &rk.to_string());
            }
            if let Some(epoch) = self.mindate_epoch_secs {
                qp.append_pair("viewedAt>=", &epoch.to_string());
            }
        }
        Ok(url)
    }

    /// Eagerly collect every matching entry into a [`Vec`].
    ///
    /// # Errors
    /// Any transport / parse [`Error`].
    pub async fn collect(self) -> Result<Vec<HistoryEntry>> {
        let url = self.build_url()?;
        let url_str = url.as_str().to_owned();
        let mut out: Vec<HistoryEntry> = Vec::new();
        let mut page = PageRange::first(self.page_size);
        let cap = self.max_results.unwrap_or(usize::MAX);
        loop {
            let start_s = page.start.to_string();
            let size_s = page.size.to_string();
            let headers: [(&str, &str); 2] = [
                (HEADER_CONTAINER_START, start_s.as_str()),
                (HEADER_CONTAINER_SIZE, size_s.as_str()),
            ];
            let body = self.http.get_bytes_with_headers(&url_str, &headers).await?;
            let body_str = std::str::from_utf8(&body)
                .map_err(|e| Error::Config(format!("history body not utf-8: {e}")))?;
            let mc: MediaContainer<HistoryEntryDto> =
                MediaContainer::from_json(body_str, "Metadata")?;
            let meta_for_advance = mc.meta.clone();
            for dto in mc.items {
                let entry = dto.into_domain(self.http.clone(), self.base_url.clone())?;
                out.push(entry);
                if out.len() >= cap {
                    return Ok(out);
                }
            }
            match page.advance_with(&meta_for_advance) {
                Some(next) => page = next,
                None => return Ok(out),
            }
        }
    }

    /// Stream entries as they arrive, page by page.
    ///
    /// The returned stream yields one `Result<HistoryEntry>` per
    /// entry; pages are fetched lazily as the stream is polled.
    /// Honors [`Self::max_results`] across pages.
    #[must_use]
    pub fn stream(self) -> HistoryStream {
        HistoryStream::new(self)
    }
}

// -----------------------------------------------------------------------------
// HistoryStream — page-fetching futures::Stream impl.
// -----------------------------------------------------------------------------

type PageFut = Pin<Box<dyn Future<Output = Result<HistoryPage>> + Send>>;

/// Lazy stream of history entries. Construct via
/// [`HistoryQuery::stream`].
pub struct HistoryStream {
    query: HistoryQuery,
    url: String,
    page: Option<PageRange>,
    buffer: std::collections::VecDeque<HistoryEntry>,
    in_flight: Option<PageFut>,
    yielded: usize,
    done: bool,
}

impl std::fmt::Debug for HistoryStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HistoryStream")
            .field("url", &self.url)
            .field("query", &self.query)
            .field("page", &self.page)
            .field("yielded", &self.yielded)
            .field("done", &self.done)
            .field("buffered", &self.buffer.len())
            .field("in_flight", &self.in_flight.is_some())
            .finish()
    }
}

impl HistoryStream {
    fn new(query: HistoryQuery) -> Self {
        let url = query.build_url().map(|u| u.to_string()).unwrap_or_default();
        let page = Some(PageRange::first(query.page_size));
        Self {
            query,
            url,
            page,
            buffer: std::collections::VecDeque::new(),
            in_flight: None,
            yielded: 0,
            done: false,
        }
    }

    fn cap(&self) -> usize {
        self.query.max_results.unwrap_or(usize::MAX)
    }

    fn spawn_fetch(&mut self) {
        let Some(page) = self.page else {
            self.done = true;
            return;
        };
        let url = self.url.clone();
        let http = self.query.http.clone();
        let base_url = self.query.base_url.clone();
        let fut = Box::pin(async move {
            let start_s = page.start.to_string();
            let size_s = page.size.to_string();
            let headers: [(&str, &str); 2] = [
                (HEADER_CONTAINER_START, start_s.as_str()),
                (HEADER_CONTAINER_SIZE, size_s.as_str()),
            ];
            let body = http.get_bytes_with_headers(&url, &headers).await?;
            let body_str = std::str::from_utf8(&body)
                .map_err(|e| Error::Config(format!("history body not utf-8: {e}")))?;
            let mc: MediaContainer<HistoryEntryDto> =
                MediaContainer::from_json(body_str, "Metadata")?;
            let mut entries = Vec::with_capacity(mc.items.len());
            for dto in mc.items {
                entries.push(dto.into_domain(http.clone(), base_url.clone())?);
            }
            Ok(HistoryPage {
                meta: mc.meta,
                entries,
            })
        });
        self.in_flight = Some(fut);
    }
}

struct HistoryPage {
    meta: crate::xml::MediaContainerMeta,
    entries: Vec<HistoryEntry>,
}

impl Stream for HistoryStream {
    type Item = Result<HistoryEntry>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            // Cap reached?
            if self.yielded >= self.cap() {
                return Poll::Ready(None);
            }
            // Drain buffer first.
            if let Some(entry) = self.buffer.pop_front() {
                self.yielded += 1;
                return Poll::Ready(Some(Ok(entry)));
            }
            // Stream finished?
            if self.done && self.in_flight.is_none() {
                return Poll::Ready(None);
            }
            // Start a fetch if we don't have one running.
            if self.in_flight.is_none() {
                self.spawn_fetch();
                if self.done {
                    continue;
                }
            }
            // Poll the in-flight fetch.
            let Some(fut) = self.in_flight.as_mut() else {
                continue;
            };
            match fut.as_mut().poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(page)) => {
                    self.in_flight = None;
                    self.buffer.extend(page.entries);
                    // Advance pagination.
                    self.page = self.page.and_then(|p| p.advance_with(&page.meta));
                    if self.page.is_none() {
                        self.done = true;
                    }
                }
                Poll::Ready(Err(e)) => {
                    self.in_flight = None;
                    self.done = true;
                    return Poll::Ready(Some(Err(e)));
                }
            }
        }
    }
}

// -----------------------------------------------------------------------------
// PlexServer integration.
// -----------------------------------------------------------------------------

impl PlexServer {
    /// Start a [`HistoryQuery`] for playback history on this server.
    ///
    /// Default sort is `viewedAt:desc` (most-recent first). Apply
    /// filters via the builder, then call
    /// [`HistoryQuery::collect`] or [`HistoryQuery::stream`].
    #[must_use]
    pub fn history(&self) -> HistoryQuery {
        HistoryQuery::new(self.http().clone(), self.base_url().clone())
    }
}

// -----------------------------------------------------------------------------
// DTO.
// -----------------------------------------------------------------------------

/// Wire-format DTO for a single `/status/sessions/history/all` entry.
///
/// History rows carry the standard metadata attributes plus three
/// extra session-derived fields (`accountID`, `deviceID`,
/// `historyKey`). We deserialise them alongside [`MetadataDto`] via
/// serde's `flatten`.
#[derive(Debug, Deserialize)]
struct HistoryEntryDto {
    #[serde(rename = "accountID", default)]
    account_id: u64,
    #[serde(rename = "deviceID", default)]
    device_id: u64,
    #[serde(rename = "historyKey", default)]
    history_key: Option<String>,
    /// epoch seconds — overrides `MetadataDto`'s `last_viewed_at`.
    #[serde(rename = "viewedAt", default)]
    viewed_at: Option<i64>,
    #[serde(flatten)]
    metadata: MetadataDto,
}

impl HistoryEntryDto {
    fn into_domain(self, http: HttpClient, base_url: Url) -> Result<HistoryEntry> {
        let section_id = self.metadata.library_section_id_for_playlist().unwrap_or(0);
        let section_ref = LibrarySectionRef {
            id: section_id,
            http,
            base_url,
        };
        let item = self.metadata.into_library_item(section_ref)?;
        let viewed_at = self
            .viewed_at
            .and_then(|s| DateTime::<Utc>::from_timestamp(s, 0));
        Ok(HistoryEntry {
            history_key: self.history_key,
            account_id: self.account_id,
            device_id: self.device_id,
            viewed_at,
            item,
        })
    }
}

// -----------------------------------------------------------------------------
// Unit tests.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::HttpClient;
    use crate::config::ClientConfig;
    use crate::util::ids::ClientIdentifier;

    fn dummy_http() -> HttpClient {
        let cfg = ClientConfig::builder(ClientIdentifier::new("t").unwrap())
            .build()
            .unwrap();
        HttpClient::new(cfg).unwrap()
    }

    fn query() -> HistoryQuery {
        HistoryQuery::new(dummy_http(), Url::parse("http://pms.local:32400").unwrap())
    }

    #[test]
    fn url_includes_default_sort() {
        let url = query().build_url().unwrap();
        assert_eq!(url.path(), "/status/sessions/history/all");
        assert!(url.query().unwrap().contains("sort=viewedAt%3Adesc"));
    }

    #[test]
    fn url_threads_account_filter() {
        let url = query().account(42).build_url().unwrap();
        assert!(url.query().unwrap().contains("accountID=42"));
    }

    #[test]
    fn url_threads_section_filter() {
        let url = query().library_section(7).build_url().unwrap();
        assert!(url.query().unwrap().contains("librarySectionID=7"));
    }

    #[test]
    fn url_threads_rating_key_as_metadata_item_id() {
        let url = query().rating_key(RatingKey(123)).build_url().unwrap();
        assert!(url.query().unwrap().contains("metadataItemID=123"));
    }

    #[test]
    fn url_threads_mindate_as_epoch_seconds() {
        let t = DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap();
        let url = query().mindate(t).build_url().unwrap();
        // `>=` is percent-encoded in query position.
        let q = url.query().unwrap();
        assert!(q.contains("viewedAt%3E%3D=1700000000"), "got query: {q}");
    }

    #[test]
    fn page_size_zero_clamped_to_one() {
        let q = query().page_size(0);
        assert_eq!(q.page_size, 1);
    }

    #[test]
    fn max_results_threads_through_to_cap() {
        let q = query().max_results(7);
        assert_eq!(q.max_results, Some(7));
    }

    #[test]
    fn multiple_filters_all_present() {
        let url = query()
            .account(1)
            .library_section(2)
            .rating_key(RatingKey(3))
            .build_url()
            .unwrap();
        let q = url.query().unwrap();
        assert!(q.contains("accountID=1"));
        assert!(q.contains("librarySectionID=2"));
        assert!(q.contains("metadataItemID=3"));
    }

    #[test]
    fn history_entry_dto_parses_minimal_movie() {
        let body = serde_json::json!({
            "accountID": 1,
            "deviceID": 2,
            "historyKey": "/status/sessions/history/100",
            "viewedAt": 1_700_000_000,
            "ratingKey": "42",
            "key": "/library/metadata/42",
            "title": "Arrival",
            "type": "movie",
            "librarySectionID": 1,
        });
        let dto: HistoryEntryDto = serde_json::from_value(body).unwrap();
        let http = dummy_http();
        let base = Url::parse("http://pms.local:32400").unwrap();
        let entry = dto.into_domain(http, base).unwrap();
        assert_eq!(entry.account_id, 1);
        assert_eq!(entry.device_id, 2);
        assert_eq!(
            entry.history_key.as_deref(),
            Some("/status/sessions/history/100")
        );
        assert!(entry.viewed_at.is_some());
        match entry.item {
            LibraryItem::Movie(m) => assert_eq!(m.title, "Arrival"),
            other => panic!("expected Movie, got {other:?}"),
        }
    }
}
