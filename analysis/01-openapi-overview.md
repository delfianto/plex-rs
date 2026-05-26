# Plex Media Server OpenAPI — Overview & Rust Mapping Notes

Source spec: `openapi.json` (OpenAPI 3.1.0, ~1.3 MB / 32 073 lines). This document is the starting point for designing the `plex-rs` crate's request/response types and module layout.

---

## 1. Top-level info

```json
{
  "openapi": "3.1.0",
  "info": { "title": "Plex Media Server", "version": "1.2.2", "license": { "name": "Apache 2.0" } },
  "servers": [{
    "url": "https://{IP-description}.{identifier}.plex.direct:{port}",
    "variables": {
      "IP-description": { "default": "1-2-3-4" },
      "identifier":     { "default": "0123456789abcdef0123456789abcdef" },
      "port":           { "default": "32400" }
    }
  }]
}
```

- **Title**: "Plex Media Server"; **version**: `1.2.2` (current published API; "0.0" was the pre-publication implicit version).
- **Single server template** based on the `*.plex.direct` SNI trick. Real clients also hit `http://<lan-ip>:32400` and `https://plex.tv`/`https://clients.plex.tv` for the auth flows described in `info.description`. The crate will need to accept arbitrary base URLs, not just the templated one.
- **Content negotiation** (quoted from `info.description`):

  > "The API supports responses in both XML and JSON, and clients can request one or the other using the standard `Accept` HTTP header. **The default is XML, so JSON will only be returned if it's explicitly requested (`Accept: application/json`)**. New applications should use JSON."

  Implication: the Rust client must always send `Accept: application/json` by default.

- **Headers / query-string equivalence** (`info.description`):

  > "These are referred to as headers throughout documentation, but all `X-Plex-` headers can also be sent as query string arguments."

  This matters for transcode URLs that have to be embedded into players that cannot set headers — every header parameter should also be expressible as a query parameter.

- **Key resolution rule** — keys returned in responses may be relative or absolute. Resolution is relative-URL-like but assumes a trailing slash on the base. Examples from the spec:

  ```
  /library/sections/ + home                          => /library/sections/home
  /library/sections  + /library/sections/home        => /library/sections/home
  ```

  A `key` that starts with `https://` or `view://` is taken as-is.

- **API versioning** is via the `X-Plex-Pms-Api-Version` header; default is `0.0`. Spec changelog records additions through `1.2.2` (`audioLayout`, `videoCodec`/`audioCodec`/`subtitleCodec` section endpoints).

---

## 2. Tag inventory

The spec declares 24 tags. Operations use 5 additional tag names not declared in `tags` (marked **(undeclared)**). Counts below are operations carrying that tag (counted at the operation level — one tag per op in this spec).

| Tag | Ops | One-line description (from spec) |
|---|---:|---|
| Activities | 2 | Monitor/cancel async operations via `X-Plex-Activity` UUIDs and SSE/WebSocket. |
| Butler | 5 | Periodic maintenance tasks (DB, thumbnails, analysis). |
| Collections **(undeclared)** | 1 | `POST /library/collections` — split out from Library Collections by accident. |
| Content | 13 | Read paths under a content directory (`library/sections/{id}/...`). |
| DVRs | 12 | DVR setup, channel tuning, lineup management. |
| Devices | 13 | Media grabbers (SSDP-discovered tuners etc.) under `/media/grabbers`. |
| Download Queue **(undeclared)** | 9 | Sync/offline download queue (`/downloadQueue`). Added in API 1.0.0. |
| EPG | 13 | Electronic program guide: lineups, channels, countries, regions, airing. |
| Events | 2 | EventSource (SSE) + WebSocket notification streams. |
| General | 4 | Root info, `/identity`, security resources/transient tokens. |
| Hubs | 14 | Hubs (rows of items). Global, by section, by metadata, plus admin "manage". |
| Library | 84 | Catch-all: library admin, sections, items, marker/intro/credit detection, streams, parts, BIF, tags, refresh. Largest tag by far. |
| Library Collections | 3 | Mutate items in a collection (paired with the `Collections` tag). |
| Library Playlists | 13 | Mutate playlists (CRUD + items + generators). |
| Live TV | 4 | Active live-TV playback sessions and HLS segments. |
| Log | 3 | Client-to-server logging (single line, multi-line, Papertrail). |
| Metadata Agents **(undeclared)** | 12 | `/media/providers/metadata` agent + group CRUD. Added in API 1.2.0. |
| Play Queue | 9 | Playqueue create/get + items/shuffle/move/reset. |
| Playlist | 3 | Read playlists (separate from "Library Playlists" which writes). |
| Preferences **(undeclared)** | 3 | `/:/prefs` get/set server preferences. |
| Provider | 4 | `/media/providers` discovery + add/refresh/delete. |
| Rate | 1 | `PUT /:/rate` — rate a metadata item. |
| Search | 2 | Hub search (text + voice). |
| Status | 6 | Active sessions, background tasks, playback history, terminate. |
| Subscriptions | 10 | Recording subscriptions + scheduled-recording inspection. |
| Timeline | 3 | `/:/scrobble`, `/:/unscrobble`, `/:/timeline`. |
| Transcoder **(undeclared)** | 5 | `/{transcodeType}/:/transcode/universal/...` + `/photo/:/transcode`. |
| UltraBlur | 2 | Compute UltraBlur color palettes / blurred images. |
| Updater | 3 | Check/apply/status of PMS self-update. |

