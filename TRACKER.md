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
- [x] **`src/traits/reload.rs`** — `Reload` trait with
  `async fn reload(self) -> Result<Self::Full>`. Impl on Movie /
  Show / Season / Episode / Artist / Album / Track. Re-fetches via
  `GET /library/metadata/<rk>` and re-runs the appropriate `into_*`
  conversion. _2 wiremock integration tests._
- [x] **`src/traits/playable.rs`** — `Playable::direct_play_url()`
  returns a token-bearing URL pointing at the first part's wire key,
  ready to hand to an external media player. Impl on Movie / Episode
  / Track. Transcoded-stream URL builder defers. _1 wiremock
  integration test._
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
- [-] **3.7 `src/traits/capabilities.rs`** — `capabilities!` declarative
  macro. **OUT OF SCOPE.** This was a proposed *internal* refactor:
  a meta-macro that emits the impl matrix from analysis/08§2 in one
  place. Zero direct user-facing value — the existing
  `declare_edit_field_trait!` and `declare_tag_trait!` macros already
  handle the per-trait expansion without duplication. Adding a
  third macro layer would obscure the call sites with no payoff.
- [x] **3.8 `src/traits/edit_batch.rs`** — `EditBatch` collects
  field edits, lock toggles, tag replaces, and tag removes for one
  item and flushes them as a single PUT. `EditBatchExt` adds
  `.batch()` to every type implementing `EditField`. Builder API:
  low-level `.set_field/lock_field/replace_tags/remove_tags` plus
  ergonomic shortcuts (`set_title`, `set_year`, `replace_genres`,
  `replace_directors`, etc.) mirroring the per-trait method names.
  Empty batch short-circuits without an HTTP call. Wire format
  combines `EditField` and `EditTags` patterns into one query
  string with shared `id=<rk>&type=<n>` prefix. _11 unit tests +
  4 wiremock integration tests._
- [-] **3.9 `src/library/filters.rs`** — client-side `__` operator
  namespace. **OUT OF SCOPE.** This would duplicate the typed
  `FilterBuilder` API with python-style `genre__exact="Sci-Fi"`
  string sugar. Only useful as a porting aid for users translating
  python-plexapi scripts line-by-line — Rust callers reach for the
  typed builder. Keeping one canonical filter surface.
- [x] **3.10 `src/library/smart_filter.rs`** — read-only parser for
  Plex smart-playlist / smart-collection filter URIs.
  `SmartFilter::from_uri(s)` accepts a bare query string, a bare
  `/library/sections/<sid>/all?...` path, a full `library://` URI
  with the inner path percent-encoded under `/directory/`, or any
  other absolute URL. Produces typed breakdown: section_id, libtype,
  ordered FilterClause list (field + FilterOp + values), group
  markers (Push/Pop/And/Or), combined sort, and `extra` for
  unrecognised keys. `split_pair` handles the boundary `=` correctly
  for compound operators (`==`, `!==`, `>>=`, `<<=`, `<=`, `>=`,
  `!=`, `&=`). Permissive percent decoder handles both `+`-as-space
  and `%HH` escapes. _20 unit tests._

## M4 — Playback / playlists / collections / queues / sessions / history

- [~] **4.1 `src/media/playlist.rs`** — Playlist read surface + delete.
  PlaylistKind enum. PlexServer::playlists() + Playlist::items() +
  Playlist::delete(). Create/rename/add/remove/move items defer
  (need `?uri=` server-URI construction). _3 wiremock integration tests._
- [x] **4.2/4.4 `src/playback/play_queue.rs`** — `PlayQueue` and
  `PlayQueueItem`. `PlexServer::create_play_queue()` returns a
  `CreatePlayQueue` builder with `.from_item(&item)`, `.from_items(&[&item])`,
  `.from_playlist(&pl)`, plus toggles for shuffle, repeat, continuous,
  includeChapters, includeRelated, and start_at(key). `PlexServer::play_queue(id)`
  fetches an existing queue. Queue mutation methods are self-consuming
  and return refreshed snapshots: `refresh()`, `add_item(item, play_next)`,
  `move_item(item_id, after_id)`, `remove_item(item_id)`, `clear()`.
  URI construction: `server://<MID>/com.plexapp.plugins.library<key>` for
  single items, `library:///directory/<pct(/library/metadata/RK1,RK2,...)>`
  for lists, `playlistID=<rk>` for playlists. Wire-spelling exceptions
  (`playQueueID`, `playQueueItemID`, `playQueueSourceURI`) are handled
  via explicit `#[serde(rename)]` rather than camelCase auto-conversion.
  `HttpClient::get_bytes_for_method` is the new method-parametric
  primitive supporting the PUT/DELETE mutation paths.
  _7 unit tests + 9 wiremock integration tests._
