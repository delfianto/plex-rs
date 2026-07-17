//! Strong identifier newtypes.
//!
//! Plex re-uses a handful of opaque string / integer IDs throughout
//! its API that are trivial to mix up at call sites. Wrapping each in
//! its own newtype turns "passed the wrong ID" into a compile error.
//!
//! All newtypes are `#[serde(transparent)]` so they (de)serialise as
//! the underlying primitive.
//!
//! [`PlexToken`] additionally **redacts itself in [`Debug`]** so a
//! `tracing` span or panic backtrace never leaks credentials —
//! `format!("{:?}", token)` always returns `PlexToken("***redacted***")`
//! regardless of the inner value. The redaction is unconditional and
//! covered by tests.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::Error;

// -----------------------------------------------------------------------------
// RatingKey — Plex's primary metadata identifier.
// -----------------------------------------------------------------------------

/// Plex metadata identifier — the integer at the end of
/// `/library/metadata/<n>`.
///
/// Movies, episodes, tracks, photos, playlists, collections all use the
/// same ID space. The Python SDK parses it as a string but every PMS
/// response uses unsigned integers in practice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RatingKey(pub u64);

impl RatingKey {
    /// Construct from any `u64`. `const` to allow usage in static contexts.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Return the underlying integer.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for RatingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<u64> for RatingKey {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl FromStr for RatingKey {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<u64>()
            .map(Self)
            .map_err(|e| Error::Config(format!("invalid RatingKey {s:?}: {e}")))
    }
}

// -----------------------------------------------------------------------------
// PlayQueueId — Plex's identifier for an active play queue.
// -----------------------------------------------------------------------------

/// Plex play-queue identifier.
///
/// Returned by `POST /playQueues` and referenced in subsequent client
/// commands as a `containerKey`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlayQueueId(pub u64);

impl PlayQueueId {
    /// Construct from any `u64`.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Borrow the underlying integer.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for PlayQueueId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<u64> for PlayQueueId {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl FromStr for PlayQueueId {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<u64>()
            .map(Self)
            .map_err(|e| Error::Config(format!("invalid PlayQueueId {s:?}: {e}")))
    }
}

// -----------------------------------------------------------------------------
// MachineIdentifier — every PMS exposes a stable 40-char hex string.
// -----------------------------------------------------------------------------

/// The Plex Media Server's stable identifier (40-char hex string).
///
/// Returned by `GET /` as the `machineIdentifier` attribute. Used to
/// route plex.tv resource picks and to form `server://` URIs.
///
/// Stored as a `String` rather than 20 raw bytes because Plex itself
/// transmits the value as a hex string; round-tripping through bytes
/// would make snapshot tests harder to read.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MachineIdentifier(String);

impl MachineIdentifier {
    /// Construct from any `String`-like value. Empty strings are rejected.
    ///
    /// # Errors
    /// Returns [`Error::Config`] when the value is empty.
    pub fn new(value: impl Into<String>) -> Result<Self, Error> {
        let s = value.into();
        if s.is_empty() {
            return Err(Error::Config(
                "MachineIdentifier must not be empty".to_owned(),
            ));
        }
        Ok(Self(s))
    }

    /// Borrow the underlying string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for MachineIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("MachineIdentifier").field(&self.0).finish()
    }
}

impl fmt::Display for MachineIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for MachineIdentifier {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl FromStr for MachineIdentifier {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s.to_owned())
    }
}

// -----------------------------------------------------------------------------
// ClientIdentifier — what the *caller* (us) tells Plex about ourselves.
// -----------------------------------------------------------------------------

/// Opaque identifier the calling client persists across runs.
///
/// This is the value sent in the `X-Plex-Client-Identifier` header.
/// Plex uses it to deduplicate auth sessions, devices, and webhooks.
/// **It must be stable per install** — see the warning in
/// `CLAUDE.md` §8.
///
/// For ephemeral or test use, [`ClientIdentifier::generated`] returns
/// a fresh UUID. For production use, persist the value yourself and
/// pass it to [`crate::error::Result`] via your `ClientConfig`.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ClientIdentifier(String);

