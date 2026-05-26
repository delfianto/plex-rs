# `PlexServer` — Deep Analysis

Primary file: `/Users/dwi.elfianto/Projects/Fun/plex-rs/python-plexapi/plexapi/server.py` (1293 LOC).
Companion files referenced throughout:

- `/Users/dwi.elfianto/Projects/Fun/plex-rs/python-plexapi/plexapi/settings.py` (185 LOC)
- `/Users/dwi.elfianto/Projects/Fun/plex-rs/python-plexapi/plexapi/alert.py` (97 LOC)
- `/Users/dwi.elfianto/Projects/Fun/plex-rs/python-plexapi/plexapi/base.py` (relevant: `PlexSession.stop()` at L1094, `fetchItems` at L227)
- `/Users/dwi.elfianto/Projects/Fun/plex-rs/python-plexapi/plexapi/library.py` (`Library`, `Hub` class at L2194)

`PlexServer` is the root `PlexObject` of the library. Almost every other class hangs off of an instance of it because attribute access through `PlexObject.fetchItem`/`fetchItems` requires `self._server.query(...)`. Its `key = '/'` (server.py:100) — the root document. The class is also peculiar in that it inherits from `PlexObject` while simultaneously being the `server` argument that `PlexObject.__init__` expects, which forces the unusual `super().__init__(self, data, self.key)` call in the constructor (server.py:110).

---

## 1. Construction (`__init__`, server.py:102-110)

```python
def __init__(self, baseurl=None, token=None, session=None, timeout=None):
    self._baseurl = baseurl or CONFIG.get('auth.server_baseurl', 'http://localhost:32400')
    self._baseurl = self._baseurl.rstrip('/')
    self._token = logfilter.add_secret(token or CONFIG.get('auth.server_token'))
    self._showSecrets = CONFIG.get('log.show_secrets', '').lower() == 'true'
    self._session = session or requests.Session()
    self._timeout = timeout or TIMEOUT
    data = self.query(self.key, timeout=self._timeout)
    super(PlexServer, self).__init__(self, data, self.key)
```

Key behaviors:

- **`baseurl` parsing** is minimal. There is no URL validation, no scheme inference, and no IPv6 special-casing. The only normalization is `rstrip('/')` so that `self.url(key)` (server.py:879) can blindly concatenate `key`, which must always start with `/`. The default `http://localhost:32400` is loaded from `CONFIG['auth.server_baseurl']` when not supplied, where `CONFIG` is the global `plexapi.CONFIG` (an `INI`-backed object).
- **Token handling** runs the token through `logfilter.add_secret(...)` (`plexapi/__init__.py`) so any subsequent debug logging will redact it. The token is *stored verbatim* — there is no encryption at rest — and gets injected as `X-Plex-Token` header in `_headers` (server.py:156-162). `self._showSecrets` is a flag read once from config that decides whether `url()` will append `?X-Plex-Token=…` to constructed URLs (used heavily for image transcode URLs and resource URLs, server.py:879-886).
- **Session injection** simply takes the user-supplied `requests.Session` or creates a default one. There is no connection pooling tuning, no retry config, no TLS hardening. All HTTP verbs are accessed off this session (`self._session.get`, `.put`, `.post`, `.delete`).
- **Eager vs lazy loading**: the only eager call is `self.query(self.key)` (i.e. `GET /`), parsed in `_loadData` (server.py:112-154). Everything else is lazy. Notice the two `@cached_data_property` decorators wrapping `library`, `settings`, `_systemAccounts`, `_systemDevices`, `_myPlexAccount` (server.py:167-182, 271-317). They defer requests until first access and then memoize. `cached_data_property` is the variant of `cached_property` that gets invalidated by `_invalidateCacheAndLoadData` whenever the underlying `_data` element is reloaded.
- **`_baseurl` vs `_serverIdentity`**: there is no field called `_serverIdentity`. The instance does carry `machineIdentifier` (loaded from `/` at server.py:126), `version`, and `friendlyName`. The closer thing is `Identity` (server.py:1277-1293) which is a separate lightweight object returned by `.identity()` and contains *only* `claimed`, `machineIdentifier`, `version`. The conceptual split is: `_baseurl` is the transport endpoint, `machineIdentifier` is the server's identity in the cluster (used to build `server://…` URIs in `_uriRoot()` server.py:164-165 and the Plex Web URL in `_buildWebURL` server.py:1018-1034). Notice `_uriRoot` returns the literal string `server://<machineIdentifier>/com.plexapp.plugins.library` — this is the URI scheme used throughout Plex for cross-server media references, hub items, etc.

The constructor's single `GET /` call also dictates the canonical PMS XML "root" response — fields parsed at server.py:114-154 include every server capability flag (`allowCameraUpload`, `allowSync`, `allowMediaDeletion`, `hubSearch`, `multiuser`, `myPlexSubscription`, `photoAutoTag`, `voiceSearch`, `sync`), platform info (`platform`, `platformVersion`, `version`), and transcoder availability (`transcoderAudio`, `transcoderVideo`, `transcoderSubtitles`, `transcoderLyrics`, `transcoderPhoto`, plus the lists `transcoderVideoBitrates`, `transcoderVideoQualities`, `transcoderVideoResolutions`). `ownerFeatures` and `diagnostics` are comma-or-space-separated lists run through `utils.toList`. `updatedAt` becomes a `datetime` via `utils.toDatetime` (epoch seconds). The single integer field that is cast inline (rather than via `utils.cast`) is `transcoderActiveVideoSessions` (server.py:142) — it defaults to `0` rather than `None`.

