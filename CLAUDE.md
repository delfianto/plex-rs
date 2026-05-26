# CLAUDE.md

Guidance for Claude Code (and other agents) working in this repository.

---

## 1. Project mission

`plex-rs` is an **idiomatic, fully-async Rust 2024 client library** for the
Plex Media Server HTTP API and the `plex.tv` cloud (MyPlex) API.

The non-negotiable goals are:

1. **Feature parity (at minimum) with [python-plexapi](https://github.com/pkkid/python-plexapi)**.
   Anything the Python binding can do against a real Plex Media Server, this
   crate must be able to do — auth (token + PIN/OAuth), resource discovery,
   library browsing, search, playback control, sessions, playlists,
   collections, play queues, history, sync, media editing, hub discovery,
   webhook ingestion, server settings.
2. **Modern Rust best practice.** Rust 2024 edition. `cargo clippy
   --all-targets --all-features -- -D warnings` and `cargo fmt --check` must
   pass. No `unsafe` unless justified in code comments and reviewed.
3. **Proven correct.** Every public function, every parser, every request
   builder, every error path is covered by unit tests. The integration test
   layer drives a mocked Plex server end-to-end. CI is the source of truth.
4. **Production-grade ergonomics.** Typed errors (`thiserror`), strong
   newtypes for IDs, builder patterns for filters and searches, streaming
   pagination, retry/backoff for transient failures, timeouts everywhere.

If a change cannot satisfy all four, stop and surface the tradeoff to the
user before proceeding.

---

## 2. Reference material

The user shared a `blob:` URL for the Plex API spec; those are
browser-local and cannot be fetched from this environment. Use these
canonical public sources instead:

- **Python parity baseline:** <https://github.com/pkkid/python-plexapi> —
  treat this as the "behavioural spec". When in doubt about what an
  endpoint returns or what a feature should look like, read the
  corresponding `python-plexapi` module first.
- **Community OpenAPI:** <https://plexapi.dev/> — best machine-readable
  description of PMS endpoints. Useful for generating typed structs and
  cross-checking parameter names.
- **PMS URL commands (unofficial but authoritative):**
  <https://forums.plex.tv/t/plex-media-server-url-commands/> — covers the
  long tail of undocumented endpoints.
- **plex.tv / MyPlex auth:**
  <https://forums.plex.tv/t/authenticating-with-plex/609370> — PIN flow,
  X-Plex-Token semantics, required client headers.

Do **not** rely on training data alone for endpoint shapes — always verify
against one of the above before committing parsing logic.

---

## 3. Crate layout

This is a single library crate with optional examples. Keep the structure
flat enough that newcomers can navigate it; resist over-modularization.

```
plex-rs/
├── Cargo.toml                # workspace-style metadata; see §5
├── CLAUDE.md                 # this file
├── README.md                 # user-facing intro
├── LICENSE                   # MIT
├── deny.toml                 # cargo-deny config (licenses, advisories)
├── rust-toolchain.toml       # pinned toolchain, see §4
├── .github/workflows/ci.yml  # fmt + clippy + test + deny + doc
├── src/
│   ├── lib.rs                # crate root: re-exports + crate docs
│   ├── client.rs             # HttpClient: reqwest wrapper, auth headers, retries
│   ├── config.rs             # ClientConfig builder (timeouts, user agent, identifier)
│   ├── error.rs              # Error enum + Result alias (thiserror)
│   ├── headers.rs            # X-Plex-* header construction
│   ├── auth/
│   │   ├── mod.rs            # MyPlexAccount, token sign-in
│   │   ├── pin.rs            # PIN/OAuth flow
│   │   └── token.rs          # PlexToken newtype + redaction in Debug
│   ├── myplex/
│   │   ├── mod.rs            # MyPlexAccount: user, resources, devices, friends
│   │   ├── resources.rs      # ResourceConnection picking (local vs relay)
│   │   └── devices.rs
│   ├── server/
│   │   ├── mod.rs            # PlexServer: capabilities, identity, settings
│   │   ├── sessions.rs       # current sessions, transcode sessions
│   │   ├── history.rs
│   │   ├── system.rs         # /system/*, accounts, butler tasks
│   │   └── settings.rs       # PreferencesAdapter
│   ├── library/
│   │   ├── mod.rs            # Library, LibrarySection enum
│   │   ├── section.rs        # MovieSection, ShowSection, MusicSection, PhotoSection
│   │   ├── search.rs         # hub + search-v2 + filter builder
│   │   └── filters.rs        # FilterBuilder, FieldType, Operator
│   ├── media/
│   │   ├── mod.rs            # PlexObject trait, common Media/Part/Stream
│   │   ├── video.rs          # Movie, Show, Season, Episode, Clip
│   │   ├── audio.rs          # Artist, Album, Track
│   │   ├── photo.rs          # PhotoAlbum, Photo
│   │   └── playlist.rs       # Playlist, Collection
│   ├── playback/
│   │   ├── mod.rs            # PlayQueue
│   │   ├── client.rs         # PlexClient (remote control)
│   │   └── transcode.rs      # transcode URL builder, decision endpoint
│   ├── sync.rs
│   ├── webhook.rs            # webhook payload deser + Axum/Tower extractor (feature)
│   ├── xml.rs                # XML <-> serde wrappers, MediaContainer envelope
│   └── util/
│       ├── mod.rs
│       ├── ids.rs            # RatingKey, MachineIdentifier, ClientIdentifier newtypes
│       └── time.rs           # Plex epoch-ms <-> chrono::DateTime conversions
├── tests/                    # integration tests; one file per surface area
│   ├── auth.rs
│   ├── library_movie.rs
│   ├── library_show.rs
│   ├── library_music.rs
│   ├── search.rs
│   ├── sessions.rs
│   ├── playlist.rs
│   └── webhook.rs
├── tests/fixtures/           # captured XML/JSON responses (sanitized)
│   ├── myplex/
│   ├── server/
│   ├── library/
│   └── README.md             # how fixtures were captured + sanitization rules
├── examples/
│   ├── list_libraries.rs
│   ├── search_movie.rs
│   ├── pin_signin.rs
│   └── webhook_server.rs
└── benches/                  # criterion benches for parser hot paths
    └── parse_library.rs
```

Add new modules only when an existing one would exceed ~600 lines or mixes
unrelated concerns. Do **not** pre-create empty folders for parts of the
API you have not started — fewer placeholders, easier review.

---

## 4. Toolchain

Pin via `rust-toolchain.toml`:

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
profile = "minimal"
```

- **Edition:** `2024` (Rust 1.85+). MSRV is whatever the current stable
  pinned in `rust-toolchain.toml` is — do not promise older MSRV.
- **rustfmt:** default config, no custom `rustfmt.toml`. Keep formatting
  decisions out of code review.
- **clippy:** crate-level lints in `lib.rs`:
  ```rust
  #![deny(
      clippy::all,
      clippy::pedantic,
      clippy::nursery,
      missing_docs,
      missing_debug_implementations,
      unreachable_pub,
      rust_2024_compatibility,
  )]
  #![allow(
      clippy::module_name_repetitions, // common when re-exporting
      clippy::missing_errors_doc,      // covered by typed Error
  )]
  ```
  If you need to allow a lint locally, use `#[allow(clippy::...)]` with a
  one-line `// why:` comment. Bulk-allowing pedantic in a module is a
  smell — push back on it.