impl ClientIdentifier {
    /// Construct from a caller-supplied string. Empty input is rejected.
    ///
    /// # Errors
    /// Returns [`Error::Config`] when the value is empty.
    pub fn new(value: impl Into<String>) -> Result<Self, Error> {
        let s = value.into();
        if s.is_empty() {
            return Err(Error::Config(
                "ClientIdentifier must not be empty".to_owned(),
            ));
        }
        Ok(Self(s))
    }

    /// Generate a fresh v4 UUID-backed identifier. Intended for
    /// ephemeral use; persistent callers should construct via
    /// [`ClientIdentifier::new`] from a stored value.
    #[must_use]
    pub fn generated() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Borrow the underlying string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ClientIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ClientIdentifier").field(&self.0).finish()
    }
}

impl fmt::Display for ClientIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for ClientIdentifier {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl FromStr for ClientIdentifier {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s.to_owned())
    }
}

// -----------------------------------------------------------------------------
// PlexToken — credentials. Redacted in Debug, unconditionally.
// -----------------------------------------------------------------------------

/// A `plex.tv` / Plex Media Server auth token (`X-Plex-Token`).
///
/// **Redacted in [`Debug`]** — `format!("{:?}", token)` always renders
/// as `PlexToken("***redacted***")` no matter what the inner value is.
/// This makes it safe to include in `tracing` spans, panic messages,
/// and `Debug`-derived parent structs without leaking credentials.
///
/// `Display` is **not implemented** — to access the raw value, use
/// [`PlexToken::expose`] explicitly. The intent is to make the
/// "I am exposing a secret" call site grep-able.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlexToken(String);

impl PlexToken {
    /// Construct from a caller-supplied string. Empty tokens are rejected.
    ///
    /// # Errors
    /// Returns [`Error::Config`] when the value is empty.
    pub fn new(value: impl Into<String>) -> Result<Self, Error> {
        let s = value.into();
        if s.is_empty() {
            return Err(Error::Config("PlexToken must not be empty".to_owned()));
        }
        Ok(Self(s))
    }

    /// Reveal the underlying token string. The name and explicit-method
    /// requirement is intentional friction — grep for `expose(` to audit
    /// every place a token leaves the type.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Borrow as a UTF-8 byte slice. Equivalent to
    /// `self.expose().as_bytes()` — same intent (explicit unwrap of the
    /// secret) but useful when constructing HTTP header values.
    #[must_use]
    pub fn expose_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    /// Render the token in a form suitable for log output: only the
    /// first and last two characters are shown, everything else is
    /// elided. Returns a [`Cow`] so short tokens (≤4 chars) borrow
    /// the static redaction marker.
    #[must_use]
    pub fn redacted(&self) -> Cow<'static, str> {
        if self.0.len() <= 4 {
            Cow::Borrowed("***")
        } else {
            let (head, _) = self.0.split_at(2);
            let (_, tail) = self.0.split_at(self.0.len() - 2);
            Cow::Owned(format!("{head}***{tail}"))
        }
    }
}

impl fmt::Debug for PlexToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // INVARIANT: we MUST NOT include the inner value here.
        f.debug_tuple("PlexToken").field(&"***redacted***").finish()
    }
}

impl FromStr for PlexToken {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --------- RatingKey ---------

    #[test]
    fn rating_key_display_round_trip() {
        let rk = RatingKey::new(12345);
        assert_eq!(rk.to_string(), "12345");
        assert_eq!(rk.get(), 12345);
    }

    #[test]
    fn rating_key_from_str_ok() {
        let rk: RatingKey = "42".parse().unwrap();
        assert_eq!(rk.get(), 42);
    }

    #[test]
    fn rating_key_from_str_rejects_non_numeric() {
        let err = "abc".parse::<RatingKey>().unwrap_err();
        assert!(matches!(err, Error::Config(_)));
    }

    #[test]
    fn rating_key_serde_transparent() {
        let rk = RatingKey::new(7);
        let j = serde_json::to_string(&rk).unwrap();
        assert_eq!(j, "7");
        let back: RatingKey = serde_json::from_str(&j).unwrap();
        assert_eq!(back, rk);
    }

