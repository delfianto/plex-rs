# Implementation tracker

Progress against the M0..M5 milestone plan in
[`analysis/11-rust-mapping-recommendations.md`](analysis/11-rust-mapping-recommendations.md) §10.

**Discipline:** every step must end with the crate green under
`cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`,
`cargo test --all-features`, and `RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --all-features`.
No step is "done" until all four pass.

Legend:&nbsp; `[ ]` pending &nbsp;·&nbsp; `[~]` in progress &nbsp;·&nbsp; `[x]` done &nbsp;·&nbsp; `[-]` deferred / out of scope

---

## M0 — Foundations  ✅ DONE

Pure-Rust primitives + HTTP transport layer. **All gates green.**

**Stats:** 137 unit tests + 8 wiremock integration tests = 145 passing, 0 failing.
**Code:** ~3 400 lines across 12 modules.

- [x] **0.0 Bootstrap** — `Cargo.toml`, `rust-toolchain.toml`, `deny.toml`, CI workflow, lib.rs stub. (done in earlier session)
- [x] **0.1 `src/error.rs`** — `Error` enum + `Result` alias. Maps `reqwest::Error`, `quick_xml::DeError`, `serde_json::Error`, `url::ParseError`. Wire-level kinds: `Unauthorized`, `NotFound`, `Api { status, message }`, `Timeout`, `Auth`, `Internal`. From analysis/02§6. _11 tests._
- [x] **0.2 `src/util/ids.rs`** — `RatingKey(u64)`, `MachineIdentifier(String)`, `ClientIdentifier(String)`, `PlexToken(String)`. All `#[serde(transparent)]`. `PlexToken` has a hand-written `Debug` that prints `PlexToken("***redacted***")`. From CLAUDE.md §6.3. _18 tests._
- [x] **0.3 `src/util/time.rs`** — Plex epoch-secs ↔ `DateTime<Utc>` and ISO-date helpers + serde adapter for the `addedAt`-style string-encoded fields. _9 tests._
- [x] **0.4 `src/util/search_type.rs`** — `SearchType` enum mirroring `python-plexapi/plexapi/utils.py:35`. `Unknown(u32)` forward-compat. Serde via `from`/`into u32`. _8 tests._
- [x] **0.5 `src/util/sanitize.rs`** — fixture sanitiser, 14 rules + IPv4/IPv6 dispatched in code (the `regex` crate lacks look-around). Idempotent. _19 tests._
- [x] **0.6 `src/util/mod.rs`** — re-exports the four util modules.
- [x] **0.7 `src/uri.rs`** — `PlexUri` enum with parser + `Display`. 7 schemes from analysis/07§8: `Server`, `LibraryItem`, `LibraryDirectory`, `Playlist`, `PlayQueueContainer`, `Device`, `SecurityToken`. Hand-written prefix parser (no `winnow`/`nom` dep). Round-trip-stable. _23 tests._
- [x] **0.8 `src/xml/mod.rs`** — `MediaContainer<T>` generic envelope + `MediaContainerMeta`. JSON-first parser dispatching on caller-supplied items-list field name. _13 tests._
- [x] **0.9 `src/pagination.rs`** — `PageRange { start, size }` + `advance_with()` helper. Header-based via `X-Plex-Container-Start` / `-Size`. Streaming iterator deferred to M0.12 once `HttpClient` lands. _10 tests._
- [x] **0.10 `src/headers.rs`** — `PlexIdentity` builder emitting 10 `X-Plex-*` headers + `Accept: application/json`. Strict ASCII validation on values (rejects non-ASCII and control chars). _7 tests._
- [x] **0.11 `src/config.rs`** — `ClientConfig` + `ClientConfigBuilder`. Required identity + optional token, request/connect timeouts, retry tuple (count + base/max delay). Invariants enforced at `build()`. _10 tests._
- [x] **0.12 `src/client.rs`** — `HttpClient`: `reqwest::Client` wrapper. Default `Accept: application/json` via identity headers. Retry/backoff with full-jitter exponential (`retry_delay`, pure & unit-tested). `Debug` redacts the token. Status-to-`Error` mapping centralised in one place. _9 unit tests._
- [x] **0.13 Wire-up** — re-exports in `lib.rs`. `tests/m0_http_smoke.rs` proves the foundation end-to-end against `wiremock`: identity headers reach the wire, JSON deserialisation works, 401/404/api-error mapping all green, retries succeed on transient 5xx and give up cleanly on permanent ones. _8 wiremock integration tests._

