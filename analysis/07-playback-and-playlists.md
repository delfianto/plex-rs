# 07 — Playback, Remote Control, Playlists, Collections, Sync

The action layer of `python-plexapi`: pushing a movie to a TV, mutating playlists, building play queues, mobile sync, and Sonos. Files: `client.py`, `playqueue.py`, `playlist.py`, `collection.py`, `sync.py`, `sonos.py`. Shared URI dialects (`server://`, `library://`, `playlist://`) cataloged in §8.

---

## 1. `PlexClient` — connecting to a remote player

A `PlexClient` (`client.py:17`) is a *player* (Plex for iOS/Apple TV, Plexamp, Plex Media Player, etc.) exposing the **Plex Companion / Remote Control API** on its own HTTP port. It can be pointed at the device directly or ride a `PlexServer` connection and proxy commands through PMS.

### 1.1 Construction & connection

`__init__` (`client.py:64-85`) accepts `server`, `baseurl`, `identifier`, `token`, `session`, `timeout`. Falls back to `auth.client_baseurl` / `auth.client_token` config, default `http://localhost:32433` (historical Companion port). `connect=True` (default) calls `self.connect()`.

`connect()` (`client.py:91-118`) GETs `/resources` (= `PlexClient.key`, `client.py:62`), returning `<Player .../>` elements per controllable surface. With `identifier` provided it picks the matching `machineIdentifier`; else first.

`_loadData` (`client.py:124-151`) hydrates standard fields (`machineIdentifier`, `product`, `protocolCapabilities` split on `,`, `protocolVersion`, `platform`, `title`) plus *session-only* fields (`device`, `state`, `vendor`, `address`, `local`, `relayed`, `secure`, `userID`) only set when the object came from `/status/sessions`.

### 1.2 `proxyThroughServer` — the two execution paths

`proxyThroughServer(value=True, server=None)` (`client.py:161-175`) sets `self._proxyThroughServer = True`. Inside `sendCommand` (`client.py:219-220`):

```python
proxy = self._proxyThroughServer if proxy is None else proxy
query = self._server.query if proxy else self.query
```

So either **Direct** (`self.query(path)` against `self._baseurl`, requires LAN reachability) or **Proxied** (`self._server.query(path)`, PMS forwards via its WebSocket channel to my.plex.tv). The path is identical; only the URL prefix changes.

### 1.3 The command protocol — IDs, headers, path

`sendCommand` (`client.py:200-246`) is the choke point for every nav/playback method.

- **Path shape:** `/player/<controller>/<command>?<params>` where `<controller>` is the first segment (`playback`, `navigation`, `timeline`, `mirror`).
- **Command-ID sequencing:** `self._commandId` starts at `0` (`client.py:76`); `_nextCommandId()` (`client.py:87-89`) pre-increments and the value goes into `commandID=<n>` (`client.py:230`). Companion uses it to correlate responses and discard stale commands.
- **Target header:** `X-Plex-Target-Client-Identifier: <machineIdentifier>` (`client.py:215`). The only "addressing" in the headers — needed in proxied mode so PMS knows which connected player to fan to.
- **Capability gate:** if `controller not in self.protocolCapabilities`, logs debug but still tries (`client.py:216-217`). Players advertise e.g. `playback,navigation,timeline,mirror,playqueues`.
- **PTP keep-alive:** for `ptp`/`Plex Media Player`, if 80+s since last call, `sendCommand` recurses into `timeline/poll` first (`client.py:223-228`).
- **Sloppy-XML workaround:** Plex for Android/Plexamp/Samsung return `b'OK'` or malformed `<Response>` on success — `ElementTree.ParseError` swallowed for those products (`client.py:235-246`).

### 1.4 Timelines

`timelines(wait=0)` (`client.py:571-582`) hits `/player/timeline/poll?commandID=<n>&wait=0` and builds `ClientTimeline`s (`client.py:600-635`). Result cached for 1s. `timeline` (`client.py:584-587`) returns the first non-`stopped`. `isPlayingMedia(includePaused=True)` (`client.py:589-597`) checks `playing`/`paused`.

`ClientTimeline` carries the full playback context: `state`, `time`/`duration` ms, `playQueueID`, `playQueueItemID`, `playQueueVersion`, `ratingKey`, `containerKey`, `audioStreamId`, `volume`, `repeat`, `shuffle`, plus server coords (`address`, `port`, `protocol`, `machineIdentifier`, `providerIdentifier`) identifying which PMS is streaming.

---

## 2. Remote control command inventory

Everything funnels through `sendCommand`. On-wire: `GET <baseurl>/player/<endpoint>?type=<mtype>&commandID=<n>&...` + `X-Plex-Target-Client-Identifier` header. `type` defaults to `'video'` (`DEFAULT_MTYPE`, `client.py:13`); needed because a client may have music in background, photos foreground, video buffered simultaneously.

### 2.1 Navigation (`/player/navigation/...`)