---

## 2. Identity Endpoints

Three closely related but distinct endpoints carry "who is this server" information:

| Endpoint | Caller | Returns | Fields parsed |
|---|---|---|---|
| `GET /` | `__init__` via `self.query(self.key)` server.py:109 | `<MediaContainer …>` root | the ~40 attributes in `_loadData` (server.py:114-154) |
| `GET /identity` | `identity()` server.py:184-187 → returns `Identity` (server.py:1277-1293) | `<MediaContainer claimed="…" machineIdentifier="…" version="…"/>` | `claimed: bool`, `machineIdentifier: str`, `version: str` |
| `GET /myplex/account` | `account()` server.py:189-192 → returns `Account` (server.py:1051-1094) | `<MyPlex …>` | `username`, `mappingState`, `mappingError`, `mappingErrorMessage`, `signInState`, `publicAddress`, `publicPort`, `privateAddress`, `privatePort`, `subscriptionFeatures: list`, `subscriptionActive: bool`, `subscriptionState` |

Sample `/identity` body:

```xml
<MediaContainer size="0" claimed="1"
  machineIdentifier="abc123def456…"
  version="1.40.4.8679-424562606"/>
```

Sample `/` (root) body shape (truncated):

```xml
<MediaContainer size="…" allowCameraUpload="0" allowChannelAccess="1"
  allowMediaDeletion="1" allowSharing="1" allowSync="0" backgroundProcessing="1"
  certificate="1" companionProxy="1" friendlyName="MyServer"
  hubSearch="1" machineIdentifier="abc123…" multiuser="1"
  myPlex="1" myPlexMappingState="mapped" myPlexSigninState="ok"
  myPlexSubscription="1" myPlexUsername="me@example.com"
  ownerFeatures="camera_upload,cloudsync,…"
  platform="Linux" platformVersion="5.15.0-1041-azure"
  pluginHost="1" readOnlyLibraries="0" requestParametersInCookie="1"
  streamingBrainVersion="5" sync="0"
  transcoderActiveVideoSessions="0" transcoderAudio="1"
  transcoderLyrics="1" transcoderPhoto="1" transcoderSubtitles="1"
  transcoderVideo="1"
  transcoderVideoBitrates="64,96,208,…"
  transcoderVideoQualities="0,1,2,…"
  transcoderVideoResolutions="128,128,160,…"
  updatedAt="1706548112" updater="1"
  version="1.40.4.8679-424562606" voiceSearch="0">
  <!-- Directory children: butler, channels, clients, hubs, library, … -->
</MediaContainer>
```

The root response also includes child `<Directory>` elements (clients, hubs, library, etc.) but `_loadData` does not parse them — they are reachable via the named endpoints.

`createToken()` (server.py:229-235) hits `GET /security/token?type=delegation&scope=all` to mint a delegated access token for sharing with a user-agent. It returns the `token` attribute or `None` for unclaimed servers.

`claim()` (server.py:194-205) is `POST /myplex/claim?token=<claim-token>`; `unclaim()` (server.py:207-212) is `DELETE /myplex/account`. Both return an `Account` parsed from the response.

---

## 3. Sessions

`sessions()` (server.py:803-805) is a pure passthrough:

```python
return self.fetchItems('/status/sessions')
```

`fetchItems` reads the `<MediaContainer>` returned, walks its `<Video>`/`<Track>`/`<Photo>` children and resolves them to the appropriate `PlexObject` subclass via `utils.PLEXOBJECTS`. Because each session XML element carries the full media metadata plus a `<Player>`, `<User>`, `<Session>`, and possibly `<TranscodeSession>` sub-element, the deserialized objects are `Movie`/`Episode`/`Track`/`Photo` instances that have been mixed in with `PlexSession` (`plexapi/base.py:1019`+). Each carries `sessionKey`, `player`, `user`, `session`, `transcodeSession` attributes.

Sample session XML:

```xml
<MediaContainer size="1">
  <Video sessionKey="42" key="/library/metadata/1234" ratingKey="1234"
         type="movie" title="Big Buck Bunny" viewOffset="120000" duration="596475">
    <Media id="…" videoCodec="h264" audioCodec="aac" container="mp4">
      <Part id="…" key="/library/parts/…/file.mp4" container="mp4">…</Part>
    </Media>
    <User id="1" title="admin" thumb="…"/>
    <Player address="10.0.0.5" device="iPhone" machineIdentifier="abc" platform="iOS"
            product="Plex for iOS" state="playing" title="iPhone XS" version="8.3"/>
    <Session id="abcdef-session-id" bandwidth="2334" location="lan"/>
    <TranscodeSession key="/transcode/sessions/abcdef" throttled="0"
                      progress="35.4" speed="6.2" videoDecision="copy"
                      audioDecision="transcode" sourceVideoCodec="h264"
                      sourceAudioCodec="aac" videoCodec="h264" audioCodec="aac"/>
  </Video>
</MediaContainer>
```

`transcodeSessions()` (server.py:807-809) returns the transcode rows from `GET /transcode/sessions` directly (not nested under a media item).

**Killing a session** is *not* a method on `PlexServer`. It is implemented on the session object itself: `PlexSession.stop(reason='')` at `plexapi/base.py:1094-1105`:

```python
key = '/status/sessions/terminate'
params = {'sessionId': self.session.id, 'reason': reason}
return self._server.query(key, params=params)
```

