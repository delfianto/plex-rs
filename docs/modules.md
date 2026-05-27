# Module reference

Per-module deep dive. Modules are organised by top-level
directory in `src/`. For high-level architecture decisions that
span multiple modules, see [`architecture.md`](./architecture.md).

| Module | LOC* | Purpose |
|---|---:|---|
| [`client.rs`](#srcclientrs) | ~330 | `HttpClient` — the single async I/O surface |
| [`config.rs`](#srcconfigrs) | ~190 | `ClientConfig` builder |
| [`error.rs`](#srcerrorrs) | ~120 | typed `Error` enum + status mapping |
| [`headers.rs`](#srcheadersrs) | ~190 | X-Plex-* identity headers |
| [`pagination.rs`](#srcpaginationrs) | ~210 | header-based pagination primitives |
| [`uri.rs`](#srcurirs) | ~370 | `PlexUri` enum covering 7 schemes |
| [`server.rs`](#srcserverrs) | ~430 | `PlexServer` connection + identity |
| [`util/`](#srcutil) | ~910 | newtypes, time helpers, sanitiser |
| [`xml/`](#srcxml) | ~240 | `MediaContainer<T>` generic envelope |
| [`auth/`](#srcauth) | ~510 | PIN sign-in, password+2FA |
| [`library/`](#srclibrary) | ~1240 | sections + FilterBuilder + SmartFilter parser |
| [`media/`](#srcmedia) | ~3200 | every leaf type (Movie/Show/…/Playlist/Collection) |
| [`traits/`](#srctraits) | ~1450 | M3 capability trait architecture |
| [`server/`](#srcserver) | ~1890 | sessions + history + settings + admin |
| [`playback/`](#srcplayback) | ~1520 | PlayQueue + PlexClient + transcode URLs |
| [`myplex/`](#srcmyplex) | ~2300 | all plex.tv endpoints |
| [`alerts/`](#srcalerts) | ~630 | WebSocket alert stream (feature) |
| [`webhook/`](#srcwebhook) | ~390 | inbound webhook + axum extractor (feature) |
| [`discover_gdm/`](#srcdiscover_gdm) | ~190 | GDM local UDP discovery (feature) |

\* Excluding inline `#[cfg(test)]` modules. Counts approximate.

---

## `src/client.rs`

`HttpClient` is the single point of network I/O. Everything
funnels through it.

Key surface:

- `HttpClient::new(config) -> Result<Self>` — builds the inner
  `reqwest::Client` with the identity headers baked in as default
  headers. The token is one of them, so changing the token requires
  a fresh `HttpClient` (cheap — `reqwest::Client` is `Arc`-backed).
- `get_json::<T>(url)`, `get_bytes(url)`, `put_no_body(url)`,
  `delete(url)`, `post_json::<B, T>(url, body)` — the standard
  primitives.
- `get_json_with_headers::<T>(url, &[(&str, &str)])` /
  `get_bytes_with_headers` — pagination's escape hatch.
- `get_bytes_for_method(method, url)` (crate-private) —
  method-parametric primitive used by PlayQueue's PUT/DELETE
  mutations.
- `inner()` (crate-private) — escape hatch for modules that need
  to bypass the standard JSON-with-retry envelope (just the
  password sign-in flow today).
- `retry_delay(attempt, base, max)` — free function for the
  full-jitter exponential backoff math. Unit-tested in isolation.
- Custom `Debug` impl that elides the token and the inner reqwest
  client.

**Design notes:**

- The retry envelope and status-to-`Error` mapping live entirely
  here — no other module decides what a 401 becomes.
- `Clone` is implemented and cheap (just clones an inner `Arc`
  and the `ClientConfig`).

---

## `src/config.rs`

`ClientConfig` and `ClientConfigBuilder`.

Required:
- `client_identifier: ClientIdentifier` (stable per install)

Optional:
- `token: Option<PlexToken>`
- `identity: PlexIdentity` (defaults if absent)
- `request_timeout: Duration` (default 30s)
- `connect_timeout: Duration` (default 8s)
- `max_retries: u32` (default 3)
- `retry_base_delay: Duration` (default 250ms)
- `retry_max_delay: Duration` (default 30s)
- `user_agent: Option<String>` (overrides the default)

Builder is typestate-light: `.client_identifier()` is required;
everything else has a setter that returns `Self`. `.build()`
returns `Result<ClientConfig>`.

---

## `src/error.rs`

The crate-wide `Error` enum. See
[`architecture.md`](./architecture.md#error-model) for the variant
list and design rationale. `Result<T>` alias for convenience.

Two key methods:
- `Error::from_status(status, body, path) -> Error` — central
  HTTP-status-to-variant mapping.
- `Error::is_retryable() -> bool` — pure classifier used by the
  retry envelope.

---

## `src/headers.rs`

`PlexIdentity` carries the 10 `X-Plex-*` headers Plex expects on
every request: product, version, client identifier, platform,
device, device name, model, screen resolution, screen density,
provides.

`PlexIdentity::headers(token)` constructs the `reqwest::HeaderMap`
to bake into `reqwest::Client::default_headers`. Strict ASCII
validation rejects any non-ASCII value with `Error::InvalidHeader`.

The default identity uses `plex-rs` as product and a generated
client identifier, but it's overridable on `ClientConfig`. Long-
lived agents should provide a stable client identifier for proper
session continuity.

---

## `src/pagination.rs`

`PageRange { start, size }` + `advance_with(&meta)`.

The interesting bit is `advance_with` — given a `MediaContainerMeta`
from the response, decide whether to fetch another page:

```rust
match (meta.total_size, meta.size, self.size) {
    (Some(total), _, _) if self.start + meta.size >= total => None,
    (_, returned, requested) if returned < requested => None,
    _ => Some(PageRange::new(self.start + self.size, self.size)),
}
```

Conservative: stops when the server returned fewer items than
asked, even when total_size is unknown.

---

## `src/uri.rs`

`PlexUri` enum covering 7 schemes that show up in Plex's smart
filter / playlist / play-queue URIs:

- `Server { mid }` — `server://<machine-id>/...`
- `LibraryItem { item_key }` — `library://...`
- `LibraryDirectory { path }` — `library:///directory/...`
- `Playlist { rk }` — `playlist:///...`
- `PlayQueueContainer { id, args }` — `/playQueues/...`
- `Device { id }` — `https://plex.tv/devices/.../sync_items`
- `SecurityToken { rest }` — `/security/token`

Hand-written prefix parser, round-trip stable, no `winnow`/`nom`
dependency. Used by the smart-filter URI parser
(`src/library/smart_filter.rs`).

---

## `src/server.rs`

`PlexServer` — the connected-to-a-PMS handle.

Construction:
- `PlexServer::connect(base_url, token) -> Result<Self>` — eagerly
  hits `GET /` to validate the connection and populate
  `ServerIdentity`.
- `connect_with_config(base_url, config)` — same but with custom
  `ClientConfig`.
- `from_http(base_url, http)` — reuse an existing `HttpClient`.
- `__test_new(...)` — `#[doc(hidden)]` constructor that skips
  the identity probe, for tests that just want to hand a
  `PlexServer` to URL-building code.

`ServerIdentity` carries: `machine_identifier`, `version`,
`friendly_name`, `platform`, `my_plex_*` flags, `livetv`,
`allow_media_deletion`, `allow_sharing`, `updated_at`.

Accessors: `library()`, `playlists()`, `sessions()`, `history()`,
`settings()`, `activities()`, `butler_tasks()`, `updater_status()`,
`bandwidth_stats()`, `resource_stats()`, `play_queue(id)`,
`create_play_queue()`. These come from the per-feature modules
under `src/server/` and `src/playback/`.

`PlexBool` (`pub(crate)` flexible-boolean deserializer) handles
the wire-format inconsistency where Plex sometimes emits booleans
as `"0"`/`"1"` and sometimes as `0`/`1` and sometimes as
`true`/`false`. Used by every DTO that needs it.

---

## `src/util/`

| File | Contents |
|---|---|
| `ids.rs` | `RatingKey`, `MachineIdentifier`, `ClientIdentifier`, `PlexToken`, `PlayQueueId` — all `#[serde(transparent)]`. `PlexToken` has the hand-written redacted `Debug`. |
| `time.rs` | Epoch-seconds and ISO-8601 parsers, serde adapters for string-encoded epoch fields. |
| `search_type.rs` | `SearchType` enum mirroring python-plexapi's `utils.py:35` table. Forward-compat `Unknown(u32)`. |
| `sanitize.rs` | Fixture sanitiser used during analysis — replaces tokens, IPs, machine IDs with deterministic placeholders. 14 regex rules + IPv4/IPv6 classifier. |
| `mod.rs` | Re-exports the four sub-modules. |

---

## `src/xml/`

`MediaContainer<T>` — the generic envelope Plex wraps every
listing response in. `MediaContainer::from_json(body, items_key)`
parses an arbitrary `<MediaContainer>{ <T-list-named-items_key> }`
shape.

`MediaContainerMeta` carries the common scalar fields every
container emits: `size`, `total_size`, `offset`, `identifier`,
`media_tag_prefix`, `media_tag_version`, `title`, `title1`,
`title2`, `library_section_id`, `library_section_title`,
`library_section_uuid`, `more`, `allow_sync`.

---

## `src/auth/`

Three Plex authentication flows:

- **`pin.rs`** — `MyPlexPinLogin` for the PIN / OAuth flow.
  `start()` mints a fresh PIN via plex.tv; `poll()` returns
  `Ok(Some(token))` once claimed, `Ok(None)` while pending;
  `wait(timeout, interval)` is the convenience polling loop.
- **`password.rs`** — `MyPlexPasswordLogin` for the
  username/password flow. `sign_in()` posts the credentials;
  on a 2FA-protected account, returns `Error::TwoFactorRequired`.
  Caller then calls `sign_in_with_code(login, password, otp)`.
  Wire format: `application/x-www-form-urlencoded` POST to
  `/api/v2/users/signin`.
- **Direct token** — no module; just pass the token to
  `PlexServer::connect` or `MyPlexClient::new`.

Both PIN and password modules expose `.with_endpoint(url)` for
test replicas.

---

## `src/library/`

| File | Contents |
|---|---|
| `mod.rs` | `Library`, `LibrarySection`, `LibrarySectionRef`, `SectionKind`. Section listing methods (movies, shows, artists, photoalbums, collections), search, recentlyAdded, onDeck, unwatched, filter(). |
| `filters.rs` | `FilterBuilder` with `.equal`/`.not_equal`/`.exact`/`.starts_with`/`.ends_with`/`.gt`/`.lt` named methods. `FilterOp` enum maps to wire-format operator suffixes per python-plexapi `library.py:1442-1460`. Sort + limit + offset + page-size + libtype. |
| `smart_filter.rs` | Read-only parser for Plex smart-playlist / smart-collection filter URIs. `SmartFilter::from_uri(s)` produces typed `(section_id, libtype, clauses, group_markers, sort)` breakdown. |

`LibrarySectionRef { id, http, base_url }` is the back-reference
every leaf metadata type carries so M3 edit traits can construct
mutation URLs without re-traversing through `PlexServer`. See
[`design-patterns.md`](./design-patterns.md#library-section-back-reference).

---

## `src/media/`

The largest module. Every Plex leaf type lives here.

| File | Types |
|---|---|
| `video.rs` | `Movie`, `Show`, `Season`, `Episode` + shared `MetadataDto` |
| `audio.rs` | `Artist`, `Album`, `Track` |
| `photo.rs` | `Photoalbum`, `Photo`, `PhotoEntry` sum type |
| `playlist.rs` | `Playlist`, `PlaylistKind` |
| `collection.rs` | `Collection` (with full M3 edit-trait composition) |
| `streams.rs` | `Media`, `MediaPart`, `Stream` (sum type of `Video`/`Audio`/`Subtitle`/`Lyric`/`Unknown`) |
| `tags.rs` | `Tag`, `TagKind` enum (Genre/Director/Writer/Country/Producer/Role/Collection/Label/Mood/Style/Other) |
| `markers.rs` | `Marker`, `Chapter`, `MarkerKind` |
| `mod.rs` | `LibraryItem` sum type — every leaf type a mixed listing can return. Has `.title()`, `.rating_key()`, `.key()`, `.list_type()` accessors. |

The `MetadataDto` is shared by every leaf — Plex's wire shape is
the same for movies, episodes, tracks, photos with only a few
fields per kind being optional. `MetadataDto::into_library_item`
dispatches on the `type` discriminator to produce the right
`LibraryItem` variant.

`Movie::reload()`, `Episode::reload()`, etc. (the `Reload` trait)
re-fetch `/library/metadata/<rk>` and produce the "full" version
of an item — listings emit partials.

---

## `src/traits/`

M3 trait architecture. See
[`architecture.md`](./architecture.md#trait-architecture-m3) for
the diagram.

| File | Contents |
|---|---|
| `mod.rs` | `PlexObject` supertrait. `impl_plex_object!` and `impl_plex_object_with_type!` macros that install it on the 8 leaf types. |
| `played_unplayed.rs` | `PlayedUnplayed::mark_played/mark_unplayed` via `/:/scrobble` + `/:/unscrobble`. |
| `ratable.rs` | `Ratable::rate(Option<f32>)`. `-1` clears; range-validated 0..=10. |
| `reload.rs` | `Reload::reload(self) -> Result<Self::Full>`. Re-fetches and re-converts. |
| `playable.rs` | `Playable::direct_play_url()` — token-bearing URL pointing at the first part's wire key. |
| `edit_field.rs` | `EditField` universal primitive + `FieldValue` enum + 8 field-specific traits (`EditTitle`/`EditSummary`/`EditYear`/etc.) generated via `declare_edit_field_trait!`. |
| `edit_tags.rs` | `EditTags` universal primitive + 10 tag-family traits (`HasGenres`/`HasCollections`/etc.) generated via `declare_tag_trait!`. |
| `images.rs` | `HasArtUrl`/`HasPosterUrl`/`HasThemeUrl` (URL builders) + `HasArtLock`/`HasPosterLock`/`HasThemeLock` (lock toggles). |
| `edit_batch.rs` | `EditBatch` — one PUT for many edits. `EditBatchExt` adds `.batch()` to every type with `EditField`. |

Macros (`#[macro_export]`) are exposed so downstream users can
declare new field/tag traits matching the same shape.

---

## `src/server/`

Sub-modules of `PlexServer` for the specific endpoint families:

| File | Methods on PlexServer |
|---|---|
| `sessions.rs` | `.sessions()` → `Vec<PlayingSession>`. Each session can `.stop(reason)`. |
| `history.rs` | `.history()` → `HistoryQuery` builder. Filter by account/section/rating_key/mindate; terminate with `.collect()` or `.stream()`. |
| `settings.rs` | `.settings()` → `Settings`. Read via `.get(id)/.all()/.group()`. Write via `.set(server, id, value)` or `.set_many(server, updates)`. Client-side validation. |
| `admin.rs` | `.activities()`, `.butler_tasks()`, `.updater_status()`, `.bandwidth_stats(&opts)`, `.resource_stats()`. Monitoring read-only surfaces. |

---

## `src/playback/`

| File | Contents |
|---|---|
| `play_queue.rs` | `PlayQueue`, `PlayQueueItem`, `CreatePlayQueue` builder. Server-side queue API. |
| `client.rs` | `PlexClient` — remote control of a Plex player on `:32500`. 11 nav commands + 11 playback commands + flagship `play_media(&server, &queue, offset_ms)`. |
| `transcode.rs` | `TranscodeOptions` builder + `build_for(&server, item_key)` → `Url`. Universal transcoder URL for HLS / DASH. |

`PlexClient` is the largest type here — see
[`architecture.md`](./architecture.md) and
[`api-coverage.md`](./api-coverage.md#playback-control)
for the full command list.

---

## `src/myplex/`

Every plex.tv-hosted endpoint family. All of them hang off
`MyPlexClient`.

| File | Methods on MyPlexClient |
|---|---|
| `mod.rs` | `MyPlexClient::new(token, cid, identity)`, `with_base/with_discover_base/with_metadata_base` overrides. |
| `resources.rs` | `.resources()`, `.resource(name)`. `MyPlexResource::connect()` races concurrent probes across every preferred URI. |
| `devices.rs` | `.devices()` → `Vec<MyPlexDevice>`. `device.delete(&client)` revokes per-device token. |
| `friends.rs` | `.friends()` → `Vec<MyPlexUser>`. `.remove_friend(id)`. |
| `home.rs` | `.home_users()` → `Vec<MyPlexHomeUser>`. (Read-only.) |
| `webhooks.rs` | `.webhooks()`, `.add_webhook(url)` (idempotent), `.delete_webhook(url)`, `.set_webhooks(&urls)`. |
| `watchlist.rs` | `.watchlist()` / `.watchlist_with(&opts)`, `.add_to_watchlist(rk)`, `.remove_from_watchlist(rk)`. |
| `discover.rs` | `.discover_search(query, &opts)` → `Vec<DiscoverItem>`. |
| `metadata_provider.rs` | `.user_state(rk)`, `.scrobble(rk)`, `.unscrobble(rk)`. |

The three plex.tv base URLs (`plex.tv`, `discover.provider.plex.tv`,
`metadata.provider.plex.tv`) are kept on `MyPlexClient` with
separate `with_*_base` overrides so integration tests can point
at distinct wiremock replicas.

---

## `src/alerts/`

Real-time PMS event stream over WebSocket. **Feature-gated:
`alerts`** pulls in `tokio-tungstenite`.

`Alerts::connect(&server)` opens `ws://...../:/websockets/notifications`
with `X-Plex-Token=...` in the query (the WS endpoint doesn't
accept the standard X-Plex headers).

Returns `Alerts: Stream<Item = Result<AlertEvent>>` where each
frame (which may carry multiple inner notifications) flattens to
N stream items.

`AlertEvent` variants: `Playing`, `Timeline`, `Activity`,
`TranscodeSession` (with `TranscodeLifecycle::Start/Update/End`),
`Status`, `Reachability`, `Setting`,
`BackgroundProcessingQueue`, `Unknown { kind, raw }`.

Per-variant DTOs cover the documented fields per the analysis
notes (now consolidated in [`api-coverage.md`](./api-coverage.md)).
`Alerts::connect_with_url(ws_url)` is the test-friendly primitive
the integration tests drive against a `tokio::net::TcpListener` +
`accept_hdr_async` WS replica.

---

## `src/webhook/`

Inbound Plex webhook handling. **Feature-gated: `webhook-axum`**.

`WebhookPayload::from_json(raw)` decodes the JSON payload Plex
ships as the `payload` form field. `WebhookEvent` enum covers the
12 documented events (media.play/pause/resume/stop/scrobble/rate,
library.on.deck/new, admin.database.backup/corrupted, device.new,
playback.started) plus `Unknown(String)`.

`impl FromRequest<S> for WebhookPayload` provides the
axum extractor: parses multipart/form-data, finds the `payload`
text field, decodes the JSON, captures any `thumb` binary
attachment as `Bytes`. Typed `WebhookRejection` maps to 400 Bad
Request via `IntoResponse`.

`WebhookMetadata` is a small projection (rating_key, key, type,
title, grandparent_title, library_section_*) with the full raw
JSON flattened into `raw: serde_json::Value` for unprojected
fields.

The complementary webhook URL registration on plex.tv lives in
[`src/myplex/webhooks.rs`](#srcmyplex), not here.

---

## `src/discover_gdm/`

Local LAN PMS discovery via raw UDP multicast. **Feature-gated:
`discovery`** (pulls in `tokio/net`).

GDM is HTTP/1.0 `M-SEARCH` payload over multicast to
`239.0.0.250:32414` for servers (`:32412` for clients, not
implemented). **Not mDNS** — that's a common confusion; the
Bonjour libraries don't apply.

`discover_local_servers(timeout)` sends one M-SEARCH and collects
replies until the timeout. Each reply parses to a `GdmEntry`
with `name`, `port`, `version`, `host`, `resource_identifier`.
Dedup is by resource-identifier (so multi-homed PMS doesn't
return duplicate entries).

`GdmEntry::base_url()` builds the PMS HTTP URL.