| Method | Endpoint | Extra params | File:line |
|---|---|---|---|
| `contextMenu` | `navigation/contextMenu` | — | `client.py:262-264` |
| `goBack` | `navigation/back` | — | `client.py:266-268` |
| `goToHome` | `navigation/home` | — | `client.py:270-272` |
| `goToMusic` | `navigation/music` | — | `client.py:274-276` |
| `moveDown` | `navigation/moveDown` | — | `client.py:278-280` |
| `moveLeft` | `navigation/moveLeft` | — | `client.py:282-284` |
| `moveRight` | `navigation/moveRight` | — | `client.py:286-288` |
| `moveUp` | `navigation/moveUp` | — | `client.py:290-292` |
| `nextLetter` | `navigation/nextLetter` | — | `client.py:294-296` |
| `pageDown` | `navigation/pageDown` | — | `client.py:298-300` |
| `pageUp` | `navigation/pageUp` | — | `client.py:302-304` |
| `previousLetter` | `navigation/previousLetter` | — | `client.py:306-308` |
| `select` | `navigation/select` | — | `client.py:310-312` |
| `toggleOSD` | `navigation/toggleOSD` | — | `client.py:314-316` |

### 2.2 Mirror (`/player/mirror/details`) — show a page

`goToMedia(media, **params)` (`client.py:318-338`) navigates to (but doesn't play) an item. Serializes the *server's* coordinates so the player can fetch detail:

```
GET /player/mirror/details
    ?machineIdentifier=<srv-id>
    &address=<srv-host>
    &port=<srv-port>
    &protocol=http|https
    &key=<media.key>
    &token=<delegation-token>
    &commandID=<n>
```

The delegation token comes from `media._server.createToken()` (`server.py:229-235`) — `GET /security/token?type=delegation&scope=all` returns a scoped token so the player can fetch metadata without holding the admin token.

### 2.3 Playback (`/player/playback/...`)

| Method | Endpoint | Body params (besides `type`, `commandID`) | File:line |
|---|---|---|---|
| `pause(mtype)` | `playback/pause` | — | `client.py:346-352` |
| `play(mtype)` | `playback/play` | — | `client.py:354-360` |
| `stop(mtype)` | `playback/stop` | — | `client.py:422-428` |
| `skipNext(mtype)` | `playback/skipNext` | — | `client.py:381-387` |
| `skipPrevious(mtype)` | `playback/skipPrevious` | — | `client.py:389-395` |
| `skipTo(key, mtype)` | `playback/skipTo` | `key=<media.key>` | `client.py:397-404` |
| `stepBack(mtype)` | `playback/stepBack` | — | `client.py:406-412` |
| `stepForward(mtype)` | `playback/stepForward` | — | `client.py:414-420` |
| `seekTo(offset, mtype)` | `playback/seekTo` | `offset=<ms>` | `client.py:372-379` |
| `setRepeat(repeat, mtype)` | `playback/setParameters` | `repeat=0\|1\|2` | `client.py:430-437` |
| `setShuffle(shuffle, mtype)` | `playback/setParameters` | `shuffle=0\|1` | `client.py:439-446` |
| `setVolume(volume, mtype)` | `playback/setParameters` | `volume=0..100` | `client.py:448-455` |
| `setParameters(...)` | `playback/setParameters` | any of `repeat`/`shuffle`/`volume` | `client.py:529-547` |
| `setAudioStream(id, mtype)` | `playback/setStreams` | `audioStreamID` | `client.py:457-464` |
| `setSubtitleStream(id, mtype)` | `playback/setStreams` | `subtitleStreamID` | `client.py:466-473` |
| `setVideoStream(id, mtype)` | `playback/setStreams` | `videoStreamID` | `client.py:475-482` |
| `setStreams(...)` | `playback/setStreams` | any of the three IDs | `client.py:549-567` |
| `refreshPlayQueue(id, mtype)` | `playback/refreshPlayQueue` | `playQueueID` | `client.py:362-370` |
| `playMedia(media, offset, ...)` | `playback/playMedia` | see below | `client.py:484-527` |

Repeat: `0=off`, `1=repeatone`, `2=repeatall` (`client.py:434`). Shuffle: `0=off`, `1=on` (`client.py:443`). `type` is `music`/`photo`/`video` — asymmetric with PlayQueue/Playlist which use `audio` (coerced at `client.py:507-508`).

### 2.4 `playMedia` — start-playback

`playMedia(media, offset=0, **params)` (`client.py:484-527`) is the only command that teaches the player about new content. Request:

```
GET /player/playback/playMedia
    ?providerIdentifier=com.plexapp.plugins.library
    &machineIdentifier=<srv-id>
    &protocol=http
    &address=<srv-host>
    &port=<srv-port>
    &offset=<ms>
    &key=<media.key>                          # or playqueue.selectedItem.key if media has no key
    &type=video|music|photo
    &containerKey=/playQueues/<id>?window=100&own=1
    &token=<delegation-token>
    &commandID=<n>
```

Steps inside `playMedia`:

1. Determine media type: `media.playlistType` (Playlist), else `media.items[0].listType` (PlayQueue), else `media.listType`. `audio→music` (`client.py:498-508`).
2. **Always** materialize a PlayQueue. If already a `PlayQueue`, used as-is; else `media._server.createPlayQueue(media)` (`client.py:510`) — wraps `PlayQueue.create` (§3). The player never plays a raw item — always a server queue.
3. `containerKey=/playQueues/<id>?window=100&own=1` — `own=1` transfers ownership; `window=100` requests ±100 surrounding items.
4. `createToken()` mints a delegation token included as `token=...`.

`key` comes directly from `media.key`: `/library/metadata/12345` (video), `/playlists/678` (playlist), `/library/metadata/12855/station/<uuid>` (station).

---

## 3. `PlayQueue` — server-side cursor

PMS manages "what is playing now / next" — the player asks for `/playQueues/<id>?window=...`. Full lifecycle in `playqueue.py`.

### 3.1 Anatomy

`_loadData` (`playqueue.py:37-64`) pulls out:

- `playQueueID`, `playQueueVersion` — version bumps on every mutation; used by clients for diff/conflict detection.
- `playQueueSelectedItemID`, `playQueueSelectedItemOffset`, `playQueueSelectedMetadataItemID` — cursor. `selectedItem` (`playqueue.py:64`) indexes `self.items[playQueueSelectedItemOffset]`.
- `playQueueShuffled`, `playQueueTotalCount`, `playQueueLastAddedItemID` (Up Next boundary).
- `playQueueSourceURI` — original creation URI (§8).

`items` (`playqueue.py:66-68`) lazily parses `<Track>`/`<Video>`/`<Photo>` via `findItems`. Each item carries `playQueueItemID` (distinct from `ratingKey`) — position-bound, required by `moveItem`/`removeItem`.

### 3.2 Reading a queue — `PlayQueue.get`

`PlayQueue.get(server, playQueueID, own=False, center=None, window=50, includeBefore=True, includeAfter=True)` (`playqueue.py:100-137`):

```
GET /playQueues/<playQueueID>
    ?own=0|1                      # take ownership transfer
    &window=50                    # items on each side of center/selected
    &includeBefore=1
    &includeAfter=1
    &center=<playQueueItemID>     # if provided, shift the window without moving selectedItem
```

### 3.3 Creating a queue — `PlayQueue.create`

`PlayQueue.create(server, items, startItem=None, shuffle=0, repeat=0, includeChapters=1, includeRelated=1, continuous=0)` (`playqueue.py:139-194`). This is a POST.

The URI for the new queue depends on the input shape:

- **List of items** (`playqueue.py:174-178`): joins `ratingKey`s with `,`, URL-encodes them into a library directory URI:
  ```
  uri = library:///directory/<urlencoded(/library/metadata/<k1>,<k2>,...)>
  type = <items[0].listType>
  ```
- **Single Playlist** (`playqueue.py:180-183`):
  ```
  type = items.playlistType
  playlistID = items.ratingKey
  uri = server://<machineId>/com.plexapp.plugins.library/playlists/<rk>
  ```
- **Single item** (`playqueue.py:184-185`):
  ```
  uri = server://<machineId>/com.plexapp.plugins.library<item.key>
  type = items.listType
  ```

`startItem` (`playqueue.py:187-188`) adds `&key=<startItem.key>` to tell PMS which item should be the queue's selected position at creation time.

Full request:

```
POST /playQueues
    ?includeChapters=1
    &includeRelated=1
    &repeat=0
    &shuffle=0
    &continuous=0                 # if 1, queue keeps appending (next episodes for shows)
    &type=video|audio|photo
    &uri=<scheme:...>
    [&playlistID=<rk>]
    [&key=<startItem.key>]
```

Response is the full queue XML; the constructor wraps it (`playqueue.py:191-194`).

### 3.4 The "radio station" shortcut — `fromStationKey`

`PlayQueue.fromStationKey(server, key)` (`playqueue.py:196-229`) wraps `PlayQueue.create` for the special case of music station keys like `/library/metadata/12855/station/8bd39616-...?type=10`. It hardcodes `type=audio` and builds the `server://<id>/com.plexapp.plugins.library<key>` URI. Used together with `PlexClient.playMedia(pq)` to start station playback.

### 3.5 Mutations

| Method | HTTP | Path | Body | File:line |
|---|---|---|---|---|
| `addItem(item, playNext=False)` | PUT | `/playQueues/<id>?uri=<library://uuid/item/library/metadata/<rk>>[&playlistID=<rk>][&next=1]` | — | `playqueue.py:231-260` |
| `moveItem(item, after=None)` | PUT | `/playQueues/<id>/items/<pqItemID>/move[?after=<otherPqItemID>]` | — | `playqueue.py:262-289` |
| `removeItem(item)` | DELETE | `/playQueues/<id>/items/<pqItemID>` | — | `playqueue.py:291-307` |
| `clear()` | DELETE | `/playQueues/<id>/items` | — | `playqueue.py:309-314` |
| `refresh()` | GET | `/playQueues/<id>` | — | `playqueue.py:316-321` |

Notes:

- `addItem` uses a *different* URI scheme than `create`: per-library-section `library://<sectionUUID>/item/library/metadata/<rk>` (`playqueue.py:251-252`). UUID is `item.section().uuid` — the only place using the `item` resource type.
- `moveItem` takes queue-local `playQueueItemID`, not `ratingKey`. `getQueueItem` (`playqueue.py:85-98`) resolves library items to queue twins.
- Every mutation response is fed through `_invalidateCacheAndLoadData(data)` to keep version/cursor correct.

---

## 4. `Playlist`

Three `playlistType` values (`playlist.py:36, 65`): `video`, `audio`, `photo`. Two flavours: regular (ordered) and smart (`smart=1`, filter URI). Radio stations (`radio=1`) appear here but are read-only — `_items` returns `[]` (`playlist.py:186-187`).

### 4.1 Creating regular playlists — `Playlist._create`

`Playlist._create(server, title, items)` (`playlist.py:343-365`):

```
POST /playlists
    ?uri=server://<machineId>/com.plexapp.plugins.library/library/metadata/<rk1>,<rk2>,...
    &type=video|audio|photo
    &title=<title>
    &smart=0
```

Items must share `listType` (`playlist.py:355-356`). URI built from `server._uriRoot()` (`server.py:164-165`) = `server://<machineId>/com.plexapp.plugins.library`.

### 4.2 Creating smart playlists — `Playlist._createSmart`

`Playlist._createSmart(server, title, section, limit, libtype, sort, filters, **kwargs)` (`playlist.py:367-382`):

```
POST /playlists
    ?uri=server://<machineId>/com.plexapp.plugins.library/library/sections/<sectionId>/all?<filter-query>
    &type=movie|show|episode|artist|track|...
    &title=<title>
    &smart=1
```

Key difference: URI points at a *library section search*, not concrete `ratingKey`s. `LibrarySection._buildSearchKey(...)` (`playlist.py:375-376`) emits e.g. `/library/sections/4/all?type=1&genre=Action&sort=titleSort:asc`. PMS persists this as `content` (`playlist.py:55`) and re-runs the search on every fetch.

`updateFilters` (`playlist.py:300-327`) PUTs the new URI: `PUT /playlist/<rk>/items?uri=server://...sections/.../all?<new-query>`.

### 4.3 Creating from M3U — `_createFromM3U`

Music-only. `_createFromM3U(server, title, section, m3ufilepath)` (`playlist.py:384-399`):

```
POST /playlists/upload?sectionID=<sectionId>&path=<m3u-path>
```

`path` is on the PMS host — PMS reads the M3U from its own filesystem. A follow-up `server.playlists(guid__endswith=m3ufilepath)` finds the new playlist and `editTitle` renames it.

### 4.4 Public `create` dispatcher

`Playlist.create(...)` (`playlist.py:401-443`) dispatches:
- `m3ufilepath` → `_createFromM3U`
- `smart=True` → `_createSmart` (and forbids passing `items`)
- otherwise → `_create`

### 4.5 Item-level mutations

| Method | HTTP | Endpoint | Body | File:line |
|---|---|---|---|---|
| `addItems(items)` | PUT | `/playlists/<rk>/items?uri=<server://...,/library/metadata/<rk1>,<rk2>...>` | — | `playlist.py:216-249` |
| `removeItems(items)` | DELETE | `/playlists/<rk>/items/<playlistItemID>` (one per item) | — | `playlist.py:251-272` |
| `moveItem(item, after=None)` | PUT | `/playlists/<rk>/items/<playlistItemID>/move[?after=<otherId>]` | — | `playlist.py:274-298` |
| `updateFilters(...)` (smart only) | PUT | `/playlists/<rk>/items?uri=<server://...sections/.../all?...>` | — | `playlist.py:300-327` |
| `_edit(**kwargs)` (title/summary/etc) | PUT | `/playlists/<rk>?<kv>` | — | `playlist.py:329-337` |
| `delete()` | DELETE | `/playlists/<rk>` | — | `playlist.py:339-341` |

Smart predicates `BadRequest` on add/remove/move (`playlist.py:226, 262, 287`).

`addItems` groups items by source server (`playlist.py:233`) and issues *one PUT per source server* — cross-server playlists store items by owning server's URI root. `removeItems` uses `playlistItemID` not `ratingKey`; `_getPlaylistItemID(item)` (`playlist.py:124-129`) resolves it — same dual-id pattern as `PlayQueue`.

### 4.6 Reading items

`_items` (`playlist.py:184-206`) calls `self.fetchItems(self.key + '/items')` then, for any item whose `sourceURI` points at a *different* server, opens a separate `PlexServer` via `myPlexAccount().resource(serverID).connect()` and reparents the item. Cross-server playlists work transparently this way.

### 4.7 Sync & copy

`Playlist.sync(...)` (`playlist.py:455-507`) builds a `SyncItem` with `location = playlist:///<urlencoded(guid)>` (`playlist.py:495`), picks media settings per type, forwards to `myPlexAccount().sync()`. See §6.

`copyToUser(user)` (`playlist.py:445-453`): `switchUser(user)` → new `PlexServer` → `Playlist.create(server=that, title=self.title, items=self.items())`. Original untouched; clone created in target user's library.

---

## 5. `Collection`

`Collection` (`collection.py:11-555`) is structurally a sibling of `Playlist` but lives *inside a library section* — collections stored under `/library/metadata/<collectionRk>` rather than `/playlists/<rk>`. Membership endpoints under `/library/collections/...` and `<collection.key>/items/...`.

### 5.1 Subtypes

`collection.subtype` (`collection.py:88`) is one of:
- video: `movie`, `show`, `season`, `episode` (`collection.py:148`)
- audio: `artist`, `album`, `track` (`collection.py:153`)
- photo: `photoalbum`, `photo` (`collection.py:158`)

`listType` derives from this. `metadataType` is just an alias for `subtype` (`collection.py:142-143`).

### 5.2 Three advanced settings

Three dropdowns compile to `editAdvanced(<intkey>=<intval>)`:

- `modeUpdate(mode)` (`collection.py:249-275`) — visibility: `default→-1`, `hide→0`, `hideItems→1`, `showItems→2`.
- `sortUpdate(sort)` (`collection.py:277-304`) — order, forbidden on smart: `release→0`, `alpha→1`, `custom→2`.
- `filterUserUpdate(user)` (`collection.py:222-247`) — smart-only, whose watch state drives filters: `admin→0`, `user→1`.

### 5.3 CRUD

`Collection._create(server, title, section, items)` (`collection.py:415-440`):
```
POST /library/collections
    ?uri=server://<machineId>/com.plexapp.plugins.library/library/metadata/<rk1>,<rk2>,...
    &type=<searchType(itemType)>           # int — utils.searchType maps "movie"→1, "show"→2, etc.
    &title=<title>
    &smart=0
    &sectionId=<section.key>
```

`Collection._createSmart(...)` (`collection.py:442-457`):
```
POST /library/collections
    ?uri=server://<machineId>/com.plexapp.plugins.library/library/sections/<id>/all?<filter-query>
    &type=<searchType(libtype)>
    &title=<title>
    &smart=1
    &sectionId=<section.key>
```

Wire-format differences from `Playlist._create`: extra `sectionId` parameter; `type` is integer (`utils.searchType` maps `movie→1`, `show→2`, `artist→8`, `album→9`, `track→10`) instead of `'video'`/`'audio'`/`'photo'`.

`addItems`, `removeItems`, `moveItem`, `updateFilters` (`collection.py:306-409`) mirror `Playlist` but use `ratingKey` directly (no `playlistItemID`):

| Method | HTTP | Endpoint | File:line |
|---|---|---|---|
| `addItems` | PUT | `<key>/items?uri=server://...,/library/metadata/<rk1>,<rk2>...` | `collection.py:306-334` |
| `removeItems` | DELETE | `<key>/items/<ratingKey>` (one per item) | `collection.py:336-355` |
| `moveItem` | PUT | `<key>/items/<ratingKey>/move[?after=<rk>]` | `collection.py:357-378` |
| `updateFilters` (smart only) | PUT | `<key>/items?uri=server://...sections/.../all?...` | `collection.py:380-409` |
| `delete()` | DELETE | (inherited from `PlexPartialObject`) `<key>` | `collection.py:411-413` |

`<key>` = `/library/metadata/<ratingKey>` (`collection.py:77` strips trailing `/children`).

### 5.4 `visibility()` and ManagedHub

`visibility()` (`collection.py:206-216`) GETs `/hubs/sections/<sectionID>/manage?metadataItemId=<rk>`. Returns a `ManagedHub` describing how the collection appears on Plex Home. If no hub exists, fabricates one client-side with identifier `custom.collection.<sectionID>.<rk>`.

### 5.5 Sync hook

`Collection.sync(...)` (`collection.py:496-549`) creates a `SyncItem` with `location = library:///directory/<urlencoded(<key>/children?excludeAllLeaves=1)>` (`collection.py:536-537`). `excludeAllLeaves=1` grabs children but not container leaves (matters for show collections).

---

## 6. `SyncItem` — legacy mobile sync

`plexapi/sync.py` covers "Mobile Sync" — offline downloads. **Legacy** (Plex deprecated mobile sync), but still wired into `MyPlexAccount.sync()` and called by `Playlist.sync()` / `Collection.sync()` / per-media `sync()`.

Module docstring (`sync.py:1-24`): to act as a sync-target, set `plexapi.X_PLEX_PROVIDES = 'sync-target'`, spoof platform/device (e.g. iPhone iOS 11.4.1), set `BASE_HEADERS['X-Plex-Sync-Version'] = '2'`. Required because Plex hardcodes transcoding profiles per device.

### 6.1 `SyncItem` — model

`SyncItem` (`sync.py:32-105`) fields:
- `id`, `version`, `machineIdentifier` (source PMS), `clientIdentifier` (target device).
- `rootTitle`, `title`, `metadataType`, `contentType` (`video`/`audio`/`photo`).
- `status` — `Status` value object (`sync.py:131-167`): `state` (`completed`, `pending`, ...), counts (`itemsCount`, `itemsCompleteCount`, `itemsDownloadedCount`, `itemsReadyCount`, `itemsSuccessfulCount`), `totalSize`, `failureCode`/`failure`.
- `mediaSettings` — `MediaSettings` (`sync.py:170-238`): `maxVideoBitrate`, `videoQuality`, `videoResolution`, `audioBoost`, `musicBitrate`, `photoQuality`, `photoResolution`, `subtitleSize`. Factories `createVideo`/`createMusic`/`createPhoto` pick from `VIDEO_QUALITIES`/`PHOTO_QUALITIES` (`sync.py:276-311`).
- `policy` — `Policy(scope, unwatched, value)` (`sync.py:241-273`). `scope='all'` syncs all matching; `scope='count'` caps at `value`. `Policy.create(limit, unwatched)` flips scope automatically.
- `location` — URI describing what to sync:
  - `playlist:///<urlencoded(guid)>` (set by `Playlist.sync`, `playlist.py:495`)
  - `library:///directory/<urlencoded(<collection-key>/children?excludeAllLeaves=1)>` (set by `Collection.sync`, `collection.py:536-537`)

### 6.2 Endpoints

| Method | HTTP | Path | File:line |
|---|---|---|---|
| `SyncItem.getMedia()` | GET | `<server>/sync/items/<id>` | `sync.py:85-89` |
| `SyncItem.markDownloaded(media)` | PUT | `<server>/sync/<clientIdentifier>/item/<ratingKey>/downloaded` | `sync.py:91-99` |
| `SyncItem.delete()` | DELETE | `https://plex.tv/devices/<clientId>/sync_items/<id>` | `sync.py:101-105` |
| `SyncList` fetch | GET | `https://plex.tv/devices/<clientId>/sync_items` | `sync.py:108-128` |

`SyncItem.delete` and listing go to **plex.tv**, not PMS — sync state is centralized; content delivery is per-server.

`SyncItem.server()` (`sync.py:78-83`) maps `machineIdentifier` to a `MyPlexResource` so `.getMedia()` connects to the source PMS.

### 6.3 Quality presets

`VIDEO_QUALITIES` (`sync.py:276-281`) — parallel arrays of `bitrate`/`videoResolution`/`videoQuality` indexed by `VIDEO_QUALITY_*` (`sync.py:283-294`). `VIDEO_QUALITY_ORIGINAL = -1` sends empty strings (no transcoding). `AUDIO_BITRATE_*` (`sync.py:296-299`): 96/128/192/320 kbps. `PHOTO_QUALITIES` (`sync.py:301-306`): four resolutions → JPEG-quality ints (24, 49, 74, 99).

### 6.4 Not in this file

No `SyncItem` creation helper here. Creation is `MyPlexAccount.sync(syncItem, client=..., clientId=...)` in `myplex.py`, which POSTs to plex.tv. Factories on `Playlist.sync` / `Collection.sync` / per-media `.sync()` build a `SyncItem` template before handing it off.

---

## 7. `PlexSonosClient` — the plex.tv-relayed special case

`PlexSonosClient` (`sonos.py:9-115`) subclasses `PlexClient` to retarget command flow to a Plex Sonos relay. Sonos speakers don't speak Companion locally — `https://sonos.plex.tv` translates Companion commands to Sonos.

### 7.1 What's different

`__init__` (`sonos.py:48-68`):
- Hardcodes `self._baseurl = "https://sonos.plex.tv"`.
- Uses `MyPlexAccount` token (`account._token`), not a per-server token.
- Skips `connect()` — speaker descriptor comes from caller (`MyPlexAccount.sonos_speakers()`).
- Sets dummy `_last_call`, `_proxyThroughServer=False`, `_timeline_cache_timestamp=0` to satisfy `sendCommand` invariants.

Inherited methods (`pause`, `play`, `seekTo`, navigation, `setVolume`) work unchanged — `sendCommand` appends to `_baseurl`, sonos.plex.tv accepts the same paths.

### 7.2 Overridden `playMedia`

`PlexSonosClient.playMedia` (`sonos.py:70-115`) is overridden because:

1. Sonos only supports audio — non-audio raises `BadRequest("Sonos currently only supports music for playback")` (`sonos.py:80-83`).
2. Sonos demands the *caller's* identity *and* the target speaker's id, so headers are inlined via `**params`: `X-Plex-Client-Identifier: <X_PLEX_IDENTIFIER>`, `X-Plex-Token: <server._token>` (PMS token, not account), `X-Plex-Target-Client-Identifier: <speaker machineIdentifier>`, `commandID: <next>`.
3. `containerKey` drops `window=100` — Sonos uses `/playQueues/<id>?own=1` only.

On-wire:
```
GET https://sonos.plex.tv/player/playback/playMedia
    ?type=music
    &providerIdentifier=com.plexapp.plugins.library
    &containerKey=/playQueues/<id>?own=1
    &key=<media.key>
    &offset=0
    &machineIdentifier=<srv-id>
    &protocol=http
    &address=<srv-host>
    &port=<srv-port>
    &token=<delegation-token>
    &commandID=<n>
    &X-Plex-Client-Identifier=<this-client>
    &X-Plex-Token=<server-token>
    &X-Plex-Target-Client-Identifier=<speaker-id>
```

The relay forwards to speaker and PMS as needed.

---

## 8. URI scheme inventory

Plex uses URI strings as opaque references in query parameters and stored playlist content. Variants below cover this layer.

| Scheme prefix | Shape | Meaning | Where used |
|---|---|---|---|
| `server://` | `server://<machineId>/com.plexapp.plugins.library<key>` | Any single library item on a specific PMS. `<key>` typically `/library/metadata/<rk>` or `/library/metadata/<rk1>,<rk2>,...` or `/library/sections/<id>/all?<query>` or `/playlists/<rk>` | `Playlist._create` (`playlist.py:360`), `Playlist._createSmart` (`playlist.py:377`), `Playlist.addItems` (`playlist.py:243`), `Playlist.updateFilters` (`playlist.py:322`), `Collection._create` (`collection.py:435`), `Collection._createSmart` (`collection.py:452`), `Collection.addItems` (`collection.py:329`), `Collection.updateFilters` (`collection.py:404`), `PlayQueue.create` single-item branch (`playqueue.py:185`), `PlayQueue.fromStationKey` (`playqueue.py:223`). Built by `server._uriRoot()` (`server.py:164-165`). |
| `library://` (UUID form) | `library://<sectionUUID>/item/library/metadata/<rk>` | A single library *item* relative to a section UUID. The `item` resource segment is specific to PlayQueue mutations. | `PlayQueue.addItem` (`playqueue.py:251-252`). The UUID is `item.section().uuid`. |
| `library://` (directory form) | `library:///directory/<urlencoded(<path>)>` | A library "directory" — typically a comma-joined metadata list or a collection's `children` listing. Note the empty authority (`library:///` with three slashes). | `PlayQueue.create` for the list-of-items branch (`playqueue.py:176-177`); `Collection.sync` for the sync location (`collection.py:536-537`). |
| `playlist://` | `playlist:///<urlencoded(<playlist-guid>)>` | Identifies a playlist by GUID for sync purposes. | `Playlist.sync` (`playlist.py:495`). |
| `/playQueues/<id>?...` (containerKey) | not a scheme per se, but a path | Reference to the currently-active PlayQueue when starting playback. | `PlexClient.playMedia` containerKey (`client.py:520`); `PlexSonosClient.playMedia` containerKey (`sonos.py:100`). |
| `https://plex.tv/devices/<clientId>/sync_items` | absolute URL | Per-device sync queue at plex.tv. | `SyncList.key` (`sync.py:116`); `SyncItem.delete` (`sync.py:103-105`). |
| `/security/token` | path | Mints delegation tokens used to embed in `playMedia` / `goToMedia` params. | `server.createToken()` (`server.py:229-235`), consumed at `client.py:334-336` and `client.py:523-525`. |

### Important asymmetries

- **`server://` vs `library://`** — `server://` carries an explicit PMS machine identifier; used when the target server must be explicit. `library://<UUID>/item/...` is used inside a queue (queue already knows its source server) where the UUID disambiguates sections.
- **comma-joined ratingKeys** — `PlayQueue.create` (list branch), `Playlist.addItems`, `Collection.addItems` join `ratingKey`s with `,`. Plex idiom for "many in one request."
- **`type` parameter naming** — string (`video`/`audio`/`photo`/`music`) in some places, library `listType` in others, numeric `searchType` (`1`=movie, `2`=show, `8`=artist, `9`=album, `10`=track) in Collection endpoints (`collection.py:437, 454`). Playlist uses strings. PlayQueue mixes. PlexClient playback uses `video`/`music`/`photo` with `audio→music` coercion (`client.py:507-508`).

---

## 9. Full endpoint reference

| Domain | Method | Path | Purpose | File:line |
|---|---|---|---|---|
| Client | GET | `<player>/resources` | List controllable surfaces on the player (`connect`) | `client.py:62`, `:98-99` |
| Client | GET | `<player>/player/<controller>/<command>?commandID=<n>&...` | Generic Companion command (every nav/playback method) | `client.py:230-234` |
| Client | GET/POST | `<player>/player/timeline/poll?wait=<s>&commandID=<n>` | Poll active timelines | `client.py:571-580`, `:603` |
| Client | GET | `<player>/player/navigation/(home\|back\|moveUp\|moveDown\|moveLeft\|moveRight\|select\|...)` | UI navigation | `client.py:262-316` |
| Client | GET | `<player>/player/mirror/details?machineIdentifier=&address=&port=&protocol=&key=&token=` | Navigate to media detail page on the client | `client.py:338` |
| Client | GET | `<player>/player/playback/(play\|pause\|stop\|skipNext\|skipPrevious\|skipTo\|stepBack\|stepForward\|seekTo)?type=&...` | Transport controls | `client.py:346-428` |
| Client | GET | `<player>/player/playback/setParameters?volume=&shuffle=&repeat=&type=` | Set volume/shuffle/repeat | `client.py:529-547` |
| Client | GET | `<player>/player/playback/setStreams?audioStreamID=&subtitleStreamID=&videoStreamID=&type=` | Switch streams | `client.py:549-567` |
| Client | GET | `<player>/player/playback/playMedia?providerIdentifier=&machineIdentifier=&protocol=&address=&port=&offset=&key=&type=&containerKey=&token=` | Start playback against a play queue | `client.py:484-527` |
| Client | GET | `<player>/player/playback/refreshPlayQueue?playQueueID=&type=` | Tell client to re-pull the queue | `client.py:362-370` |
| Sonos | GET | `https://sonos.plex.tv/player/playback/playMedia?type=music&...` (custom headers) | Start audio playback on a Sonos speaker | `sonos.py:94-115` |
| PMS | POST | `/playQueues?type=&uri=&shuffle=&repeat=&continuous=&includeChapters=&includeRelated=[&playlistID=][&key=]` | Create a PlayQueue | `playqueue.py:190-194` |
| PMS | GET | `/playQueues/<id>?own=&window=&includeBefore=&includeAfter=[&center=]` | Fetch a PlayQueue (with optional ownership transfer) | `playqueue.py:133-135` |
| PMS | GET | `/playQueues/<id>` | Refresh queue from server | `playqueue.py:318-319` |
| PMS | PUT | `/playQueues/<id>?uri=<library://uuid/item/...>[&playlistID=][&next=]` | Append/insert into Up Next | `playqueue.py:257-258` |
| PMS | PUT | `/playQueues/<id>/items/<pqItemID>/move[?after=<otherPqItemID>]` | Reorder queue item | `playqueue.py:286-287` |
| PMS | DELETE | `/playQueues/<id>/items/<pqItemID>` | Remove one queue item | `playqueue.py:304-305` |
| PMS | DELETE | `/playQueues/<id>/items` | Clear queue | `playqueue.py:311-312` |
| PMS | POST | `/playlists?uri=<server://...>&type=&title=&smart=0` | Create regular playlist | `playlist.py:363-364` |
| PMS | POST | `/playlists?uri=<server://...sections/.../all?...>&type=&title=&smart=1` | Create smart playlist | `playlist.py:380-381` |
| PMS | POST | `/playlists/upload?sectionID=&path=` | Create playlist by reading M3U on PMS host | `playlist.py:394-395` |
| PMS | PUT | `/playlists/<rk>/items?uri=<server://...,/library/metadata/...>` | Add items to playlist | `playlist.py:246-247` |
| PMS | DELETE | `/playlists/<rk>/items/<playlistItemID>` | Remove one playlist item | `playlist.py:270-271` |
| PMS | PUT | `/playlists/<rk>/items/<playlistItemID>/move[?after=<otherId>]` | Reorder playlist item | `playlist.py:291-297` |
| PMS | PUT | `/playlists/<rk>/items?uri=<server://...sections/.../all?...>` | Update smart playlist filter | `playlist.py:325-326` |
| PMS | PUT | `/playlists/<rk>?<edits>` | Edit playlist metadata (title etc.) | `playlist.py:335-336` |
| PMS | DELETE | `/playlists/<rk>` | Delete playlist | `playlist.py:341` |
| PMS | POST | `/library/collections?uri=<server://...>&type=<int>&title=&smart=0&sectionId=` | Create regular collection | `collection.py:438-439` |
| PMS | POST | `/library/collections?uri=<server://...sections/.../all?...>&type=<int>&title=&smart=1&sectionId=` | Create smart collection | `collection.py:455-456` |
| PMS | PUT | `/library/metadata/<rk>/items?uri=<server://...,/library/metadata/...>` | Add items to collection | `collection.py:332-333` |
| PMS | DELETE | `/library/metadata/<rk>/items/<itemRk>` | Remove one collection item | `collection.py:353-354` |
| PMS | PUT | `/library/metadata/<rk>/items/<itemRk>/move[?after=<otherRk>]` | Reorder collection item | `collection.py:372-377` |
| PMS | PUT | `/library/metadata/<rk>/items?uri=<server://...sections/.../all?...>` | Update smart collection filter | `collection.py:407-408` |
| PMS | GET | `/hubs/sections/<sectionID>/manage?metadataItemId=<rk>` | Fetch ManagedHub for collection visibility | `collection.py:208-209` |
| PMS | GET | `/security/token?type=delegation&scope=all` | Mint delegation token for client.playMedia | `server.py:234` |
| PMS | GET | `/sync/items/<id>` | List media inside a sync item | `sync.py:88-89` |
| PMS | PUT | `/sync/<clientId>/item/<ratingKey>/downloaded` | Mark a sync item as downloaded | `sync.py:98-99` |
| plex.tv | GET | `https://plex.tv/devices/<clientId>/sync_items` | List sync items for a device | `sync.py:116-128` |
| plex.tv | DELETE | `https://plex.tv/devices/<clientId>/sync_items/<id>` | Delete a sync item | `sync.py:103-105` |

---

## Quick mental model

- **PlexServer** owns content & queues; **PlexClient** is a controllable player; **plex.tv** owns cross-device state (sync, Sonos relay).
- Every playback start materializes a `PlayQueue` on PMS first, then tells the player via `containerKey=/playQueues/<id>?own=1`.
- `commandID` is monotonic per-client; in the query string of every Companion command.
- Smart playlists/collections store a **filter URI** as `content`; editing = re-PUT. URI always a library-section search URL prefixed by `server://<machineId>/com.plexapp.plugins.library`.
- Regular playlist/collection mutations use comma-joined `ratingKey` lists inside a `server://` URI — single and bulk look identical on the wire.
- `playlistItemID` / `playQueueItemID` are *position identifiers*, not `ratingKey` — required by position-targeted mutations.
- `proxyThroughServer` swaps the URL prefix only — the way to control players off-LAN.
- `PlexSonosClient` = `PlexClient` + `sonos.plex.tv` baseurl + audio-only + hand-rolled `playMedia` with caller identity.
- `SyncItem` is half-deprecated; builds a `location` URI and gets POSTed to plex.tv via `MyPlexAccount.sync()`.
