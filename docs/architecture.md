# Architecture

This document covers the crate-wide design decisions that show up
across multiple modules. Per-module details live in
[`modules.md`](./modules.md); concrete idioms in
[`design-patterns.md`](./design-patterns.md).

---

## Async-first, single I/O surface

Every method that talks to a network endpoint is `async fn` and
funnels through one type: `HttpClient` (`src/client.rs`). There
is no synchronous variant, no blocking façade, and no parallel
"low-level" / "high-level" stacks. `reqwest::blocking` is
explicitly forbidden (CLAUDE.md §14).

The rationale:

- Plex APIs are network-bound. There is no scenario where blocking
  is genuinely faster than async.
- `tokio::runtime::Runtime::block_on` wraps any of our methods for
  callers who really do want a sync façade. The crate doesn't owe
  them a second API.
- Centralising I/O in `HttpClient` lets us apply identity headers,
  `Accept: application/json` negotiation, status-to-`Error`
  mapping, and retry/backoff exactly once.

`HttpClient` is `Clone` and internally `Arc`-backed (via
`reqwest::Client`), so every sub-type (`PlexServer`, `Library`,
`MyPlexClient`, `PlayQueue`, …) carries its own handle without
lifetime gymnastics.

### Five HTTP primitives + two WebSocket / UDP escapes

The HTTP layer exposes a small set of methods:

| Method | What it does |
|---|---|
| `get_json::<T>(url)` | GET, deserialize JSON response as `T` |
| `get_bytes(url)` | GET, return raw response bytes |
| `get_json_with_headers::<T>(url, headers)` | GET with per-request headers (pagination) |
| `get_bytes_with_headers(url, headers)` | GET with per-request headers, raw bytes |
| `put_no_body(url)` | PUT with empty body (used by edit endpoints) |
| `delete(url)` | DELETE with no body |
| `post_json::<B, T>(url, body)` | POST JSON body, deserialize response |
| `get_bytes_for_method(method, url)` | crate-private method-parametric primitive (for PlayQueue mutations) |
| `inner()` | crate-private escape hatch — used by the password sign-in flow to drive its own form-POST envelope |

The two non-HTTP transports each live in their own feature-gated
module and bypass `HttpClient` entirely:

- **WebSocket alerts** (`src/alerts/`) uses `tokio-tungstenite`
  directly. The alerts endpoint authenticates via a query-string
  token, not the X-Plex headers `HttpClient` would inject.
- **UDP discovery** (`src/discover_gdm/`) uses raw
  `tokio::net::UdpSocket` for the GDM multicast protocol —
  HTTP/1.0 `M-SEARCH` over multicast. Not mDNS.

---

## The six endpoint families

A surprising amount of crate complexity comes from the fact that
Plex talks to **six different base URLs**:

| Base | Purpose | Module |
|---|---|---|
| `http(s)://<pms-host>:32400` | The Plex Media Server itself | `PlexServer` |
| `http://<player-host>:32500` | A player device for remote control | `PlexClient` |
| `https://plex.tv` | Account management — sign-in, devices, friends, home, webhooks | `MyPlexClient` |
| `https://discover.provider.plex.tv` | Watchlist + Discover catalogue search | `MyPlexClient::discover_base()` |
| `https://metadata.provider.plex.tv` | Per-user cloud-catalogue state + scrobble | `MyPlexClient::metadata_base()` |
| `wss://<pms-host>:32400` | WebSocket alerts | `alerts::Alerts` |

Each gets its own typed handle. `MyPlexClient::with_discover_base()` /
`with_metadata_base()` overrides exist so integration tests can
point at wiremock replicas.

---

## Type strategy

### Newtypes for every ID

Plex emits multiple kinds of identifiers that are easy to mix up.
Each gets a wrapper struct:

| Newtype | Wire format | Why |
|---|---|---|
| `RatingKey(u64)` | numeric | per-PMS metadata id |
| `MachineIdentifier(String)` | 40-char hex | per-server stable id |
| `ClientIdentifier(String)` | UUID-ish | per-installation stable id |
| `PlayQueueId(u64)` | numeric | per-queue id |
| `PlexToken(String)` | opaque secret | auth credential — **redacted in Debug** |

Each is `#[serde(transparent)]` so the wire format stays clean
while the Rust type system catches "I passed a machine id where I
meant a rating key" mistakes at compile time.

`PlexToken` is special: its hand-written `Debug` impl prints
`PlexToken("***redacted***")` so tokens never leak into log
output. Every struct that carries one composes the redaction
automatically.

### DTO + Domain split

For each Plex JSON / XML shape that produces a domain object,
there are two types:

- **DTO** (`pub(crate)` or `pub` doc-hidden) — `#[derive(Deserialize)]`
  struct with `#[serde(rename)]` attributes matching Plex's exact
  wire field names. Lives next to the domain type or in
  `src/xml/` for shared envelopes.
