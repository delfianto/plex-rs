# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
once it reaches 1.0. Pre-1.0 the minor version may contain breaking changes;
each breaking change is listed under **Breaking** in its release entry.

## [Unreleased]

### Added
- Project bootstrap: `Cargo.toml`, `rust-toolchain.toml`, `deny.toml`, CI
  workflow, lint baseline in `src/lib.rs`.
- `CLAUDE.md` — contributor guide and project charter.
- `analysis/` — deep-dive notes on the `python-plexapi` reference
  implementation that this crate targets for feature parity.
- `TRACKER.md` — milestone-by-milestone implementation tracker.
- **M0 (Foundations)** — full HTTP transport layer plus shared primitives:
  - `error` — `Error` enum, `Result` alias, status-to-error mapping,
    retryability classifier.
  - `util::ids` — `RatingKey`, `MachineIdentifier`, `ClientIdentifier`,
    `PlayQueueId`, `PlexToken` (with redacted `Debug`).
  - `util::time` — Plex epoch-seconds and ISO-date helpers + serde
    adapter for stringified epoch fields.
  - `util::search_type` — full `SearchType` enum mirroring
    `python-plexapi`'s `utils.py:35` table, with forward-compat
    `Unknown(u32)`.
  - `util::sanitize` — fixture sanitiser with 14 regex rules + IPv4/IPv6
    classifier; idempotent.
  - `uri` — `PlexUri` enum covering 7 schemes (`server://`, `library://`,
    `library:///directory/`, `playlist:///`, `/playQueues/...`,
    `https://plex.tv/devices/.../sync_items`, `/security/token`),
    round-trip-stable.
  - `xml` — `MediaContainer<T>` generic envelope collapsing the 12
    `mediaContainerWith*` schemas.
  - `pagination` — `PageRange` + `advance_with()` using the header-based
    `X-Plex-Container-Start/-Size` pagination.
  - `headers` — `PlexIdentity` builder emitting the 10 `X-Plex-*`
    headers + `Accept: application/json`, with strict ASCII validation.
  - `config` — `ClientConfig` builder with timeouts and retry policy
    invariants.
  - `client` — `HttpClient`: JSON-first content negotiation, full-jitter
    exponential backoff retries, status-to-`Error` mapping, token-safe
    `Debug`.
- **Playable trait — direct-play URL builder.** First piece of
  external-player integration:
  - `traits::Playable` — single `direct_play_url() -> Option<Url>`
    method that returns the absolute URL of the first
    [`MediaPart`]'s wire key, with `X-Plex-Token` embedded as a
    query parameter so external players (VLC, mpv, browser
    `<video>` elements, …) can stream without setting auth
    headers themselves.
  - Returns `None` when either (a) the item's `Vec<Media>` is
    empty (typically because the metadata came from a listing
    endpoint that omits `Media[]` — call `Reload::reload()`
    first), or (b) the bound HTTP client has no token configured.
  - Implementors: Movie / Episode / Track. Photo and Photoalbum
    intentionally not — Plex serves images from a different path
    family and the use case is different.
  - Defensive: when the wire key already contains `?…`, the
    method appends `&X-Plex-Token=` instead of `?`.
  - Transcoded streaming URL construction (`/video/:/transcode/
    universal/start.<container>` with quality / decision
    negotiation) defers to a follow-up. Most external players
    can decode the source format directly, so `direct_play_url`
    is the right primitive to ship first.
  - `tests/m4_playable.rs` — 1 wiremock integration test
    covering the partial→reload→playable-URL walk, asserting
    the token shows up in the query.

- **M3.2 (Reload trait)** — re-fetch a partial item with full
  detail:
  - `traits::Reload` — single `reload(self) -> Result<Self::Full>`
    method (consumes `self` because the caller will replace their
    previously-held partial). Associated type `Full` always equals
    `Self` in this crate (no separate partial / full type — the
    same struct carries an empty or populated `Vec<Media>` /
    `Vec<Tag>` / etc. depending on which endpoint emitted it).
  - Underlying request: `GET /library/metadata/<rating_key>`.
  - Implemented on Movie / Show / Season / Episode / Artist /
    Album / Track. Each impl calls the crate-private
    `fetch_metadata` helper and dispatches into the appropriate
    `into_*` conversion to re-build the leaf with the section ref
    preserved.
  - Listing endpoints (`/library/sections/<id>/all`, search,
    recentlyAdded, …) return partial metadata; for tag/media/
    marker access callers can now `.reload().await` to upgrade.
  - `tests/m3_reload.rs` — 2 wiremock integration tests covering
    partial→full upgrade and 404 → `Error::NotFound` propagation.

- **M5.8 (GDM local discovery)** — zero-config PMS discovery on the
  LAN. Behind the `discovery` Cargo feature.
  - `discover_gdm::GdmEntry` — one discovered server. Carries
    source `SocketAddr`, machine identifier, friendly name, port,
    version, content type, last-updated epoch, and a `headers`
    map of all returned key/value pairs (case-insensitive).
  - `GdmEntry::base_url()` — convenience helper that builds an
    `http://<source-ip>:<port>/` URL for handing to
    `PlexServer::connect`.
  - `discover_gdm::discover_local_servers(timeout)` — sends an
    HTTP/1.0 `M-SEARCH * HTTP/1.0\r\n\r\n` payload to multicast
    `239.0.0.250:32414` and collects replies for `timeout`,
    deduping by `Resource-Identifier`. Replies with no
    machine-identifier (rare; pre-1.0 PMS builds) are returned at
    the end of the list.
  - Implementation note: this is **raw UDP, not mDNS** — Plex's
    GDM protocol predates and is incompatible with multicast-DNS
    despite occasional confusion. `tokio::net::UdpSocket` (gated
    on the new `tokio/net` feature, also pulled in by the
    `discovery` Cargo feature) drives the send and the recv loop.
  - `tests`: 5 unit tests covering header parsing, non-HTTP
    tolerance, unknown-header preservation, base URL
    construction, and the no-port case.

- **M5.1 (PIN sign-in)** — first plex.tv authentication flow:
  - `auth::MyPlexPinLogin` — in-progress PIN sign-in. Built via
    `start(client_identifier, identity)` (or
    `start_with_client(http)` for callers who already have a
    configured `HttpClient`). Holds the PIN id, the 4-char code
    to show the user, and the expiry time.
  - `MyPlexPinLogin::code()` — the code the user types at
    `https://plex.tv/link`.
  - `MyPlexPinLogin::poll()` — `Ok(Some(token))` once the user
    has claimed the PIN, `Ok(None)` while still pending,
    `Err(Error::Auth)` if the PIN expires before being claimed.
  - `MyPlexPinLogin::wait(timeout, interval)` — convenience
    polling loop. Sleeps `interval` between polls; surfaces
    `Error::Auth` when `timeout` elapses or the PIN expires.
  - Wire endpoints: `POST https://plex.tv/api/v2/pins?strong=true`
    creates the PIN; `GET https://plex.tv/api/v2/pins/<id>` polls
    for claim. Identity headers from the constructor are honored
    on both calls — `X-Plex-Client-Identifier` must match between
    create and poll (a known plex.tv pitfall).
  - Wiremock integration tests skipped — the endpoint URL is
    hard-coded; a future iteration will add a
    `with_endpoint(base)` knob for test override. Unit tests
    cover DTO parsing of both create and claimed responses.