- **cargo-deny:** `cargo deny check` runs in CI. Approved licenses: MIT,
  Apache-2.0, BSD-2/3-Clause, ISC, Zlib, MPL-2.0, Unicode-DFS-2016. Reject
  GPL/AGPL transitively (Plex itself is closed-source; keep the client
  permissive).
- **cargo-audit / cargo-deny advisories:** fail CI on RUSTSEC advisories.
- **No unstable features.** Stable Rust only.

---

## 5. Dependencies

Keep the tree small and well-maintained. Default features:

```toml
[dependencies]
reqwest      = { version = "0.12", default-features = false, features = ["rustls-tls", "json", "stream", "gzip", "http2"] }
tokio        = { version = "1",    features = ["rt", "macros", "sync", "time"] }
serde        = { version = "1",    features = ["derive"] }
serde_json   = "1"
quick-xml    = { version = "0.36", features = ["serialize"] }
thiserror    = "1"
url          = "2"
chrono       = { version = "0.4", default-features = false, features = ["serde", "clock"] }
uuid         = { version = "1",   features = ["v4", "serde"] }
tracing      = "0.1"
bytes        = "1"
futures-util = "0.3"
async-trait  = "0.1"  # only if needed for object-safe traits

[features]
default          = ["rustls"]
rustls           = []                 # alias / placeholder for docs
native-tls       = ["reqwest/native-tls"]
webhook-axum     = ["dep:axum"]       # off by default
discovery        = ["dep:mdns-sd"]    # GDM/mDNS local server discovery

[dev-dependencies]
tokio        = { version = "1", features = ["full", "test-util"] }
wiremock     = "0.6"
insta        = { version = "1", features = ["yaml", "redactions"] }
pretty_assertions = "1"
rstest       = "0.21"
criterion    = { version = "0.5", features = ["html_reports"] }
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

Rules:

- **One HTTP stack.** `reqwest` only. No `hyper` directly except inside
  the `webhook-axum` feature.
- **No `openssl`.** `rustls` by default; `native-tls` opt-in.
- **No `async-trait` unless an `&dyn Trait` boundary actually requires
  it.** Rust 2024 supports native AFIT — prefer it.
- **No `lazy_static`, no `once_cell`** — use `std::sync::OnceLock` /
  `LazyLock` (stable since 1.80).
- **No blocking calls** in library code. `reqwest::blocking` is forbidden.
- Before adding a new dependency, justify it in the PR description and
  check `cargo deny check bans`. Prefer std + a small helper to pulling
  in a transitive tree.

---

## 6. API design

### 6.1 Naming
- Match Plex/python-plexapi vocabulary so users coming from python-plexapi
  feel at home: `library`, `section`, `hub`, `play_queue`, `rating_key`,
  `machine_identifier`.
- Snake_case for fields/methods (Rust); the XML serde mapping handles the
  `camelCase`/`PascalCase` Plex uses on the wire (`#[serde(rename = "...")]`
  or `#[serde(rename_all = "camelCase")]`).