- **Domain** (`pub`, `#[non_exhaustive]`) — the public type with
  meaningful field names, properly typed values (`u64` instead of
  `String` for IDs, `DateTime<Utc>` instead of epoch strings),
  and convenience methods.

Conversion is via `From<Dto> for Domain` or
`impl Dto { fn into_domain(self) -> Result<Domain> }`. This is
where validation lives — a `MachineIdentifier::new(raw)` call
that returns `Error::Config` for the empty case, for instance.

The split lets us evolve the public API without breaking wire
parsing, and vice versa.

### Forward-compatible enums

Every enum that mirrors a Plex string discriminator carries an
`Unknown(String)` variant:

```rust
pub enum AlertEvent {
    Playing(PlayingNotification),
    Timeline(TimelineEntry),
    // ...known variants...
    Unknown { kind: String, raw: serde_json::Value },
}
```

`SearchType`, `WebhookEvent`, `AlertEvent`, `SectionKind`,
`PlaylistKind`, `SettingKind`, `MarkerKind`, `TagKind` all use
this pattern. New Plex versions can introduce types we don't
know about and callers still get a useful (if untyped) value.

For payload-bearing enums (alerts, webhooks), the `Unknown`
variant preserves the full raw JSON so callers can still extract
fields the crate doesn't model.

---

## Error model

One crate-wide `Error` enum with `thiserror::Error` and `#[from]`
conversions:

```rust
#[non_exhaustive]
pub enum Error {
    Transport(reqwest::Error),
    Api { status: StatusCode, message: String },
    Unauthorized,
    Forbidden(String),
    NotFound { resource: String },
    Auth(String),
    TwoFactorRequired,
    Timeout(Duration),
    Xml(quick_xml::DeError),
    Json(serde_json::Error),
    Url(url::ParseError),
    InvalidHeader(String),
    Config(String),
    Internal(&'static str),
}
```

Key choices:

- **HTTP statuses get mapped at one place.** `Error::from_status`
  in `client.rs` is the only function that decides which variant
  a non-2xx response becomes. Centralisation means `Unauthorized`
  vs `NotFound` vs `Api { status, message }` is consistent across
  modules.
- **`TwoFactorRequired` is distinct from `Unauthorized`.** Both
  are 401 on the wire, but the password sign-in path inspects the
  response body to disambiguate. Callers can pattern-match on the
  Rust enum to decide whether to retry with an OTP.
- **`#[non_exhaustive]`.** New variants can land in minor versions
  without breaking exhaustive matches downstream.
- **No raw `reqwest::Error` in public method signatures.**
  Everything wraps. `Error::Transport` carries the underlying
  reqwest error for callers that want to drill in.
- **`Error::is_retryable()`** — pure classifier used by the retry
  envelope. `Timeout`, transient transports, 5xx, 408, 425, 429
  are retryable; everything else isn't.

---

## Retry envelope

The HTTP retry loop is **full-jitter exponential backoff**:

```
delay = uniform_random([0, min(max, base * 2^(attempt-1))])
```

- Pure math: `retry_delay()` in `client.rs` is a free function
  with no I/O, unit-tested in isolation.
- Only retryable kinds (per `Error::is_retryable`) trigger a
  retry. A 404 never retries; a 503 does.
- Caps and base delay come from `ClientConfig`. Defaults: 3
  retries, 250ms base, 30s cap.
- Jitter source is a cheap thread-local PCG. Not cryptographic
  but sufficient to disperse retries across a fleet.

This is one place where the crate goes beyond python-plexapi,
which has no retry layer.

---

## Trait architecture (M3)

The metadata-edit surface uses **extension traits per capability**
rather than one monolithic interface. Decision documented in
`analysis/11` §5; outcome:

```
PlexObject                              ← supertrait (rating_key, type_id, section_ref)
├── PlayedUnplayed                      ← /:/scrobble, /:/unscrobble
├── Ratable                             ← /:/rate
├── Reload                              ← reload the item
├── Playable                            ← direct_play_url()
├── EditField                           ← one field edit primitive
│   ├── EditTitle / EditSummary / EditYear / EditTagline / …
│   └── HasArtLock / HasPosterLock / HasThemeLock
├── EditTags                            ← tag-list edit primitive
│   ├── HasGenres / HasCollections / HasLabels / HasDirectors / …
│   └── (10 tag-family traits)
├── HasArtUrl / HasPosterUrl / HasThemeUrl
└── EditBatch / EditBatchExt           ← multi-op single-PUT (built on EditField + EditTags)
```

Each domain leaf (Movie, Show, Episode, …) `impl`s exactly the
capability traits that apply to it. Plex doesn't let you rate a
photo, so `Photo` doesn't `impl Ratable`. The type system enforces
the matrix.