So the call pattern is `plex.sessions()[0].stop("Hogging the server")` which produces `GET /status/sessions/terminate?sessionId=abcdef-session-id&reason=Hogging+the+server`. Note this uses GET via `query()` (because no `method=` is passed) — PMS treats it as an action endpoint.

`currentBackgroundProcess()` (server.py:734-736) is `GET /status/sessions/background` and surfaces transient processing jobs.

---

## 4. History

`history()` (server.py:651-676) hits `/status/sessions/history/all`:

```python
args = {'sort': 'viewedAt:desc'}
if ratingKey:        args['metadataItemID'] = ratingKey
if accountID:        args['accountID'] = accountID
if librarySectionID: args['librarySectionID'] = librarySectionID
if mindate:          args['viewedAt>'] = int(mindate.timestamp())
key = f'/status/sessions/history/all{utils.joinArgs(args)}'
return self.fetchItems(key, maxresults=maxresults)
```

Notable points:

- The parameter aliases are *not* what you'd expect: `ratingKey` is sent as `metadataItemID` and `mindate` is sent using the operator-suffixed key `viewedAt>` (PMS supports `>`, `<`, `>=`, `<=`, `!=` directly in the query string).
- `mindate.timestamp()` casts to int (epoch seconds). This is the canonical way to bound history by time and is recommended for performance because PMS scans the whole history table otherwise.
- `accountID=1` is always the admin (see also `bandwidth()` at server.py:1004-1006 which special-cases the lookup).
- **Pagination** is handled by `fetchItems(maxresults=…)` in `plexapi/base.py:227`. The default container window is `X_PLEX_CONTAINER_SIZE` (defined in `plexapi/__init__.py`, typically 100), and `fetchItems` performs container-paged requests using `X-Plex-Container-Start` / `X-Plex-Container-Size` headers (set up in `_query` lower in base.py). `maxresults` short-circuits the loop once the requested count is reached, which is critical for history because watch history can run to tens of thousands of rows on a busy server.
- No `librarySectionID`/`accountID` validation — bad IDs return an empty `<MediaContainer/>`.

Returned items are `MovieHistory` / `EpisodeHistory` / `TrackHistory` / `PhotoHistory` (subclasses of the media type mixed with `PlexHistory` from base.py:1108+).

---

## 5. System: Accounts, Devices, Butler, Agents

Note up front: **there is no `accounts()` method and no `butler()` method on `PlexServer`** despite the task description hinting at them. The actual surface is:

### `systemAccounts()` / `systemAccount(id)` (server.py:271-290)
- Endpoint: `GET /accounts`
- Cached via `_systemAccounts: @cached_data_property` at server.py:271-275
- Items are `SystemAccount` (server.py:1126-1154), TAG `Account`. Fields: `id: int`, `key: str (/accounts/<id>)`, `name: str`, `defaultAudioLanguage`, `defaultSubtitleLanguage`, `autoSelectAudio: bool`, `subtitleMode: int`, `thumb`. Backwards-compat aliases `accountID = id`, `accountKey = key`.

### `systemDevices()` / `systemDevice(id)` (server.py:292-311)
- Endpoint: `GET /devices`
- Cached
- Items are `SystemDevice` (server.py:1157-1178), TAG `Device`. Fields: `clientIdentifier`, `createdAt: datetime`, `id: int`, `key = '/devices/<id>'`, `name`, `platform`. **Not the same as a `MyPlexDevice`** — these are PMS-side device records used for bandwidth attribution.

### `myPlexAccount()` (server.py:313-324)
- Doesn't hit PMS; constructs a `MyPlexAccount` against `https://plex.tv` using the *same* server token and session. Cached. Will fail with 401 if you used a delegate (non-owner) token.

### `butlerTasks()` / `runButlerTask(name)` (server.py:587-612)
- `GET /butler` → list of `ButlerTask` (server.py:1252-1274). Fields: `name: str` (e.g. `BackupDatabase`, `RefreshLibraries`, `OptimizeDatabase`, `DeepMediaAnalysis`, `GenerateAutoTags`, `BackupDatabaseAndAuxiliary`), `title`, `description`, `enabled: bool`, `interval: int (days)`, `scheduleRandomized: bool`.
- `POST /butler/<task>` to run on demand. There is no cancel method, but the underlying PMS endpoint is `DELETE /butler/<task>` (not implemented here).

### `activities` property (server.py:214-220)
- `GET /activities` → list of `Activity` (server.py:1097-1108) with `cancellable: bool`, `progress: int`, `title`, `subtitle`, `type`, `uuid`. The PMS endpoint for cancellation is `DELETE /activities/<uuid>` — also not exposed by `PlexServer`.

### `agents(mediaType=None)` (server.py:222-227)
- `GET /system/agents` or `GET /system/agents?mediaType=<int>` where `<int>` comes from `utils.searchType(mediaType)` (the `SEARCHTYPES` table at `utils.py:35-55`: `movie=1`, `show=2`, `artist=8`, etc.).
- Items decode into `plexapi.media.Agent` (now mostly deprecated — modern PMS exposes scanners under `/library/sections/.../agents` and via the Scanner mixin, but the legacy endpoint still works).
- Language/agent capabilities (which languages and scanners are available) come through the same call — the XML carries `<Agent>` rows with `<MediaType>` children that include `<Language code="en-US"/>` sub-elements.

