//! Image-surface traits.
//!
//! Plex emits three image-family paths on metadata items:
//!
//! | Wire field | Plex term     | Rust trait pair                 |
//! | ---------- | ------------- | ------------------------------- |
//! | `thumb`    | Poster        | [`HasPosterUrl`] + [`HasPosterLock`] |
//! | `art`      | Background    | [`HasArtUrl`] + [`HasArtLock`]  |
//! | `theme`    | Theme song    | [`HasThemeUrl`] + [`HasThemeLock`] |
//!
//! Each pair splits responsibilities:
//!
//! - **`*Url`** traits expose a single `*_url()` method returning
//!   the absolute URL built against the server's base. They only
//!   require [`PlexObject`] — every leaf type with the wire field
//!   can implement them.
//! - **`*Lock`** traits add `lock_*()` / `unlock_*()` that toggle
//!   the per-field `<field>.locked` flag via [`EditField`]. They
//!   require [`EditField`] which in turn requires the leaf to live
//!   under a `LibrarySectionRef`.
//!
//! Full image CRUD (`set_*`, `upload_*_url`, `upload_*_bytes`,
//! `delete_*`) needs Plex's `POST /library/metadata/<rk>/<kind>`
//! endpoints and `POST`-with-bytes on the HTTP client; lands in a
//! follow-up iteration alongside the `HasArt` / `HasPoster` /
//! `HasTheme` super-traits.

use url::Url;

use crate::error::Result;
use crate::traits::{EditField, PlexObject};

// -----------------------------------------------------------------------------
// HasArtUrl — background image.
// -----------------------------------------------------------------------------

/// Read the background-art image URL.
pub trait HasArtUrl: PlexObject {
    /// Wire path for the background art (`art` attribute). Returns
    /// `None` when PMS hasn't assigned one.
    fn art_path(&self) -> Option<&str>;

    /// Absolute URL to the background art, resolved against the
    /// server base URL.
    ///
    /// # Errors
    /// Returns [`crate::Error::Url`] if the wire path cannot be
    /// joined to the base URL.
    fn art_url(&self) -> Result<Option<Url>> {
        match self.art_path() {
            None => Ok(None),
            Some(p) => Ok(Some(self.base_url().join(p)?)),
        }
    }
}

/// Toggle the per-field lock on background art.
pub trait HasArtLock: HasArtUrl + EditField {
    /// Mark the background-art field as locked (PMS will not
    /// overwrite it on the next metadata refresh).
    ///
    /// # Errors
    /// Any transport [`crate::Error`].
    fn lock_art(&self) -> impl std::future::Future<Output = Result<()>> + Send
    where
        Self: Sync,
    {
        self.lock_field("art", true)
    }

    /// Mark the background-art field as unlocked.
    ///
    /// # Errors
    /// Any transport [`crate::Error`].
    fn unlock_art(&self) -> impl std::future::Future<Output = Result<()>> + Send
    where
        Self: Sync,
    {
        self.lock_field("art", false)
    }
}

// -----------------------------------------------------------------------------
// HasPosterUrl — thumb / poster image.
// -----------------------------------------------------------------------------

/// Read the poster / thumb image URL.
///
/// Plex stores posters in the `thumb` field on the wire — confusingly
/// named, since the "thumb" is conceptually the full poster.
pub trait HasPosterUrl: PlexObject {
    /// Wire path for the poster (`thumb` attribute).
    fn thumb_path(&self) -> Option<&str>;

    /// Absolute URL to the poster, resolved against the server base.
    ///
    /// # Errors
    /// Returns [`crate::Error::Url`] if the wire path cannot be
    /// joined to the base URL.
    fn poster_url(&self) -> Result<Option<Url>> {
        match self.thumb_path() {
            None => Ok(None),
            Some(p) => Ok(Some(self.base_url().join(p)?)),
        }
    }
}

/// Toggle the per-field lock on poster (`thumb`).
pub trait HasPosterLock: HasPosterUrl + EditField {
    /// Mark the poster as locked.
    ///
    /// # Errors
    /// Any transport [`crate::Error`].
    fn lock_poster(&self) -> impl std::future::Future<Output = Result<()>> + Send
    where
        Self: Sync,
    {
        self.lock_field("thumb", true)
    }

    /// Mark the poster as unlocked.
    ///
    /// # Errors
    /// Any transport [`crate::Error`].
    fn unlock_poster(&self) -> impl std::future::Future<Output = Result<()>> + Send
    where
        Self: Sync,
    {
        self.lock_field("thumb", false)
    }
}

