# API coverage

What's implemented, organized by Plex endpoint family. Wire-format
notes for the trickier parts of each surface.

| Status | Meaning |
|:---:|---|
| ✅ | Fully implemented; no realistic caller is missing anything |
| 🟡 | Partial — core operations work, peripheral ones deferred |
| 🚫 | Explicitly out-of-scope; see [`out-of-scope.md`](./out-of-scope.md) |
| ⬜ | Not yet implemented; reasonable target if someone files an issue |

---

## Authentication

| Status | Surface | Module |
|:---:|---|---|
| ✅ | Direct token (caller has one) | `ClientConfig::token` / `PlexServer::connect` |
| ✅ | PIN / OAuth | `auth::MyPlexPinLogin` |
| ✅ | Username + password + 2FA | `auth::MyPlexPasswordLogin` |
| ⬜ | Sign-out | — |

The PIN flow polls `/api/v2/pins/<id>` after creating one with
`/api/v2/pins?strong=true`. The password flow POSTs form-encoded
data to `/api/v2/users/signin` and surfaces `Error::TwoFactorRequired`
(distinct from `Error::Unauthorized`) when the response carries
the `code: 1029` envelope.

---

## Server discovery + connection

| Status | Surface | Module |
|:---:|---|---|
| ✅ | List server resources from plex.tv | `MyPlexClient::resources` |
| ✅ | Look up resource by name | `MyPlexClient::resource(name)` |
| ✅ | Race concurrent connect probes | `MyPlexResource::connect` |
| ✅ | Local LAN discovery (GDM multicast) | `discover_gdm::discover_local_servers` |
| ✅ | Eager connect via `GET /` identity probe | `PlexServer::connect` |
| 🚫 | mDNS/Bonjour | (GDM is the actual Plex protocol; mDNS doesn't apply) |

`MyPlexResource::connect()` is the flagship — it races every
preferred connection URI (local → remote → relay; https → http;
shared resources skip local) with `FuturesUnordered` and returns
the first `PlexServer` that answers `GET /` within the per-attempt
timeout. The per-resource access token is used, not the account
token.

`discover_gdm` is gated behind the `discovery` Cargo feature
because it needs `tokio/net` and most callers don't need LAN
discovery.

---

## Library + sections

| Status | Surface | Module |
|:---:|---|---|
| ✅ | List sections (Movie/Show/Music/Photo) | `Library::sections` |
| ✅ | Movies / Shows / Artists / Photoalbums per section | `LibrarySection::{movies, shows, artists, photoalbums}` |
| ✅ | Search within section | `LibrarySection::search(title)` |
| ✅ | recentlyAdded, onDeck, unwatched | `LibrarySection::{recently_added, on_deck, unwatched}` |
| ✅ | Filter builder with full operator suite | `LibrarySection::filter(&builder)` + `FilterBuilder` |
| ✅ | Mixed-content listings (search, hubs, etc.) | `LibraryItem` sum type |
| ✅ | Smart filter URI parser (read-only) | `library::SmartFilter::from_uri` |
| ⬜ | Hub-based universal search (`/hubs/search`) | — |
| ⬜ | Section refresh / scan triggers | — |

`FilterBuilder` covers all 8 documented operator suffixes
(`=`, `!=`, `==`, `!==`, `<=`, `>=`, `>>=`, `<<=`, `&=`) via
named methods (`equal`, `not_equal`, `exact`, `starts_with`,
`ends_with`, `gt`, `lt`). Sort + limit + offset + page-size +
libtype filtering is integrated.

`SmartFilter` parses Plex's stored smart-playlist filter URIs
back into typed clauses — useful for inspecting or migrating
existing smart playlists.

---

## Media domain

| Status | Type | Notes |
|:---:|---|---|
| ✅ | `Movie` | Full metadata + media chain + tags + markers + chapters |
| ✅ | `Show` / `Season` / `Episode` | Full hierarchy with parent/grandparent refs |
| ✅ | `Artist` / `Album` / `Track` | Same `MetadataDto` infrastructure |
| ✅ | `Photoalbum` / `Photo` | `PhotoEntry` sum for mixed children |
| ✅ | `Playlist` / `Collection` | Read + delete; collections have full M3 edit composition |
| ✅ | `Media` / `MediaPart` / `Stream` | Full chain with `Video`/`Audio`/`Subtitle`/`Lyric`/`Unknown` variants |
| ✅ | `Tag` (Genre/Director/Writer/Country/Producer/Role/Collection/Label/Mood/Style) | 10 named families + `Other(String)` |
| ✅ | `Marker` (Intro/Credits/Commercial) + `Chapter` | Attached to Movies and Episodes |

`Reload` trait is implemented on every leaf — re-fetches
`/library/metadata/<rk>` and returns the "full" version of an
item (listings emit partials with empty `media[]`).

`Playable::direct_play_url()` produces a token-bearing URL
pointing at the first part's wire key, ready for VLC/mpv. Use
the transcoder URL instead when you need bandwidth caps or
format conversion.

---

## Metadata editing (M3)

| Status | Trait | Wire path |
|:---:|---|---|
| ✅ | `PlayedUnplayed::mark_played/_unplayed` | `GET /:/scrobble` / `GET /:/unscrobble` |
| ✅ | `Ratable::rate(Option<f32>)` | `PUT /:/rate?...&rating=<v>` (`-1` clears) |
| ✅ | `Reload::reload(self)` | `GET /library/metadata/<rk>` |
| ✅ | `Playable::direct_play_url()` | builds URL, no I/O |
| ✅ | `EditField::edit_field/lock_field` + 8 specific traits | `PUT /library/sections/<sid>/all?id=<rk>&type=<n>&<field>.value=<v>&<field>.locked=<L>` |
| ✅ | `EditTags::replace_tags/remove_tags` + 10 specific traits | same path with `<field>[N].tag.tag=<v>` shape |
| ✅ | `HasArtUrl/HasPosterUrl/HasThemeUrl` (URL builders) | builds URL, no I/O |
| ✅ | `HasArtLock/HasPosterLock/HasThemeLock` (lock toggles) | `PUT ...&<field>.locked=<L>` (no `.value` pair) |
| ✅ | `EditBatch` + `EditBatchExt` (single-PUT multi-op) | combines EditField + EditTags shapes |
| ⬜ | Image upload (POST binary) | Needs `post_bytes` on HttpClient — not yet implemented |
| 🚫 | `Splittable`/`Matchable` (less-used split/match endpoints) | Deferred |
| 🚫 | `capabilities!` macro (internal refactor) | Out-of-scope — current macros sufficient |

The wire format gets surprisingly tricky:
- `EditField`: `field.value=v&field.locked=L`
- `EditTags` add: `field[0].tag.tag=v0&field[1].tag.tag=v1&field.locked=L`
- `EditTags` remove: `field[].tag.tag-=csv&field.locked=L` (note trailing `-` sigil)
- Image lock-only: `field.locked=L` with no `.value` pair (`EditField::lock_field`)

`EditBatch` combines all of these into one query string. The
`build_query` method is `pub(crate)` exposed for unit-testing the
exact wire shape.

---

## Playback control

| Status | Surface | Notes |
|:---:|---|---|
| ✅ | PlayQueue create from item / list of items / playlist | `PlexServer::create_play_queue()` builder |
| ✅ | PlayQueue get | `PlexServer::play_queue(id)` |
| ✅ | PlayQueue add / move / remove / clear / refresh | self-consuming methods returning fresh snapshot |
| ✅ | `PlexClient::connect(player_url, token, mid, cid)` | direct-to-player handle |
| ✅ | Navigation: up/down/left/right, select, back, context menu, home, music, page up/down | 11 commands |
| ✅ | Playback: play, pause, stop, skipNext, skipPrevious, seekTo, stepForward, stepBack, setVolume, setRepeat, setShuffle | 11 commands |
| ✅ | `play_media(&server, &queue, offset_ms)` composes the full payload | flagship |
| ✅ | Direct-play URL builder | `Playable::direct_play_url()` |
| ✅ | Transcoded streaming URL builder | `TranscodeOptions::build_for(&server, key)` |
| ⬜ | Per-stream language/subtitle selection mid-playback (`setStreams`) | — |
| ⬜ | Transcode decision endpoint (`/video/:/transcode/universal/decision`) | External players consume the manifest URL directly without it |

`PlexClient` uses a monotonic `commandID` (sequenced with
`Arc<AtomicU64>`) so concurrent cloned handles don't trample
each other. Wire requirement.

`play_media` derives every payload field from the `PlexServer`
and `PlayQueue` so callers don't hand-build the protocol /
address / port / containerKey / token chain.

---

## Monitoring + sessions

| Status | Surface | Path |
|:---:|---|---|
| ✅ | List current playback sessions | `GET /status/sessions` |
| ✅ | Stop a session | `GET /status/sessions/terminate?sessionId=<key>&reason=<text>` |
| ✅ | Playback history (paginated, with filters) | `GET /status/sessions/history/all` |
| ✅ | Delete a history row | `DELETE <historyKey>` |
| ✅ | Server activities | `GET /activities` |
| ✅ | Butler scheduled tasks | `GET /butler` |
| ✅ | Updater status (current version + pending releases) | `GET /updater/status` |
| ✅ | Bandwidth statistics with filter builder | `GET /statistics/bandwidth?...` |
| ✅ | Resource statistics (CPU/RAM) | `GET /statistics/resources?timespan=6` |
| ✅ | Real-time alert stream (WebSocket) | `wss://...:32400/:/websockets/notifications?X-Plex-Token=...` |
| ⬜ | Run butler tasks on demand (`POST /butler/<task>`) | — |
| ⬜ | Apply updates (`PUT /updater/apply`) | — |
| ⬜ | Filesystem browse (`GET /services/browse/<path>`) | Setup-flow only |

`PlexServer::history()` returns a `HistoryQuery` builder with
`.account(id)`, `.library_section(id)`, `.rating_key(rk)`,
`.mindate(dt)`, `.max_results(n)`, `.page_size(n)`. Terminate
with `.collect()` (eager) or `.stream()` (lazy
`futures::Stream<Item=Result<HistoryEntry>>`).

The alerts WebSocket (gated behind the `alerts` feature) is the
real-time complement to webhook ingest. `AlertEvent` covers
`Playing`/`Timeline`/`Activity`/`TranscodeSession`/`Status`/
`Reachability`/`Setting`/`BackgroundProcessingQueue` + `Unknown`.

The mutation paths (running butler tasks, applying updates) are
deliberately deferred — monitoring agents typically observe but
don't drive.

---

## Server settings

| Status | Surface | Path |
|:---:|---|---|
| ✅ | List + read all preferences | `GET /:/prefs` |
| ✅ | Set one preference (with client-side validation) | `PUT /:/prefs?<id>=<v>` |
| ✅ | Set many preferences in one PUT | same path, multiple pairs |
| ✅ | `SettingValue` typed enum (Text/Int/Double/Bool/Enum + Other) | wire is always string; this enum captures declared kind |
| ✅ | `EnumValues` typed enum (List vs key:label Mapping) | parses both Plex shapes |
| ✅ | Reject unknown ids / wrong value kinds / out-of-enum values client-side | before any network call |

Client-side validation means a typo in a setting name produces
`Error::NotFound` immediately rather than after a server round-trip.

---

## plex.tv account services

| Status | Surface | Path |
|:---:|---|---|
| ✅ | List shared friends | `GET /api/users/` (XML) |
| ✅ | Remove a friend | `DELETE /api/friends/<id>` |
| ✅ | List Plex Home users | `GET /api/home/users` (XML) |
| ✅ | List registered devices | `GET /devices.xml` (XML-only — v2 JSON has different shape) |
| ✅ | Revoke a device's token | `DELETE /devices/<id>.xml` |
| ✅ | List webhook URLs | `GET /api/v2/user/webhooks` |
| ✅ | Add / remove / set webhook URLs | `POST /api/v2/user/webhooks` with form `urls[]=...` |
| 🟡 | Friends mutation beyond remove (invite, modify share settings) | Out of scope for now |
| 🟡 | Home users mutation (add / restrict / unrestrict / switch) | Out of scope — PIN/2FA UX complications |
| 🚫 | Claim tokens for fresh PMS install | Out-of-scope (one-shot, mostly UI flow) |
| 🚫 | Sonos integration | Out-of-scope (Plex+Sonos owners only) |

Three XML-only endpoints — for those, `quick-xml`'s serde adapter
with `@attribute`-style renames is used.

---

## Cloud catalogue (Discover + metadata.provider)

| Status | Surface | Path |
|:---:|---|---|
| ✅ | Watchlist list | `GET discover.provider.plex.tv/library/sections/watchlist/<filter>` |
| ✅ | Watchlist add | `PUT discover.provider.plex.tv/actions/addToWatchlist?ratingKey=<rk>` |
| ✅ | Watchlist remove | `PUT discover.provider.plex.tv/actions/removeFromWatchlist?ratingKey=<rk>` |
| ✅ | Discover full-text catalogue search | `GET discover.provider.plex.tv/library/search?query=...` |
| ✅ | User state on cloud catalogue (view count, viewed_at, watchlisted_at) | `GET metadata.provider.plex.tv/library/metadata/<rk>/userState` |
| ✅ | Scrobble on cloud catalogue (mark watched globally) | `GET metadata.provider.plex.tv/actions/scrobble?key=<rk>&identifier=...` |
| ✅ | Unscrobble | `GET metadata.provider.plex.tv/actions/unscrobble?...` |
| 🚫 | Availability metadata (which streaming services carry the title) | Out-of-scope (niche audience) |

The cloud catalogue uses **hex `ratingKey`s** extracted from
`plex://kind/<hex>` GUIDs, not the numeric `RatingKey` you get
from a PMS library. Both `WatchlistItem` and `DiscoverItem`
pre-extract this for callers via the `rating_key` field.

Mutation endpoints are surprisingly all `PUT` (watchlist) or
`GET` (scrobble/unscrobble) — Plex API quirk preserved
verbatim.

---

## Webhook ingest

| Status | Surface | Notes |
|:---:|---|---|
| ✅ | Decode JSON payload from raw string | `WebhookPayload::from_json` |
| ✅ | Parse all 12 documented event types + Unknown | `WebhookEvent` enum |
| ✅ | Sub-payloads: account, server, player, metadata | typed projections + raw passthrough |
| ✅ | Capture optional `thumb` binary attachment | `Bytes` field |
| ✅ | `axum::FromRequest` extractor with typed rejection → 400 | gated behind `webhook-axum` feature |

Webhooks arrive as `multipart/form-data` POST with a single
`payload` text field carrying the JSON. The extractor parses
multipart, finds the payload, and produces a `WebhookPayload`.
The complementary URL registration on plex.tv lives in
`MyPlexClient::webhooks` / `add_webhook` / `delete_webhook` /
`set_webhooks`.

---

## Forward-compat gaps

These features could be added with reasonable effort if anyone
files an issue with a use case:

- Hub-based universal search (`/hubs/search?query=...`)
- Library section refresh / scan triggers
- Per-stream language/subtitle selection during playback (`/playqueues/.../setStreams`)
- Transcode decision endpoint (full quality-negotiation flow)
- Image upload (POST binary to `/library/metadata/<rk>/posters`)
- Plex Home user mutation (add/remove/restrict)
- Running butler tasks on demand
- Applying PMS updates

These aren't in [`out-of-scope.md`](./out-of-scope.md) because
they're not formally rejected — just not yet implemented.