The per-family edit traits (`EditTitle`, `HasGenres`, etc.) all
have **default-method bodies** that delegate to the underlying
primitive (`EditField::edit_field("title", …)` /
`EditTags::replace_tags("genre", …)`). Implementors just write
`impl EditTitle for Movie {}` with no body. Two declarative
macros (`declare_edit_field_trait!`, `declare_tag_trait!`)
generate the trait + default body in one line each.

`EditBatch` builds on the same wire format but accumulates
operations and flushes them in one PUT. It composes via the
generic supertrait — any type that `impl`s `EditField` gets
`.batch()` for free.

---

## Pagination

Plex paginates listing responses via **request headers**, not
query parameters:

| Header | Meaning |
|---|---|
| `X-Plex-Container-Start` | zero-based offset of the first item |
| `X-Plex-Container-Size` | max items the server may return |

The response echoes these back in the `MediaContainer` envelope
as `offset` and `size`, plus `totalSize` for the full count.

`PageRange { start, size }` (`src/pagination.rs`) captures the
request-side window. `PageRange::advance_with(&meta)` decides
whether more pages remain, given the response metadata.

The History endpoint (`PlexServer::history()`) is the first
real consumer and exposes both `.collect().await` (eager `Vec`)
and `.stream()` (lazy `futures::Stream`) shapes. The stream
honors `.max_results` across page boundaries and drops in-flight
fetches cleanly on cancellation.

---

## XML where forced, JSON elsewhere

Plex Media Server defaults to XML but accepts
`Accept: application/json` on almost every endpoint. The crate's
`HttpClient` always sends `Accept: application/json` by default.

A small handful of endpoints are XML-only:

- `/devices.xml` on plex.tv — the device-registry endpoint
- `/api/users/` on plex.tv — friends list
- `/api/home/users` on plex.tv — Home users

For those, the module uses `quick-xml`'s serde adapter with
`@attribute`-style renames. The error variant `Error::Xml` wraps
`quick_xml::DeError` so callers see a uniform error type.

The two parsers (JSON + XML) live in dedicated DTO sections; the
domain types are oblivious to which wire format they came from.

---

## Concurrency and cancellation

The crate is designed to be cancellation-safe. Specifically:

- No `Mutex` is ever held across `.await`. (CLAUDE.md §6.2.)
- Dropping any future cancels the underlying request cleanly —
  `reqwest`'s cancellation propagates through TLS.
- `MyPlexResource::connect()` races concurrent probes with
  `FuturesUnordered`. The first success wins; the losers are
  dropped, which cancels their in-flight TCP/TLS handshakes.
- `Alerts` (the WebSocket stream) drops in-flight frames on
  drop; the `Stream` impl is poll-cancel safe.
- `PlexClient` (player remote control) uses an internal
  `Arc<AtomicU64>` for command-ID sequencing so concurrent
  cloned handles don't trample each other's command ordering.

The crate does **not** spawn tasks on the caller's behalf.
Anywhere there's parallelism, it's bounded by an explicit
`FuturesUnordered` driven by the caller's polling. (CLAUDE.md §6.2
forbids `tokio::spawn` in library code.)

---

## What lives in `lib.rs` vs `crate::*`

The crate root re-exports everything a typical caller needs:

```rust
use plex_rs::{
    PlexServer, MyPlexClient, MyPlexPinLogin, MyPlexPasswordLogin,
    Library, LibraryItem, Movie, Episode, Track, Photo,
    PlayQueue, PlexClient, MediaType,
    Alerts, AlertEvent,
    WebhookPayload, WebhookEvent,
    // ...
};
```

The module path (`plex_rs::media::video::Movie`) still works for
people who prefer explicit namespacing. The shallow re-exports
are for the common case.

Doc-hidden symbols (`PlexServer::__test_new`, etc.) are
`pub` for test-crate use but `#[doc(hidden)]` so they don't
pollute the rendered docs.

---

## Lint baseline

The crate compiles under a strict lint set:

```rust
#![forbid(unsafe_code)]
#![deny(clippy::all, clippy::correctness, clippy::suspicious,
        clippy::perf, clippy::style, missing_docs,
        missing_debug_implementations, unreachable_pub,
        rust_2024_compatibility)]
#![warn(clippy::pedantic, clippy::nursery, clippy::cargo)]
```

Every public item has `///` docs. `missing_docs` is `deny`. The
practical effect is: when you add a new public function, you must
document it, or `cargo doc --no-deps -- -D warnings` (one of the
four CI gates) fails.

A handful of clippy lints are crate-globally `allow`d, with
rationale at the allow site. Per-module allowances are rare and
always commented.

---

## What the crate is *not*

- **Not a CLI.** That'd be a separate crate.
- **Not a TUI.** Same.
- **Not a media transcoder reimplementation.** It builds Plex's
  transcoder URL; it doesn't decode video.
- **Not a synchronous API.** Wrap with `tokio::runtime::block_on`
  if you need it.
- **Not feature-complete for python-plexapi parity.** Six items
  formally out-of-scope; see [`out-of-scope.md`](./out-of-scope.md).