- **M5.2 (password + 2FA sign-in)** — second plex.tv auth flow:
  - `auth::MyPlexPasswordLogin` — username/password (and optional
    OTP) sign-in. Built via `new(client_identifier, identity)` or
    `with_client(http)`; `with_endpoint(url)` overrides the
    plex.tv endpoint for test-replica use.
  - `sign_in(login, password)` — happy path; returns the minted
    `PlexToken`. On 2FA-protected accounts surfaces
    `Error::TwoFactorRequired` so the caller can prompt for the
    code and retry.
  - `sign_in_with_code(login, password, verification_code)` —
    same flow with the OTP appended.
  - Wire: `POST https://plex.tv/api/v2/users/signin` with
    `application/x-www-form-urlencoded` body
    (`login`/`password`/`rememberMe`/`verificationCode`). 2FA is
    detected by inspecting the JSON error envelope for
    `code: 1029` (with a `"verification code"` substring
    fallback for legacy responses).
  - `HttpClient::inner()` is now `pub(crate)` so the auth module
    can drive the POST with its own status mapping — the only
    place in the crate that bypasses the standard JSON-with-retry
    envelope.

- **M3.8 (EditBatch transaction)** — single PUT for multi-field
  edits, eliminating N round-trips for bulk library cleanup:
  - `EditBatch::new(&item)` or `item.batch()` (via `EditBatchExt`)
    starts a builder. Chain `.set_field/.lock_field/.replace_tags/
    .remove_tags` (low-level) or convenience shortcuts
    (`set_title`, `set_year`, `replace_genres`, `replace_directors`,
    `replace_writers`, etc. mirroring the per-trait method names).
  - `.execute().await` flushes the queued ops in one PUT. Wire
    format combines the EditField (`<field>.value=v&<field>.locked=L`)
    and EditTags (`<field>[N].tag.tag=v`, `<field>[].tag.tag-=csv`)
    shapes into a single query string sharing the `id` / `type`
    prefix.
  - Empty batch is a no-op — short-circuits without an HTTP call.
  - `EditBatchExt` auto-implements on every type with `EditField`,
    so the `.batch()` method appears on Movie, Show, Season,
    Episode, Artist, Album, Track, Collection without per-type
    boilerplate.

- **M5.6 (metadata_provider userState + scrobble)** — per-user
  state on Plex's cloud catalogue. Where the M3
  `PlayedUnplayed` trait marks items watched on a single PMS,
  the metadata provider tracks watched-state on the global
  Discover catalogue and propagates it to every subscribed PMS:
  - `MyPlexClient::user_state(rating_key)` reads
    `view_count`, `view_offset_ms`, `view_state_complete`,
    `viewed_leaf_count`, `last_viewed_at`, `watchlisted_at` from
    `metadata.provider.plex.tv/library/metadata/<rk>/userState`.
    `UserState::is_played()` / `is_on_watchlist()` are
    convenience helpers.
  - `MyPlexClient::scrobble(rk)` / `unscrobble(rk)` mark
    watched / unwatched on the cloud catalogue. Wire endpoints
    are plain `GET` (despite being mutations) — Plex quirk
    preserved verbatim.
  - Permissive timestamp parser accepts epoch-number,
    epoch-string, and ISO-8601 shapes, since the
    metadata_provider endpoint has shipped each at different
    points.

- **M5.5 (plex.tv Watchlist)** — the user-level "to watch" list
  on the Plex cloud catalogue (distinct from any single PMS
  library):
  - `MyPlexClient::watchlist()` returns the full list with
    default options. `watchlist_with(&opts)` accepts a
    `WatchlistOptions` builder with `.with_filter(...)` (All /
    Available / Released — the path segment), `.with_kind(...)`
    (Movie / Show — serialized to `?type=1`/`?type=2`),
    `.with_sort("field:dir")`, and `.with_max_results(n)`.
  - `MyPlexClient::add_to_watchlist(rating_key)` /
    `remove_from_watchlist(rating_key)` mutate by hex rating key.
    The rating key is the trailing segment of a `plex://kind/<hex>`
    GUID; `WatchlistItem::rating_key` is pre-extracted for callers
    that already have an item in hand.
  - `WatchlistItem` is intentionally a separate type from
    `LibraryItem`. Watchlist entries refer to the global Plex
    cloud catalogue, not a specific PMS section, so the
    section-attached trait machinery doesn't apply. The full raw
    JSON payload is flattened into `raw: serde_json::Value` for
    callers that need fields beyond the projection (genres, cast,
    etc.).
  - `MyPlexClient` gains `with_discover_base(url)` and
    `with_metadata_base(url)` overrides alongside the existing
    `with_base(url)` so tests can point at wiremock replicas of
    the three distinct plex.tv endpoints
    (`https://plex.tv`, `https://discover.provider.plex.tv`,
    `https://metadata.provider.plex.tv`).

- **M5.4 (MyPlex devices list + revoke)** — sibling to the
  resource + webhooks surface:
  - `MyPlexClient::devices()` fetches every device registered to
    the signed-in account via `GET /devices.xml`. Returns
    `Vec<MyPlexDevice>`; each device carries the plex.tv numeric
    `id` needed for the delete path, plus friendly name, product,
    platform, hardware class / model / vendor, capabilities,
    `client_identifier`, per-device access token, public
    address, screen resolution and density, and the registry
    timestamps (`created_at`, `last_seen_at`).
  - `MyPlexDevice::delete(&client)` hits
    `DELETE /devices/<id>.xml`, revoking the per-device token
    without touching other devices or the account token.
  - `MyPlexDevice::is_server()` / `is_player()` are convenience
    capability checks. `provides` is exposed as a `Vec<String>`
    so other capabilities (`controller`, `sync-target`,
    `pubsub-player`) can be matched directly.
  - Per-device tokens are wrapped in [`PlexToken`] so `Debug`
    redacts them (a hand-written `Debug` impl on `MyPlexDevice`
    drops other potentially noisy fields and includes the token
    via its own redacted formatter).
  - XML-only endpoint — the v2 JSON resource endpoint
    (`MyPlexResource` from M5.3) returns a different shape that
    doesn't carry the integer `id` field. Parsed via `quick-xml`'s
    serde adapter with `@attribute`-style renames.
  - Timestamps arrive as epoch-seconds strings on this endpoint
    (not ISO 8601); the parser handles both formats.