- Avoid `get_` prefixes on methods. `server.sessions()` not
  `server.get_sessions()`.

### 6.2 Async
- All I/O methods are `async fn` returning `Result<T, Error>`.
- Use AFIT (`async fn` in traits) for the `PlexObject`/`Playable`/
  `Searchable` traits; gate behind `#[allow(async_fn_in_trait)]` only if
  the API is genuinely not meant to be object-safe.
- Cancellation safety: never hold a `Mutex` across `.await`.
- Stream paginated endpoints with `impl Stream<Item = Result<T>>` rather
  than collecting into a `Vec`. Provide a `.try_collect().await` example
  in docs.

### 6.3 Newtypes for IDs

Plex uses several string/int IDs that are easy to mix up. Wrap them:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RatingKey(pub u64);

#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlexToken(String);
```

`PlexToken` must have a hand-written `Debug` impl that prints
`PlexToken("***redacted***")` — never log raw tokens. Same for any
auth header constructed from it.

### 6.4 Builders
Filter/search composition uses the typestate builder pattern so that
`.execute()` is only callable once required fields are set. See
`library/filters.rs`. Don't expose `Default::default()` on builders that
have required fields.

### 6.5 Errors

Single crate-level enum:

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("HTTP transport: {0}")]
    Transport(#[from] reqwest::Error),

    #[error("plex returned {status}: {message}")]
    Api { status: http::StatusCode, message: String },

    #[error("authentication failed: {0}")]
    Auth(String),

    #[error("unauthorized — token missing or expired")]
    Unauthorized,

    #[error("not found: {0}")]
    NotFound(String),

    #[error("xml parse: {0}")]
    Xml(#[from] quick_xml::DeError),

    #[error("json parse: {0}")]
    Json(#[from] serde_json::Error),

    #[error("invalid url: {0}")]
    Url(#[from] url::ParseError),

    #[error("timeout after {0:?}")]
    Timeout(std::time::Duration),

    #[error("internal invariant violated: {0}")]
    Internal(&'static str),
}
pub type Result<T> = std::result::Result<T, Error>;
```