**Total operations: 258** across **205 paths**.

The Library tag is overloaded — it bundles section admin, metadata mutation, marker/intro detection, streams, parts (BIF), tags, and people. Rust modules should subdivide further (see §9).

---

## 3. Path inventory (grouped by tag)

`(J)` = the 200 response declares `application/json`. `(—)` = 200 declares no body content type (typically returns XML by default, image data, or empty body for `204`-like cases). `(X)` = explicit non-JSON content (image/audio/HLS/text).

Across all 200 responses: 134 `application/json`, 103 with no content schema, plus image/audio/HLS variants. Many endpoints that "are" JSON-capable do not declare `application/json` and will need integration testing to confirm.

### Activities
- `GET /activities` → `activitiesGetSlash` — Get all activities (J)
- `DELETE /activities/{activityId}` → `activitiesDeleteActivity` — Cancel a running activity (—)

### Butler
- `GET /butler` → `butlerGetSlash` — Get all Butler tasks (J)
- `POST /butler` → `butlerPostSlash` — Start all Butler tasks (—)
- `DELETE /butler` → `butlerDeleteSlash` — Stop all Butler tasks (—)
- `POST /butler/{task}` → `butlerPostTask` — Start a single Butler task (—)
- `DELETE /butler/{task}` → `butlerDeleteTask` — Stop a single Butler task (—)

### Collections / Library Collections
- `POST /library/collections` → `libraryCollectionPostSlash` — Create a collection (J)
- `PUT /library/collections/{collectionId}/items` → `libraryCollectionCollectionPutItems` — Add items to a collection (—)
- `PUT /library/collections/{collectionId}/items/{itemId}` → `libraryCollectionCollectionPutItemsItem` — Remove an item from a collection (—)
- `PUT /library/collections/{collectionId}/items/{itemId}/move` → `libraryCollectionCollectionPutItemsItemMove` — Reorder (—)

### Content (read paths under a content directory)
- `GET /library/collections/{collectionId}/composite/{updatedAt}` → `libraryCollectionCollectionGetComposite` (X: image)
- `GET /library/collections/{collectionId}/items` → `libraryCollectionCollectionGetItems` (J)
- `GET /library/metadata/{ids}` → `libraryMetadataGetSlash` (J) — central metadata read
- `GET /library/sections/{sectionId}/albums` → `librarySectionGetAlbums` (J)
- `GET /library/sections/{sectionId}/all` → `librarySectionGetAll` (J) — primary "list everything"
- `GET /library/sections/{sectionId}/allLeaves` → `librarySectionGetAllLeaves` (J)
- `GET /library/sections/{sectionId}/arts` → `librarySectionGetArts` (J)
- `GET /library/sections/{sectionId}/categories` → `librarySectionGetCategories` (J)
- `GET /library/sections/{sectionId}/cluster` → `librarySectionGetCluster` (J)
- `GET /library/sections/{sectionId}/computePath` → `librarySectionGetComputePath` (J)
- `GET /library/sections/{sectionId}/location` → `librarySectionGetLocations` (J)
- `GET /library/sections/{sectionId}/moment` → `librarySectionGetMoment` (J)
- `GET /library/sections/{sectionId}/nearest` → `librarySectionGetNearest` (J)

### DVRs · Devices · Download Queue · EPG (Live TV families)

operationIds follow `livetvDvr*`, `mediaGrabber*`, `downloadQueue*`, `livetvEpg*` / `mediaProviderEpg*`.

- **DVRs** (12): `GET|POST /livetv/dvrs`, `GET|DELETE /livetv/dvrs/{dvrId}`, `POST .../channels/{channel}/tune`, `PUT|DELETE .../devices/{deviceId}`, `PUT|DELETE .../lineups`, `PUT .../prefs`, `POST|DELETE .../reloadGuide`.
- **Devices** (13): `GET /media/grabbers`; under `/media/grabbers/devices` — `GET|POST`, `POST /discover`, `GET|PUT|DELETE /{deviceId}`, `PUT /{deviceId}/channelmap`, `GET /{deviceId}/channels`, `PUT /{deviceId}/prefs`, `POST|DELETE /{deviceId}/scan`, `GET /{deviceId}/thumb/{version}` (X: image).
- **Download Queue** (9): `POST /downloadQueue`, `GET /downloadQueue/{queueId}`, `POST .../add`, `GET .../item/{itemId}/{decision|media}`, `GET .../items`, `GET|DELETE .../items/{itemId}`, `POST .../items/{itemId}/restart`. Added API 1.0.0.
- **EPG** (13): `GET /livetv/epg/{channelmap,channels,countries,languages,lineup,lineupchannels}`, `GET /livetv/epg/countries/{country}/{epgId}/{lineups,regions}`, `.../regions/{region}/lineups`. Plus the provider-scoped `GET /tv.plex.providers.epg.{identifier}:{deviceId}/{grid,lineups/dvr/channels,watchnow,watchnow/all}`. The literal colon in the path is non-standard. Added API 1.2.1.