- **M5.4 (webhook URL registration on plex.tv)** — the management
  half of the webhook story (M5.9 ships the receiver):
  - `MyPlexClient::webhooks()` lists the URLs currently registered
    on the account. Plex's v2 endpoint returns either a top-level
    JSON array, a wrapped `{"webhooks":[...]}` envelope, or XML
    depending on Accept negotiation; the parser handles all three.
  - `MyPlexClient::add_webhook(url)` appends a URL to the list and
    POSTs the merged set. Idempotent — duplicates are no-ops.
  - `MyPlexClient::delete_webhook(url)` removes a URL and POSTs
    the filtered set. Returns `Error::NotFound` when the URL
    isn't registered (so callers can tell "already gone" from
    "successfully removed").
  - `MyPlexClient::set_webhooks(&urls)` is the underlying full-list
    replace. Passing an empty slice clears every webhook (matches
    python-plexapi's empty-list semantics).
  - Wire: `GET/POST https://plex.tv/api/v2/user/webhooks` with
    `application/x-www-form-urlencoded` body `urls[]=u1&urls[]=u2`.
    The POST bypasses HttpClient's standard JSON envelope (same
    pattern as the password sign-in flow).

- **M5.9 (Webhook ingest with axum extractor)** — completes the
  real-time inbound story alongside M5.7 alerts. Where alerts
  pulls events via a WebSocket the crate opens, webhooks receive
  events as HTTP POSTs to a user-run endpoint.
  - `WebhookEvent` enum covers all 12 documented events
    (`media.play`/`pause`/`resume`/`stop`/`scrobble`/`rate`,
    `library.on.deck`/`new`, `admin.database.backup`/`corrupted`,
    `device.new`, `playback.started`) plus an `Unknown(String)`
    variant for forward compatibility.
  - `WebhookPayload` carries the parsed event plus account / server
    / player / metadata sub-payloads. `WebhookMetadata` is a
    minimal projection (rating_key, key, type, title,
    grandparentTitle, librarySectionType, etc.) with the full
    payload flattened into `raw: serde_json::Value` for callers
    that need fields beyond the projection.
  - `WebhookPayload::from_json(raw)` — decode just the JSON
    portion. Use this when the receiver is not an axum handler.
  - `impl FromRequest<S> for WebhookPayload` — the axum
    extractor. Parses Plex's `multipart/form-data` POST, finds the
    `payload` text field, decodes the JSON, and captures any
    `thumb` binary attachment as `Bytes`.
  - `WebhookRejection` is a typed error → 400 Bad Request with
    a useful diagnostic body (not multipart, missing payload,
    invalid JSON, multipart I/O error).
  - Gated behind the `webhook-axum` Cargo feature, which pulls
    in `axum` with `multipart` + `http1` + `json` + `tokio`.

- **M4.8 (PlexServer Settings)** — read and write server preferences:
  - `PlexServer::settings()` returns a `Settings` snapshot fetched
    from `GET /:/prefs`. Each preference becomes a typed `Setting`
    with id, label, summary, group, current and default values,
    hidden/advanced/secure flags, and (for enum-typed settings) a
    list of valid options.
  - `SettingKind` enum: `Text`, `Int`, `Double`, `Bool`, `Enum`,
    `Other(String)` (forward-compat for undocumented kinds Plex
    might add).
  - `SettingValue` enum: `Text(String)`, `Int(i64)`, `Double(f64)`,
    `Bool(bool)`. Plex emits everything as strings on the wire;
    `SettingValue::parse(&kind, raw)` does the type-driven
    conversion. `.to_wire()` produces the spelling Plex expects on
    write (e.g. `Bool(true)` → `"true"`).
  - `EnumValues` enum: `List(Vec<String>)` for plain
    `low|medium|high`, `Mapping(Vec<(String, String)>)` for
    `0:Off|1:On`-style key:label pairs. Parsed automatically based
    on whether the wire string contains `:`.
  - Mutation: `Settings::set(server, id, value).await` writes one
    preference (PUT `/:/prefs?id=value`) and reloads; or
    `set_many(server, updates).await` batches multiple writes into
    one PUT. Both consume `self` and return a refreshed snapshot.
  - Client-side validation: unknown id → `Error::NotFound`;
    wrong-kind value → `Error::Config`; out-of-enum value →
    `Error::Config`. All happen before any network call, so the
    caller never sees a PMS-side rejection for these cases.
  - Read accessors: `all()`, `get(id)`, `group(name)`,
    `group_names()`, `len()`, `is_empty()`. Settings are sorted by
    id (BTreeMap-backed).
  - Two-phase staging commit (python-plexapi's `_setValue`
    pattern, where `setting.set(v)` stages and `settings.save()`
    commits) is intentionally NOT replicated — explicit
    `set`/`set_many` composes more cleanly with Rust's
    `await`-based ergonomics and avoids hidden mutable state.

- **M5.7 (Alerts WebSocket)** — real-time monitoring of PMS events:
  - `alerts::Alerts::connect(&server)` opens a WebSocket to
    `/:/websockets/notifications`. Authentication is via the
    `X-Plex-Token` query parameter — Plex's WS endpoint ignores
    HTTP headers, so the standard `X-Plex-*` identity baked into
    `HttpClient` doesn't apply here.
  - `Alerts` implements `futures::Stream<Item=Result<AlertEvent>>`.
    Frames that carry multiple inner notifications flatten so each
    `.next().await` returns exactly one event.
  - `AlertEvent` enum: `Playing`, `Timeline`, `Activity`,
    `TranscodeSession` (with `TranscodeLifecycle::{Start, Update,
    End}` discriminator), `Status`, `Reachability`, `Setting`,
    `BackgroundProcessingQueue`, and `Unknown { kind, raw }` for
    forward-compat.
  - Per-variant DTOs (`PlayingNotification`, `TimelineEntry`,
    `ActivityNotification`/`ActivityBody`,
    `TranscodeSessionNotification`, `StatusNotification`,
    `ReachabilityNotification`, `SettingNotification`,
    `BackgroundProcessingQueueNotification`) cover the documented
    fields. `TimelineEntry::state` follows Plex's documented state
    table (0=created, 1=processing, 2=matching, 3=metadata
    download, 4=metadata process, 5=done, 9=deleted).
  - `Alerts::connect_with_url(ws_url)` — escape hatch for advanced
    callers (e.g. tunneled PMS, custom WebSocket proxies) and the
    integration-test path. Skips the PMS base-URL derivation.
  - Reconnection: not wrapped in the crate. Documented as a
    caller-driven loop using `client::retry_delay` (already pub).
    Keeps the surface minimal and lets callers compose with
    `tokio::select!` shutdown signals.
  - Gated behind the `alerts` Cargo feature, which pulls in
    `tokio-tungstenite` with `connect` + `rustls-tls-webpki-roots`.
  - Integration tests stand up a `tokio::net::TcpListener` +
    `accept_hdr_async` WS replica that records the handshake URI
    (asserts the token + path are correct), emits a sequence of
    typed frames, then closes cleanly — exercises the full
    transport + decoder + stream wrapping end-to-end.