### Updater (server.py:614-649)
- `checkForUpdate(force=True, download=False)` → `PUT /updater/check?download={0,1}`, then `GET /updater/status` → `Release` (server.py:1111-1123: `download_key`, `version`, `added`, `fixed`, `downloadURL`, `state`).
- `isLatest()`, `canInstallUpdate()` (reads `canInstall` attribute off the status root), `installUpdate()` → `PUT /updater/apply`.

---

## 6. Settings

`settings()` (the `@cached_data_property` at server.py:178-182) issues `GET /:/prefs`, returning a `Settings` container.

The XML shape:

```xml
<MediaContainer size="…" identifier="com.plexapp.system.preferences">
  <Setting id="FriendlyName" label="Friendly name"
           summary="This name will be used to identify this media server to other computers on your network…"
           type="text" default="" value="MyServer"
           hidden="0" advanced="0" group="general"/>
  <Setting id="ButlerStartHour" label="Butler tasks begin"
           summary="Specifies the hour at which scheduled tasks may begin."
           type="int" default="2" value="2"
           hidden="0" advanced="0" group="butler"
           enumValues="0:00:00|1:01:00|2:02:00|…|23:23:00"/>
  <Setting id="DvrIncrementalEpgLoader" label="…"
           type="bool" default="0" value="1"
           hidden="1" advanced="1" group="general"/>
  <Setting id="LogLevel" label="Logging level"
           type="int" default="3" value="3"
           enumValues="0:ERROR|1:WARN|2:INFO|3:DEBUG|4:VERBOSE"
           group="general"/>
</MediaContainer>
```

The `Settings` class (settings.py:9-85) is a dict-like container with namespace-style attribute access:

```python
def __getattr__(self, attr):       # settings.py:21
    if attr.startswith('_'): …
    return self.get(attr).value

def __setattr__(self, attr, value):  # settings.py:29
    if not attr.startswith('_'):
        return self.get(attr).set(value)
    self.__dict__[attr] = value
```

So `plex.settings.FriendlyName` reads the value, `plex.settings.FriendlyName = "newname"` stages a write. Note `utils.lowerFirst` is applied to the key in `_loadData` (settings.py:37) — IDs are stored under camelCase-with-lowered-first-letter, so `FriendlyName` → `friendlyName` internally. This means both `plex.settings.FriendlyName` and `plex.settings.friendlyName` resolve to the same key because `get()` (settings.py:47-52) re-applies `lowerFirst`.

`groups()` (settings.py:54-61) buckets settings by their `group` attribute (e.g. `general`, `butler`, `network`, `dlna`, `extras`, `transcoder`, `library`, `myPlex`). `group(name)` returns the list directly.

### Individual `Setting` validation (settings.py:88-166)

```python
TYPES = {
    'bool':   {'type': bool,  'cast': _bool_cast, 'tostr': _bool_str},
    'double': {'type': float, 'cast': float,      'tostr': str},
    'int':    {'type': int,   'cast': int,        'tostr': str},
    'text':   {'type': str,   'cast': str,        'tostr': str},
}
```

- `_cast()` is applied on load to both `value` and `default`. Note `enum` is not in `TYPES` — when `self.type == 'enum'` the value is left as-is (settings.py:130-132). But for `bool`/`int`/`text`/`double` the raw XML attribute is coerced.
- `_getEnumValues()` (settings.py:134-148) handles two flavors:
  - Pipe-and-colon `0:ERROR|1:WARN|…` → `dict` keyed by cast key
  - Pipe-only `Auto|Always|Never` → `list`
- `set(value)` (settings.py:150-162) validates:
  1. `isinstance(value, TYPES[self.type]['type'])` — passing an int to a text setting raises `BadRequest`
  2. If `enumValues` is set, the value must be in it
  3. The serialized form is stashed on `_setValue`; it doesn't hit the network yet
- `save()` (settings.py:71-85) collects every `_setValue` and issues `PUT /:/prefs?<k1>=<v1>&<k2>=<v2>…` using `urllib.parse.quote` per value (note: only values are quoted; keys are taken raw). After PUT, it calls `self.reload()` which re-fetches `GET /:/prefs` and invalidates the inner `Setting` instances via `_invalidateCacheAndLoadData` (settings.py:39).

There is also a `Preferences(Setting)` subclass (settings.py:169-184) registered with `TAG = 'Setting'`, `FILTER = 'preferences'`. It implements `_default()` for reverting a setting to default via `PUT <initpath>/prefs?<id>=<default>` (where `initpath` is the parent item's path, e.g. a library section's). This is the per-section preferences hook, distinct from `/:/prefs`.

---

## 7. Hubs

`PlexServer.continueWatching()` (server.py:799-801) is the only Hub-shaped helper directly on the server:

```python
return self.fetchItems('/hubs/continueWatching/items')
```

That's a single hub flattened to its items. For *all* home hubs, code typically goes through `plex.library.hubs(...)` (library.py:120-140) which hits `GET /hubs` (no section ID) — these are the "home" hubs visible to the calling user, e.g.:

- `home.continueWatching`
- `home.ondeck`
- `home.movies.recent`
- `home.television.recent`
- `tv.inprogress`
- `playlists.recent`

Each `<Hub>` element decodes into `Hub` (library.py:2194-2270): `context`, `hubKey`, `hubIdentifier`, `key`, `more: bool`, `random: bool`, `size: int`, `style`, `title`, `type`. Children are partial items (lightweight metadata) accessible via `_partialItems`. If `more=1`, calling `.items()` performs a follow-up `GET <hub.key>` to pull the full list.