- [~] **4.3 `src/media/collection.rs`** — Collection read surface +
  delete + full M3 edit-trait composition (Ratable / EditTitle /
  EditSummary / EditTags / HasGenres / HasCollections / HasLabels /
  art + poster URL + lock). Add/remove items, mode/sort tweaks,
  smart-collection mutation defer. _3 wiremock integration tests._
- [x] **4.4/4.5 `src/playback/client.rs`** — `PlexClient` remote
  control. `PlexClient::connect(base_url, access_token,
  machine_identifier, client_identifier)` builds a handle pointing
  at a player's HTTP endpoint (typically port 32500). Commands hit
  `/player/{controller}/{cmd}` with `X-Plex-Target-Client-Identifier`
  header + monotonic `commandID` query param (sequenced via
  `Arc<AtomicU64>` so cloned handles serialise correctly).
  Navigation: `move_up/_down/_left/_right`, `select`, `back`,
  `context_menu`, `go_to_home`, `go_to_music`, `page_up/_down`.
  Playback: `play`, `pause`, `stop`, `skip_next`, `skip_previous`,
  `seek_to(ms, mtype)`, `step_forward`, `step_back`,
  `set_volume(0..=100, mtype)`, `set_repeat(mode, mtype)`,
  `set_shuffle(bool, mtype)`. `MediaType` enum (Video/Music/Photo)
  and `RepeatMode` enum (Off/One/All) with `.as_wire()` accessors.
  Flagship: `play_media(&server, &queue, offset_ms)` composes
  `providerIdentifier`/`machineIdentifier`/`protocol`/`address`/
  `port`/`offset`/`key`/`type`/`containerKey`/`token` — derives
  every value from the supplied `PlexServer` + `PlayQueue` so
  callers don't have to hand-build the payload.
  _4 unit tests + 10 wiremock integration tests (including the
  full play_media composition verified against a separate PMS
  mock)._
- [x] **4.6 `src/playback/transcode.rs`** — universal transcoder URL
  builder. `TranscodeOptions::new().protocol(Hls).max_video_bitrate(8000)
  .video_resolution("1920x1080").build_for(&server, item_key)` produces
  a token-bearing `/video|/audio/:/transcode/universal/start.{m3u8|mpd}`
  URL. Supports HLS / DASH, video/audio stream kinds, fast seek, copy
  timestamps, force-transcode / force-re-encode, max video bitrate
  (clamped ≥ 64), video resolution (WxH validated client-side), quality
  / subtitle-size / audio-boost (clamped to 100 / 200 / 200), LAN/WAN
  location hint, platform override, session id override. PMS token
  forwarded in the URL via `X-Plex-Token`. Decision endpoint deferred —
  external players don't need it; they just hit the manifest URL.
  _15 unit tests._ New `PlexServer::__test_new` doc-hidden constructor
  bypasses the identity probe for test-only contexts.
- [~] **4.6 `src/server/sessions.rs`** — `PlayingSession` + `SessionUser` +
  `SessionPlayer` + `TranscodeSession` + `PlayState` enum.
  `PlexServer::sessions()` + `PlayingSession::stop(reason)`. Transcode-only
  listing and history endpoint defer. _3 wiremock integration tests._
- [x] **4.7 `src/server/history.rs`** — `PlexServer::history()` returns
  a `HistoryQuery` builder with `.account(id)`, `.library_section(id)`,
  `.rating_key(rk)`, `.mindate(dt)`, `.max_results(n)`, `.page_size(n)`.
  Default sort `viewedAt:desc` (matches python-plexapi). Terminate with
  `.collect().await` (eager `Vec`) or `.stream()` (lazy
  `futures::Stream<Item=Result<HistoryEntry>>`). Pagination via the
  `X-Plex-Container-Start/-Size` request headers through the new
  `HttpClient::get_bytes_with_headers` primitive; `PageRange::advance_with`
  drives the loop. `HistoryEntry` carries `account_id`, `device_id`,
  `history_key`, `viewed_at`, and the full `LibraryItem` (via
  `MetadataDto::into_library_item`). `HistoryEntry::delete(http, base)`
  hits `DELETE <history_key>`. _9 unit tests + 6 wiremock integration
  tests (incl. cross-page pagination with header assertions, max_results
  cap, streaming, DELETE)._