- **M4.5 (PlexClient remote control)** — the second half of the
  playback story; completes the "create a queue, then tell a
  player to consume it" flow that python-plexapi users expect:
  - `playback::PlexClient` — handle pointing at a Plex player's
    HTTP endpoint (typically port 32500). Built via
    `connect(base_url, access_token, machine_identifier,
    client_identifier)`; the `access_token` is the **player's**
    per-resource token, not the account token.
  - Navigation commands: `move_up`, `move_down`, `move_left`,
    `move_right`, `select`, `back`, `context_menu`, `go_to_home`,
    `go_to_music`, `page_up`, `page_down`. All hit
    `/player/navigation/{cmd}`.
  - Playback commands: `play`, `pause`, `stop`, `skip_next`,
    `skip_previous`, `seek_to(position_ms, mtype)`,
    `step_forward`, `step_back`, `set_volume(0..=100, mtype)`,
    `set_repeat(RepeatMode, mtype)`,
    `set_shuffle(bool, mtype)`. Each command requires a
    `MediaType` (Video / Music / Photo) so a single player can
    multiplex foreground video and background music.
  - `MediaType` and `RepeatMode` enums with `.as_wire()`
    accessors mapping to the spellings Plex expects.
  - Flagship: `play_media(&server, &queue, offset_ms)` —
    composes the gnarly playMedia payload (providerIdentifier,
    machineIdentifier, protocol, address, port, offset, key,
    type, containerKey, token) by deriving every value from the
    supplied `PlexServer` and `PlayQueue`. Callers don't
    hand-build the URL.
  - Command-ID protocol: Plex requires `commandID` to increase
    monotonically per caller; we sequence with an internal
    `Arc<AtomicU64>` so cloned `PlexClient` handles serialise
    correctly across threads.
  - The `X-Plex-Target-Client-Identifier` header is set to the
    player's machine identifier on every command.
  - Test coverage includes the full play_media payload verified
    against a separate PMS mock, command-ID monotonicity over a
    sequence of three commands, and value clamping
    (`set_volume(255)` → `volume=100`).

- **M4.4 (PlayQueue create/get/mutate)** — server-side playback
  queues, the unit Plex players consume:
  - `PlexServer::create_play_queue()` returns a `CreatePlayQueue`
    builder. Source methods: `.from_item(&item)`,
    `.from_items(&[&item, ...])`, `.from_playlist(&playlist)`.
    Flag setters: `.shuffle(bool)`, `.repeat(bool)`,
    `.continuous(bool)`, `.include_chapters(bool)`,
    `.include_related(bool)`, `.start_at(key)`. Terminate with
    `.execute().await`.
  - `PlexServer::play_queue(id)` fetches an existing queue.
  - `PlayQueue` carries `id`, `version`, `total_count`,
    `selected_item_id/_offset/_metadata_item_id`, `shuffled`,
    `source_uri`, `identifier`, and the `items: Vec<PlayQueueItem>`.
    Each `PlayQueueItem` exposes its `play_queue_item_id` plus the
    standard `LibraryItem` metadata via serde `flatten` on
    `MetadataDto` (same trick `PlayingSession` and `HistoryEntry`
    use).
  - Mutation methods consume `self` and return refreshed snapshots
    after the server response: `refresh()` (re-GET),
    `add_item(&item, play_next: bool)` (PUT to `/{id}` with `uri=`
    and optional `next=1`), `move_item(item_id, after_id)` (PUT
    `/items/{iid}/move?after=...`), `remove_item(item_id)` (DELETE
    `/items/{iid}`), `clear()` (DELETE `/items`).
  - Wire spellings: `playQueueID`, `playQueueItemID`,
    `playQueueSourceURI`, etc. all carry an explicit
    `#[serde(rename)]` because Plex preserves the `ID`/`URI`
    capitalization that `camelCase` auto-conversion mangles.
  - Source URI construction is the trickiest part: a single item
    becomes `server://<MID>/com.plexapp.plugins.library<key>`; a
    list becomes
    `library:///directory/<percent-encoded(/library/metadata/RK1,RK2,...)>`
    using a hand-rolled RFC 3986-strict percent-encoder (the
    `form_urlencoded` `+`-for-space convention would be wrong
    here); a playlist passes `playlistID=<rk>` instead of `uri`.
  - `LibraryItem` gains two new accessors used by the URI
    construction: `key()` (returns the wire `/library/metadata/<rk>`
    path) and `list_type()` (returns `"video"` / `"audio"` /
    `"photo"` based on the variant).
  - `HttpClient::get_bytes_for_method(method, url)` —
    crate-private method-parametric primitive used by PlayQueue's
    PUT/DELETE mutations. Reuses the standard retry envelope.

- **M4.7 (Playback history with pagination)** — first paginated
  endpoint in the crate; exercises the previously-shipped
  `PageRange` machinery end-to-end:
  - `PlexServer::history()` returns a `HistoryQuery` builder.
    Filter methods: `.account(id)`, `.library_section(id)`,
    `.rating_key(rk)`, `.mindate(DateTime<Utc>)` /
    `.mindate_epoch_secs(secs)`, `.max_results(n)`, `.page_size(n)`.
    Default sort is `viewedAt:desc` (matches python-plexapi).
  - Wire: `GET /status/sessions/history/all` with filter query
    params and `X-Plex-Container-Start` / `-Size` request headers
    for pagination. The 1.40-era PMS quirk where `mindate` is
    sent as `viewedAt>=` is preserved verbatim.
  - Terminate with `.collect()` for an eager `Vec<HistoryEntry>`,
    or `.stream()` for a lazy `futures::Stream` that fetches
    pages on demand and honors `.max_results` across page
    boundaries. The stream is `Send` and drops its in-flight
    fetch cleanly on cancellation.
  - `HistoryEntry` carries the standard `LibraryItem` (via serde
    `flatten` over `MetadataDto`, same trick sessions uses) plus
    the history-only fields `account_id`, `device_id`,
    `history_key`, `viewed_at`. Calling `.delete(http, base)` on
    an entry issues `DELETE <history_key>`.
  - `HttpClient::get_bytes_with_headers` and
    `get_json_with_headers` — new primitives that thread
    per-request headers through the standard retry envelope.
    First consumer is history; future paginated endpoints
    (e.g. `/library/sections/<id>/all`) will reuse them.