Map HTTP status codes to `NotFound`/`Unauthorized`/`Api` in one place
(`client.rs::map_response`). Never bubble `reqwest::Error` directly out of
a public method — wrap it.

### 6.6 Logging
Use `tracing` only. Every HTTP call goes through one instrumented method
that emits a span with `method`, `path`, `status`, `elapsed_ms`. Never log
query strings or headers that may contain `X-Plex-Token`.

---

## 7. XML vs JSON

Plex Media Server defaults to XML but accepts `Accept: application/json`
on most endpoints. Some endpoints (notably parts of MyPlex) only return
XML. Approach:

- Always send `Accept: application/json` first and parse with
  `serde_json`. This is faster and produces better diagnostics.
- For endpoints that only speak XML, parse with `quick-xml`'s serde
  support. Wrap the envelope in a generic `MediaContainer<T>`.
- Keep the parser layer **separate from the domain types**: parsers
  produce raw DTOs in `xml::dto`, which are then converted to the public
  domain structs via `From` / `TryFrom`. This lets us unit-test parsing
  without touching HTTP and lets us evolve the public API without
  breaking wire compatibility.

---

## 8. Authentication

Support all three flows python-plexapi supports:

1. **Direct token** — caller already has an `X-Plex-Token`. Cheapest path,
   used by most server-only consumers.
2. **Username + password** — `POST /api/v2/users/signin`. Will likely
   require 2FA; surface `Error::Auth("2fa_required")` and accept a
   `verification_code` on retry.
3. **PIN / OAuth flow** — `POST /api/v2/pins`, poll until claimed, then
   the response yields an auth token. Provide both a blocking-friendly
   helper (`account.pin_login().await?.wait_for_token().await?`) and a
   manual one (`poll_pin(id).await?`).

Every request — authenticated or not — must include the standard
`X-Plex-*` client identification headers (`Product`, `Version`,
`ClientIdentifier`, `Platform`, `Device`, `DeviceName`). These come from
`ClientConfig`; never hardcode them in the request layer.

`ClientIdentifier` must be **stable per install**. Provide
`ClientConfig::generated()` for ephemeral use and document that callers
running long-lived agents should persist their own UUID.

---

## 9. Testing strategy

Coverage target: **≥90 % line, ≥85 % branch** on the library crate.
Enforce with `cargo tarpaulin` (or `cargo llvm-cov`) in CI; PRs that drop
coverage by >1 % require justification.

### 9.1 Unit tests
- Live next to the code they cover (`#[cfg(test)] mod tests`).
- Pure functions, parsers, header builders, URL builders, error mapping,
  filter builder validation — all unit tested without any network or
  filesystem.

### 9.2 Parser tests
- Fixtures live under `tests/fixtures/`. Each fixture is a real captured
  response, sanitized (tokens replaced with `REDACTED_TOKEN`, IPs with
  `127.0.0.1`, machine identifiers with deterministic placeholders).
- Use `insta` for snapshot assertions on parsed structures so wire-format
  drift shows up as a reviewable diff.

### 9.3 Integration tests (`tests/`)
- Spin up a `wiremock::MockServer`, register expected requests, point the
  `PlexServer` at it, exercise the feature, assert on both the captured
  request and the returned domain object.
