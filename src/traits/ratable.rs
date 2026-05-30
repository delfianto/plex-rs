//! [`Ratable`] — set / clear the user's personal rating for an item.
//!
//! Wire endpoint: `PUT /:/rate?key=<rating_key>&identifier=com.plexapp.plugins.library&rating=<r>`.
//! Plex's `r` is a float in `[0.0, 10.0]` (the 0–5 star scale times
//! two — 5 stars = `10.0`, half a star = `1.0`). Passing `-1` clears
//! the user's rating entirely.

use crate::error::{Error, Result};
use crate::traits::PlexObject;

/// Permitted maximum rating (5 stars). Matches Plex's wire scale.
const MAX_RATING: f32 = 10.0;
/// Wire sentinel for "no rating".
const CLEAR_SENTINEL: f32 = -1.0;

/// Set or clear the user's personal rating for an item.
///
/// Implementors are leaf types whose metadata carries a `rating`
/// field on the wire (Movie / Show / Episode / Album / Track). Photos
/// and photoalbums also accept ratings but are not implemented in M3.3
/// because the read side doesn't yet surface `rating` on them.
pub trait Ratable: PlexObject {
    /// Set the user's rating to `value`, or clear it when `None`.
    ///
    /// `value` must be in `0.0..=10.0`. Half-star granularity (`0.5`,
    /// `1.0`, …, `10.0`) is what the Plex web UI emits; finer values
    /// are accepted but get rounded by the server.
    ///
    /// # Errors
    /// - [`Error::Config`] when `value` is outside `0.0..=10.0`.
    /// - Any transport [`Error`].
    fn rate(&self, value: Option<f32>) -> impl std::future::Future<Output = Result<()>> + Send
    where
        Self: Sync,
    {
        async move {
            let wire = match value {
                None => CLEAR_SENTINEL,
                Some(v) if (0.0..=MAX_RATING).contains(&v) => v,
                Some(v) => {
                    return Err(Error::Config(format!(
                        "rating {v} outside the allowed range 0.0..=10.0",
                    )));
                }
            };
            let url = self.base_url().join(&format!(
                "/:/rate?key={rk}&identifier=com.plexapp.plugins.library&rating={wire}",
                rk = self.rating_key(),
            ))?;
            // Plex requires PUT for /:/rate. Empty body.
            self.http().put_no_body(url.as_str()).await
        }
    }
}
