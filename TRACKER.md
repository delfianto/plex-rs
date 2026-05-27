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

- [~] **2.1-2.2 `src/media/video.rs`** — **Movie, Show, Season, Episode
  landed.** Movie (24 scalar fields, `LibrarySection::movies()`); TV
  hierarchy: `LibrarySection::shows()` for type=2 listing,
  `Show::seasons()` and `Season::episodes()` via
  `GET /library/metadata/<rk>/children`. `Show::watch_progress()`,
  `Episode::season_episode_label()` convenience helpers. Shared
  parent/grandparent back-references on Season/Episode. Clip / Extra
  and `*Session` / `*History` composition types in follow-ups. _5 unit
  tests + 6 wiremock integration tests across `tests/m2_movies.rs`
  and `tests/m2_tv.rs`._
- [x] **2.3 `src/media/audio.rs`** — Artist / Album / Track using the shared
  `MetadataDto`. `LibrarySection::artists()` for `?type=8`,
  `Artist::albums()` and `Album::tracks()` via the shared
  `list_children` helper. `Track::disc_number` maps Plex's
  `parentIndex` (the per-track parent is the album; index is
  position-on-disc — `parentIndex` is the disc number, easy to
  swap). _2 wiremock integration tests covering the full walk._
- [x] **2.4 `src/media/photo.rs`** — `Photoalbum`, `Photo`, `PhotoEntry`
  sum type for mixed children. `Photoalbum::children()` dispatches on
  the wire `type` discriminator (added to `MetadataDto` as
  `metadata_type`); `sub_albums()` / `photos()` convenience filters.
  `LibrarySection::photoalbums()` for `?type=14` top-level listing.
  _1 wiremock integration test exercising the mixed-children walk._
- [x] **2.5 `src/media/streams.rs`** — Full Media → MediaPart → Stream chain.
  Stream is a sum type (`Video|Audio|Subtitle|Lyric|Unknown`) dispatched
  on Plex's `streamType` integer; `Unknown(StreamCommon)` preserves
  forward-compat for new stream kinds. Per-variant fields cover codec,
  language, dimensions, frame rate, channel layout, default/selected/
  forced flags, external-track keys. Movie / Episode / Track / Photo
  now carry `media: Vec<Media>` populated when the source endpoint
  emits it (always empty for plain listings). _2 unit tests covering
  the full chain incl. Unknown forward-compat path._
- [x] **2.6 `src/media/tags.rs`** — `Tag` + `TagKind` enum covering 10
  named families + `Other(String)` forward-compat. Movie/Show/Episode/
  Album/Track gain `tags: Vec<Tag>`. _4 unit tests._
- [x] **2.7 `src/media/markers.rs`** — `Marker { kind, start_ms, end_ms,
  final_credits }` with `MarkerKind` enum (`Intro|Credits|Commercial|Other`).
  `Chapter { id, title, index, start_ms, end_ms, thumb }`. Both attached
  as `Vec<…>` to Movie and Episode. `Marker::duration_ms()` /
  `Marker::contains(time_ms)` convenience helpers. _7 unit tests._
- [ ] **2.7 `src/xml/dto/metadata.rs`** — DTO per `<Video>` / `<Track>` / `<Photo>` / `<Directory>` shape.
- [x] **2.8 `LibrarySection::search/recently_added/on_deck/unwatched`** —
  Mixed-content listing endpoints. New `LibraryItem` sum type
  (`Movie|Show|Season|Episode|Artist|Album|Track|Photoalbum|Photo`)
  dispatches on the wire `type` discriminator. `LibraryItem::title()` /
  `rating_key()` accessors hide the variant. Hub search (universal,
  cross-section) deferred to a follow-up. _4 wiremock integration tests._
- [x] **2.9 `src/library/filters.rs`** — `FilterBuilder` with typed
  named ops mapping to Plex's wire suffixes per python-plexapi
  `library.py:1442-1460`. `FilterOp` enum (`Default | Not | Exact |
  NotExact | StartsWith | EndsWith | GreaterThan | LessThan |
  AndValues`). Sort + limit + offset + page-size + libtype.
  `LibrarySection::filter(&builder)` executes returning
  `Vec<LibraryItem>`. _16 unit tests + 2 wiremock integration tests._