## M1 — Minimum viable client  ✅ DONE (token sign-in slice)

First wire I/O. Token sign-in only (PIN/password defer to M5). Single
vertical slice: `PlexServer::connect → identity → library().sections()`.

- [-] **1.1-1.2 auth/** — deferred. Synthesis typestate was over-engineered
  for a single sign-in flow; the M0 `ClientConfig::token()` setter already
  satisfies M1's needs. The typestate machine lands in M5 alongside PIN
  and password+2FA per [analysis/11 §9].
- [-] **1.3 `myplex/`** — deferred to M5 (requires plex.tv `/api/v2/user`
  which is in M5's MyPlex slice).
- [x] **1.4-1.5 `src/server.rs`** — `PlexServer::connect(url, token)`,
  `connect_with_config()`, `from_http()` for tests. `GET /` populates
  `ServerIdentity` (`machine_identifier`, `version`, `friendly_name`,
  platform, MyPlex linkage flags, capabilities). `ping()` hits
  `/identity` as a lightweight reachability probe. _4 unit tests._
- [x] **1.6-1.7 `src/library.rs`** — `Library::sections()`. `SectionKind`
  enum `Movie | Show | Music | Photo | Other(String)` for forward-compat.
  `LibrarySection` carries the descriptive fields + a `LibrarySectionRef`
  back-link that future edit traits will use to build
  `PUT /library/sections/<id>/all?...` mutation URLs (analysis/11 §2.4).
  _5 unit tests._
- [x] **1.8 DTO** — `DirectoryDto` (in `library.rs`) parses the
  `<Directory>` shape Plex returns. Shared `PlexBool` flexible-boolean
  deserialiser exposed `pub(crate)` from `server.rs` so it doesn't
  duplicate across modules. The synthesis's `src/xml/dto/` directory
  layout will materialise in M2 when there are enough DTOs to warrant
  the split.
- [x] **1.9 `tests/m1_server_library.rs`** — 5 end-to-end wiremock tests:
  parses root identity with mixed-form booleans, 401 → `Unauthorized`,
  lists 4 sections (movie/show/music/podcast=Other), `LibrarySectionRef`
  builds the edit URL correctly, ping hits `/identity`.

**Stats:** 148 unit tests + 13 wiremock integration tests = 161 passing.

## M2 — Read-only media domain

Read parity with python-plexapi. DTO + From conversion for every leaf.

- [~] **2.1 `src/media/video.rs`** — **Movie landed** (24 scalar fields,
  DTO parsing, `LibrarySection::movies()` listing). Show / Season / Episode
  / Clip / Extra and `*Session` / `*History` composition types in
  follow-ups. _5 unit tests + 2 wiremock integration tests._
- [ ] **2.2 `src/media/audio.rs`** — `Artist`, `Album`, `Track`.
- [ ] **2.3 `src/media/photo.rs`** — `Photoalbum`, `Photo`.
- [ ] **2.4 `src/media/media_stream.rs`** — `Media`, `MediaPart`, `Stream` enum (`Video | Audio | Subtitle | Lyric`).
- [ ] **2.5 `src/media/tags.rs`** — `Tag { kind, value, … }`, 14-variant `TagKind` collapsed per analysis/06§F.
- [ ] **2.6 `src/media/markers.rs`** — `Marker { kind, … }`, `Chapter`.
- [ ] **2.7 `src/xml/dto/metadata.rs`** — DTO per `<Video>` / `<Track>` / `<Photo>` / `<Directory>` shape.
- [ ] **2.8 `src/library/search.rs`** — `LibrarySection::all()`, `::search(title)`, `::recently_added()`, `::on_deck()`, hub search.
- [ ] **2.9 `src/library/filters.rs`** — `FilterBuilder` (analysis/11§7), server-side namespace only.
- [ ] **2.10 Parser snapshot tests via insta** — every leaf has at least one fixture-driven snapshot test.

## M3 — Edit / tag / lock traits

Largest single trait-architecture investment.

- [ ] **3.1 `src/traits/mod.rs`** — `PlexObject`, `Reload`.
- [ ] **3.2 `src/traits/playable.rs`** — `Playable`, `PlayedUnplayed`.
- [ ] **3.3 `src/traits/ratable.rs`** — `Ratable`.
- [ ] **3.4 `src/traits/editable.rs`** — `EditField`, `EditTags`, field-specific traits (`EditTitle`, `EditSummary`, …).
- [ ] **3.5 `src/traits/images.rs`** — `HasArtUrl`, `HasArt`, `HasPoster*`, `HasTheme`, `HasLogo`, `HasSquareArt`.
- [ ] **3.6 `src/traits/search.rs`** — `Splittable`, `Matchable`, `Watchlistable`.
- [ ] **3.7 `src/traits/capabilities.rs`** — `capabilities!` declarative macro that emits the impl matrix from analysis/08§2.
- [ ] **3.8 `src/batch.rs`** — `EditBatch` transaction (analysis/08§3.1).
- [ ] **3.9 `src/library/filters.rs`** — client-side `__` operator namespace via `client(|q| …)` closure.
- [ ] **3.10 `src/library/smart_filter.rs`** — `push/pop/and/or` URI parser (read-only).

## M4 — Playback / playlists / collections / queues / sessions / history

- [ ] **4.1 `src/playback/mod.rs`** — `PlayQueue` create/get/mutate.
- [ ] **4.2 `src/media/playlist.rs`** — regular + smart + M3U creation, mutations.
- [ ] **4.3 `src/media/collection.rs`** — regular + smart, mutations, `ManagedHub` visibility (read).
- [ ] **4.4 `src/playback/client.rs`** — `PlexClient`, command protocol (14 nav + 19 playback + mirror).
- [ ] **4.5 `src/playback/transcode.rs`** — `/transcode/universal` URL builder + decision endpoint.
- [ ] **4.6 `src/server/sessions.rs`** — `sessions()`, `transcode_sessions()`, `PlexSession::stop()`.
- [ ] **4.7 `src/server/history.rs`** — `history()` with operator-suffix DSL.
- [ ] **4.8 `src/server/settings.rs`** — `Settings` + `Setting`, two-phase commit via staging slot.
- [ ] **4.9 `src/server/{butler,activities,updater,statistics,transcode,browse}.rs`** — long tail.

## M5 — Real-time / discovery / cloud catalogue / webhooks

- [ ] **5.1 `src/auth/pin.rs`** — PIN flow typestate (analysis/11§9).
- [ ] **5.2 `src/auth/password.rs`** — password + 2FA state machine.
- [ ] **5.3 `src/myplex/resources.rs`** — `MyPlexResource`, parallel connect race with TLS error surfacing.
- [ ] **5.4 `src/myplex/{devices,friends,home,webhooks,claim,sonos}.rs`** — long tail.
- [ ] **5.5 `src/discover/`** — watchlist + JSON Discover search + availability.
- [ ] **5.6 `src/metadata_provider/`** — userState + GET scrobble.
- [ ] **5.7 `src/alerts/`** — WebSocket stream + reconnect with backoff + typed events (analysis/11§8).
- [ ] **5.8 `src/discover_gdm/`** — raw-UDP GDM scan.
- [ ] **5.9 `src/webhook/`** — payload deser + Axum extractor (feature-gated).
- [ ] **5.10 `src/playback/sync.rs`** — legacy mobile sync (best-effort).

---

## Per-step checklist

For each item above, the green-bar definition:

1. `cargo fmt --all -- --check` → clean
2. `cargo clippy --all-targets --all-features -- -D warnings` → clean
3. `cargo clippy --all-targets --no-default-features -- -D warnings` → clean
4. `cargo test --all-features` → all pass (new tests included)
5. `RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --all-features` → clean
6. Public items have `///` docs
7. Tests cover happy path + at least one error path
8. `CHANGELOG.md` updated under `[Unreleased]`
9. This file: tick the checkbox

If any check fails, the step stays `[~]` and we iterate.