- **M5.3 (MyPlexResource + parallel connect race)** — `MyPlex`
  resource discovery and the canonical "find a server, then go":
  - `myplex::MyPlexClient` — authenticated handle to plex.tv.
    Built via `new(token, client_identifier, identity)` or
    `with_client(http)`; `with_base(url)` overrides the plex.tv
    base URL for test replicas.
  - `MyPlexClient::resources()` — fetch every server and player
    visible to the signed-in account.
    `GET /api/v2/resources?includeHttps=1&includeRelay=1` returns
    a JSON array; the response yields a `Vec<MyPlexResource>`.
  - `MyPlexClient::resource(name)` — convenience case-insensitive
    name lookup.
  - `MyPlexResource` — name, product, platform, client_identifier
    (typed as `MachineIdentifier`), provides (CSV split into
    `Vec<String>`), owned/presence/relay/etc. flags, per-resource
    access token (redacted in `Debug`), and the connection list.
  - `MyPlexResource::preferred_connections(ssl)` — sorted list of
    candidate URIs. Ordering: location outer (local → remote →
    relay) then scheme inner (https → http). Local URIs are
    skipped when `owned == false` (shared resources).
  - `MyPlexResource::connect()` / `connect_with_options(opts)` —
    flagship method. Races every preferred connection URI in
    parallel using `FuturesUnordered` and returns the first
    `PlexServer` to answer `GET /`. Each probe uses the
    per-resource access token, not the account token. Dropping
    losing probes cancels their in-flight requests.
  - `ConnectOptions` — `ssl` filter, `per_attempt_timeout`
    (default 8s), `client_identifier`, and `identity` overrides.
    All builder-style setters.
  - `ResourceConnection` — `protocol`, `address`, `port`, `uri`,
    `local`, `relay`, `ipv6` (PascalCase `IPv6` on the wire,
    handled by an explicit `#[serde(rename)]`).

- **M4.3 (Sessions — list + stop)** — current-playback surface:
  - `server::sessions::PlayingSession` — one currently-playing
    session. Carries the played `LibraryItem`, view offset, plus
    nested `SessionUser`, `SessionPlayer`, and optional
    `TranscodeSession`.
  - `server::sessions::PlayState` — enum
    `Playing | Paused | Buffering | Stopped | Other(String)`.
  - `server::sessions::SessionUser` / `SessionPlayer` /
    `TranscodeSession` — typed views of Plex's nested
    `<User>` / `<Player>` / `<TranscodeSession>` children. Player
    captures both LAN and remote-public addresses, device /
    product / platform identifiers, and local + controllable
    booleans (controllable means the server can issue
    remote-control commands back to that player).
  - `PlexServer::sessions()` — `GET /status/sessions` returns
    `Vec<PlayingSession>`.
  - `PlayingSession::stop(reason)` — `GET /status/sessions/terminate`
    with `sessionId` + optional `reason` (Plex requires GET on
    this mutation, same shape as `/:/scrobble` — preserved on the
    wire).
  - `MetadataDto`'s session-only fields (`sessionKey`,
    `viewOffset`, nested `User`/`Player`/`TranscodeSession`) live
    on a dedicated `SessionItemDto` that flattens the standard
    metadata fields so the existing `into_library_item` dispatch
    handles the item directly.
  - Transcode-only listing (`GET /transcode/sessions`) and the
    paginated history endpoint defer.
  - `tests/m4_sessions.rs` — 3 wiremock integration tests
    covering a mixed Movie + Track session list (with and
    without transcode), session termination with reason, and
    termination without reason.

- **M4.2 (Collection — list, items, delete)** — section-attached
  named groupings:
  - `media::Collection` — section-scoped collection with rating
    key, title, subtype (matches owning section kind), smart flag,
    leaf/child counts, collection_mode / collection_sort, composite
    image, thumb, art, timestamps, GUID, and a
    `LibrarySectionRef` back-link.
  - Unlike `Playlist`, Collection IS section-attached — so it
    composes naturally with the M3 trait suite. Implements
    `PlexObject` (metadata type 18), `Ratable`, `EditField`,
    `EditTitle`, `EditSummary`, `EditTags`, `HasGenres`,
    `HasCollections`, `HasLabels`, `HasArtUrl` + `HasArtLock`,
    `HasPosterUrl` + `HasPosterLock` — all the editing surface
    inherited from the foundational traits.
  - `LibrarySection::collections()` — `GET /library/sections/<id>/collections`
    returning `Vec<Collection>`.
  - `Collection::items()` — `GET /library/collections/<rk>/children`
    returning `Vec<LibraryItem>`.
  - `Collection::delete()` — `DELETE /library/collections/<rk>`.
  - Add / remove items, mode / sort tweaks, smart-collection
    mutation defer to follow-up iterations.
  - `tests/m4_collections.rs` — 3 wiremock integration tests
    covering list (static + smart), item walk, and DELETE.

- **M4.1 (Playlist — list, items, delete)** — first piece of the
  playback layer:
  - `media::Playlist` — server-level (not section-attached) ordered
    item collection. Holds the `HttpClient` and base URL directly
    so it can hit `/playlists/<rk>` endpoints. Carries the rating
    key, title, kind, smart flag, content URI (for smart
    playlists), duration, leaf counts, composite image,
    timestamps, GUID.
  - `media::PlaylistKind` enum (`Audio | Video | Photo | Other`)
    discriminating on Plex's `playlistType` wire field.
  - `PlexServer::playlists()` — `GET /playlists` listing all
    playlists on the server.
  - `Playlist::items()` — `GET /playlists/<rk>/items` returning
    `Vec<LibraryItem>` (mixed kinds dispatched on wire `type`).
    The `librarySectionID` Plex emits on each playlist item is
    wired into the per-item `LibrarySectionRef` so future edits
    can route through the right section.
  - `Playlist::delete()` — `DELETE /playlists/<rk>`, consumes
    `self`.
  - Smart playlist creation/mutation, item add/remove/move, and
    rename defer to follow-up iterations — they need
    server-URI construction for the `?uri=` parameter and the
    `playlistItemID` shadow keys (analysis/07 §4).
  - `tests/m4_playlists.rs` — 3 wiremock integration tests
    covering list (mixed static + smart), item walk with section
    back-link, and the DELETE endpoint shape.