- Each public flow (list libraries, get movie, mark watched, create
  playlist, fetch sessions, webhook ingest, PIN login) needs at least
  one happy-path and one failure-path integration test.

### 9.4 What we explicitly do **not** do
- No tests that hit a real plex.tv or a real PMS by default. If we add an
  optional "live" test suite later, gate it behind
  `--features live-tests` and skip unless `PLEX_TEST_TOKEN` is set. CI
  never runs live tests.

### 9.5 Fixture capture
Document the capture process in `tests/fixtures/README.md`:
1. Run a real PMS locally, point `examples/dump_fixtures` at it.
2. The dumper writes raw bodies to disk, then runs a sanitizer that
   replaces tokens/IPs/identifiers per a regex table.
3. The sanitizer is itself unit-tested — fixtures must not regress.

---

## 10. Continuous integration

`.github/workflows/ci.yml` runs on every PR and push to `main`:

| Job              | Command                                                              |
| ---------------- | -------------------------------------------------------------------- |
| `fmt`            | `cargo fmt --all -- --check`                                         |
| `clippy`         | `cargo clippy --all-targets --all-features -- -D warnings`           |
| `test`           | `cargo test --all-features --workspace`                              |
| `test-no-default`| `cargo test --no-default-features`                                   |
| `doc`            | `RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --all-features`      |
| `deny`           | `cargo deny check`                                                   |
| `coverage`       | `cargo llvm-cov --all-features --fail-under-lines 90`                |
| `msrv`           | (optional) `cargo +<pinned> check`                                   |

All jobs must be green before merge. **Never** add `continue-on-error:
true` to silence a flaky job — fix the flake.

---

## 11. Documentation

- Every public item needs a `///` doc comment. `missing_docs` is `deny`.
- Every doc comment for a fallible function lists its error variants
  under an `# Errors` section.
- Every doc comment for an async function whose cancellation has
  side-effects has a `# Cancel safety` section.
- Use runnable doc examples wherever possible; gate them with
  `# tokio_test::block_on(async { ... })` so they execute under
  `cargo test --doc` without requiring a real server. Mark examples
  needing a live server as `no_run`.
- Crate-level `lib.rs` opens with a short tour: auth → server → library
  → media → playback. Mirror the structure python-plexapi uses in its
  own README so users can port snippets easily.

---

## 12. Versioning and stability

- Pre-1.0: every minor bump may contain breaking changes; document them
  in `CHANGELOG.md` (Keep a Changelog format) and tag the commit.
- Once we hit 1.0 we follow strict semver. Don't ship 1.0 until all
  parity-with-python-plexapi items in §13 are done **and** the public
  surface has been reviewed for "are we OK living with this for two
  years".

---

## 13. Feature parity checklist (parity baseline = python-plexapi)

Track progress in `docs/parity.md` (create when first item lands). The
broad surface areas, in roughly the order they should be implemented:

- [ ] Client identity headers + `ClientConfig` builder
- [ ] `HttpClient` with retry/backoff + `Accept: application/json`
- [ ] Error mapping + token redaction
- [ ] `MyPlexAccount`: token sign-in
- [ ] `MyPlexAccount`: password + 2FA sign-in
- [ ] `MyPlexAccount`: PIN/OAuth flow
- [ ] `MyPlexAccount`: resources (servers, players), connection picking
- [ ] `MyPlexAccount`: users/friends, sharing, invites
- [ ] `PlexServer`: identity, capabilities, settings (read + write)
- [ ] `PlexServer`: accounts, butler, scheduled tasks
- [ ] `Library`: list sections, refresh, scan, deletion
- [ ] `MovieSection`: all/search/recentlyAdded/onDeck/filterFields/hubs
- [ ] `ShowSection`: shows/seasons/episodes hierarchy + traversal
- [ ] `MusicSection`: artists/albums/tracks
- [ ] `PhotoSection`: albums/photos
- [ ] Universal `search()` + `searchV2` hubs
- [ ] Filter builder (advanced filters, sorts, field discovery)
- [ ] Media metadata edit (title, summary, poster upload, artwork)
- [ ] Mark watched / unwatched, set view offset, rate
- [ ] Playlists (audio, video, photo): CRUD, add/remove items, reorder
- [ ] Collections: CRUD, smart collections, mode/order
- [ ] PlayQueues: create, fetch, advance
- [ ] Sessions: current, stop, history
- [ ] Transcode session inspection + URL builder
- [ ] Clients/players: list, navigation, playback control
- [ ] Sync (legacy mobile sync) — best-effort, document gaps
- [ ] Webhook payload deser + optional Axum extractor
- [ ] mDNS/GDM discovery (feature `discovery`)