- [-] Client-side `__icontains`/`__gte`/etc. namespace deferred to M3
  alongside the trait architecture per analysis/11 §7.4 (smart-filter
  round-trip is out of scope for v1).
- [ ] **2.10 Parser snapshot tests via insta** — every leaf has at least one fixture-driven snapshot test.

## M3 — Edit / tag / lock traits

Largest single trait-architecture investment.

- [x] **3.1 `src/traits/mod.rs`** — `PlexObject` supertrait (http /
  base_url / rating_key accessors). Two macros `impl_plex_object*!`
  install it on Movie/Show/Season/Episode/Artist/Album/Track.
- [~] **3.2 `src/traits/played_unplayed.rs`** — `PlayedUnplayed` with
  mark_played/mark_unplayed via GET `/:/scrobble` + `/:/unscrobble`.
  Implemented on Movie/Show/Season/Episode/Artist/Album/Track.
  `Playable` (full play-control surface) defers to M4 alongside
  PlayQueue + PlexClient. _3 wiremock integration tests._
- [x] **3.3 `src/traits/ratable.rs`** — `Ratable` with `rate(Option<f32>)`.
  PUT `/:/rate?key=<rk>&identifier=com.plexapp.plugins.library&rating=<v>`.
  Client-side range validation (0..=10), `-1` sentinel for clear.
  Impl on Movie/Show/Episode/Album/Track. _3 wiremock integration tests._
- [~] **3.4 `src/traits/edit_field.rs`** — `EditField` universal
  primitive + `FieldValue` enum + `EditTitle` / `EditSummary`
  field-specific traits. `PlexObject` extended with `section_ref()`
  and `metadata_type_id()`. The remaining ~30 field-specific
  traits follow. _3 unit tests + 3 wiremock integration tests._
- [~] **3.5 `src/traits/edit_tags.rs`** — `EditTags` (replace_tags /
  remove_tags) + `HasGenres` / `HasCollections` per-family traits.
  Remove sigil `<field>[].tag.tag-=csv` matched per analysis/08 §3.4.
  Add semantics defer to EditBatch (need read-modify-write of current
  list). _3 wiremock integration tests._
- [~] **3.6 `src/traits/images.rs`** — `HasArtUrl` + `HasArtLock`,
  `HasPosterUrl` + `HasPosterLock`, `HasThemeUrl` + `HasThemeLock`.
  URL builders + lock toggles via new `EditField::lock_field()`
  primitive. Full HasArt CRUD (upload/replace/delete) defers — needs
  multipart POST on HttpClient. `HasLogo` / `HasSquareArt` defer
  similarly. _4 wiremock integration tests._
- [ ] **3.6 `src/traits/search.rs`** — `Splittable`, `Matchable`, `Watchlistable`.
- [ ] **3.7 `src/traits/capabilities.rs`** — `capabilities!` declarative macro that emits the impl matrix from analysis/08§2.
- [ ] **3.8 `src/batch.rs`** — `EditBatch` transaction (analysis/08§3.1).
- [ ] **3.9 `src/library/filters.rs`** — client-side `__` operator namespace via `client(|q| …)` closure.
- [ ] **3.10 `src/library/smart_filter.rs`** — `push/pop/and/or` URI parser (read-only).

## M4 — Playback / playlists / collections / queues / sessions / history

- [~] **4.1 `src/media/playlist.rs`** — Playlist read surface + delete.
  PlaylistKind enum. PlexServer::playlists() + Playlist::items() +
  Playlist::delete(). Create/rename/add/remove/move items defer
  (need `?uri=` server-URI construction). _3 wiremock integration tests._
- [ ] **4.2 `src/playback/mod.rs`** — `PlayQueue` create/get/mutate.
- [~] **4.3 `src/media/collection.rs`** — Collection read surface +
  delete + full M3 edit-trait composition (Ratable / EditTitle /
  EditSummary / EditTags / HasGenres / HasCollections / HasLabels /
  art + poster URL + lock). Add/remove items, mode/sort tweaks,
  smart-collection mutation defer. _3 wiremock integration tests._
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