### Events
- `GET /:/eventsource/notifications` → `eventsourceGetSlash` — SSE stream (X: text/event-stream)
- `GET /:/websocket/notifications` → `websocketGetSlash` — WS upgrade (not a normal request/response)

### General
- `GET /` → `getSlash` — PMS info / capabilities (J)
- `GET /identity` → `getIdentity` — machine identifier (J)
- `GET /security/resources` → `securityGetResources`
- `POST /security/token` → `securityPostToken` — exchange/derive transient tokens

### Hubs
- `GET /hubs`, `/hubs/items`, `/hubs/continueWatching`, `/hubs/promoted`
- `GET /hubs/metadata/{metadataId}[, /related, /postplay]`
- `GET /hubs/sections/{sectionId}`
- `GET|POST|DELETE /hubs/sections/{sectionId}/manage`
- `PUT /hubs/sections/{sectionId}/manage/move`
- `PUT|DELETE /hubs/sections/{sectionId}/manage/{identifier}`

### Library (84 ops — see `/tmp` ops dump for the full list; key clusters below)
- **Section CRUD**: `GET|POST /library/sections/all`, `DELETE /library/sections/all/refresh`, `GET|POST /library/sections/refresh`, `GET|PUT|DELETE /library/sections/{sectionId}`, `PUT /library/sections/{sectionId}/all`, prefs, refresh, analyze, emptyTrash, indexes, intros.
- **Section metadata derived**: `audioCodec`, `audioLayout`, `videoCodec`, `subtitleCodec`, `autocomplete`, `collections`, `common`, `composite/{updatedAt}`, `filters`, `firstCharacters`, `sorts`.
- **Metadata item mutation**: `GET|PUT|DELETE /library/metadata/{ids}` plus an enormous family of side-effect endpoints (`addetect`, `analyze`, `chapterThumbs`, `credits`, `extras` GET/POST, `file`, `index`, `intro`, `marker` POST/PUT/DELETE, `match`, `matches`, `media/{mediaItem}` DELETE, `merge`, `prefs`, `refresh`, `related`, `similar`, `split`, `subtitles`, `tree`, `unmatch`, `users/top`, `voiceActivity`, `allLeaves`, `nearest`, `{element}` POST/PUT/DELETE/GET).
- **People**: `GET /library/people/{personId}[, /media]`.
- **Streams**: `GET|PUT|DELETE /library/streams/{streamId}.{ext}`, `/levels`, `/loudness`.
- **Parts (files/BIF)**: `PUT /library/parts/{partId}`, `GET /library/parts/{partId}/indexes/{index}[/{offset}]`, `GET /library/parts/{partId}/{changestamp}/{filename}` (X: arbitrary container).
- **Misc**: `GET /library/all`, `DELETE /library/caches`, `PUT /library/clean/bundles`, `POST /library/file`, `GET /library/matches`, `GET /library/media/{mediaId}/chapterImages/{chapter}` (X: image), `GET /library/metadata/augmentations/{augmentationId}`, `PUT /library/optimize`, `GET /library/randomArtwork`, `GET /library/tags`, `GET /library/sections/prefs`.

### Playlists (read + write merged)

- **Playlist** (read, 3): `GET /playlists`, `GET /playlists/{playlistId}`, `GET /playlists/{playlistId}/items`. operationIds `playlistGet*`.
- **Library Playlists** (write, 13): `POST /playlists`, `POST /playlists/upload`; `PUT|DELETE /playlists/{playlistId}`; generators: `GET /playlists/{playlistId}/generators`, `PUT|DELETE /playlists/{playlistId}/items`, `GET|PUT|DELETE /playlists/{playlistId}/items/{generatorId}`, `GET /playlists/{playlistId}/items/{generatorId}/items`, `PUT /playlists/{playlistId}/items/{generatorId}/{metadataId}/{action}`, `PUT /playlists/{playlistId}/items/{playlistItemId}/move`.

### Live TV (4)
- `GET /livetv/sessions[, /{sessionId}]`
- `GET /livetv/sessions/{sessionId}/{consumerId}/index.m3u8` (X: HLS) and `.../{segmentId}` (X: TS).

### Log (3)
- `POST /log` (multi-line), `PUT /log` (single-line), `POST /log/networked` (Papertrail).