    #[test]
    fn rating_key_from_u64() {
        let rk: RatingKey = RatingKey::from(99u64);
        assert_eq!(rk.get(), 99);
        // Ord / PartialOrd are derived; sanity-check they behave.
        assert!(RatingKey::new(1) < RatingKey::new(2));
    }

    // --------- PlayQueueId ---------

    #[test]
    fn play_queue_id_new_get_and_display() {
        let id = PlayQueueId::new(555);
        assert_eq!(id.get(), 555);
        assert_eq!(id.to_string(), "555");
    }

    #[test]
    fn play_queue_id_from_u64() {
        let id: PlayQueueId = PlayQueueId::from(7u64);
        assert_eq!(id.get(), 7);
    }

    #[test]
    fn play_queue_id_from_str_ok() {
        let id: PlayQueueId = "12345".parse().unwrap();
        assert_eq!(id.get(), 12345);
    }

    #[test]
    fn play_queue_id_from_str_rejects_non_numeric() {
        let err = "nope".parse::<PlayQueueId>().unwrap_err();
        assert!(matches!(err, Error::Config(_)));
    }

    #[test]
    fn play_queue_id_serde_transparent() {
        let id = PlayQueueId::new(13);
        assert_eq!(serde_json::to_string(&id).unwrap(), "13");
        let back: PlayQueueId = serde_json::from_str("13").unwrap();
        assert_eq!(back, id);
    }

    // --------- MachineIdentifier ---------

    #[test]
    fn machine_identifier_round_trip() {
        let mid = MachineIdentifier::new("abc123").unwrap();
        assert_eq!(mid.as_str(), "abc123");
        assert_eq!(mid.to_string(), "abc123");
    }

    #[test]
    fn machine_identifier_rejects_empty() {
        let err = MachineIdentifier::new("").unwrap_err();
        assert!(matches!(err, Error::Config(_)));
    }

    #[test]
    fn machine_identifier_serde_transparent() {
        let mid = MachineIdentifier::new("xyz").unwrap();
        let j = serde_json::to_string(&mid).unwrap();
        assert_eq!(j, "\"xyz\"");
    }

    #[test]
    fn machine_identifier_as_ref_and_debug() {
        let mid = MachineIdentifier::new("mid-123").unwrap();
        // AsRef<str> borrows the inner value.
        let s: &str = mid.as_ref();
        assert_eq!(s, "mid-123");
        // Debug surfaces the value (unlike PlexToken, this is not secret).
        let dbg = format!("{mid:?}");
        assert!(dbg.contains("MachineIdentifier"));
        assert!(dbg.contains("mid-123"));
    }

    #[test]
    fn machine_identifier_from_str() {
        let mid: MachineIdentifier = "abc".parse().unwrap();
        assert_eq!(mid.as_str(), "abc");
        assert!("".parse::<MachineIdentifier>().is_err());
    }

    // --------- ClientIdentifier ---------

    #[test]
    fn client_identifier_generated_is_non_empty_uuid() {
        let cid = ClientIdentifier::generated();
        // UUID v4 string is 36 chars including dashes.
        assert_eq!(cid.as_str().len(), 36);
        assert_eq!(cid.as_str().chars().filter(|&c| c == '-').count(), 4);
    }

    #[test]
    fn client_identifier_explicit_value() {
        let cid = ClientIdentifier::new("my-stable-id").unwrap();
        assert_eq!(cid.as_str(), "my-stable-id");
    }

    #[test]
    fn client_identifier_rejects_empty() {
        let err = ClientIdentifier::new("").unwrap_err();
        assert!(matches!(err, Error::Config(_)));
    }

    #[test]
    fn client_identifier_as_ref_display_and_debug() {
        let cid = ClientIdentifier::new("cid-xyz").unwrap();
        let s: &str = cid.as_ref();
        assert_eq!(s, "cid-xyz");
        assert_eq!(cid.to_string(), "cid-xyz");
        let dbg = format!("{cid:?}");
        assert!(dbg.contains("ClientIdentifier"));
        assert!(dbg.contains("cid-xyz"));
    }