// -----------------------------------------------------------------------------
// HasThemeUrl — theme song path.
// -----------------------------------------------------------------------------

/// Read the theme-song URL (Show only).
pub trait HasThemeUrl: PlexObject {
    /// Wire path for the theme song (`theme` attribute).
    fn theme_path(&self) -> Option<&str>;

    /// Absolute URL to the theme song.
    ///
    /// # Errors
    /// Returns [`crate::Error::Url`] if the wire path cannot be
    /// joined to the base URL.
    fn theme_url(&self) -> Result<Option<Url>> {
        match self.theme_path() {
            None => Ok(None),
            Some(p) => Ok(Some(self.base_url().join(p)?)),
        }
    }
}

/// Toggle the per-field lock on the theme song.
pub trait HasThemeLock: HasThemeUrl + EditField {
    /// Mark the theme as locked.
    ///
    /// # Errors
    /// Any transport [`crate::Error`].
    fn lock_theme(&self) -> impl std::future::Future<Output = Result<()>> + Send
    where
        Self: Sync,
    {
        self.lock_field("theme", true)
    }

    /// Mark the theme as unlocked.
    ///
    /// # Errors
    /// Any transport [`crate::Error`].
    fn unlock_theme(&self) -> impl std::future::Future<Output = Result<()>> + Send
    where
        Self: Sync,
    {
        self.lock_field("theme", false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::LibrarySectionRef;
    use crate::{ClientConfig, ClientIdentifier, HttpClient, RatingKey};

    /// Minimal [`PlexObject`] carrying the three image paths so the
    /// default `*_url()` builders can be exercised without any I/O.
    struct ImageStub {
        section_ref: LibrarySectionRef,
        art: Option<String>,
        thumb: Option<String>,
        theme: Option<String>,
    }

    impl PlexObject for ImageStub {
        fn section_ref(&self) -> &LibrarySectionRef {
            &self.section_ref
        }
        fn rating_key(&self) -> RatingKey {
            RatingKey::new(1)
        }
        fn metadata_type_id(&self) -> u32 {
            1
        }
    }

    impl HasArtUrl for ImageStub {
        fn art_path(&self) -> Option<&str> {
            self.art.as_deref()
        }
    }
    impl HasPosterUrl for ImageStub {
        fn thumb_path(&self) -> Option<&str> {
            self.thumb.as_deref()
        }
    }
    impl HasThemeUrl for ImageStub {
        fn theme_path(&self) -> Option<&str> {
            self.theme.as_deref()
        }
    }

    fn stub(art: Option<&str>, thumb: Option<&str>, theme: Option<&str>) -> ImageStub {
        let cfg = ClientConfig::builder(ClientIdentifier::new("t").unwrap())
            .build()
            .unwrap();
        let http = HttpClient::new(cfg).unwrap();
        ImageStub {
            section_ref: LibrarySectionRef {
                id: 1,
                http,
                base_url: url::Url::parse("http://plex.local:32400/").unwrap(),
            },
            art: art.map(ToOwned::to_owned),
            thumb: thumb.map(ToOwned::to_owned),
            theme: theme.map(ToOwned::to_owned),
        }
    }

    #[test]
    fn art_url_resolves_against_base() {
        let s = stub(Some("/library/metadata/1/art/9"), None, None);
        let url = s.art_url().unwrap().unwrap();
        assert_eq!(url.path(), "/library/metadata/1/art/9");
        assert_eq!(url.host_str(), Some("plex.local"));
    }

    #[test]
    fn poster_url_resolves_against_base() {
        let s = stub(None, Some("/library/metadata/1/thumb/9"), None);
        let url = s.poster_url().unwrap().unwrap();
        assert_eq!(url.path(), "/library/metadata/1/thumb/9");
    }

    #[test]
    fn theme_url_resolves_against_base() {
        let s = stub(None, None, Some("/library/metadata/1/theme/9"));
        let url = s.theme_url().unwrap().unwrap();
        assert_eq!(url.path(), "/library/metadata/1/theme/9");
    }

    #[test]
    fn all_url_builders_return_none_when_path_absent() {
        let s = stub(None, None, None);
        assert!(s.art_url().unwrap().is_none());
        assert!(s.poster_url().unwrap().is_none());
        assert!(s.theme_url().unwrap().is_none());
    }
}