- **M3.4/M3.5 expansion (macro-driven trait suite)** — fills out
  the field- and tag-family ergonomic-trait surface using two
  small declarative macros:
  - `declare_edit_field_trait!(TraitName, method_name, wire_field)`
    emits a `trait TraitName: EditField` with a single
    string-typed `method_name(value, locked)` method bound to the
    given wire field name. Used to land `EditTagline`,
    `EditStudio`, `EditContentRating`, `EditSortTitle` (wire form
    `titleSort` — Plex schema inconsistency preserved),
    `EditOriginalTitle`.
  - `EditYear` — hand-written numeric variant (the macro is
    string-only).
  - `declare_tag_trait!(TraitName, replace_fn, remove_fn, wire_field)`
    emits a `trait TraitName: EditTags` with `replace_*` /
    `remove_*` method pair. Used to land `HasDirectors`,
    `HasWriters`, `HasCountries`, `HasProducers`, `HasRoles`,
    `HasLabels`, `HasMoods`, `HasStyles`.
  - Both macros are `#[macro_export]` so downstream crates and
    examples can declare additional traits the same way.
  - Implementor coverage per leaf is now extensive:
    - Movie/Show/Episode: Tagline, Studio, ContentRating,
      SortTitle, OriginalTitle, Year, plus all 8 tag families
      (Genre, Collection, Director, Writer, Country, Producer,
      Role, Label).
    - Album: SortTitle, Studio, Year, Genre, Collection, Label,
      Mood, Style.
    - Artist: SortTitle, Genre, Collection, Label, Mood, Style.
    - Track: SortTitle, OriginalTitle, Genre, Collection.
    - Season: SortTitle only (limited edit surface on the wire).
  - No new tests — the wire-form correctness is already proven
    by `m3_edit_field.rs` and `m3_edit_tags.rs`; macro expansion
    just multiplies the surface.