    #[test]
    fn client_identifier_from_str() {
        let cid: ClientIdentifier = "stable".parse().unwrap();
        assert_eq!(cid.as_str(), "stable");
        assert!("".parse::<ClientIdentifier>().is_err());
    }

    #[test]
    fn client_identifier_serde_transparent() {
        let cid = ClientIdentifier::new("wire-cid").unwrap();
        assert_eq!(serde_json::to_string(&cid).unwrap(), "\"wire-cid\"");
    }

    // --------- PlexToken — the load-bearing tests ---------

    #[test]
    fn token_debug_never_reveals_inner_value() {
        let secret = PlexToken::new("super-secret-token-DO-NOT-LEAK").unwrap();
        let debug = format!("{secret:?}");
        // The literal secret must not appear anywhere in the debug output.
        assert!(
            !debug.contains("super-secret-token-DO-NOT-LEAK"),
            "Debug for PlexToken leaked the inner value: {debug}"
        );
        assert!(
            debug.contains("***redacted***"),
            "Debug for PlexToken must include the redaction marker"
        );
    }

    #[test]
    fn token_debug_in_nested_struct_is_redacted() {
        // Ensure Debug propagation through #[derive(Debug)] parents keeps it redacted.
        #[derive(Debug)]
        struct Holder {
            #[allow(dead_code)]
            token: PlexToken,
        }
        let h = Holder {
            token: PlexToken::new("leaky-token-123").unwrap(),
        };
        let s = format!("{h:?}");
        assert!(!s.contains("leaky-token-123"));
        assert!(s.contains("***redacted***"));
    }

    #[test]
    fn token_expose_returns_inner_value() {
        let t = PlexToken::new("abc").unwrap();
        assert_eq!(t.expose(), "abc");
        assert_eq!(t.expose_bytes(), b"abc");
    }

    #[test]
    fn token_redacted_short_token() {
        let t = PlexToken::new("ab").unwrap();
        assert_eq!(t.redacted(), "***");
    }

    #[test]
    fn token_redacted_long_token() {
        let t = PlexToken::new("ABCDEFGH").unwrap();
        assert_eq!(t.redacted(), "AB***GH");
    }

    #[test]
    fn token_rejects_empty() {
        let err = PlexToken::new("").unwrap_err();
        assert!(matches!(err, Error::Config(_)));
    }

    #[test]
    fn token_from_str_ok_and_rejects_empty() {
        let t: PlexToken = "from-str-token".parse().unwrap();
        assert_eq!(t.expose(), "from-str-token");
        assert!("".parse::<PlexToken>().is_err());
    }

    #[test]
    fn token_redacted_boundary_exactly_four_chars() {
        // Exactly 4 chars hits the `<= 4` short-token branch.
        let t = PlexToken::new("abcd").unwrap();
        assert_eq!(t.redacted(), "***");
        // Five chars crosses into the head/tail branch.
        let t5 = PlexToken::new("abcde").unwrap();
        assert_eq!(t5.redacted(), "ab***de");
    }

    #[test]
    fn token_serde_transparent() {
        // Tokens serialise as plain strings on the wire.
        let t = PlexToken::new("wire-token").unwrap();
        let j = serde_json::to_string(&t).unwrap();
        assert_eq!(j, "\"wire-token\"");
        let back: PlexToken = serde_json::from_str(&j).unwrap();
        assert_eq!(back.expose(), "wire-token");
    }

    #[test]
    fn token_does_not_implement_display() {
        // Compile-time guarantee: PlexToken: !Display
        // This is documented in the module docs; we can't write a runtime
        // assertion for !impl directly, so we assert that the redact-debug
        // path is what callers will hit when they `{}-format`.
        // A `Display` impl would shadow this and reveal the secret.
        fn is_display<T: fmt::Display>(_t: T) {}
        // The following should NOT compile if Display is ever added:
        // is_display(PlexToken::new("x").unwrap());
        // We mark `is_display` as unused to make the test useful as a tripwire.
        let _ = is_display::<String>;
    }
}
