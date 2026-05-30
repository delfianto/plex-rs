//! plex.tv authentication flows.
//!
//! Three flows are supported:
//!
//! - **Direct token** — caller already has an `X-Plex-Token`; no
//!   sign-in needed. Pass it to [`crate::PlexServer::connect`].
//! - **PIN / OAuth** — implemented here as [`MyPlexPinLogin`]. The
//!   user enters a 4-char code at `plex.tv/link`, and this crate
//!   polls until plex.tv attaches an auth token to the PIN.
//! - **Password + 2FA** — implemented here as
//!   [`MyPlexPasswordLogin`]. Use when the calling application
//!   genuinely needs to handle the user's password (e.g. an admin
//!   automation that cannot prompt a human). Two-factor accounts
//!   surface [`crate::Error::TwoFactorRequired`] so callers can
//!   prompt for an OTP and retry.
//!
//! The PIN flow is the recommended onboarding path because it
//! doesn't require the calling application to handle the user's
//! password directly.

pub mod password;
pub mod pin;

pub use password::MyPlexPasswordLogin;
pub use pin::MyPlexPinLogin;