- **M3.6 (Image URL + lock traits)** — six new traits across three
  image families:
  - `HasArtUrl` / `HasArtLock` — background-art (`art` wire field).
  - `HasPosterUrl` / `HasPosterLock` — poster (`thumb` wire field —
    Plex's confusing wire name for the full poster).
  - `HasThemeUrl` / `HasThemeLock` — theme song (`theme` wire field,
    Show-only).
  - `*Url` traits expose `*_url() -> Result<Option<Url>>` builders
    that resolve against the server base URL.
  - `*Lock` traits add `lock_*()` / `unlock_*()`. These emit just
    `<field>.locked=<0|1>` (no `.value` pair) — Plex's lock-toggle
    wire path differs from regular value edits. Implemented by a
    new `EditField::lock_field(field, locked)` primitive added to
    the foundational edit trait.
  - Implementors:
    - Movie / Show / Season / Episode: art + poster + (Show-only) theme.
    - Artist / Album: art + poster.
    - Track: poster only (Tracks inherit album art on the wire).
  - Full `HasArt` / `HasPoster` CRUD (`set_*` / `upload_*` /
    `delete_*`) needs Plex's `POST /library/metadata/<rk>/<kind>`
    endpoints + `post_bytes()` on the HTTP client; deferred to a
    follow-up iteration.
  - `tests/m3_images.rs` — 4 wiremock integration tests covering
    URL resolution for art + poster and the lock/unlock toggles
    on art. The lock test exposed (and the fix corrected) the
    distinction between value-edit (`<field>.value=` + `<field>.locked=`)
    and lock-only (`<field>.locked=`) wire forms.

- **M3.5 (EditTags + HasGenres + HasCollections)** — tag-family
  mutations:
  - `traits::EditTags` — two low-level primitives:
    - `replace_tags(field, items, locked)` — emits
      `<field>[i].tag.tag=v` per item plus `<field>.locked=<0|1>`,
      replacing the entire list.
    - `remove_tags(field, items, locked)` — emits the magic
      `<field>[].tag.tag-=csv` remove sigil (analysis/08 §3.4),
      stripping the named tags.
  - `traits::HasGenres` / `traits::HasCollections` — first
    per-family ergonomic traits, default-bodied via `EditTags`
    with the right field string baked in. The remaining tag
    families (Director, Writer, Country, Producer, Role, Label,
    Mood, Style) follow the same one-line pattern.
  - Implementors: Movie / Show / Episode (Genre + Collection),
    Album / Track / Artist (Genre + Collection where the wire
    schema supports it).
  - "Add" semantics (read-modify-write — fetch current list,
    prepend new) deferred to the EditBatch transaction in a
    future iteration. For now, callers can compose
    `replace_tags(field, [&existing..., &new..], …)` themselves.
  - `tests/m3_edit_tags.rs` — 3 wiremock integration tests
    covering replace, remove (with the trailing `-` sigil), and
    the collection-family alias.

- **M3.4 (EditField + EditTitle + EditSummary)** — the universal
  metadata-edit primitive:
  - `traits::EditField` — single low-level `edit_field(field,
    value, locked)` method that emits the wire-format URL Plex
    actually expects:
    `PUT /library/sections/<section_id>/all?id=<rating_key>&type=<N>&<field>.value=<v>&<field>.locked=<0|1>`.
    The endpoint is on the *section*, not the metadata item, even
    though the item is what's being edited — see analysis/11 §2.4.
    The `LibrarySectionRef` back-link on every leaf carries the
    `section_id` and (via the `metadata_type_id()` accessor added
    in this commit) the `type` discriminator.
  - `traits::FieldValue` — typed enum (`Str | Int | Float |
    Bool`) with `From` impls for `&str`, `String`, `i64`, `i32`,
    `u32`, `u16`, `f32`, `bool`. Display renders the wire form
    (e.g. `Bool(true)` → `"1"`).
  - `traits::EditTitle` / `traits::EditSummary` — first
    field-specific traits, default-bodied via `EditField`.
    `impl EditTitle for Movie {}` is all a leaf type needs. The
    remaining ~30 field-specific traits (`EditTagline`,
    `EditContentRating`, `EditStudio`, `EditYear`, …) follow the
    same one-line pattern; they land in a follow-up iteration.
  - `PlexObject` gains `section_ref()` (returning
    `&LibrarySectionRef`) and `metadata_type_id()` (returning the
    `?type=N` integer) as required methods; `http()` and
    `base_url()` become default-derived. Every leaf type's
    `impl_plex_object*!` macro invocation now also threads the
    type discriminator.
  - Implementors of `EditField` / `EditTitle` / `EditSummary`:
    Movie / Show / Season / Episode / Artist / Album / Track.
    Photoalbum and Photo are excluded for now — their edit
    surface differs slightly and lands with the photo-specific
    traits.
  - `tests/m3_edit_field.rs` — 3 wiremock integration tests
    proving the section-keyed wire shape, the lock-flag round-trip,
    and percent-encoding of special characters.

- **M3.3 (Ratable trait)** — set / clear the user's personal rating
  on an item:
  - `traits::Ratable` — single `rate(Option<f32>)` method. `None`
    clears (wire sentinel `-1`); `Some(v)` requires
    `v ∈ [0.0, 10.0]` (Plex's 0-to-5-stars × 2 scale).
    Out-of-range values surface as `Error::Config` before any
    HTTP traffic.
  - Wire endpoint: `PUT /:/rate?key=<rating_key>&identifier=com.plexapp.plugins.library&rating=<value>`.
  - Implemented on Movie / Show / Episode / Album / Track.
    Season's rating field is rarely user-set on the wire so the
    impl is intentionally omitted.
  - `tests/m3_ratable.rs` — 3 wiremock integration tests covering
    the happy path, the `None` clear, and client-side range
    validation.

- **M3.1 (Foundational traits + PlayedUnplayed)** — first mutation
  surface and the trait architecture it rides on:
  - `traits::PlexObject` — supertrait every capability trait
    builds on. Three accessors: `http()` → `&HttpClient`,
    `base_url()` → `&Url`, `rating_key()` → `RatingKey`. Implemented
    on Movie / Show / Season / Episode / Artist / Album / Track via
    two small `impl_plex_object*!` macros.
  - `traits::PlayedUnplayed` — `view_count()` reader plus default
    bodies for `is_played()`, `mark_played()`, `mark_unplayed()`.
    The two `mark_*` methods issue `GET /:/scrobble` and
    `/:/unscrobble` respectively with
    `key=<rating_key>&identifier=com.plexapp.plugins.library`. Plex
    requires GET for these despite them being mutations; preserved
    on the wire (analysis/11 §4.10) but exposed as `mark_*` verbs
    on the public surface.
  - Implemented on Movie / Episode / Show / Season / Album /
    Artist / Track — every type that carries a `view_count` field
    on the wire. The trait uses Rust 2024 AFIT (async fn in
    traits) so callers don't need the `async_trait` macro.
  - Inherent `is_played()` methods preserved on Movie/Episode/Track
    alongside the trait so callers don't have to import the trait
    just to read the boolean.
  - `tests/m3_played_unplayed.rs` — 3 wiremock integration tests
    covering the scrobble + unscrobble endpoint shapes plus the
    inherent/trait `is_played` agreement.

- **M2.9 (FilterBuilder)** — typed search expression builder for the
  section-listing surface:
  - `library::FilterBuilder` — fluent, named-op API:
    `.equal()` / `.not_equal()` / `.exact()` / `.not_exact()` /
    `.starts_with()` / `.ends_with()` / `.gt()` / `.lt()` /
    `.and_values()` / `.clause(field, FilterOp, value)`. Plus
    `.sort_by()` / `.sort_by_desc()` / `.limit()` / `.offset()` /
    `.page_size()` / `.libtype()`.
  - `library::FilterOp` enum maps every named op to the canonical
    Plex wire suffix per python-plexapi `library.py:1442-1460`
    (`=`, `!=`, `==`, `!==`, `<=`, `>=`, `>>=`, `<<=`, `&=`).
  - `library::SortDirection` (`Asc | Desc`) renders as
    `field:asc` / `field:desc`.
  - `FilterBuilder::build_query()` emits the URL query string
    suffix with RFC 3986 percent-encoding.
  - `LibrarySection::filter(&builder)` executes the filter
    against `GET /library/sections/<id>/all?<query>` and parses
    the response as `Vec<LibraryItem>`.
  - `src/library.rs` → `src/library/mod.rs`; `filters` is the
    first sub-module.
  - Client-side `__icontains`/`__gte` Python-style suffixes
    deferred to M3.
  - `tests/m2_filter.rs` — 2 wiremock integration tests covering
    the full chain wire form and the empty-builder fallback.

- **M2.8 (LibraryItem + mixed-content listings)** — search and
  curated-list surfaces:
  - `media::LibraryItem` — sum type discriminating on Plex's wire
    `type` field. Nine variants: Movie / Show / Season / Episode /
    Artist / Album / Track / Photoalbum / Photo.
    `LibraryItem::title()` / `rating_key()` hide the variant.
  - `MetadataDto::into_library_item()` performs the dispatch;
    unknown `type` values surface as `Error::Config`.
  - `LibrarySection::search(title)` — `GET /library/sections/<id>/all?title=<q>`
    using a hand-written RFC 3986 percent-encoder (no `url`-crate
    dependency for query construction).
  - `LibrarySection::recently_added()` —
    `GET /library/sections/<id>/recentlyAdded`.
  - `LibrarySection::on_deck()` —
    `GET /library/sections/<id>/onDeck`.
  - `LibrarySection::unwatched()` —
    `GET /library/sections/<id>/unwatched`.
  - All four return `Vec<LibraryItem>` so callers can pattern-match
    on the variant.
  - `tests/m2_search.rs` — 4 wiremock integration tests covering
    title search, mixed-type recently-added, empty on-deck, and
    unknown-`type` error propagation.

- **M2.7 (Read-only media — Markers + Chapters)** — playable-video
  navigation surfaces:
  - `media::Marker` — auto-detected intro/credits/commercial range
    with `start_ms`, `end_ms`, and a `final_credits` flag for the
    end-of-show credits (Plex's post-credits-scene detection).
    `Marker::duration_ms()` and `Marker::contains(time_ms)`
    convenience helpers.
  - `media::MarkerKind` enum (`Intro | Credits | Commercial |
    Other(String)`) — `Other` preserves wire-format strings Plex
    adds later.
  - `media::Chapter` — embedded DVD-style scene index entry with
    optional title, index, end time, and per-chapter thumb.
  - `Movie` and `Episode` gain `markers: Vec<Marker>` and
    `chapters: Vec<Chapter>`. Music and photos don't carry these
    on the wire — left off.

- **M2.6 (Read-only media — Tags)** — `Genre`/`Director`/`Writer`/
  `Country`/`Producer`/`Role`/`Collection`/`Label`/`Mood`/`Style`
  child elements collapsed into a unified `Tag` type:
  - `media::Tag` carries `kind: TagKind`, `value`, optional `id`
    (numeric Plex tag id used by edit operations), `role` and
    `thumb` (for actor `<Role>` entries), and `filter` (the
    smart-filter URI Plex uses for "find more like this").
  - `media::TagKind` enum with all 10 known families plus
    `Other(String)` forward-compat.
  - `Movie`, `Show`, `Episode`, `Album`, `Track` gain
    `tags: Vec<Tag>` populated by `MetadataDto::collect_tags()`.
    `Artist`, `Photo`, `Photoalbum`, `Season` don't carry tags on
    the wire — left out by design.
  - `Field` (per-field edit-lock indicator) intentionally not
    modelled as a `Tag` — different shape, lands with the edit
    traits in M3.

- **M2.5 (Read-only media — Media/Part/Stream chain)** — file-level
  metadata for every playable type:
  - `media::Media` — one re-encode of a playable item (quality /
    container variant). Holds duration, bitrate, dimensions,
    aspect ratio, audio channels + codec, video codec, container,
    frame rate + resolution buckets, optimised-for-streaming flag,
    and a `Vec<MediaPart>` of the underlying files.
  - `media::MediaPart` — one file on disk. Carries the download
    key, filesystem path, size, container, duration,
    has-thumbnail / optimised-for-streaming flags, and a
    `Vec<Stream>` of contained tracks.
  - `media::Stream` — sum type
    `Video(VideoStream) | Audio(AudioStream) | Subtitle(SubtitleStream) | Lyric(LyricStream) | Unknown(UnknownStream)`
    dispatched on Plex's `streamType` discriminator. Per-variant
    fields cover codec, language, dimensions, frame rate, channel
    layout, bitrate, sampling rate, bit depth, default/selected/
    forced flags, display titles, and external-track keys.
  - `Movie`, `Episode`, `Track`, `Photo` gain a
    `media: Vec<Media>` field populated when the source endpoint
    emits `Media[]` (always empty for plain `?type=N` listings;
    populated on `/library/metadata/<rk>` direct fetches).
  - Shared `MetadataDto` learns the wire-format `Media[]` →
    `MediaDto[]` mapping; conversion methods now pre-compute the
    typed chain.

- **M2.4 (Read-only media — Photos)** — Photoalbum / Photo:
  - `media::Photoalbum` — top-level photo container, supports
    nesting. `children()` returns a `PhotoEntry` sum type mixing
    sub-albums and photos; `sub_albums()` / `photos()` filter
    convenience helpers built on top.
  - `media::Photo` — single photo (or video clip in a photo
    section) with parent-album back-reference, EXIF caption,
    capture year, position index, GUID. Width/height land with
    Media/Part/Stream in M2.5.
  - `media::PhotoEntry` — `Album(Photoalbum) | Photo(Photo)` sum
    type for mixed listings.
  - `MetadataDto` gains a `metadata_type` field (renamed from the
    wire `type`) so the photo path can dispatch on
    `photoalbum`/`photo`/`clip` discriminators.
  - `LibrarySection::photoalbums()` — `?type=14` dispatch on
    `SectionKind::Photo`.
  - `tests/m2_photos.rs` — 1 wiremock integration test covering
    the full mixed-children walk and both convenience filters.

- **M2.3 (Read-only media — Music hierarchy)** — Artist / Album / Track:
  - `media::Artist` — top-level music entity with `child_count`
    (number of albums), bio summary, image surface, and
    `Artist::albums()` listing helper.
  - `media::Album` — parent (artist) typed back-reference, year +
    release date, label/studio, leaf_count + viewed_leaf_count,
    rating. `Album::tracks()` lists tracks.
  - `media::Track` — leaf playable with parent (album) +
    grandparent (artist) back-references, index (track within
    disc), `disc_number` (mapped from Plex's `parentIndex`, which
    is intentionally counterintuitive), duration, view count + offset,
    `original_title` for compilation per-track artist, GUID.
    `Track::is_played()` helper.
  - `LibrarySection::artists()` — `?type=8` dispatch on
    `SectionKind::Music`. Shared `list_typed()` boilerplate-eliminator
    from M2.2 reused.
  - `tests/m2_music.rs` — 2 wiremock integration tests covering
    Artist → Album → Track walk and kind-mismatch error path.

- **M2.2 (Read-only media — TV hierarchy)** — Show / Season / Episode:
  - `media::Show` — 24 scalar fields including child_count (seasons),
    leaf_count (total episodes), viewed_leaf_count (played episodes),
    theme path, network/studio. `Show::seasons()` lists seasons via
    `GET /library/metadata/<rk>/children`. `Show::watch_progress()`
    returns `viewed_leaf_count / leaf_count`.
  - `media::Season` — parent_rating_key (typed `RatingKey`),
    parent/show metadata back-link, index (season number),
    leaf_count / child_count / viewed_leaf_count.
    `Season::episodes()` lists episodes via the same `/children`
    endpoint.
  - `media::Episode` — parent (season) and grandparent (show) typed
    back-references, index (episode number), parent_index (season
    number), summary, duration, view-count/offset, full image and
    GUID surface. `Episode::season_episode_label()` returns the
    `S01E03`-style display label.
  - `LibrarySection::shows()` — analogous to `movies()`, dispatches
    on `SectionKind::Show` and queries with `?type=2`. Internal
    `list_typed()` helper eliminates the listing-method
    boilerplate.
  - `tests/m2_tv.rs` — 4 wiremock integration tests covering the
    full Show → Season → Episode walk plus the kind-mismatch error
    path.

- **M2.1 (Read-only media — Movie)** — first content type:
  - `media::Movie` — 24 scalar fields covering Plex's `<Video type="movie">`
    payload (rating_key, title, year, summary, rating triple, duration,
    view count + offset, GUIDs, thumb/art paths, timestamps).
  - `LibrarySection::movies()` lists every movie in a movie section via
    `GET /library/sections/<id>/all?type=1`; returns `Error::Config`
    when called on a non-movie section.
  - `Movie::is_played()`, `Movie::thumb_url()` convenience accessors.
  - `tests/m2_movies.rs` — 2 wiremock integration tests.

- **M1 (Minimum viable client)** — first wire I/O surface, token sign-in
  only:
  - `HttpClient` is now `Clone` (reqwest's underlying client is
    `Arc`-shared, so cloning is cheap).
  - `server::PlexServer` — `connect(url, token)`,
    `connect_with_config()`, `from_http()`, `identity()`, `library()`,
    `ping()`. Eagerly parses `GET /` into `ServerIdentity`.
  - `server::ServerIdentity` — captures machine identifier, version,
    friendly name, platform, MyPlex linkage flags, capabilities. Parses
    Plex's flexible boolean encoding (`"0"`/`"1"`/`0`/`1`/`true`).
  - `library::Library` — bound to a PMS, exposes `sections()`.
  - `library::LibrarySection` — typed section with `SectionKind` enum
    (`Movie | Show | Music | Photo | Other`) and a `LibrarySectionRef`
    back-link for future edit-trait URL construction.
  - `tests/m1_server_library.rs` — 5 wiremock-driven end-to-end tests
    covering identity parsing, 401 surfacing, section listing, and
    edit-URL construction.

[Unreleased]: https://github.com/justdewey/plex-rs/compare/HEAD...HEAD