Each item lands as its own PR with: domain types + parser + builder +
unit tests + at least one integration test + doc example + checklist tick.

---

## 14. Coding etiquette in this repo

These are project-specific rules; they extend (not replace) the global
guidance Claude already has.

1. **No speculative abstractions.** If we only have one library section
   type implemented, don't introduce a `LibrarySection` trait yet. Add
   the trait when the second implementation arrives.
2. **No re-implementing what reqwest already does.** Connection pooling,
   gzip, redirects, cookie jars — configure them, don't recreate them.
3. **Parser DTOs are crate-private.** Public domain types never expose
   raw `quick_xml` or `serde_json` types in their signatures.
4. **No `.unwrap()` / `.expect()` in library code** outside of `const`
   contexts or genuinely-infallible situations with a `// SAFETY:`-style
   comment explaining why. `unwrap()` in tests is fine.
5. **No `println!` / `eprintln!` in library code.** Use `tracing`.
6. **No `tokio::spawn` in library code** unless the API explicitly
   returns a handle. Callers own their runtime.
7. **Cancellation:** assume every `.await` may be dropped. Don't leave
   the server in a half-mutated state — issue mutations as single
   requests rather than multi-step orchestrations when possible.
8. **Breaking changes get a CHANGELOG entry in the same PR.**

---

## 15. Working with this codebase as an agent

When you (Claude) pick up a task here:

1. **Read `docs/parity.md` first** (once it exists) to see what's done
   and what's next. Don't duplicate completed work.
2. **Run the full check suite before declaring done:**
   ```bash
   cargo fmt --all -- --check \
     && cargo clippy --all-targets --all-features -- -D warnings \
     && cargo test --all-features \
     && cargo doc --no-deps --all-features
   ```
   "It compiles" is not "done". "Tests pass" is not "done" unless new
   tests cover the new code.
3. **For any new endpoint:** add or extend a fixture in
   `tests/fixtures/`, write the parser DTO + domain conversion, write a
   `wiremock`-backed integration test, tick the parity checklist.
4. **Never commit a real Plex token, real machine identifier, real IP,
   or real username** to fixtures or tests. The sanitizer exists for a
   reason; run it.
5. **When in doubt about an endpoint's shape**, read python-plexapi's
   implementation of the equivalent method — its source is the most
   reliable behavioural reference we have.
6. **Don't expand scope.** If a task is "implement movie search", do not
   also refactor the error enum, rename a module, or add a benchmarking
   harness. File a follow-up note instead.
7. **If a test is flaky, fix it or quarantine it with an issue link.**
   Never `#[ignore]` without a tracked reason.

---

## 16. Out of scope

To keep the crate focused, these are explicitly **not** goals (at least
not for 1.0):

- A CLI binary. (Could be a separate crate later — `plex-rs-cli`.)
- A TUI. (Same.)
- Re-implementing Plex's transcoder or DLNA layer.
- Direct media file I/O (downloading/uploading media bytes other than
  through Plex's own endpoints).
- Synchronous (blocking) API surface. Callers who need blocking can wrap
  with `tokio::runtime::Runtime::block_on`.

If a user requests something in this list, raise it before implementing.