- [x] **4.8 `src/server/settings.rs`** — `Settings` + `Setting`.
  `PlexServer::settings()` returns a fully-loaded `Settings` snapshot
  via `GET /:/prefs`. `Setting` carries id, label, summary, kind
  (Text/Int/Double/Bool/Enum + Other for forward-compat),
  default and current `SettingValue`, hidden/advanced/secure flags,
  group name, and (for enum settings) `enum_values` as either a
  flat `List` or a `Mapping` of `key:label` pairs. Mutation:
  `set(server, id, value).await` writes one preference and reloads;
  `set_many(server, updates).await` batches into one PUT. Client-side
  validation rejects unknown ids, wrong-kind values, and
  out-of-enum choices before issuing a network call. Two-phase
  staging commit (python-plexapi's `_setValue` pattern) deferred
  in favor of explicit single-write or explicit batch — composes
  more cleanly with Rust's `await`-based ergonomics.
  `Settings::all()`, `get(id)`, `group(name)`, `group_names()`
  cover the read surface. _12 unit tests + 7 wiremock integration
  tests (load, single set, batched set_many, all three client-side
  validation paths, empty set_many)._
- [ ] **4.9 `src/server/{butler,activities,updater,statistics,transcode,browse}.rs`** — long tail.

## M5 — Real-time / discovery / cloud catalogue / webhooks

- [~] **5.1 `src/auth/pin.rs`** — `MyPlexPinLogin` with start/poll/wait.
  Plain struct (not typestate); typestate machine deferred (the three
  states map cleanly to `Result<Option<PlexToken>>` from `poll()`).
  Wiremock integration tests deferred pending a `with_endpoint(base)`
  override for the plex.tv URL. _2 DTO unit tests._
- [x] **5.2 `src/auth/password.rs`** — `MyPlexPasswordLogin` with
  `sign_in()` and `sign_in_with_code()`. Form-urlencoded POST to
  `plex.tv/api/v2/users/signin`. Inspects the response body on 401
  for the `code: 1029` envelope (with a `"verification code"`
  substring fallback) and surfaces it as `Error::TwoFactorRequired`
  distinct from `Error::Unauthorized`. Test endpoint override via
  `with_endpoint()`. Crate-private `HttpClient::inner()` accessor
  lets the auth module drive the POST with custom status mapping.
  _8 unit tests + 4 wiremock integration tests._
- [x] **5.3 `src/myplex/{mod,resources}.rs`** — `MyPlexClient` +
  `MyPlexResource` + `ResourceConnection`. `MyPlexClient::resources()`
  fetches `GET /api/v2/resources?includeHttps=1&includeRelay=1`,
  `resource(name)` finds case-insensitively. `MyPlexResource::connect`
  uses `FuturesUnordered` to race concurrent probes across every
  preferred connection URI (local→remote→relay, https→http;
  shared resources skip the local set) and returns the first
  `PlexServer` that answers `GET /`. `ConnectOptions` exposes ssl
  filter, per-attempt timeout, identifier, and identity overrides.
  Per-resource access token honored on the winning probe.
  Endpoint overridable via `MyPlexClient::with_base(url)` for test
  replicas. _11 unit tests + 4 wiremock integration tests._
- [~] **5.4 `src/myplex/{webhooks,devices,friends,home}.rs`** —
  - **webhooks**: `MyPlexClient::webhooks()`, `add_webhook(url)`
    (idempotent), `delete_webhook(url)` (NotFound if absent),
    `set_webhooks(&urls)` (empty slice clears). Form-encoded POST
    to `/api/v2/user/webhooks`; parser handles JSON-array, wrapped
    envelope, AND XML responses. _7 unit tests + 6 wiremock._
  - **devices**: `MyPlexClient::devices()` lists every device
    registered to the account via `GET /devices.xml` (XML-only on
    Plex's side); each `MyPlexDevice` carries id, name, product,
    platform, client_identifier, per-device token (Debug-redacted),
    public_address, screen_resolution/density, created_at,
    last_seen_at, and connection URIs. `is_server()` / `is_player()`
    helpers. `MyPlexDevice::delete(&client)` revokes the token via
    `DELETE /devices/<id>.xml`. quick-xml's serde adapter drives
    the XML parsing with `@attribute`-style renames. _9 unit
    tests + 3 wiremock integration tests._
  - **friends**: `MyPlexClient::friends()` lists shared accounts
    via `GET /api/users/` (XML). `MyPlexUser` carries id, username,
    title, email, thumb, home/restricted flags, allow_sync /
    allow_channels / allow_camera_upload, and per-share access
    token (Debug-redacted). `remove_friend(id)` via
    `DELETE /api/friends/<id>`. _6 unit tests + 2 wiremock
    integration tests._
  - **home**: `MyPlexClient::home_users()` lists Plex Home
    sub-accounts via `GET /api/home/users` (XML). `MyPlexHomeUser`
    carries id, title, username, email, thumb, plus admin /
    protected / restricted / guest flags. Mutation (add /
    remove / restrict) deferred — admin-UI workflow with PIN
    complications. _5 unit tests + 1 wiremock._
  - [-] **claim** — **OUT OF SCOPE.** A claim token is a one-shot
    credential generated to bind a fresh Plex Media Server install
    to a plex.tv account. It's used exactly once per server
    lifetime, almost always through the PMS web setup UI; very few
    callers ever need to mint one programmatically. Excluding it
    keeps the auth surface focused.
  - [-] **sonos** — **OUT OF SCOPE.** Sonos integration endpoints
    (`https://sonos.plex.tv/resources`) target the specific Plex
    for Sonos product. Audience is Plex Pass holders who also own
    Sonos hardware AND want to drive it programmatically — a tiny
    minority of users. Excluded to keep the crate footprint
    focused on broadly-useful surfaces.
- [~] **5.5 `src/myplex/watchlist.rs`** — Discover watchlist.
  `MyPlexClient::watchlist()` / `watchlist_with(&opts)` lists,
  `add_to_watchlist(rk)` / `remove_from_watchlist(rk)` mutate.
  `WatchlistOptions` builder with `filter` (All/Available/Released),
  `kind` (Movie/Show via numeric SearchType), `sort` (string
  `field:dir`), `max_results`. `WatchlistItem` is a minimal
  projection (rating_key extracted from `plex://kind/<hex>` guid,
  title, year, summary, thumb, art, ratings, watchlisted_at) with
  full raw payload flattened in. New `with_discover_base` /
  `with_metadata_base` overrides on MyPlexClient for testing.
  _9 unit tests + 6 wiremock integration tests._
  Discover search shipped — see entry below.

- [x] **5.5b `src/myplex/discover.rs`** — `MyPlexClient::discover_search
  (query, opts)` runs full-text search against
  `discover.provider.plex.tv/library/search`. `DiscoverOptions` builder
  with `limit` (default 30), `kind` (Movie/Show — serialized as
  `searchTypes=movies` / `tv`), and `providers` (default `"discover"`,
  overridable for `discover,PLEXAVOD,PLEXTVOD`). `DiscoverItem` mirrors
  WatchlistItem shape (guid + extracted rating_key, kind, title, year,
  summary, ratings, content_rating, score) with full raw payload
  preserved. Flattens results across all `SearchResults` buckets
  (`external`, `library`, etc.). _7 unit tests + 4 wiremock
  integration tests._
  - [-] **availability metadata** — **OUT OF SCOPE.** The
    "available on Netflix / Disney+ / etc." overlay endpoint
    serves recommendation apps that map cloud-catalogue items to
    consumer streaming services. Narrow audience, narrow utility;
    the same data is available via third-party sources (JustWatch
    API etc.) for callers who actually need it.
- [x] **5.6 `src/myplex/metadata_provider.rs`** —
  `MyPlexClient::user_state(rk)` reads cloud per-user state
  (view_count, view_offset_ms, view_state_complete, last_viewed_at,
  watchlisted_at) from `metadata.provider.plex.tv/library/metadata/<rk>/userState`.
  `scrobble(rk)` / `unscrobble(rk)` mark watched / unwatched on the
  cloud catalogue (which then propagates to subscribed PMS). Wire
  endpoints are HTTP GET despite being mutations — quirk preserved.
  `is_played()` / `is_on_watchlist()` convenience helpers on
  UserState. Permissive timestamp parser accepts epoch-number,
  epoch-string, and ISO-8601 shapes.
  _9 unit tests + 4 wiremock integration tests._
- [x] **5.7 `src/alerts/`** — `Alerts::connect(&server)` opens a
  WebSocket to `/:/websockets/notifications` (with `X-Plex-Token`
  via query param — Plex's WS endpoint doesn't accept standard
  X-Plex-* headers). Returns `Alerts: Stream<Item=Result<AlertEvent>>`
  yielding one event per inner notification (frames carrying N
  entries flatten to N yields). `AlertEvent` enum: Playing,
  Timeline, Activity, TranscodeSession (with `TranscodeLifecycle::
  {Start, Update, End}`), Status, Reachability, Setting,
  BackgroundProcessingQueue, plus `Unknown { kind, raw }` for
  forward-compat. Per-variant DTO captures the documented fields
  (PlaySessionStateNotification's sessionKey/state/viewOffset/userID/
  transcodeSession, TimelineEntry's itemID/state/title/type with
  the documented state values 0/1/2/3/4/5/9). `Alerts::connect_with_url`
  is the test-friendly primitive that the integration tests drive
  against a `tokio::net::TcpListener` + `accept_hdr_async` WS replica.
  Reconnect with backoff documented as a usage pattern (caller-driven
  loop using `retry_delay`) rather than wrapping it inside the crate
  — keeps the surface minimal and lets callers compose with their
  own shutdown signals. Gated behind the `alerts` Cargo feature
  (pulls in `tokio-tungstenite` with `connect` + `rustls-tls-webpki-roots`).
  _12 unit tests + 3 wiremock/WS integration tests (full end-to-end
  handshake + multi-frame sequencing + clean-close termination +
  unreachable-URL error path)._
- [x] **5.8 `src/discover_gdm/`** — `GdmEntry` + `discover_local_servers`
  via raw `tokio::net::UdpSocket` multicast to 239.0.0.250:32414
  with HTTP/1.0 `M-SEARCH` payload. Dedup by Resource-Identifier.
  `GdmEntry::base_url()` builds the PMS URL. Gated on `discovery`
  feature; `tokio/net` only pulled in when the feature is on.
  _5 unit tests._
- [x] **5.9 `src/webhook/`** — inbound Plex webhook handling.
  `WebhookPayload::from_json(raw)` decodes the JSON document Plex
  sends as the `payload` form field. `WebhookEvent` enum covers
  all 12 documented events (media.play/pause/resume/stop/scrobble/rate,
  library.on.deck/new, admin.database.backup/corrupted, device.new,
  playback.started) plus `Unknown(String)` for forward-compat.
  Sub-payloads: `WebhookAccount`, `WebhookServer`, `WebhookPlayer`,
  `WebhookMetadata` (minimal projection with `raw: serde_json::Value`
  flattened in for fields beyond the projection). For axum users,
  `impl FromRequest<S> for WebhookPayload` provides the extractor:
  parses multipart/form-data, finds the `payload` field, decodes
  the JSON, and captures any `thumb` attachment as `Bytes`. Typed
  `WebhookRejection` enum maps to 400 Bad Request via
  `IntoResponse`. Gated behind the `webhook-axum` Cargo feature
  (axum with multipart + http1 + json + tokio features).
  _9 unit tests + 6 wiremock/axum integration tests (full
  end-to-end with axum::serve listener, real multipart bodies sent
  via reqwest, all three rejection paths, thumb capture)._
- [-] **5.10 `src/playback/sync.rs`** — legacy mobile sync.
  **OUT OF SCOPE.** Plex deprecated the legacy "sync library to
  phone" API in favor of the newer Download feature, which uses a
  different endpoint family. The legacy endpoints still exist for
  backward compatibility with very old mobile-app builds but Plex
  itself recommends not using them. Investing in implementing a
  deprecated API surface is wasted effort.

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
