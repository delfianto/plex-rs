//! Header-based pagination primitives.
//!
//! Plex Media Server paginates listing responses via two request
//! headers — **not** query parameters as the casual reader might
//! expect (see [`analysis/01-openapi-overview.md`](../analysis/01-openapi-overview.md)
//! §4.2 and [`analysis/11-rust-mapping-recommendations.md`](../analysis/11-rust-mapping-recommendations.md)
//! §4.3):
//!
//! | Header                       | Meaning                              |
//! | ---------------------------- | ------------------------------------ |
//! | `X-Plex-Container-Start`     | Zero-based offset of the first item. |
//! | `X-Plex-Container-Size`      | Maximum items the server may return. |
//!
//! The response echoes these back inside the `<MediaContainer>` as
//! `offset` and `size`, plus `totalSize` for the full count. This
//! module captures that contract as [`PageRange`] and provides a
//! [`PageRange::advance_with`] helper that produces the next page
//! (or [`None`] when the listing is fully consumed).

use crate::xml::MediaContainerMeta;

/// Request-side header name carrying the page start offset.
///
/// The default value is `0` if absent — the same convention Plex uses.
pub const HEADER_CONTAINER_START: &str = "X-Plex-Container-Start";

/// Request-side header name carrying the page size cap.
///
/// PMS itself defaults to `50` when absent; we let callers be explicit.
pub const HEADER_CONTAINER_SIZE: &str = "X-Plex-Container-Size";

/// A request-side pagination window.
///
/// Construct one to ask the server for `size` items starting at
/// `start`. Use [`PageRange::headers`] to render the pair Plex
/// expects. After a response lands, call [`PageRange::advance_with`]
/// to get the next page or [`None`] when the listing is complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PageRange {
    /// Offset of the first item on the requested page.
    pub start: u32,
    /// Maximum items to return on the requested page.
    pub size: u32,
}

impl PageRange {
    /// Construct the first page of size `size`.
    #[must_use]
    pub const fn first(size: u32) -> Self {
        Self { start: 0, size }
    }

    /// Construct an explicit `(start, size)` page.
    #[must_use]
    pub const fn new(start: u32, size: u32) -> Self {
        Self { start, size }
    }

    /// Render this page as the two request headers Plex expects.
    /// Returns `(name, value)` pairs as `&'static str` keys and owned
    /// `String` values.
    #[must_use]
    pub fn headers(self) -> [(&'static str, String); 2] {
        [
            (HEADER_CONTAINER_START, self.start.to_string()),
            (HEADER_CONTAINER_SIZE, self.size.to_string()),
        ]
    }

    /// Given the [`MediaContainerMeta`] from the response to *this*
    /// request, compute the next page — or [`None`] if the listing is
    /// fully consumed.
    ///
    /// The decision rule is intentionally conservative:
    /// - If `total_size` is known and `start + meta.size >= total_size`,
    ///   the listing is complete.
    /// - If `total_size` is unknown but the server returned fewer items
    ///   than requested (`meta.size < self.size`), we treat that as end
    ///   of listing too.
    /// - Otherwise advance by `self.size` items.
    #[must_use]
    pub const fn advance_with(self, meta: &MediaContainerMeta) -> Option<Self> {
        // Defensive: a zero-sized request can't make progress.
        if self.size == 0 {
            return None;
        }
        // Server returned fewer than asked → no more pages.
        if meta.size < self.size {
            return None;
        }
        // Compute the next offset; saturate rather than overflow.
        let next_start = self.start.saturating_add(self.size);
        if let Some(total) = meta.total_size {
            if next_start >= total {
                return None;
            }
        }
        Some(Self {
            start: next_start,
            size: self.size,
        })
    }

    /// Convenience: returns `true` when this page is the very first one
    /// (`start == 0`).
    #[must_use]
    pub const fn is_first(self) -> bool {
        self.start == 0
    }
}

impl Default for PageRange {
    /// A sensible default page: 50 items starting at offset 0,
    /// matching PMS's own default `X-Plex-Container-Size`.
    fn default() -> Self {
        Self::first(50)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta_of(size: u32, total: Option<u32>) -> MediaContainerMeta {
        MediaContainerMeta {
            size,
            total_size: total,
            ..MediaContainerMeta::default()
        }
    }

    #[test]
    fn first_page_helpers() {
        let p = PageRange::first(50);
        assert_eq!(p.start, 0);
        assert_eq!(p.size, 50);
        assert!(p.is_first());
    }

    #[test]
    fn default_matches_pms_50() {
        let p = PageRange::default();
        assert_eq!(p, PageRange::first(50));
    }

    #[test]
    fn renders_both_headers() {
        let p = PageRange::new(100, 25);
        let h = p.headers();
        assert_eq!(h[0], (HEADER_CONTAINER_START, "100".to_owned()));
        assert_eq!(h[1], (HEADER_CONTAINER_SIZE, "25".to_owned()));
    }

    #[test]
    fn advance_stops_when_response_short() {
        // Asked 50, got 12 → no more pages.
        let p = PageRange::first(50);
        let meta = meta_of(12, None);
        assert!(p.advance_with(&meta).is_none());
    }

    #[test]
    fn advance_stops_at_known_total() {
        // Asked 50 starting at 100; total is 120. Next would be 150 → done.
        let p = PageRange::new(100, 50);
        let meta = meta_of(50, Some(120));
        assert!(p.advance_with(&meta).is_none());
    }

    #[test]
    fn advance_continues_when_more_remain() {
        // Asked 50 at 0; got 50; total 200 → next is 50.
        let p = PageRange::first(50);
        let meta = meta_of(50, Some(200));
        let next = p.advance_with(&meta).unwrap();
        assert_eq!(next, PageRange::new(50, 50));
        assert!(!next.is_first());
    }

    #[test]
    fn advance_continues_when_total_unknown_but_full_page_returned() {
        let p = PageRange::first(50);
        let meta = meta_of(50, None);
        let next = p.advance_with(&meta).unwrap();
        assert_eq!(next, PageRange::new(50, 50));
    }

    #[test]
    fn advance_handles_zero_size_safely() {
        let p = PageRange::new(0, 0);
        let meta = meta_of(0, None);
        assert!(p.advance_with(&meta).is_none());
    }

    #[test]
    fn advance_saturates_on_overflow() {
        // start very large, size doesn't fit; saturation prevents UB.
        let p = PageRange::new(u32::MAX - 5, 50);
        let meta = meta_of(50, None);
        // Saturated to u32::MAX, which equals total_size cap if any.
        let next = p.advance_with(&meta).unwrap();
        assert_eq!(next.start, u32::MAX);
    }

    #[test]
    fn advance_with_exact_boundary() {
        // total = 100, returned 50, started at 50 — done.
        let p = PageRange::new(50, 50);
        let meta = meta_of(50, Some(100));
        assert!(p.advance_with(&meta).is_none());
    }
}
