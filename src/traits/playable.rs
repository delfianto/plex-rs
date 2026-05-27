//! [`Playable`] — leaf types that expose a direct-play URL.
//!
//! "Direct play" means handing an external media player (VLC, mpv,
//! a browser `<video>` element, …) a URL it can stream without any
//! transcoding. Plex serves direct-play files from
//! `<base>/library/parts/<part_id>/<unix>/<filename>` — the `key`
//! attribute on every [`crate::MediaPart`].
//!
//! The URL needs `X-Plex-Token` embedded as a query parameter
//! because the external player won't carry the token in HTTP
//! headers the way `plex-rs` itself does.
//!
//! Full transcoded-streaming URL construction (`/video/:/transcode/
//! universal/start.<container>` with quality / decision negotiation)
//! lands in a follow-up — for now [`Playable::direct_play_url`] gives
//! callers the original-file URL, which is what most external
//! players want when they can decode the source.

use url::Url;

use crate::traits::PlexObject;

/// Items that have a direct-play URL.
///
/// Implementors are leaf types whose metadata carries a
/// `Vec<Media>` with at least one `MediaPart`: [`crate::Movie`],
/// [`crate::Episode`], [`crate::Track`]. [`crate::Photo`] is
/// intentionally **not** a `Playable` — its file is served from a
/// different path family and the use case is different (image
/// fetch, not media stream).
pub trait Playable: PlexObject {
    /// Wire key (relative path) of the first part of the first
    /// media version. Returns `None` when the item's media chain
    /// is empty — typically because the metadata came from a
    /// listing endpoint that omits `Media[]` (see
    /// [`crate::Reload`] to upgrade).
    fn first_part_key(&self) -> Option<&str>;

    /// Construct a direct-play URL ready to hand to an external
    /// media player.
    ///
    /// Embeds `X-Plex-Token` in the query string so the external
    /// player can stream without needing to set the auth header
    /// itself. Returns `None` when:
    /// - the item's media chain is empty (call [`crate::Reload::reload`]
    ///   first), or
    /// - the bound [`crate::HttpClient`] has no token configured.
    fn direct_play_url(&self) -> Option<Url> {
        let part_key = self.first_part_key()?;
        let token = self
            .section_ref()
            .http
            .config()
            .token
            .as_ref()?
            .expose()
            .to_owned();
        // `part_key` may already contain a `?…` suffix in some
        // Plex builds. Defensive: only add the leading `?` when no
        // query exists.
        let separator = if part_key.contains('?') { '&' } else { '?' };
        let path = format!("{part_key}{separator}X-Plex-Token={token}");
        self.section_ref().base_url.join(&path).ok()
    }
}