**Server hubs vs library-section hubs**: server-wide hubs are global aggregations (cross-section, e.g. all the "recently added" items the user can see across the entire server). Library-level hubs at `/hubs/sections/<id>` (library.py:713-716) are scoped to a single section and include section-specific hubs (e.g. `tv.inprogress`, `season.unviewed`). The server's `home.*` hubs also respect the user's home filter rules and Home preferences set in `/hubs/sections/<id>/manage` (library.py:704+).

`plex.search(...)` (covered in §10) uses `/hubs/search`, which is a completely different beast — it's an on-demand search Hub stream rather than a static surface.

---

## 8. Clients / Players

`clients()` (server.py:400-414):

```python
for elem in self.query('/clients'):
    port = elem.attrib.get('port')
    if not port:
        ports = self._myPlexClientPorts() if ports is None else ports
        port = ports.get(elem.attrib.get('machineIdentifier'))
    baseurl = f"http://{elem.attrib['host']}:{port}"
    items.append(PlexClient(baseurl=baseurl, server=self,
                            token=self._token, data=elem, connect=False))
```

Notable behaviors:

- The `/clients` XML carries `<Server>` rows per controllable client (PMS calls them servers in this context, confusingly). Each row has `host`, `port`, `name`, `machineIdentifier`, `protocol`, `protocolCapabilities`, `product`, `version`, etc.
- Some clients omit `port` (mobile clients behind NAT discovery). The fallback `_myPlexClientPorts()` (server.py:326-341) queries `plex.tv` via `MyPlexAccount.devices()` and harvests the port from the device's first `connection` entry (issue #126 referenced in the docstring).
- `connect=False` means the `PlexClient` does not eagerly hit `/resources` on the client itself; it carries the metadata from the `<Server>` element directly. Later calls (`PlexClient.proxyThroughServer(False)` etc.) will attempt to talk to the client directly.

`client(name)` (server.py:416-430) iterates `clients()` and matches either `client.title` or `client.machineIdentifier`. Raises `NotFound` otherwise.

There is no separate "connect to player" step in this module; the actual playback methods live on `PlexClient`. The standard pattern:

```python
client = plex.client("Living Room TV")
movie = plex.library.section("Movies").get("Big Buck Bunny")
client.playMedia(movie)
```

`PlexClient.stop()` (client.py:422) and `playMedia()` use the `/player/playback/...` endpoints on the client's baseurl.

---

## 9. Transcoding

### Photo / image transcoder

`transcodeImage()` (server.py:835-877) builds a URL — it does not actually request anything. It composes params and returns a server URL with the token appended (`includeToken=True` at server.py:877). The resulting URL is something like:

```
http://localhost:32400/photo/:/transcode?
  url=/library/metadata/1234/thumb/1700000000
  &height=300&width=200
  &minSize=1&upscale=1
  &opacity=70&saturation=80&blur=3
  &background=000000&blendColor=000000
  &format=jpeg
  &X-Plex-Token=…
```

- `imageUrl` can be a relative server path (returned by `thumbUrl`/`artUrl`) or an external URL — PMS will fetch and re-encode either.
- `background` / `blendColor` `.strip('#')` so users can pass `#000000` or `000000`.
- `minSize=1` keeps the smallest dimension (used for thumbnail letterbox prevention).
- `imageFormat` is lowercased and accepts `'jpeg'` or `'png'`.

The URL is used as an `<img src>` value or fed into a download helper; you typically wouldn't call this server-side.

### `sync` / sync items

`refreshSynclist()` (server.py:888-890) → `PUT /sync/refreshSynclists` (force PMS to refetch its assigned sync items from plex.tv).
`refreshContent()` (server.py:892-894) → `PUT /sync/refreshContent` (re-evaluate which media should be synced).
`refreshSync()` (server.py:896-901) is the combo.

Other transcode-adjacent endpoints exposed:

- `optimizedItems(removeAll=None)` (server.py:716-723) — `GET /playlists?type=42` is the special "Optimized Versions" generator playlist; `removeAll=True` issues `DELETE /playlists/generators?type=42`. Items are `Optimized` from `plexapi.media`.
- `conversions(pause=None)` (server.py:725-732) — `GET /playQueues/1` returns the conversion queue items as `Conversion` objects. `pause=True/False` toggles via `PUT /:/prefs?BackgroundQueueIdlePaused={0,1}`.

---

## 10. Search

`PlexServer.search()` (server.py:761-797) is **server-wide universal search** via Hub Search:

```python
params = {'query': query, 'includeCollections': 1, 'includeExternalMedia': 1}
if limit:     params['limit'] = limit
if sectionId: params['sectionId'] = sectionId
key = f'/hubs/search?{urlencode(params)}'
for hub in self.fetchItems(key, Hub):
    if mediatype:
        if hub.type == mediatype:
            return hub._partialItems
    else:
        results += hub._partialItems
return results
```

Important nuances:

- `mediatype` is matched against `hub.type` (e.g. `'movie'`, `'show'`, `'actor'`), not against `searchType` integers. The function bails out on the first hub that matches (so it never returns more than one hub's worth when filtered).
- `sectionId` scopes the search to a single library section but the result is still a list of hubs.
- The library-level analog is `LibrarySection.search(...)` (library.py L1850+) which builds a `?type=<int>&<filters>` URL against `/library/sections/<id>/all` — that's the *structured* search supporting genre/year/sort filters via `Field` operators and produces typed items directly. The server-level `search()` returns *partial* items (light XML with only base attributes) keyed by hub category.
- `hubSearch` is a flag on the server (`self.hubSearch`) inherited from `/`. If a server has it off, the `/hubs/search` endpoint will still work but the hub diversity will be reduced.

`continueWatching()` (server.py:799-801) — `GET /hubs/continueWatching/items`. Flattened single-hub.

There is **no `onDeck()` on `PlexServer`**. The on-deck surface lives on `Library.onDeck()` (library.py:152-154) which queries `GET /library/onDeck`, and on `LibrarySection.onDeck()` (library.py:803-806) which queries `/library/sections/<id>/onDeck`. The server's "on deck" is conceptually the home hub `home.ondeck` accessible via `plex.library.hubs(identifier='home.ondeck')`.

`searchType` (mentioned in the task) is the `utils.searchType()` helper at `utils.py:239-269` translating `'movie'` → `1`, `'show'` → `2`, … . It is used by `agents(mediaType=...)` (server.py:226) but **not** by the server-level `search()` (which passes the raw `query` string and post-filters by `hub.type`). Library-section search uses `searchType` to build the `type=<int>` parameter.

---

## 11. Misc Server Helpers

### `downloadDatabases` / `downloadLogs` (server.py:563-585)

Both use `utils.download(url, token, …)` (which streams to disk via `requests`) against:

- `GET /diagnostics/databases` — zipped SQLite DBs (`com.plexapp.plugins.library.db`, `com.plexapp.plugins.library.blobs.db`)
- `GET /diagnostics/logs` — zipped Plex Media Server log set (server logs, transcoder logs, scanner logs)

`unpack=True` extracts the zip in-place. `showstatus=True` renders a progress bar via `tqdm` if installed. Token is passed as a query parameter (`utils.download` appends `?X-Plex-Token=…`).

### `refreshSynclist`, `refreshContent`, `refreshSync` — see §9.

### `_allowMediaDeletion(toggle=False)` (server.py:903-920)

Underscore-prefixed (not part of the public API), toggles PMS's media-deletion permission via `PUT /:/prefs?allowMediaDeletion={0,1}`. Defensive: refuses no-op toggles with `BadRequest`.

### `bandwidth(timespan, **kwargs)` (server.py:922-1010)

`GET /statistics/bandwidth?timespan=<n>&<filters>`. The `timespan` int is the bin granularity: `1=months, 2=weeks, 3=days, 4=hours, 6=seconds`. Filters: `accountID`, `deviceID`, `lan`, plus `at`/`bytes` with optional `<`/`>` operator suffixes. Returns `StatisticsBandwidth` (server.py:1181-1222) which has `account()` and `device()` lookups that hit the cached `systemAccounts()`/`systemDevices()`.

### `resources()` (server.py:1012-1016)

`GET /statistics/resources?timespan=6` → list of `StatisticsResources` (server.py:1225-1249). Fields: `hostCpuUtilization`, `hostMemoryUtilization`, `processCpuUtilization`, `processMemoryUtilization`, `at: datetime`, `timespan`.

### `browse(path)`, `walk(path)`, `isBrowsable(path)` (server.py:343-398)

System file browsing for picking import paths in the PMS UI. Endpoints:

- `GET /services/browse` (root drive list)
- `GET /services/browse/<base64(path)>?includeFiles={0,1}`

Items are `Path` (directory) or `File`. `walk()` mirrors `os.walk` semantics yielding `(path, paths, files)` tuples recursively.

### `createCollection`, `createPlaylist`, `createPlayQueue` (server.py:432-561)

These delegate to `Collection.create(…)`, `Playlist.create(…)`, `PlayQueue.create(…)` in their respective modules. They pass `self` as the server, so the collection/playlist will be associated with this server.

### `switchUser(user)` (server.py:237-269)

Returns a new `PlexServer` instance authenticated as a different home/managed user. It:

1. Resolves the `user` to a `MyPlexUser` via `self.myPlexAccount().user(user)` if a string was passed.
2. Calls `user.get_token(self.machineIdentifier)` — this hits plex.tv `/api/v2/home/users/<userId>/switch` (in `myplex.py`) to mint a server-scoped delegate token.
3. Constructs a fresh `PlexServer` on the same `baseurl`, with the delegate token, reusing the admin's session unless overridden.

This is the canonical "act as another user" mechanic for testing parental controls or seeing what an account can see.

### `getWebURL` / `_buildWebURL` (server.py:1018-1048)

Builds `https://app.plex.tv/desktop/#!/server/<machineIdentifier>/<endpoint>?…` (for actions) or `…/#!/media/<machineIdentifier>/com.plexapp.plugins.library?…` (for media items). Used to deep-link the web UI.

### `query(key, method=None, ...)` (server.py:738-759) — the engine

All HTTP traffic flows through here. Behavior:

1. URL = `self.url(key)` (no token unless `_showSecrets`)
2. Default method = `self._session.get`
3. Headers = `BASE_HEADERS` (plexapi/__init__.py) merged with `X-Plex-Token` when set
4. Status codes 200/201/204 are success; everything else maps:
   - 401 → `Unauthorized`
   - 404 → `NotFound`
   - any other → `BadRequest`
5. The error message includes a one-line-flattened body, the URL, and the canonical reason phrase via `requests.status_codes._codes`.
6. Returns `utils.parseXMLString(response.text)` — an `ElementTree` element, or `None` if body is empty.

This is the *only* place HTTP errors are translated to library exceptions. There is no retry, backoff, or auto-token-refresh.

---

## 12. Alerts Integration

`startAlertListener(callback=None, callbackError=None)` (server.py:811-833) instantiates `AlertListener` and starts the thread:

```python
notifier = AlertListener(self, callback, callbackError)
notifier.start()
return notifier
```

`AlertListener` (alert.py:9-97) is a `threading.Thread` subclass marked `daemon=True`. The websocket URL is built in `run()` (alert.py:56):

```python
url = self._server.url(self.key, includeToken=True).replace('http', 'ws')
```

with `self.key = '/:/websockets/notifications'`. So for an HTTPS server you get `wss://…/:/websockets/notifications?X-Plex-Token=…` and for plain HTTP you get `ws://…`. (The `.replace('http', 'ws')` is naive but works because PMS URLs always start with `http`/`https` — there's no risk of replacing the substring elsewhere thanks to `_baseurl` ending at the host:port without paths containing `http`.)

The thread runs `websocket.WebSocketApp(url, on_message=…, on_error=…).run_forever()`. The `websocket-client` dependency is imported lazily so plexapi doesn't hard-require it.

Each frame is JSON of the form:

```json
{"NotificationContainer": {
   "type": "playing|timeline|status|activity|transcodeSession.update|…",
   "size": 1,
   "PlaySessionStateNotification": [ … ] 
}}
```

`_onMessage` (alert.py:71-83) unwraps `NotificationContainer` and passes the dict to the user callback. Common notification types include:

- `playing` — `PlaySessionStateNotification`
- `timeline` — library activity (state 0=created, 1=in-progress, 2=matching, 3=downloading metadata, 4=processing metadata, 5=processed, 9=deleted) — documented in alert.py:16-25
- `status` — server status changes
- `activity` — long-running activities (matching `/activities`)
- `transcodeSession.update` and `transcodeSession.end`
- `update.statechange` — auto-update lifecycle

`stop()` (alert.py:63-69) closes the websocket. The thread cannot be restarted; users must call `startAlertListener()` again on the server.

---

## PMS Endpoint Table

Every PMS endpoint touched by this module, in source order. "Method" is the HTTP verb used; "Caller" cites file:line of the Python call.

| Method | Path | Caller (file:line) | Purpose | Response shape pointer |
|---|---|---|---|---|
| GET | `/` | server.py:109 (`__init__`) | Root capability / version document | `<MediaContainer …>` with ~40 attribs (server.py:114-154) |
| GET | `/identity` | server.py:186 (`identity()`) | Lightweight ID document | `<MediaContainer claimed machineIdentifier version/>` (server.py:1289-1293) |
| GET | `/myplex/account` | server.py:191 (`account()`) | Local cached MyPlex account view | `<MyPlex username … subscriptionFeatures …/>` (server.py:1081-1094) |
| POST | `/myplex/claim?token=<t>` | server.py:204 (`claim()`) | Claim unowned server | Returns updated `Account` doc |
| DELETE | `/myplex/account` | server.py:211 (`unclaim()`) | Unclaim from MyPlex | Returns `Account` doc |
| GET | `/library` | library.py via server.py:171 | Library root for browsing/searching | `<MediaContainer><Directory … /></MediaContainer>` |
| GET | `/library/sections/` | server.py:175 (fallback) | Sections when owner-only `/library` is forbidden | Same as above for non-admin |
| GET | `/:/prefs` | server.py:181 (`settings`) | Full settings list | `<MediaContainer><Setting id="…" type="bool|int|text|double|enum" value="…" enumValues="…"/>…</MediaContainer>` (see §6) |
| PUT | `/:/prefs?<id>=<val>…` | settings.py:84 (`save()`) | Persist setting changes | empty 200 |
| PUT | `/:/prefs?allowMediaDeletion={0,1}` | server.py:920 | Toggle media deletion | empty 200 |
| PUT | `/:/prefs?BackgroundQueueIdlePaused={0,1}` | server.py:728-730 | Pause/resume conversions | empty 200 |
| GET | `/activities` | server.py:218 (`activities`) | Running PMS activities | `<MediaContainer><Activity uuid title progress cancellable/>…` (server.py:1097-1108) |
| GET | `/system/agents[?mediaType=<int>]` | server.py:227 (`agents()`) | Metadata agents/scanners | `<MediaContainer><Agent …/>…</MediaContainer>` |
| GET | `/security/token?type=delegation&scope=all` | server.py:234 (`createToken()`) | Mint delegate token | `<… token="…"/>` |
| GET | `/accounts` | server.py:274 (`_systemAccounts`) | PMS-side accounts | `<MediaContainer><Account id name key thumb defaultAudioLanguage …/>…` (server.py:1126-1154) |
| GET | `/devices` | server.py:295 (`_systemDevices`) | PMS-side device records | `<MediaContainer><Device id clientIdentifier name platform createdAt/>…` (server.py:1157-1178) |
| GET | `/services/browse[/<base64>]?includeFiles={0,1}` | server.py:356-360 (`browse()`) | Server-side filesystem listing | `<MediaContainer><Path key …/><File key …/>…</MediaContainer>` |
| GET | `/clients` | server.py:404 (`clients()`) | List controllable clients | `<MediaContainer><Server host port name machineIdentifier protocolCapabilities/>…</MediaContainer>` |
| GET | `/diagnostics/databases` | server.py:571 (`downloadDatabases()`) | Download zipped SQLite DBs | binary zip |
| GET | `/diagnostics/logs` | server.py:583 (`downloadLogs()`) | Download zipped logs | binary zip |
| GET | `/butler` | server.py:589 (`butlerTasks()`) | List scheduled butler tasks | `<MediaContainer><ButlerTask name title description enabled interval scheduleRandomized/>…` (server.py:1252-1274) |
| POST | `/butler/<task>` | server.py:611 (`runButlerTask()`) | Trigger one butler task | empty 200 |
| PUT | `/updater/check?download={0,1}` | server.py:624 (`checkForUpdate()`) | Force update check | empty/200 |
| GET | `/updater/status` | server.py:625, 638 | Latest release info | `<MediaContainer canInstall><Release key version added fixed downloadURL state/></…>` (server.py:1111-1123) |
| PUT | `/updater/apply` | server.py:649 (`installUpdate()`) | Trigger update install | empty/200 |
| GET | `/status/sessions/history/all?<filters>` | server.py:675 (`history()`) | Watch history | Paginated `<MediaContainer><Video|Track|Photo viewedAt accountID …/>…</MediaContainer>` |
| GET | `/playlists?<filters>` | server.py:699 (`playlists()`) | All playlists | `<MediaContainer><Playlist ratingKey title playlistType/>…` |
| GET | `/playlists?type=42` | server.py:722 (`optimizedItems()`) | Optimized-versions generator | special playlist node |
| DELETE | `/playlists/generators?type=42` | server.py:720 (`optimizedItems(removeAll=True)`) | Drop all optimized versions | empty 200 |
| GET | `/playQueues/1` | server.py:732 (`conversions()`) | Conversion queue | `<MediaContainer><Conversion …/>…</MediaContainer>` |
| GET | `/status/sessions/background` | server.py:736 (`currentBackgroundProcess()`) | Background transcoding/processing jobs | `<MediaContainer><TranscodeJob …/>…</MediaContainer>` |
| GET | `/hubs/search?query=…&includeCollections=1&includeExternalMedia=1[&limit=&sectionId=]` | server.py:790 (`search()`) | Universal hub search | `<MediaContainer><Hub type title size more><Video/Track/Photo/Directory …/>…</Hub>…</MediaContainer>` (library.py:2194-2270) |
| GET | `/hubs/continueWatching/items` | server.py:801 (`continueWatching()`) | Flat continue-watching list | `<MediaContainer><Video/Episode viewOffset duration …/>…</MediaContainer>` |
| GET | `/status/sessions` | server.py:805 (`sessions()`) | Active playback sessions | See §3 sample (`<Video>` + `<Player>`+`<User>`+`<Session>`+`<TranscodeSession>`) |
| GET | `/status/sessions/terminate?sessionId=&reason=` | base.py:1104 (via `PlexSession.stop()`) | Kill a playback session | empty/200 |
| GET | `/transcode/sessions` | server.py:809 (`transcodeSessions()`) | Active transcodes | `<MediaContainer><TranscodeSession key progress speed videoDecision audioDecision …/>…</MediaContainer>` |
| URL only | `/photo/:/transcode?url=&height=&width=&…` | server.py:876 (`transcodeImage()`) | Build image transcode URL | Returns URL string; eventual GET serves transcoded JPEG/PNG |
| PUT | `/sync/refreshSynclists` | server.py:890 (`refreshSynclist()`) | Refresh sync list from plex.tv | empty 200 |
| PUT | `/sync/refreshContent` | server.py:894 (`refreshContent()`) | Re-evaluate sync content | empty 200 |
| GET | `/statistics/bandwidth?timespan=&<filters>` | server.py:1009 (`bandwidth()`) | Bandwidth statistics | `<MediaContainer><StatisticsBandwidth accountID deviceID at bytes lan timespan/>…` (server.py:1181-1222) |
| GET | `/statistics/resources?timespan=6` | server.py:1015 (`resources()`) | CPU/RAM samples | `<MediaContainer><StatisticsResources at hostCpuUtilization hostMemoryUtilization processCpuUtilization processMemoryUtilization timespan/>…` (server.py:1225-1249) |
| WS | `/:/websockets/notifications?X-Plex-Token=…` | alert.py:56 (`AlertListener.run()`) | Real-time event stream | JSON frames `{"NotificationContainer": {"type": "playing|timeline|status|activity|transcodeSession.update|…", …}}` |

Endpoints intentionally **not** wrapped in `PlexServer` but reachable through related code paths: `DELETE /butler/<task>` (cancel), `DELETE /activities/<uuid>` (cancel activity), `POST /playQueues`, `/library/onDeck` (lives on `Library`, not server), per-section hub endpoints `/hubs/sections/<id>`.

---

## Notes on naming mismatches vs the task spec

- `accounts()` does **not** exist; use `systemAccounts()`.
- `butler()` does **not** exist; use `butlerTasks()` and `runButlerTask(name)`.
- `onDeck` does **not** exist on `PlexServer`; use `plex.library.onDeck()` or `plex.library.hubs(identifier='home.ondeck')`.
- "Killing a session" lives on `PlexSession.stop()` in `plexapi/base.py:1094-1105`, not on `PlexServer`.
- `_serverIdentity` is not a real attribute; the closest analogs are `machineIdentifier` (from `/`) and the standalone `Identity` object from `/identity`.