### Metadata Agents (12, added API 1.2.0)
- `GET|POST /media/providers/metadata`, `GET|PUT|DELETE /media/providers/metadata/{providerId}`.
- `GET|POST /media/providers/metadata/group`, `GET|PUT|DELETE /media/providers/metadata/group/{groupId}`.
- `PUT|DELETE /media/providers/metadata/group/{groupId}/items/{providerId}`.

### Play Queue (9)
- `POST /playQueues`, `GET|PUT /playQueues/{playQueueId}`, `DELETE .../items`, `DELETE .../items/{playQueueItemId}`, `PUT .../items/{playQueueItemId}/move`, `PUT .../reset`, `PUT .../shuffle`, `PUT .../unshuffle`. operationIds `playQueue*`.

### Preferences (3) · Rate (1) · Search (2) · Provider (4) · General (4) · Timeline (3)
- `GET|PUT /:/prefs`, `GET /:/prefs/get`.
- `PUT /:/rate` (`putRate`).
- `GET /hubs/search`, `GET /hubs/search/voice` (`hubsGetSearch`, `hubsSearchGetVoice`).
- `GET|POST /media/providers`, `POST /media/providers/refresh`, `DELETE /media/providers/{provider}`.
- `GET /`, `GET /identity`, `GET /security/resources`, `POST /security/token`.
- `PUT /:/scrobble`, `PUT /:/unscrobble`, `POST /:/timeline`.

### Status (6)
- `GET /status/sessions`, `/sessions/background`, `/sessions/history/all`.
- `GET|DELETE /status/sessions/history/{historyId}`.
- `POST /status/sessions/terminate`.

### Subscriptions (10)
- `GET|POST /media/subscriptions`, `POST .../process`, `GET .../scheduled`, `GET .../template`.
- `GET|PUT|DELETE /media/subscriptions/{subscriptionId}`, `PUT .../move`.
- `DELETE /media/grabbers/operations/{operationId}` (cancel an in-flight recording).

### Transcoder (5)
- `GET /photo/:/transcode` → `imageTranscode` (X: image/jpeg|png|ppm).
- `GET /{transcodeType}/:/transcode/universal/decision` (J).
- `POST /{transcodeType}/:/transcode/universal/fallback`.
- `GET /{transcodeType}/:/transcode/universal/start.*` (X: HLS/mkv).
- `GET /{transcodeType}/:/transcode/universal/subtitles` (X: text/srt).

### UltraBlur (2) · Updater (3)
- `GET /services/ultrablur/colors` (J), `GET /services/ultrablur/image` (X: image).
- `GET /updater/status`, `PUT /updater/apply`, `PUT /updater/check`.

---

## 4. Recurring request shapes

### 4.1 `X-Plex-*` headers

Reserved/standard client headers (from `info.description` — only a subset are formally typed in `components.parameters`):

| Header | Required-ish | Purpose |
|---|---|---|
| `X-Plex-Client-Identifier` | yes (always) | Opaque per-install client UUID. |
| `X-Plex-Token` | yes for protected endpoints | Auth token (see §6). |
| `X-Plex-Product`, `X-Plex-Version` | recommended | App identity + version. |
| `X-Plex-Platform`, `X-Plex-Platform-Version` | recommended | OS/runtime. |
| `X-Plex-Device`, `X-Plex-Device-Vendor`, `X-Plex-Device-Name`, `X-Plex-Model`, `X-Plex-Marketplace` | optional | Device identity. |
| `X-Plex-Pms-Api-Version` | optional | API version selection (default `0.0`). |
| `X-Plex-Session-Identifier` | per-session | Carried on transcode requests; appears as components parameter #35. |
| `X-Plex-Client-Profile-Name`, `X-Plex-Client-Profile-Extra` | transcode | Used to push a transcode profile; params #29, #30. |
| `X-Plex-Activity` | response-only | UUID emitted by long-running activity-initiating endpoints. |

