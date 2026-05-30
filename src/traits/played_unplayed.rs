//! [`PlayedUnplayed`] — mark items as played / unplayed.
//!
//! Implements Plex's `/:/scrobble` and `/:/unscrobble` endpoints,
//! both of which are confusingly served via HTTP `GET` despite being
//! mutations. We mirror that on the wire;
//! the public Rust surface still uses `mark_*` verbs to make the
//! intent obvious at the call site.

use crate::error::Result;
use crate::traits::PlexObject;

/// Mark items as played / unplayed, query their current played
/// state.
///
/// The default-method bodies hit `/:/scrobble` and `/:/unscrobble`
/// with `identifier=com.plexapp.plugins.library&key=<rating_key>`.
/// Implementors only need to declare which view-count to read for
/// [`Self::is_played`] — the mutating methods compose from
/// [`PlexObject`].
pub trait PlayedUnplayed: PlexObject {
    /// The current view count for this item.
    fn view_count(&self) -> u32;

    /// `true` when [`Self::view_count`] is greater than zero.
    fn is_played(&self) -> bool {
        self.view_count() > 0
    }

    /// Mark this item as played.
    ///
    /// Issues `GET /:/scrobble?key=<rating_key>&identifier=com.plexapp.plugins.library`
    /// against the bound PMS. The request is served as `GET` because
    /// that is what Plex requires.
    ///
    /// # Errors
    /// Any [`crate::Error`] variant from the underlying transport.
    fn mark_played(&self) -> impl std::future::Future<Output = Result<()>> + Send
    where
        Self: Sync,
    {
        async move {
            scrobble(self, "/:/scrobble").await?;
            Ok(())
        }
    }

    /// Mark this item as unplayed.
    ///
    /// Issues `GET /:/unscrobble?key=<rating_key>&identifier=com.plexapp.plugins.library`.
    ///
    /// # Errors
    /// Any [`crate::Error`] variant from the underlying transport.
    fn mark_unplayed(&self) -> impl std::future::Future<Output = Result<()>> + Send
    where
        Self: Sync,
    {
        async move {
            scrobble(self, "/:/unscrobble").await?;
            Ok(())
        }
    }
}

/// Internal helper that constructs the scrobble / unscrobble URL and
/// issues the request. Centralised so the wire format lives in one
/// place — adding `X-Plex-Token` etc. happens via the HTTP client's
/// default headers.
async fn scrobble<T: PlayedUnplayed + ?Sized>(object: &T, path: &str) -> Result<()> {
    let url = object.base_url().join(&format!(
        "{path}?identifier=com.plexapp.plugins.library&key={rk}",
        rk = object.rating_key()
    ))?;
    let _ = object.http().get_bytes(url.as_str()).await?;
    Ok(())
}
