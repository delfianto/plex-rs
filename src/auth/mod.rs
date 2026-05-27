//! plex.tv authentication flows.
//!
//! Three flows are documented in
//! [`analysis/03-myplex-and-auth.md`](../../analysis/03-myplex-and-auth.md):
//!
//! - **Direct token** — caller already has an `X-Plex-Token`; no
//!   sign-in needed. Pass it to [`crate::PlexServer::connect`].
//! - **PIN / OAuth** — implemented here as [`MyPlexPinLogin`]. The
//!   user enters a 4-char code at `plex.tv/link`, and this crate
//!   polls until plex.tv attaches an auth token to the PIN.
//! - **Password + 2FA** — defers (analysis/11 §10 M5 milestone).
//!
//! The PIN flow is the recommended onboarding path because it
//! doesn't require the calling application to handle the user's
//! password directly.

pub mod pin;

pub use pin::MyPlexPinLogin;