`components.parameters` only formally declares 9 X-Plex headers (#28–#35 plus implicit `X-Plex-Token` via `securitySchemes`). All other X-Plex headers are described in prose only — the Rust client must implement a `ClientIdentity` struct that injects them as headers (and ideally also as query params for transcode URL pre-signing).

### 4.2 Pagination

From `info.description`:

```
Request:
  X-Plex-Container-Start: <offset>
  X-Plex-Container-Size:  <count>
  X-Plex-Container-Focus-Key: <key>  (alternative to start; centers a window)

Response (when paginated):
  X-Plex-Container-Start
  X-Plex-Container-Total-Size
  Body: MediaContainer { offset, size, totalSize }
```

Spec hard-codes these as response headers on most JSON 200 responses (saw them on `/library/metadata/{ids}` for example). The Rust client should expose a `Page { offset, size, total_size }` and a `paginated()` helper that fans out requests by `X-Plex-Container-Size`.

`limit` (query string) is a separate, more efficient mechanism that does not return `totalSize`. Endpoints with rich media queries support both.

### 4.3 Inclusion/exclusion query params

Recurring across many endpoints, but only sparsely repeated in the spec (`includeStorage`/`includeGrabs` ×2 each, `includeFullMetadata`, `includeDetails`, `includeBefore`/`includeAfter`, `includeAncestorMetadata`, `includeAlternateMetadataSources`, `excludeParentID`, `excludeGrandparentID`). The general framework is documented in `info.description`:

- `includeFields` / `excludeFields` (comma-separated attribute names)
- `includeElements` / `excludeElements` (comma-separated child element names)
- `includeOptionalFields` / `includeOptionalElements`

In API 1.0.0 the semantics of `includeFields` changed from "additive" to "exclusive". Rust clients should default to omitting these params.

### 4.4 Most common query params (counted across all ops)

| Param | Occurrences |
|---|---:|
| `type` | 17 |
| `uri` | 11 |
| `count` | 10 |
| `url` | 9 |
| `force` | 8 |
| `title`, `prefs`, `path`, `lineup`, `identifier`, `agent`, `after` | 6–7 |
| `key`, `language`, `name`, `onlyTransient` | 5 |
| `limit`, `offset`, `protocol`, `autoAdjustSubtitle` | 4 |
| `videoBitrate`, `videoResolution`, `videoQuality`, `subtitles`, `subtitleSize`, `secondsPerSegment`, `peakBitrate`, `photoResolution`, `source`, `year` | 3 |

`type` is the canonical Plex media-type integer (1=movie, 2=show, 4=episode, 8=artist, 9=album, 10=track, 13=photo, 15=playlist, 18=collection — see the `info.description` table).

### 4.5 Request bodies

Most mutating endpoints use **query parameters**, not request bodies — typical Plex idiom (`PUT /library/metadata/{ids}?title.value=X&title.locked=1`). Only a handful of endpoints (`POST /log`, `POST /playlists/upload`, `POST /library/file`) use real request bodies. The Rust client mostly does not need rich body serialization but does need ergonomic query-param builders.

---

## 5. Recurring response shapes

### 5.1 `MediaContainer` envelope

Every successful JSON response is wrapped:

```json
{
  "MediaContainer": {
    "size": 1, "totalSize": 5, "offset": 2,
    "identifier": "com.plexapp.plugins.library",
    "Metadata": [ ... ]   // or "Directory", "Hub", "Device", "MediaSubscription", ...
  }
}
```

Spec defines two parallel ladders of envelope types:

- `MediaContainer` (uppercase, alias) → `mediaContainer` (the body).
- 11 specializations: `mediaContainerWithMetadata` (30 refs), `mediaContainerWithPlaylistMetadata` (7), `mediaContainerWithDevice` (7), `mediaContainerWithSubscription` (6), `mediaContainerWithArtwork` (6), `mediaContainerWithStatus_properties-MediaContainer` (8), `mediaContainerWithSettings` (4), `mediaContainerWithLineup` (3), `mediaContainerWithNestedMetadata` (2), `mediaContainerWithDecision` (2).

`additionalProperties: true` is set on the body — clients **must** treat unknown fields as forward-compatible.

Hoisted attributes: when every child shares a value, the spec says the value can move to the container. So `parentTitle` could appear on the container instead of each `Metadata` child. Rust modeling must allow either site.

### 5.2 Error shape

The spec is unambiguous: errors are NOT JSON. Reusable `components.responses` for 400/403/404 all declare `text/html` with literal stub HTML:

```html
<html><head><title>Bad Request</title></head><body><h1>400 Bad Request</h1></body></html>
```

Status-code distribution across all operations: 200 (255), 404 (94), 400 (71), 403 (33), 500 (8), 503 (3), 409 (3), 204 (3), 401 (2), plus one each of 509, 501, 412, 301, 202.

Implication for Rust: there is no structured error body to deserialize. Map HTTP status → typed `PlexError` enum and ignore the HTML body except for opt-in logging.

### 5.3 Common nested objects

`metadata` (67 fields) is the central type. It carries Plex's hierarchy via `grandparent*` / `parent*` keys + `ratingKey`, plus nested arrays for `Filter`/`Genre`/`Director`/`Writer`/`Country`/`Role`/`Image`/`Guid`/`Rating`/`Sort`/`Media`/`Autotag` (each typically uses the `tag` schema).

`media` (18 fields including `Part[]`, codec, bitrate, resolution, container) is the playback variant. `part` carries the actual file: duration, chapters, file path, container, size. `stream` (29 fields) describes one audio/video/subtitle track inside a `part`.

See §7 for the ref-count ranking, which doubles as a codegen priority list. **The uppercase variants (`Hub`, `Metadata`, `MediaContainerWithMetadata`, …) are pure `$ref` aliases to the lowercase canonical schemas — drop them when code-generating.**

---

## 6. Authentication

```json
"components.securitySchemes": {
  "user_token": {
    "type": "apiKey", "in": "header", "name": "X-Plex-Token",
    "description": "Either a traditional access token or a JWT obtained through the JWT auth flow."
  }
}
"security": [{ "user_token": ["shared user", "admin"] }]
```

- Single, global apiKey scheme on `X-Plex-Token`.
- Global default `security` applies to every operation that does not override it. A handful of public endpoints (`GET /identity`, `GET /` capabilities, the SSDP discovery flows) may work without one in practice, but the spec lists token security on essentially everything.
- The token may also be passed as a `X-Plex-Token=` query parameter (per the header/query equivalence rule). Crate should expose both.
- The two declared scopes (`"shared user"`, `"admin"`) are informational — the API does not actually do OAuth scoping; tokens have inherent permissions from plex.tv.
- The token-acquisition flow (PIN-based) is documented in `info.description` but lives on `https://plex.tv/api/v2/pins`, **not** on PMS itself. The crate will need a separate `plextv` module for token bootstrap.
- `POST /security/token` (op `securityPostToken`) issues short-lived "transient tokens" — useful for sharing PMS URLs that should not leak the primary token.

---

## 7. Component schemas

**Total: 64**, of which **22 are `$ref` aliases** (uppercase camelCase variants pointing at the lowercase canonical schemas). Effective unique count ≈ 42.

Two distinct families:

### Domain entities (the things you'd store, render, or pass around in a UI)

Drop everything starting with `MediaContainer` from this list — those are envelopes/DTOs (see below).

| Schema | Role |
|---|---|
| `metadata` | The flagship type. Movie/show/season/episode/track/album/artist/photo/clip/etc. 67 fields including hierarchy keys, ratings, image refs, and nested `Media[]`, `Genre[]`, `Director[]`, etc. |
| `media` | One playback variant (resolution/codec). 18 fields plus `Part[]`. |
| `part` | One file on disk inside a `media`. Carries duration, chapters, file path, container, size. |
| `stream` | One audio/video/subtitle track inside a `part`. 29 fields. |
| `tag` | Universal "named thing with optional id" — used for Genre, Role, Director, Writer, Country, etc. |
| `hub` | A row of recommended/featured items shown on home/section dashboards. |
| `directory` | A browsable folder node (used in `Directory` arrays). |
| `librarySection` | A library section (movie library, show library, etc.). |
| `directoryType` | Type descriptor used inside a librarySection's filters. |
| `filter` | Filter descriptor (`field`, `type`, `key`). |
| `sort` | Sort descriptor (`key`, `title`, `descending`). |
| `image` / `art` / `thumb` | Image asset refs (`provider`, `url`, `key`). |
| `channel` | EPG/DVR channel. |
| `Device-items` | Inline device item used by Devices container. |
| `Lineup-items` | Inline lineup item. |
| `metadataAgentProvider`, `metadataAgentProviderGroup`, `metadataAgentProviderGroupItem` | Metadata agent configuration. |
| `mediaSubscription` | Recording subscription. |
| `mediaGrabOperation` | An in-flight or scheduled recording. |
| `serverConfiguration` | Top-level `GET /` capabilities payload. |
| `allowSync`, `content`, `items`, `key`, `title`, `type` | Tiny one-field shapes; primitives extracted for spec reuse. |

### DTOs / envelope wrappers (response containers — generate but don't surface)

`MediaContainer`, `mediaContainer`, `properties-MediaContainer`, `mediaContainerWithStatus_properties-MediaContainer`, and the eleven `mediaContainerWith{Metadata,NestedMetadata,Artwork,Settings,Subscription,Device,Lineup,PlaylistMetadata,Decision}` variants. These exist purely because the spec models the JSON envelope literally rather than via composition. In Rust, model the envelope **once** as a generic `MediaContainer<T>` and parameterize over the inner collection — this collapses 12+ schemas into 1.

### Top 30 most-referenced schemas (codegen priority order)

`MediaContainer` (66), `mediaContainerWithMetadata` (30), `metadata` (13), `tag` (10), `hub` (9), `directory` (9), `Device-items` (8), `mediaContainerWithStatus_properties-MediaContainer` (8), `properties-MediaContainer` (7), `mediaContainerWithPlaylistMetadata` (7), `mediaContainerWithDevice` (7), `mediaContainerWithSubscription` (6), `mediaContainerWithArtwork` (6), `serverConfiguration` (5), `metadataAgentProviderGroup` (4), `mediaSubscription` (4), `mediaContainerWithSettings` (4), `stream` (3), `sort` (3), `part` (3), `metadataAgentProvider` (3), `mediaGrabOperation` (3), `mediaContainerWithLineup` (3), `mediaContainer` (3), `media` (3), `metadataAgentProviderGroupItem` (2), `mediaContainerWithNestedMetadata` (2), `mediaContainerWithDecision` (2), `librarySection` (2), `items`/`filter`/`channel` (2 each).

---

## 8. Gaps and weirdness vs. python-plexapi

Cross-referenced against the 78 unique URL literals in `/Users/dwi.elfianto/Projects/Fun/plex-rs/python-plexapi/plexapi/`. **Important paths used by python-plexapi but absent from the OpenAPI spec:**

| python-plexapi path | Notes |
|---|---|
| `/accounts`, `/accounts/{id}` | Server-side account management — undocumented. |
| `/clients` | Active client devices — undocumented. |
| `/devices` (under PMS, not `/media/grabbers/devices`) | Authorized devices — undocumented. |
| `/diagnostics/databases`, `/diagnostics/logs` | Server diagnostics export — undocumented. |
| `/library/onDeck` | Continue-watching at the library root — spec only has `/hubs/continueWatching`. |
| `/library/recentlyAdded` | Recent items — undocumented. |
| `/library/sections/{id}/onDeck` | Per-section on-deck — undocumented. |
| `/library/sections/{id}/timeline` | Section timeline — undocumented. |
| `/library/sections/{id}/folder` | Browse by folder — undocumented. |
| `/library/sections/{id}/firstCharacter` | Singular form — spec has the plural `/firstCharacters`. python-plexapi uses both. |
| `/library/sections/{id}/indexes`, `/refresh` | Spec has DELETE; python-plexapi also uses GET. |
| `/library/metadata/{ratingKey}/posters`, `/art`, `/arts`, `/theme`, `/themes`, `/thumb`, `/clearLogo`, `/clearLogos`, `/squareArt`, `/squareArts` | Image-asset listing — the spec collapses these into `/{element}` generic endpoints (`libraryMetadataGetElement` / `libraryMetadataPostElement`). The asset _types_ are not enumerated in the spec; python-plexapi knows them empirically. The 1.2.0 changelog adds `squareArt`. |
| `/library/metadata/{id}/children` | Children of a metadata item — undocumented (spec has `allLeaves` and `/extras` only). |
| `/myplex/account`, `/myplex/claim` | Server-side plex.tv linkage — undocumented in the PMS spec (lives on plex.tv but accessed via PMS proxy). |
| `/resources` | Server resources — undocumented. |
| `/services/browse`, `/services/browse/{base64path}` | Filesystem browser — undocumented. |
| `/status/sessions/history` (vs `/status/sessions/history/all`) | python-plexapi uses both forms. |
| `/sync/items/{id}`, `/sync/refreshContent`, `/sync/refreshSynclists`, `/sync/{clientId}/item/{ratingKey}/downloaded` | Mobile sync feature — entirely undocumented in the spec (it has the newer `/downloadQueue` but the legacy `/sync` family is still in use). |
| `/system/agents` | Agent listing (vs spec's `/media/providers/metadata`) — older API still present. |
| `/transcode/sessions` | Active transcode sessions — spec only documents starting/decision endpoints, not enumeration. |
| `/playQueues/1` | Hard-coded id, no `{id}` — python idiom for the "current" queue. |
| `/actions/removeFromContinueWatching` | Action endpoint — undocumented. |
| `/updater/check` is documented but python uses `?download=1` query — confirm in tests. |

**Other weirdnesses inside the spec itself:**

- 5 tags are used at the operation level but not declared in `tags`: `Preferences`, `Transcoder`, `Download Queue`, `Metadata Agents`, `Collections`. Tooling that builds a tag index from `tags` will miss 30+ operations.
- `components.parameters` has 35 entries but most are numerically named (`"2"`–`"35"`) instead of by semantic name, suggesting a sloppy export from an internal tool. They're still referenced by number from individual ops.
- Almost every error response declares `text/html` with literal stub HTML rather than a real schema. No `application/problem+json`. No structured error model.
- 103 of 255 successful 200 responses declare no content type at all. Some genuinely return empty bodies (mutating endpoints), but others are simply under-documented — assume XML/JSON via `Accept` negotiation.
- The transcode endpoint path `/{transcodeType}/:/transcode/universal/start.*` uses a literal `.*` at the end (matches `start.mkv`, `start.m3u8`, etc.). Most OpenAPI codegen will choke on this — needs special handling.
- The EPG `{identifier}:{deviceId}` colon-embedded path parameter is non-standard; codegens often emit broken URL templates for it.
- `mediaContainerWithStatus_properties-MediaContainer` — the underscore-with-suffix naming pattern looks like an internal flattening artifact, not a hand-chosen name. Treat as machine-generated.

---

## 9. Recommendation for Rust mapping

### Options considered

| Approach | Pros | Cons |
|---|---|---|
| `progenitor` (Oxide's codegen) | Idiomatic async client; tower middleware; types per schema; preserves operationIds. | Strict on OpenAPI conformance — chokes on `start.*` paths, numbered parameter refs, `tv.plex.providers.epg.{identifier}:{deviceId}` literal colons. text/html error bodies won't generate useful error types. 103 missing content-type 200s become opaque. Aliased uppercase schemas duplicate Rust types. |
| `openapi-generator` (rust-reqwest or rust target) | Mature; supports a lot of OpenAPI 3.0 features. | Notoriously verbose Rust output; struggles with OpenAPI 3.1 (`type: ["string","null"]` etc.); `additionalProperties: true` becomes `serde_json::Value` everywhere; same path-syntax issues as progenitor. |
| Hand-write everything | Total control; ergonomic builder API; can model the `MediaContainer<T>` generic envelope once; integrate the undocumented python-plexapi endpoints alongside spec endpoints. | 258 operations × ~5 fields of query params each ≈ a lot of typing. Easy to drift from upstream when spec changes. |
| **Hybrid (recommended)** | Generate response/domain schemas only; hand-write the request/operation surface. | Two stages to keep in sync; needs a script to filter the spec before codegen. |

### Recommendation: **Hybrid, leaning hand-written**

1. **Hand-write the operation surface.** Build a thin `PlexClient { base_url, token, identity }` with hand-written async methods grouped by module (see §9.1). Reasons:
   - The query-string-as-request-body idiom doesn't map well to typed request bodies — most codegens produce awkward APIs for it.
   - `X-Plex-*` headers + query-string equivalence + pagination are easier to model as a custom middleware tower than to retrofit onto generated code.
   - The 30+ endpoints used by python-plexapi but missing from the spec need to coexist with spec endpoints; that requires hand-extension regardless.
   - operationIds like `libraryMetadataPostElement` are not pleasant Rust method names — rename freely.

2. **Codegen the schemas only.** Run `progenitor` (or a custom `schemars`-driven step) over just `components.schemas`, after preprocessing:
   - Strip the 22 uppercase `$ref` alias schemas.
   - Collapse the 12 `mediaContainerWith*` envelopes into a single generic `MediaContainer<T>` in hand-written code; only generate the inner `metadata`/`hub`/`directory`/etc. domain types.
   - Drop the numbered `components.parameters` (the named ones we keep manually).
   - Strip text/html error responses entirely; map status codes to a hand-written `PlexError` enum.

3. **Treat the spec as advisory, not authoritative.** The python-plexapi gaps in §8 prove the spec is incomplete. Pin the spec version (1.2.2) in-repo, but allow the Rust crate to add endpoints the spec doesn't document, with comments noting "not in OpenAPI 1.2.2; verified against PMS X.Y.Z."

### 9.1 Proposed module layout

Core (hand-written): `client`, `identity` (X-Plex-* headers), `pagination`, `error` (status → enum), `envelope` (`MediaContainer<T>` generic).

Models (codegen): `metadata`, `media`, `part`, `stream`, `tag`, `hub`, `directory`, `librarySection`, `filter`, `sort`, `image`, `channel`, `mediaSubscription`, `mediaGrabOperation`, `metadataAgentProvider`/`Group`/`GroupItem`, `serverConfiguration`.

Ops modules (hand-written, one per tag, with the 84-op Library split): `general`, `provider`, `library::{sections, content, metadata, streams, parts, people, misc}`, `hubs`, `search`, `playlists` (merges Playlist+Library Playlists), `play_queue`, `play_state` (scrobble/unscrobble/timeline/rate), `status`, `activities`, `butler`, `events` (SSE+WS), `transcoder`, `download_queue`, `livetv::{dvrs, devices, epg, sessions, subscriptions}`, `metadata_agents`, `preferences`, `updater`, `log`, `ultrablur`. Plus an `unspecced/` directory for `accounts`, `clients`, `sync`, `diagnostics`, `myplex`, etc. from python-plexapi. Splitting Library seven ways keeps each module ≲20 ops.

### 9.2 Pre-codegen sanity script

If we do feed any of the spec to `progenitor`: (1) delete numbered `components.parameters["2"]…["35"]`; (2) delete all uppercase `$ref` alias schemas; (3) strip `text/html` 4xx/5xx response `content` blocks; (4) skip `/{transcodeType}/:/transcode/universal/start.*` and hand-write that URL builder; (5) rewrite `tv.plex.providers.epg.{identifier}:{deviceId}` to a single `{providerKey}` parameter.

### 9.3 Open questions to verify against a real PMS

`Accept: application/json` actually returning JSON on the 103 endpoints with no declared content type; pagination response headers actually appearing; undocumented python-plexapi endpoints (`/library/onDeck`, `/clients`, `/sync/*`, `/library/metadata/{id}/children`) still existing; `X-Plex-Token` accepted as query param universally.

---

## Appendix A — Quick stats

- Spec size: 32 073 lines / 1 324 179 bytes.
- OpenAPI: 3.1.0.
- Paths: 205. Operations: 258. Tags declared: 24 (effective 29 including undeclared).
- Component schemas: 64 (22 aliases). Component parameters: 39 (35 numbered, 4 named).
- Total `$ref` occurrences: 473.
- 200 responses with `application/json`: 134. With other binary content: 18. With no content schema: 103.
- Error responses are HTML stubs, not JSON.
