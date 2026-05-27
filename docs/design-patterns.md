# Design patterns

Rust-specific idioms that recur throughout the crate. These are
the patterns to mimic when adding new modules — same shapes,
same conventions.

---

## DTO + Domain split

Every Plex JSON/XML shape that surfaces a public type has two
parallel definitions:

```rust
// Wire-format DTO — crate-private, mirrors Plex's exact field names.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MovieDto {
    rating_key: String,         // becomes "ratingKey" on the wire
    #[serde(rename = "type")]
    metadata_type: String,
    title: String,
    #[serde(default)]
    year: Option<u16>,
    #[serde(default, rename = "originallyAvailableAt")]
    originally_available_at: Option<String>,
    // ...
}

// Public domain type — meaningful field types, proper invariants.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Movie {
    pub rating_key: RatingKey,                       // u64 newtype, not String
    pub title: String,
    pub year: Option<u16>,
    pub originally_available_at: Option<NaiveDate>,  // parsed, not String
    pub section_ref: LibrarySectionRef,
    // ...
}

impl MovieDto {
    fn into_domain(self, section_ref: LibrarySectionRef) -> Result<Movie> {
        Ok(Movie {
            rating_key: RatingKey(self.rating_key.parse()?),
            title: self.title,
            year: self.year,
            originally_available_at: self.originally_available_at
                .as_deref()
                .map(parse_iso_date)
                .transpose()?,
            section_ref,
            // ...
        })
    }
}
```

Benefits:
- Wire-format quirks (`"ratingKey"` vs Rust's snake_case,
  `type` keyword clash, `<NumberAsString>` on numeric fields) are
  isolated to the DTO. The domain type is clean.
- Validation lives in `into_domain`. By the time you have a
  `Movie`, you can trust its invariants.
- Evolving the public API doesn't change the wire parsing, and
  vice versa.

**When to apply:** Always, for any type that comes from a Plex
response. The crate has zero `pub use crate::xml::dto::*` —
DTOs never escape.

---

## Forward-compatible enums

Every enum that mirrors a Plex string discriminator gets an
`Unknown` variant:

```rust
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum AlertEvent {
    Playing(PlayingNotification),
    Timeline(TimelineEntry),
    // ...known variants...
    Unknown {
        kind: String,
        raw: serde_json::Value,
    },
}
```

For dispatch on the `type` field, the parser tries each known
variant in turn; anything not recognized falls into `Unknown`
with the raw payload preserved.

```rust
match kind.as_str() {
    "playing"  => decode_array::<PlayingNotification>(&payload, &["PlaySessionStateNotification"], AlertEvent::Playing),
    "timeline" => decode_array::<TimelineEntry>(&payload, &["TimelineEntry"], AlertEvent::Timeline),
    // ...
    _ => vec![AlertEvent::Unknown { kind, raw: payload }],
}
```

**When to apply:** Every enum that originates from a Plex
`type=` / `event=` field. Avoids breaking changes when Plex
introduces new types — callers' match statements just fall
into the catch-all rather than failing to compile.

---

## Newtype wrappers around IDs

Plex emits several flavours of identifier that are easy to mix
up. Each gets a typed wrapper:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RatingKey(pub u64);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MachineIdentifier(String);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ClientIdentifier(String);

#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlexToken(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlayQueueId(pub u64);
```

`#[serde(transparent)]` means they serialize as their inner
value, so the wire format stays clean. The Rust type system
catches "passed a machine identifier where a rating key was
expected" at compile time.

`String`-backed ids (`MachineIdentifier`, `ClientIdentifier`,
`PlexToken`) have validation in their constructor — rejecting
empty strings with `Error::Config`. Numeric ones (`RatingKey`,
`PlayQueueId`) are plain tuples.

---

## Token redaction via custom `Debug`

`PlexToken` has a hand-written `Debug` impl:

```rust
impl fmt::Debug for PlexToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PlexToken").field(&"***redacted***").finish()
    }
}
```

This composes automatically: any struct that derives `Debug` and
carries an `Option<PlexToken>` field gets the redacted output
for free. The token never appears in tracing logs, error
messages, or test failure output.

Two structs (`MyPlexDevice`, `MyPlexUser`) override their own
`Debug` impls to drop noisier fields (long arrays of capabilities,
ISO timestamps) but they still go through `PlexToken`'s redacted
formatter for their token field.

**When to apply:** Any time the crate introduces a type that
holds a secret value (token, password hash, API key). Don't
trust derived `Debug` to keep secrets — write the impl.

---

## Builder pattern for query construction

URL/query construction goes through typed builders, never
ad-hoc `format!`:

```rust
// FilterBuilder for /library/sections/<id>/all
section
    .filter()
    .equal("genre", "Sci-Fi")
    .gt("year", 2000)
    .sort_by("titleSort", SortDirection::Asc)
    .limit(50)
    .execute()
    .await?;

// TranscodeOptions for /video/:/transcode/universal/start.m3u8
let url = TranscodeOptions::new()
    .protocol(TranscodeProtocol::Hls)
    .max_video_bitrate(8000)
    .video_resolution("1920x1080")
    .build_for(&server, item_key)?;

// CreatePlayQueue for /playQueues
let pq = plex
    .create_play_queue()
    .from_item(&movie)
    .shuffle(true)
    .execute()
    .await?;
```

Each builder method takes `mut self` and returns `Self` so
chaining works naturally. Required fields are arguments to the
factory function (or the type errors at `.execute()` time).
Optional fields default to sensible values (often "what python-
plexapi does").

**When to apply:** Any time the construction needs more than 2-3
parameters or has optional fields. Bare positional arguments
quickly become "passing five strings in a row" call sites.

---

## Self-consuming mutators

Methods that mutate server-side state and return a fresh
snapshot **consume `self`** rather than taking `&mut self`:

```rust
impl PlayQueue {
    pub async fn add_item(self, item: &LibraryItem, play_next: bool) -> Result<Self> { /* ... */ }
    pub async fn move_item(self, item_id: u64, after_id: Option<u64>) -> Result<Self> { /* ... */ }
    pub async fn remove_item(self, item_id: u64) -> Result<Self> { /* ... */ }
    pub async fn clear(self) -> Result<Self> { /* ... */ }
    pub async fn refresh(self) -> Result<Self> { /* ... */ }
}

impl Settings {
    pub async fn set(self, server: &PlexServer, id: &str, value: SettingValue) -> Result<Self> { /* ... */ }
    pub async fn set_many(self, server: &PlexServer, updates: Vec<(&str, SettingValue)>) -> Result<Self> { /* ... */ }
}
```

Benefits:
- The caller can't accidentally use a stale snapshot — the old
  `PlayQueue` is consumed; only the refreshed one survives.
- No `Mutex`/`RefCell` is needed for interior mutability across
  `.await` boundaries.
- Composes cleanly: `pq.add_item(a, true).await?.add_item(b, false).await?`.

**When to apply:** Whenever the operation logically replaces the
current view with an updated one. For mutations that just emit a
side effect without returning a meaningful new state
(`scrobble`, `mark_played`, `delete`), use `&self` and return
`Result<()>`.

---

## Streaming pagination

The `HistoryQuery` builder offers two terminator shapes:

```rust
// Eager — collect every entry into a Vec.
let entries: Vec<HistoryEntry> = plex.history()
    .account(42)
    .max_results(500)
    .collect()
    .await?;

// Lazy — yield entries as they arrive, page by page.
let mut stream = plex.history().page_size(50).stream();
while let Some(entry) = stream.next().await {
    process(entry?);
}
```

`.stream()` returns a `Pin<Box<dyn Stream<Item = Result<HistoryEntry>> + Send>>`
that fetches pages on demand and honors `.max_results` across
page boundaries. Cancellation drops the in-flight fetch cleanly.

**When to apply:** Any listing endpoint backed by `X-Plex-
Container-Start/Size` pagination. The History module is the
canonical reference; copy that shape.

---

## Library-section back-reference

Every metadata leaf type carries a `LibrarySectionRef`:

```rust
pub struct LibrarySectionRef {
    pub id: u32,
    pub(crate) http: HttpClient,
    pub(crate) base_url: Url,
}

pub struct Movie {
    pub rating_key: RatingKey,
    pub title: String,
    // ...
    pub section_ref: LibrarySectionRef,
}
```

The section ref carries the section id, an `HttpClient` handle,
and the PMS base URL. Together they let the M3 edit traits
construct `PUT /library/sections/<id>/all?id=<rk>&type=<n>&<field>.value=<v>`
URLs without re-traversing through `PlexServer`.

This is what makes `movie.rate(8.5).await?` work — the `Movie`
already knows everything it needs to mutate itself.

For domain types that come from cross-section listings
(`HistoryEntry`, `PlayingSession`, `PlayQueueItem`), the
section ref is synthesised from the embedded `librarySectionID`
field on the metadata, with a sane fallback when the field is
missing. Documented in the relevant DTO conversion code.

---

## Trait architecture: extension traits per capability

Rather than one monolithic `MediaItem` trait with `fn rate()`,
`fn edit_title()`, `fn replace_genres()`, `fn delete()`, ...
the crate uses **separate extension traits per capability**:

```rust
pub trait PlexObject {
    fn rating_key(&self) -> RatingKey;
    fn metadata_type_id(&self) -> u32;
    fn section_ref(&self) -> &LibrarySectionRef;
    // default-derived: http(), base_url()
}

pub trait Ratable: PlexObject {
    fn rate(&self, value: Option<f32>) -> impl Future<...> + Send where Self: Sync { /* default body */ }
}

pub trait EditField: PlexObject {
    fn edit_field(&self, field: &str, value: impl Into<FieldValue>, locked: bool) -> impl Future<...> + Send where Self: Sync { /* default body */ }
    fn lock_field(&self, field: &str, locked: bool) -> impl Future<...> + Send where Self: Sync { /* default body */ }
}

pub trait EditTitle: EditField {
    fn edit_title(&self, value: &str, locked: bool) -> impl Future<...> + Send where Self: Sync {
        self.edit_field("title", value, locked)
    }
}

// 8 field-specific traits + 10 tag-family traits all generated this way.
```

Implementors are leaf types:

```rust
impl PlexObject for Movie { /* required methods */ }
impl Ratable     for Movie {}
impl EditField   for Movie {}
impl EditTitle   for Movie {}
impl EditYear    for Movie {}
impl HasGenres   for Movie {}
// ...
```

Most `impl` blocks are empty — the trait provides a default
method body that calls into the universal primitive
(`EditField::edit_field`, `EditTags::replace_tags`). Adding a
new field-specific trait is one macro line:

```rust
declare_edit_field_trait!(EditTagline, edit_tagline, "tagline");
```

**When to apply:** Any time a capability applies to a subset of
domain types. Photos can't be rated; tracks have no `year`;
shows have no `media[]`. Separate traits encode this in the
type system instead of via runtime `unimplemented!()` panics.

---

## Stable test constructors behind `#[doc(hidden)] pub`

For test-only entry points that can't be tagged `pub(crate)`
(because integration tests under `tests/` need them), use the
`__test_*` naming convention with `#[doc(hidden)] pub`:

```rust
impl PlexServer {
    #[doc(hidden)]
    #[must_use]
    pub const fn __test_new(
        base_url: Url,
        http: HttpClient,
        identity: ServerIdentity,
    ) -> Self {
        Self { base_url, http, identity }
    }
}
```

The `__` prefix is a strong "don't use this" signal to anyone
who finds it via grep. `#[doc(hidden)]` keeps it out of the
rendered docs entirely.

**When to apply:** Sparingly. Most cases should still be
`pub(crate)` and have the test live in `#[cfg(test)] mod tests`
inside the same file. The `__test_*` pattern is for the
specific case where (a) an integration test needs the
constructor, and (b) the constructor genuinely has no production
use case (skipping the identity probe, for `PlexServer`).

---

## Crate-wide `#[non_exhaustive]`

Every public struct and enum gets `#[non_exhaustive]`:

```rust
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct WatchlistItem {
    pub guid: String,
    pub rating_key: String,
    // ...
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum AlertEvent {
    Playing(PlayingNotification),
    // ...
}
```

This means:
- Downstream code can't pattern-match the enum without a wildcard,
  even when they think they cover every variant. New variants
  in minor versions aren't breaking.
- Downstream code can't construct the struct with all-fields
  initializer syntax. Builders or factory methods are the only
  construction paths.

**When to apply:** Every public struct and enum in the crate.
The minor cost (downstream needs `..` in match patterns) is
worth the future-proofing.

---

## Macros for trait-family generation

The M3 trait architecture multiplies one primitive trait
(`EditField`, `EditTags`) into ~20 field- and tag-specific
traits. Hand-writing each would be ~50 lines apiece; a
declarative macro flattens this to one line:

```rust
#[macro_export]
macro_rules! declare_edit_field_trait {
    ($trait_name:ident, $method_name:ident, $wire_field:expr) => {
        pub trait $trait_name: $crate::traits::EditField {
            fn $method_name(
                &self,
                value: &str,
                locked: bool,
            ) -> impl ::std::future::Future<Output = $crate::error::Result<()>> + Send
            where
                Self: Sync,
            {
                self.edit_field($wire_field, value, locked)
            }
        }
    };
}

declare_edit_field_trait!(EditTagline,        edit_tagline,        "tagline");
declare_edit_field_trait!(EditStudio,         edit_studio,         "studio");
declare_edit_field_trait!(EditContentRating,  edit_content_rating, "contentRating");
declare_edit_field_trait!(EditSortTitle,      edit_sort_title,     "titleSort");
declare_edit_field_trait!(EditOriginalTitle,  edit_original_title, "originalTitle");
```

`#[macro_export]` so downstream users can declare their own
field traits matching the same shape.

**When to apply:** When you'd write the same boilerplate
three or more times. Two repetitions is fine; three is a macro.

---

## Permissive parsers, strict validators

Two parsing conventions live side by side:

**Permissive — for response bodies.** Accept anything Plex
might emit, falling back to safe defaults for missing or
unparseable fields:

```rust
let viewed_at = self.viewed_at
    .and_then(|s| DateTime::<Utc>::from_timestamp(s, 0));  // None on overflow

let kind = SettingKind::from_wire(&self.kind);  // Other(s) fallback for unknowns

let provides = self.provides
    .split(',')
    .map(|s| s.trim().to_owned())
    .filter(|s| !s.is_empty())  // tolerate stray commas
    .collect();
```

The crate never errors out on a single malformed field if the
overall response makes sense.

**Strict — for caller input.** Reject obvious mistakes early
with a clear error:

```rust
impl ClientIdentifier {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let v = value.into();
        if v.is_empty() {
            return Err(Error::Config("ClientIdentifier cannot be empty".to_owned()));
        }
        Ok(Self(v))
    }
}

impl Settings {
    fn validate(&self, id: &str, value: &SettingValue) -> Result<&Setting> {
        let setting = self.settings.get(id).ok_or_else(|| Error::NotFound { ... })?;
        // check that value variant matches declared kind...
        // check that value is in enum_values list when enum-typed...
    }
}

// Transcode URL builder rejects malformed video_resolution client-side
// rather than letting the server return a 500.
if let Some(res) = &self.video_resolution {
    if !is_valid_resolution(res) {
        return Err(Error::Config(format!("video_resolution must be WxH, got {res:?}")));
    }
}
```

**When to apply:** Be permissive when reading from Plex; be
strict when accepting input from the crate's user. The crate
should never let a wrong-shape input through to PMS and produce
a confusing 500.

---

## Wire-spelling exceptions via explicit `rename`

`#[serde(rename_all = "camelCase")]` works for most fields but
Plex has a handful of exceptions:

- `*ID` (capital `ID`): `playQueueID`, `playQueueItemID`,
  `accountID`, `deviceID`, `librarySectionID`, `itemID`,
  `parentItemID`, `sectionID`. `camelCase` would mangle these
  to `playQueueId`, etc.
- `*URI` (capital `URI`): `playQueueSourceURI`, `downloadURL`.
- Special cases: `IPv6` (PascalCase), `userID` (lowercase
  initial `u`).

For DTOs heavy with these, drop `rename_all` and use explicit
`#[serde(rename)]` per field:

```rust
#[derive(Debug, Deserialize)]
struct PlayQueueDto {
    #[serde(rename = "playQueueID")]
    play_queue_id: u64,
    #[serde(rename = "playQueueVersion", default)]
    play_queue_version: u32,
    #[serde(rename = "playQueueSourceURI", default)]
    play_queue_source_uri: Option<String>,
    // ...
}
```

For DTOs that are mostly camelCase with one or two exceptions,
keep `rename_all` and override the specific fields:

```rust
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TimelineEntry {
    #[serde(rename = "itemID", default)]
    item_id: u64,
    #[serde(rename = "parentItemID", default)]
    parent_item_id: Option<u64>,
    #[serde(default, rename = "type")]
    plex_type: Option<i32>,
    // these are normal camelCase:
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    state: i32,
}
```

The DTO unit tests catch wire-spelling bugs immediately — the
first time you see a "field missing" deser error, you know to
add a rename.

---

## Cargo features for optional transports

Three opt-in transports are gated:

```toml
[features]
default       = ["rustls"]
rustls        = []                  # marker — reqwest's rustls is on by default
native-tls    = ["reqwest/native-tls"]
discovery     = ["dep:tokio/net"]   # GDM UDP (or actually just net)
alerts        = ["dep:tokio-tungstenite"]
webhook-axum  = ["dep:axum"]
```

```rust
#[cfg(feature = "alerts")]
pub mod alerts;

#[cfg(feature = "webhook-axum")]
pub mod webhook;

#[cfg(feature = "discovery")]
pub mod discover_gdm;
```

Conditionally-compiled modules are entirely behind a `cfg` —
not just the contents but the module declaration itself. The
re-export in `lib.rs` is also gated. Tests for these modules
add `#![cfg(feature = "...")]` at the top so they're skipped
in the default-features test run.

**When to apply:** Any module that pulls in a heavyweight
dependency the typical user doesn't need. WebSocket
(`tokio-tungstenite`) and the full HTTP server stack (`axum`)
are the two existing examples.
