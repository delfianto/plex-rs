# 03 · MyPlex and Authentication

Source: `python-plexapi/plexapi/myplex.py` (2 616 LOC), plus
`plexapi/sonos.py`, `plexapi/mixins/watchlist.py`,
`plexapi/__init__.py`, `plexapi/config.py`, `plexapi/utils.py`.

`myplex.py` is the binding's largest module and the place plex-rs has
to make most of its early decisions.  `server.py` talks to a *single*
Plex Media Server over LAN/WAN; `myplex.py` talks to the cloud
surface — **plex.tv** and its sub-domains (`metadata`, `discover`,
`vod`, `music`, `sonos`, `clients`) — and coordinates server
discovery across that fleet.  It contains eleven concrete
`PlexObject` subclasses (`MyPlexAccount`, `MyPlexUser`,
`MyPlexInvite`, `Section`, `MyPlexServerShare`, `MyPlexResource`,
`ResourceConnection`, `MyPlexDevice`, `AccountOptOut`, `UserState`,
`GeoLocation`) and two helper classes (`MyPlexPinLogin`,
`MyPlexJWTLogin`).

---

## 1. `MyPlexAccount`

### 1.1 Construction

`MyPlexAccount.__init__` (`myplex.py:132`) accepts six call-signature
variants:

```
MyPlexAccount(token=...)                              # plain token
MyPlexAccount(username, password)                     # user/pass
MyPlexAccount(username, password, code='123456')      # user/pass + 2FA
MyPlexAccount(...,  session=requests.Session())       # custom session
MyPlexAccount(..., timeout=30)                        # custom timeout
MyPlexAccount(..., remember=False)                    # don't issue 14-day token
```

It immediately:

1. Wraps the token in `logfilter.add_secret()` so it is redacted from
   logs (`myplex.py:133`).
2. Falls back to `CONFIG.get('auth.server_token')`, then
   `auth.myplex_username` / `auth.myplex_password` from
   `~/.config/plexapi/config.ini` (and `PLEXAPI_*` env vars, via
   `config.py:23`).
3. Calls `_signin()` which either GETs `self.key`
   (`https://plex.tv/api/v2/user`) with the existing token, or POSTs
   credentials to `https://plex.tv/api/v2/users/signin` (`myplex.py:141`).
4. Hands the resulting XML to the inherited `PlexObject.__init__`,
   which calls `_loadData(data)`.

The class attribute layout is:

```
key      = 'https://plex.tv/api/v2/user'                  # myplex.py:130
SIGNIN   = 'https://plex.tv/api/v2/users/signin'          # myplex.py:118
SIGNOUT  = 'https://plex.tv/api/v2/users/signout'         # myplex.py:119
PING     = 'https://plex.tv/api/v2/ping'                  # myplex.py:124
```

### 1.2 `_loadData` shape

`_loadData` (`myplex.py:158`) parses the XML returned by
`GET /api/v2/user`.  The response is a single `<user>` element with
~30 attributes and three nested elements:

| Attribute (Python)              | XML attr                  | Cast      |
|---------------------------------|---------------------------|-----------|
| `authToken`                     | `authToken`               | str (secret) |
| `uuid`                          | `uuid`                    | str       |
| `id`                            | `id`                      | int       |
| `username`, `email`, `title`    | same                      | str       |
| `friendlyName`, `locale`, `country` | same                  | str       |
| `joinedAt`                      | `joinedAt`                | datetime  |
| `rememberExpiresAt`             | `rememberExpiresAt`       | datetime  |
| `home`, `homeAdmin`, `guest`, `restricted` | same           | bool      |
| `homeSize`, `maxHomeSize`       | same                      | int       |
| `protected`, `hasPassword`, `confirmed` | same              | bool      |
| `twoFactorEnabled`, `backupCodesCreated`, `emailOnlyAuth` | same | bool |
| `thumb`, `pin`                  | same                      | str       |
| `scrobbleTypes`                 | comma-list                | List[int] |
| `mailingListActive`, `mailingListStatus` | same             | bool/str  |
| `adsConsent*`, `experimentalFeatures`, `anonymous` | same   | str/bool  |

Nested elements:

- `<subscription active=… plan=… paymentService=… status=… subscribedAt=…>`
  + nested `<features><feature id="…"/></features>` →
  `subscriptionActive`, `subscriptionPlan`,
  `subscriptionPaymentService`, `subscriptionStatus`,
  `subscriptionSubscribedAt`, `subscriptionFeatures` (lazy via
  `@cached_data_property`, `myplex.py:218`).
- `<profile autoSelectAudio=… defaultAudioLanguage=… …>` →
  six `profile*` attributes.
- `<entitlements>` → `entitlements` (lazy, `myplex.py:223`).
- `<roles>` → `roles` (lazy, `myplex.py:227`).

`authenticationToken` (`myplex.py:231`) is a `@property` alias for
`authToken`, kept for back-compat with older code paths.

### 1.3 Request plumbing

`_headers()` (`myplex.py:242`) copies `BASE_HEADERS` and adds
`X-Plex-Token`.  `query()` (`myplex.py:250`) is a single dispatch
helper:

- Pulls `method` from `_session.get` by default.
- Logs `<METHOD> <url> <json-body>` at DEBUG.
- Treats `200/201/204` as success.
- Otherwise maps to exceptions:
  - `401` + `"verification code"` in body → `TwoFactorRequired`
  - `401` → `Unauthorized`
  - `404` → `NotFound`
  - `422` + `"Invalid token"` → `Unauthorized`
  - everything else → `BadRequest`
- Content-Type negotiation: `application/json` → `response.json()`;
  `text/plain` → stripped text; else → `utils.parseXMLString(...)`.

This is the **only** place in the binding that maps `401+verification`
to a dedicated exception, so the Rust port needs an explicit
`TwoFactorRequired` error variant.

### 1.4 `_reload`

`_reload(**kwargs)` (`myplex.py:236`) re-queries `self.key` (the
`/api/v2/user` endpoint), drops the cached element tree and calls
`_invalidateCacheAndLoadData(data)` (`PlexObject` base).  No
deep-reload contract — the kwargs are accepted only for compatibility
with the base class.

---

## 2. Sign-in flows

Three distinct flows live in this file.

### 2.1 Direct token

`MyPlexAccount(token='2ffLuB84dqLswk9skLos')` is the cheapest path.
`_signin()` short-circuits at `myplex.py:142-143`:

```
if self._token:
    return self.query(self.key), self.key
```

Single `GET https://plex.tv/api/v2/user` with `X-Plex-Token`.  No POST.
A 401 indicates the token expired; a 422 with `"Invalid token"` is
likewise mapped to `Unauthorized` by `query()`.

### 2.2 Username + password (+ 2FA)

`_signin()` builds:

```
POST https://plex.tv/api/v2/users/signin
Body (form): login=<username>
             password=<password>
             rememberMe=true|false
             verificationCode=<code>   # if 2FA enabled
```

Headers are the standard `BASE_HEADERS` (no `X-Plex-Token` yet, since
there isn't one).  Response is the same `<user>` XML as `/api/v2/user`
and is passed straight to `_loadData`.  Plex.tv returns:

- **401 with `"verification code"` in the body** → 2FA required.
  Caller catches `TwoFactorRequired`, prompts for `code`, retries with
  `code=` kwarg.
- **401 otherwise** → bad password.
- **200** → token is in the `authToken` attribute of the returned
  `<user>` element.  `_loadData` immediately reassigns `self._token` to
  this token (`myplex.py:160`) so subsequent requests carry it.

`signout()` (`myplex.py:154`) issues
`DELETE https://plex.tv/api/v2/users/signout`, which invalidates the
token server-side.

### 2.3 PIN / OAuth flow — `MyPlexPinLogin`

Defined `myplex.py:1683-1926`.  Endpoint root is
`PINS = 'https://plex.tv/api/v2/pins'`.

Step 1 — create a PIN/OAuth code (`_getCode`, `myplex.py:1849`):

```
POST https://plex.tv/api/v2/pins
Query (OAuth only): strong=True
Headers: standard BASE_HEADERS, no X-Plex-Token

Response (XML <pin ...>):
  id=<numeric pin id>
  code=<4-char PIN or strong code>
```

Step 2 — for OAuth, build a redirect URL (`oauthUrl`,
`myplex.py:1763`):

```
https://app.plex.tv/auth/#!?
    clientID=<X-Plex-Client-Identifier>
   &context[device][product]=<X-Plex-Product>
   &context[device][version]=<X-Plex-Version>
   &context[device][platform]=<X-Plex-Platform>
   &context[device][platformVersion]=<X-Plex-Platform-Version>
   &context[device][device]=<X-Plex-Device>
   &context[device][deviceName]=<X-Plex-Device-Name>
   &code=<pin code>
   [&forwardUrl=<optional callback>]
```

For 4-digit PIN flow the user types the code at `https://plex.tv/link`
(`link()` method, see §9.3, handles the *other* side of this).

Step 3 — poll for the auth token (`_checkLogin`, `myplex.py:1869`):

```
GET https://plex.tv/api/v2/pins/<id>
Response: <pin authToken=...>   # null until the user completes the link
```

`run()` (`myplex.py:1788`) starts a daemon thread that calls
`_checkLogin()` every `POLLINTERVAL = 1` second up to a 120 s default
timeout, with optional `callback(token)`.  `waitForLogin()`,
`stop()`, and `checkLogin()` (synchronous) round out the API.

### 2.4 JWT flow — `MyPlexJWTLogin`

Defined `myplex.py:1929-2451`.  This is the newer flow described at
<https://developer.plex.tv/pms/#section/API-Info/Authenticating-with-Plex>.
It uses a different host:

```
PINS = 'https://clients.plex.tv/api/v2/pins'
AUTH = 'https://clients.plex.tv/api/v2/auth'
```

Key generation: `generateKeypair()` produces an Ed25519 keypair using
`cryptography.hazmat.primitives.asymmetric.ed25519` and writes raw 32-
byte blobs (`myplex.py:2055`).  `_keyID` is `sha256(priv || pub).hex()`
and is reused as both the `kid` JWT header field and as the
`thumbprint` claim.

Endpoint inventory specific to this flow:

| Method | URL                                              | Purpose                                         |
|--------|--------------------------------------------------|-------------------------------------------------|
| POST   | `…/clients.plex.tv/api/v2/pins`                  | Same as PIN flow but body carries the public JWK (`{"jwk": …, "strong": true}`). |
| GET    | `…/clients.plex.tv/api/v2/pins/<id>?deviceJWT=…` | Poll for `authToken`, passing the client JWT.   |
| POST   | `…/clients.plex.tv/api/v2/auth/jwk`              | Register a public JWK with Plex (`X-Plex-Token` required). |
| GET    | `…/clients.plex.tv/api/v2/auth/nonce`            | Fetch nonce for embedding in client JWT.        |
| POST   | `…/clients.plex.tv/api/v2/auth/token`            | Exchange a client-signed JWT for a Plex JWT.    |
| GET    | `…/clients.plex.tv/api/v2/auth/keys`             | Fetch Plex's public JWKs for signature verification. |

Client JWT shape (`_encodeClientJWT`, `myplex.py:2131`):

```json
{
  "nonce": "<from /auth/nonce>",
  "scope": "username,email,friendly_name,restricted,anonymous,joinedAt",
  "aud": "plex.tv",
  "iss": "<X-Plex-Client-Identifier>",
  "iat": <epoch s>,
  "exp": <epoch s + 5m>
}
```

Signed `EdDSA`, header `{"kid": <thumbprint>}`.

The returned Plex JWT is decoded with the same `EdDSA` algo, audience
`['plex.tv', <client-id>]`, issuer `'plex.tv'`, required claims
`['aud', 'iss', 'exp', 'iat', 'thumbprint']`.  Verification iterates
through `/auth/keys` (newest first) until one verifies
(`myplex.py:2151`).

`verifyJWT(refreshWithinDays=1)` (`myplex.py:2252`) considers the JWT
invalid if it expires within the next N days.  `refreshJWT()` performs
the nonce → client-JWT → exchange dance.  `registerDevice()` must be
called once with `X-Plex-Token` to associate the public JWK with the
device before refresh works (`myplex.py:2217`).

---

## 3. Resource discovery — `MyPlexResource`

`MyPlexResource` (`myplex.py:1420`) wraps each entry returned by:

```
GET https://plex.tv/api/v2/resources?includeHttps=1&includeRelay=1
```

Each XML `<Device>` element has the attributes documented at
`myplex.py:1462-1485` (`accessToken`, `clientIdentifier`,
`platform`, `provides`, `owned`, `home`, `presence`,
`httpsRequired`, `relay`, `publicAddressMatches`,
`dnsRebindingProtection`, `lastSeenAt`, etc.), plus a nested
`<connections>` element that becomes a list of `ResourceConnection`
objects (lazy, `myplex.py:1487-1489`).

### 3.1 `ResourceConnection`

`myplex.py:1566-1592` — attributes `address`, `port`, `protocol`,
`local`, `relay`, `ipv6`, `uri` (the HTTPS connection URL) and a
synthesised `httpuri` (`http://<address>:<port>`).  Note: Plex itself
*only* returns the HTTPS URI; the plain-HTTP variant is fabricated
client-side.

### 3.2 Preference ordering

`DEFAULT_LOCATION_ORDER = ['local', 'remote', 'relay']` and
`DEFAULT_SCHEME_ORDER = ['https', 'http']` (`myplex.py:1459-1460`).

`preferred_connections(ssl=None, locations=None, schemes=None)`
(`myplex.py:1491`) builds a nested dict
`{location → {scheme → [urls]}}` and then flattens it in
location-major / scheme-minor order.  Key rules:

- For **non-owned** resources (shared servers), only **non-local**
  connections are included — the local IP belongs to the *owner's*
  network, not ours (`myplex.py:1514`).
- `ssl=True` strips `'http'` from the scheme list; `ssl=False` strips
  `'https'`.  Default keeps both, HTTPS first.
- A connection is classified as `relay` if `connection.relay` is true,
  else `local` if `connection.local`, else `remote`.

### 3.3 Parallel connect

`MyPlexResource.connect()` (`myplex.py:1530`) is the heart of remote
discovery:

```
cls = PlexServer if 'server' in self.provides else PlexClient
listargs = [[cls, url, self.accessToken, self._server._session, timeout]
            for url in connections]
results = utils.threaded(_connect, listargs)
return _chooseConnection('Resource', self.name, results)
```

`utils.threaded(callback, listargs)` (`utils.py:309`) is a fan-out
helper:

- Spawns one **daemon `threading.Thread`** per `listargs` entry.
- Passes a shared `Event` so callbacks can signal early completion.
- Spin-waits in 50 ms slices until either the event fires or every
  thread has exited.

Each worker `_connect(...)` (`myplex.py:2454`) constructs the target
class with `(baseurl=url, token=token, session=session,
timeout=timeout)` and records `(url, token, device|None, runtime)` in
the shared result slot.  If `X_PLEX_ENABLE_FAST_CONNECT` is set
(`config.ini` → `plexapi.enable_fast_connect = true`, false by
default), the *first* successful worker sets the event and the main
thread returns immediately.  Otherwise the main thread waits for
*all* workers, preserving the preferred order so we get the best
connection rather than the fastest.

`_chooseConnection(ctype, name, results)` (`myplex.py:2483`) logs each
attempt at DEBUG, filters to live results, and returns the first one;
or raises `NotFound(f'Unable to connect to {ctype}: {name}')` if all
fail.

### 3.4 TLS / error semantics

There is **no special handling for TLS errors**.  `_connect` catches
the bare `Exception`, logs at ERROR level, and records `None`.  The
caller never sees a TLS handshake failure distinguished from a TCP
refusal.  For plex-rs we should:

- Surface TLS errors separately (cert mismatch is common on
  `*.plex.direct` certificates if DNS rebinding protection is
  disabled).
- Honour `httpsRequired` / `dnsRebindingProtection` flags to skip
  obviously-doomed candidates rather than racing them all.

### 3.5 Discovery surface

`MyPlexAccount.resources()` (`myplex.py:313`) GETs `MyPlexResource.key`
and returns a `list[MyPlexResource]`.  `resource(name)`
(`myplex.py:302`) is a linear lookup by `name` or `clientIdentifier`.

`MyPlexAccount.history()` (`myplex.py:891`) iterates **only resources
where `provides == 'server' and owned`**, connects to each, and
aggregates per-server play history with `accountID=1`.

---

## 4. Devices — `MyPlexDevice`

Defined `myplex.py:1595-1680`.  Backed by the legacy XML endpoint:

```
GET  https://plex.tv/devices.xml       # listing
DELETE https://plex.tv/devices/<id>.xml # remove a device  (myplex.py:1666-1669)
```

Each `<Device>` carries `clientIdentifier`, `name`, `product`,
`platform`, `device`, `model`, `vendor`, `provides`, `token`,
`publicAddress`, `screenResolution`, `screenDensity`, `createdAt`,
`lastSeenAt`, and a nested `<Connection uri="…"/>` list captured
lazily as `connections: list[str]` (`myplex.py:1647-1649`).

`MyPlexDevice.connect(timeout=None)` (`myplex.py:1651`) is the same
fan-out as `MyPlexResource.connect`, but works on the *device token*
not the resource token, and treats `connections` as already
fully-qualified URIs.

`syncItems()` (`myplex.py:1671`) requires
`'sync-target' in self.provides`; otherwise it raises `BadRequest`.
Otherwise it delegates to `MyPlexAccount.syncItems(client=self)`,
which `GET`s `SyncList.key.format(clientId=self.clientIdentifier)` —
that URL template lives in `plexapi/sync.py`.

`syncList` is *not* a method on `MyPlexDevice`; the convenience name is
`syncItems()`.  The flow:

1. `account.syncItems(client=device)` →
   `GET <SyncList.key with clientId>` → returns `SyncList(account,
   data)`.
2. `account.sync(sync_item, client=device)` →
   `POST <SyncList.key with clientId>` with `params=SyncItem[...]`
   form fields (`myplex.py:820-877`).

---

## 5. Users / friends / sharing

### 5.1 Listing

```
GET https://plex.tv/api/users/           # myplex.py:1225  (MyPlexUser.key)
```

Returns an XML `<MediaContainer>` of `<User>` elements.  `MyPlexUser`
loads ~17 attributes (`username`, `email`, `id`, `home`, `restricted`,
`thumb`, allowed-feature flags, friend filter strings) and a nested
`<Server>` list which becomes `servers: list[MyPlexServerShare]`
(`myplex.py:1250`).

### 5.2 Invitations

| Action | Method | URL pattern | Body |
|--------|--------|-------------|------|
| Invite new friend | POST | `/api/servers/{machineId}/shared_servers` | JSON `{"server_id":…, "shared_server":{"library_section_ids":[…], "invited_email":<user>}, "sharing_settings":{...}}` |
| Invite by Plex ID | POST | same | `invited_id` instead of `invited_email` |
| Invite home user | POST | `/api/home/users?title=<title>` then POST shared_servers | — |
| Invite existing | POST | `/api/home/users?invitedEmail=<email>` | — |
| Accept invite | PUT  | `https://plex.tv/api/invites/requests/<inviteId>?friend=…&home=…&server=…` | — |
| Cancel sent invite | DELETE | `https://plex.tv/api/invites/requested/<inviteId>?friend=…&home=…&server=…` | — |
| Update sharing | PUT  | `/api/servers/{machineId}/shared_servers/{serverId}` | JSON shared_server.library_section_ids |
| Remove all shares | DELETE | `/api/servers/{machineId}/shared_servers/{serverId}` | — |
| Update filters | PUT  | `https://plex.tv/api/v2/sharings/{userId}?allowSync=…&filterMovies=…` | query string |
| Remove friend | DELETE | `https://plex.tv/api/v2/sharings/{userId}` | — |
| Remove home user | DELETE | `https://plex.tv/api/home/users/{userId}` | — |
| Switch home user | POST | `https://plex.tv/api/home/users/{userId}/switch?pin=…` | returns `<user authenticationToken="…"/>` |
| Set home PIN | PUT | `https://plex.tv/api/home/users/{userId}?pin=<newPin>&currentPin=…` | — |
| Set managed PIN | POST | `https://plex.tv/api/v2/home/users/restricted/{userId}?pin=…` | — |
| Remove managed PIN | POST | same with `removePin=1` | — |

`MyPlexInvite` (`myplex.py:1286`) exposes `REQUESTS =
'https://plex.tv/api/invites/requests'` (incoming) and `REQUESTED =
'https://plex.tv/api/invites/requested'` (outgoing).  Both are listed
by `pendingInvites(includeSent, includeReceived)` and join their
results.

### 5.3 Filter encoding

`_filterDictToStr` (`myplex.py:756`) takes
`{'contentRating': ['G', 'PG'], 'label': ['kid']}` and emits
`contentRating=G%2CPG|label=kid`.  Only keys `contentRating`,
`label`, `contentRating!`, `label!` are accepted; anything else raises
`BadRequest`.  Replicate exactly for Rust to maintain parity (the `!`
suffix means *exclude*).

### 5.4 Section ID resolution

`_getSectionIds(server, sections)` (`myplex.py:733`) fetches
`https://plex.tv/api/servers/{machineId}` and builds a lookup keyed by
section id, section key, and lowercase title.  Section objects are
mapped via their `LibrarySection.key`; raw strings are
case-insensitive titles.

### 5.5 `MyPlexServerShare` / `Section`

`MyPlexServerShare` (`myplex.py:1361`) carries `id`, `serverId`,
`machineIdentifier`, `name`, `lastSeenAt`, `numLibraries`,
`allLibraries`, `owned`, `pending`.

`MyPlexServerShare.sections()` GETs
`/api/servers/{machineId}/shared_servers/{serverId}` and parses the
inner `<Section>` elements (`myplex.py:1403`).

`Section` (`myplex.py:1325`) — `id`, `key`, `shared`, `title`,
`type`, plus the legacy `sectionId`/`sectionKey` aliases.
`Section.history()` resolves to the *user's accessible* server via
`server._server._server.resource(...).connect()` and calls
`server.history(accountID=…, librarySectionID=…)`.

`MyPlexUser.get_token(machineIdentifier)` (`myplex.py:1254`) walks the
`shared_servers` XML to extract the per-user `accessToken` for one
server.

---

## 6. Watchlist & Discover

`MyPlexAccount` carries four cloud-content host constants
(`myplex.py:126-129`):

```
VOD      = 'https://vod.provider.plex.tv'
MUSIC    = 'https://music.provider.plex.tv'
DISCOVER = 'https://discover.provider.plex.tv'
METADATA = 'https://metadata.provider.plex.tv'
```

These are **not on-server libraries**.  They are part of the Plex
cloud catalogue and require an account token but no Plex Media Server.

### 6.1 Watchlist read

`watchlist(filter='all'|'available'|'released', sort, libtype,
maxresults, **kwargs)` (`myplex.py:924`) builds:

```
GET https://discover.provider.plex.tv/library/sections/watchlist/<filter>
    ?includeCollections=1
    &includeExternalMedia=1
    [&sort=watchlistedAt:desc]
    [&type=1|2]   # 1=movie, 2=show, via utils.searchType
```

The response is a normal Plex media container, but each item refers to
a *guid* like `plex://movie/5d776b59ad5437001f79c6f8` that needs the
METADATA host to dereference fully.  `_toOnlineMetadata`
(`myplex.py:1157`) walks each loaded object and:

1. Constructs a throw-away `PlexServer(self.METADATA, self._token,
   session=self._session)` to act as the `_server` for those objects
   so subsequent `.reload()` calls hit `metadata.provider.plex.tv`.
2. Rewrites `obj._details_key` to set `includeUserState=1` and drop
   `includeFields`.

This is explicitly flagged as a TODO at `myplex.py:1159` — there is no
clean `MetadataProvider` class, just a `PlexServer` repurposed for the
job.

### 6.2 Watchlist mutate

```
PUT https://discover.provider.plex.tv/actions/addToWatchlist?ratingKey=<id>
PUT https://discover.provider.plex.tv/actions/removeFromWatchlist?ratingKey=<id>
```

`ratingKey` is extracted from the GUID with `guid.rsplit('/', 1)[-1]`.
`addToWatchlist`/`removeFromWatchlist` raise `BadRequest` if the item
is already / not on the list (they pre-check via `onWatchlist`).

### 6.3 User-state on Discover

```
GET  https://metadata.provider.plex.tv/library/metadata/<ratingKey>/userState
GET  https://metadata.provider.plex.tv/actions/scrobble
                  ?key=<ratingKey>&identifier=com.plexapp.plugins.library
GET  https://metadata.provider.plex.tv/actions/unscrobble?key=…&identifier=…
```

The first returns a `<UserState>` element (parsed by `UserState`,
`myplex.py:2549-2577`) with `viewCount`, `viewedLeafCount`,
`viewOffset`, `viewState ∈ {complete,…}`, `watchlistedAt`.  `onWatchlist`
just checks `bool(userState.watchlistedAt)`.

`markPlayed`/`markUnplayed` (`myplex.py:1042`, `1057`) issue **GET**
requests against `/actions/scrobble` and `/actions/unscrobble`
respectively — note that these are *not* PUT/POST, despite being
state-changing.

### 6.4 `searchDiscover`

`searchDiscover(query, limit=30, libtype=None,
providers='discover')` (`myplex.py:1072`) issues:

```
GET https://discover.provider.plex.tv/library/search
    ?query=<q>&limit=<n>&searchTypes=movies|tv|movies,tv
    &searchProviders=discover[,PLEXAVOD][,PLEXAVOD,PLEXTVOD]
    &includeMetadata=1
Accept: application/json
```

The response is JSON, not XML — `query()` notices `application/json`
in the response header and returns the decoded dict.  The function
then pulls `MediaContainer.SearchResults[?id=='external'].SearchResult`,
re-encodes each `Metadata` block as an XML string `<Video … />`
(movie) or `<Directory … />` (show), and feeds it through
`_manuallyLoadXML` so the rest of the binding treats them as normal
`Movie`/`Show` objects.  `_toOnlineMetadata` then attaches the
METADATA pseudo-server.

This XML round-trip is a wart the Rust port should fix by parsing the
JSON directly into the metadata types.

### 6.5 Adjacent surfaces

- `videoOnDemand()` → `GET https://vod.provider.plex.tv/hubs` (returns
  `Hub` objects).
- `tidal()` → `GET https://music.provider.plex.tv/hubs`.
- `streamingServices()` (defined in `mixins/watchlist.py:48`) →
  `GET https://metadata.provider.plex.tv/library/metadata/<ratingKey>/availabilities`.

### 6.6 Watchlist mixin

`mixins/watchlist.py` simply attaches `onWatchlist`,
`addToWatchlist`, `removeFromWatchlist`, and `streamingServices`
methods to media classes (`Movie`, `Show`, etc.).  Each calls back
into `MyPlexAccount`, deriving the account from `self._server` via
`server.myPlexAccount()` or — if `self._server` *is* a
`MyPlexAccount` (happens after `_toOnlineMetadata`) — using it
directly.  The `try/except AttributeError` at
`mixins/watchlist.py:13-15` is the dispatch mechanism.

---

## 7. Sonos / cloud players

`MyPlexAccount.sonos_speakers()` (`myplex.py:318`) gates on
`'companions_sonos' in self.subscriptionFeatures` — a Plex Pass perk.
It caches results for 5 seconds:

```
GET https://sonos.plex.tv/resources       # myplex.py:325
```

The XML is mapped to `PlexSonosClient` (`sonos.py:9-115`).
`PlexSonosClient` subclasses `PlexClient` and pins `_baseurl =
"https://sonos.plex.tv"`, `_token = account._token`,
`_proxyThroughServer = False`.  Lookup helpers:

- `sonos_speaker(name)` — matches `title.split('+')[0].strip()`.
- `sonos_speaker_by_id(identifier)` — `machineIdentifier.startswith(identifier)`.

`PlexSonosClient.playMedia(media, offset, **params)` issues a Plex
client `playback/playMedia` command (via inherited `sendCommand`), but
fills out additional fields:

```
type=music
providerIdentifier=com.plexapp.plugins.library
containerKey=/playQueues/<id>?own=1
key=<media key>
offset=<ms>
machineIdentifier=<media server machineId>
protocol=<http|https>
address=<server host>
port=<server port>
token=<server-issued ephemeral token via createToken()>
commandID=<auto-inc>
X-Plex-Client-Identifier=<config>
X-Plex-Token=<server token>
X-Plex-Target-Client-Identifier=<speaker machineId>
```

Critical detail: the Sonos cloud needs **both** the Plex.tv account
token (used to authenticate against `sonos.plex.tv`) **and** a
short-lived server-issued token (via `media._server.createToken()`)
that grants the Sonos relay temporary access to the user's PMS.  The
Rust port needs to wire these up the same way; otherwise playback
fails silently with a Sonos timeout.

Audio is the only supported media type today (`raise BadRequest` for
anything else, `sonos.py:83`).

---

## 8. Webhooks management

Endpoint: `WEBHOOKS = 'https://plex.tv/api/v2/user/webhooks'`
(`myplex.py:120`).

- **List**: `webhooks()` (`myplex.py:784`) →
  `GET https://plex.tv/api/v2/user/webhooks`.  XML response of
  `<webhook url="…"/>` entries; the binding extracts just the URL
  list via `listAttrs(data, 'url', etag='webhook')`.
- **Set whole list**: `setWebhooks(urls)` (`myplex.py:777`) →
  `POST https://plex.tv/api/v2/user/webhooks` with form body
  `urls[]=<u1>&urls[]=<u2>…` or, if the list is empty, `urls=` to
  clear.  Returns the new list.
- **Add one**: `addWebhook(url)` (`myplex.py:765`) reads the cached
  `_webhooks`, appends, and re-POSTs.  Important: this is
  read-modify-write against the **local** cache, so if you didn't call
  `webhooks()` first you will clobber existing entries.  The Rust port
  should always `GET` before adding.
- **Delete one**: `deleteWebhook(url)` (`myplex.py:770`) — same
  pattern, but raises `BadRequest` if the URL is not present locally.

No per-webhook DELETE endpoint exists on Plex's side.

---

## 9. Account-level claims & misc settings

### 9.1 Server claim token

`claimToken()` (`myplex.py:879`):

```
GET https://plex.tv/api/claim/token.json
Headers: X-Plex-Token (yours)
Response: {"token": "claim-xxxxxx"}
```

The returned `claim-xxxx` is what you paste into a fresh Plex Media
Server (or `PLEX_CLAIM` env var of `plexinc/pms-docker`) to bind it to
your account.  Tokens expire after ~4 minutes.

### 9.2 Public IP / GeoIP

```
GET https://plex.tv/:/ip                              # myplex.py:1182
GET https://plex.tv/api/v2/geoip?ip_address=<addr>    # myplex.py:1192
```

`publicIP()` returns the response text directly (`Content-Type:
text/plain`, handled by `query()` at `myplex.py:273`).  `geoip(ip)`
returns a `GeoLocation` with `city`, `code`, `continentCode`,
`coordinates: (lat, lng)`, `country`, `postalCode`, `subdivisions`,
`timezone`, `europeanUnionMember`, `inPrivacyRestrictedCountry`
(`myplex.py:2580-2615`).

### 9.3 PIN linking

`MyPlexAccount.link(pin)` (`myplex.py:1144`) — the *other* side of the
PIN-login flow.  An already-authenticated account claims a pending
4-digit pin code from a *new* device:

```
PUT https://plex.tv/api/v2/pins/link
Headers: X-Plex-Token (your account)
         Content-Type: application/x-www-form-urlencoded
         X-Plex-Product: Plex SSO
Body: code=<4-digit-pin>
```

### 9.4 Privacy opt-outs

```
PUT https://plex.tv/api/v2/user/privacy
    ?optOutPlayback=0|1
    &optOutLibraryStats=0|1
```

`optOut(playback=None, library=None)` (`myplex.py:789`) — params
omitted from the URL when the corresponding kwarg is `None`.

### 9.5 Online Media Sources (per-user opt-outs)

```
GET  https://plex.tv/api/v2/user/{userUUID}/settings/opt_outs
POST https://plex.tv/api/v2/user/{userUUID}/settings/opt_outs
     ?key=<source-key>&value=<opt_in|opt_out|opt_out_managed>
```

`onlineMediaSources()` (`myplex.py:905`) returns a list of
`AccountOptOut` (`myplex.py:2497`).  The class enforces
`CHOICES = {'opt_in', 'opt_out', 'opt_out_managed'}` and forbids the
`opt_out_managed` setting for `tv.plex.provider.music`
(`myplex.py:2543`).

### 9.6 View-state sync

```
GET  https://plex.tv/api/v2/user/view_state_sync   Accept: application/json
PUT  https://plex.tv/api/v2/user/view_state_sync?consent=true|false
```

`viewStateSync` (`@property`) returns the boolean `consent` field.
`enableViewStateSync()` / `disableViewStateSync()` flip it.

### 9.7 Ping (token-refresh)

`PING = 'https://plex.tv/api/v2/ping'` (`myplex.py:124`).  `ping()`
(`myplex.py:276`) is documented as "refresh the authentication token
to prevent it from expiring."  Response is `text/plain` containing
`true` or `false`.  Callers can schedule a periodic ping if they
relied on `remember=False` and want to extend without re-issuing.

### 9.8 Subscription state

Pulled from the `<subscription>` sub-element of the user XML (§1.2),
exposed as `subscriptionActive`, `subscriptionStatus`,
`subscriptionPlan`, `subscriptionPaymentService`,
`subscriptionDescription`, `subscriptionSubscribedAt`, and the lazy
`subscriptionFeatures: list[str]` and `entitlements: list[str]`.
There is **no** dedicated subscription endpoint — the data piggybacks
on `/api/v2/user`.

### 9.9 Two-factor secret rotation

Although `twoFactorEnabled` is exposed, the binding has **no API for
enabling, disabling, or rotating 2FA secrets**.  The only 2FA-related
behaviour is detecting `TwoFactorRequired` during sign-in (§2.2) and
passing `code=` on retry.  Backup-code creation is reflected via
`backupCodesCreated` but cannot be triggered.  Anything beyond
`code='…'` for sign-in must be done via the web UI.

---

## 10. Header construction — comparison to `PlexServer`

`MyPlexAccount._headers()` (`myplex.py:242`) and `PlexServer._headers`
(`server.py`) both start from the same dict produced by
`config.reset_base_headers()` (`config.py:53-68`):

```
X-Plex-Platform           = uname()[0]            # 'Darwin'
X-Plex-Platform-Version   = uname()[2]            # kernel version
X-Plex-Provides           = 'controller'
X-Plex-Product            = 'PlexAPI'
X-Plex-Version            = plexapi VERSION
X-Plex-Device             = X_PLEX_PLATFORM
X-Plex-Device-Name        = uname()[1]            # hostname
X-Plex-Client-Identifier  = hex(uuid.getnode())   # MAC-derived
X-Plex-Language           = 'en'
X-Plex-Sync-Version       = '2'
X-Plex-Features           = 'external-media'
```

Both then layer on `X-Plex-Token` when one is available.

**Differences:**

1. `MyPlexAccount.query()` (`myplex.py:250`) injects per-call headers
   via `**kwargs`, where `MyPlexJWTLogin._headers()` *additionally*
   forces `Accept: application/json` (`myplex.py:2437`).  PlexServer
   doesn't do this.
2. `MyPlexAccount.link(pin)` is the only path that overrides
   `X-Plex-Product` (sets it to `'Plex SSO'`, `myplex.py:1152`).  The
   resulting request also carries the unusual
   `Content-Type: application/x-www-form-urlencoded`.
3. Two endpoints request JSON explicitly via the `headers={'Accept':
   'application/json'}` arg passed to `query()`:
   - `searchDiscover` (`myplex.py:1087`)
   - `viewStateSync` getter (`myplex.py:1123`)
4. Several endpoints send `Content-Type: application/json` (alongside
   a `json=` body), notably `inviteFriend`, `createHomeUser`,
   `createExistingUser`, `updateFriend` — all the sharing endpoints
   (`myplex.py:372`, `400`, `446`, `634`).
5. The `MyPlexJWTLogin` and `MyPlexPinLogin` helpers do **not** carry
   `X-Plex-Token` on the initial pin-creation call — only on the
   step where the device-registration JWK is POSTed
   (`myplex.py:2194`).
6. **No extra headers beyond `BASE_HEADERS` + `X-Plex-Token`** are
   sent to plex.tv that aren't also sent to a Plex Media Server.
   Specifically there is no `X-Plex-Username`, no `X-Plex-Account-Id`,
   no per-call user-agent override.  This is good news for the Rust
   port: a single `HeaderBuilder` can serve both surfaces, with two
   small specialisations (JSON `Accept`, JSON `Content-Type`).

---

## 11. Endpoint inventory (full)

The table below covers every distinct URL constructed inside
`myplex.py` (and the directly-related `sonos.py` and
`mixins/watchlist.py`).  All endpoints require `X-Plex-Token` (the
account's main token, or a per-resource access token) **unless
otherwise noted**.

| Method | Path | Purpose | Auth |
|--------|------|---------|------|
| GET    | `https://plex.tv/api/v2/user` | Fetch the signed-in account profile (`_loadData` source) | token |
| POST   | `https://plex.tv/api/v2/users/signin` | Username/password (+ optional `verificationCode`) login | none |
| DELETE | `https://plex.tv/api/v2/users/signout` | Invalidate the current token | token |
| GET    | `https://plex.tv/api/v2/ping` | Keep-alive / token refresh | token |
| POST   | `https://plex.tv/api/v2/pins` | Create 4-digit PIN or OAuth strong-code | none |
| GET    | `https://plex.tv/api/v2/pins/{id}` | Poll a pin for `authToken` | none |
| PUT    | `https://plex.tv/api/v2/pins/link` | Bind a pending PIN to this account (`X-Plex-Product: Plex SSO`) | token |
| POST   | `https://clients.plex.tv/api/v2/pins` | JWT-flow PIN creation (carries public JWK) | none |
| GET    | `https://clients.plex.tv/api/v2/pins/{id}?deviceJWT=…` | JWT-flow PIN polling | client JWT |
| POST   | `https://clients.plex.tv/api/v2/auth/jwk` | Register a public JWK against the account | token |
| GET    | `https://clients.plex.tv/api/v2/auth/nonce` | Nonce for client-JWT signing | none |
| POST   | `https://clients.plex.tv/api/v2/auth/token` | Exchange client JWT for Plex JWT | client JWT |
| GET    | `https://clients.plex.tv/api/v2/auth/keys` | Fetch Plex public JWKs | none |
| GET    | `https://plex.tv/api/v2/resources?includeHttps=1&includeRelay=1` | List `MyPlexResource` records (servers/clients) | token |
| GET    | `https://plex.tv/devices.xml` | List `MyPlexDevice` records | token |
| DELETE | `https://plex.tv/devices/{id}.xml` | Remove a device from the account | token |
| GET    | `https://plex.tv/api/users/` | List friends and shared users (`MyPlexUser`) | token |
| GET    | `https://plex.tv/api/invites/requests` | Incoming pending invites | token |
| GET    | `https://plex.tv/api/invites/requested` | Outgoing pending invites | token |
| PUT    | `https://plex.tv/api/invites/requests/{id}?friend=…&home=…&server=…` | Accept invite | token |
| DELETE | `https://plex.tv/api/invites/requested/{id}?friend=…&home=…&server=…` | Cancel sent invite | token |
| GET    | `https://plex.tv/api/servers/{machineId}` | List sections on a server (used by `_getSectionIds`) | token |
| POST   | `https://plex.tv/api/servers/{machineId}/shared_servers` | Create a share / invite friend | token |
| PUT    | `https://plex.tv/api/servers/{machineId}/shared_servers/{serverId}` | Update share libraries | token |
| DELETE | `https://plex.tv/api/servers/{machineId}/shared_servers/{serverId}` | Remove all shares for user | token |
| GET    | `https://plex.tv/api/servers/{machineId}/shared_servers/{serverId}` | List sections of one share (`MyPlexServerShare.sections`) | token |
| PUT    | `https://plex.tv/api/v2/sharings/{userId}?…` | Update friend filters | token |
| DELETE | `https://plex.tv/api/v2/sharings/{userId}` | Remove friend | token |
| POST   | `https://plex.tv/api/home/users?title=…` | Create home user | token |
| POST   | `https://plex.tv/api/home/users?invitedEmail=…` | Invite existing user as home user | token |
| DELETE | `https://plex.tv/api/home/users/{userId}` | Remove home user | token |
| PUT    | `https://plex.tv/api/home/users/{userId}?pin=…&currentPin=…` | Set / change home PIN | token |
| POST   | `https://plex.tv/api/home/users/{userId}/switch?pin=…` | Switch to home user (returns user token) | token |
| POST   | `https://plex.tv/api/v2/home/users/restricted/{userId}?pin=…` | Set managed-user PIN | token |
| POST   | `https://plex.tv/api/v2/home/users/restricted/{userId}?removePin=1` | Remove managed-user PIN | token |
| GET    | `https://plex.tv/api/v2/user/webhooks` | List webhooks | token |
| POST   | `https://plex.tv/api/v2/user/webhooks` | Replace webhook list (`urls[]=…`) | token |
| GET    | `https://plex.tv/api/v2/user/{userUUID}/settings/opt_outs` | List online-media opt-outs | token |
| POST   | `https://plex.tv/api/v2/user/{userUUID}/settings/opt_outs?key=…&value=…` | Set one opt-out | token |
| PUT    | `https://plex.tv/api/v2/user/privacy?optOutPlayback=…&optOutLibraryStats=…` | Privacy opt-outs | token |
| GET    | `https://plex.tv/api/v2/user/view_state_sync` (JSON) | Watch-state sync consent | token |
| PUT    | `https://plex.tv/api/v2/user/view_state_sync?consent=…` | Toggle watch-state sync | token |
| GET    | `https://plex.tv/api/claim/token.json` | Get a server-claim token | token |
| GET    | `https://plex.tv/:/ip` | Public IP (text/plain) | token |
| GET    | `https://plex.tv/api/v2/geoip?ip_address=…` | IP geolocation | token |
| GET    | `https://sonos.plex.tv/resources` | List Sonos speakers | token (Plex Pass) |
| —      | `https://sonos.plex.tv/…` (player commands via `PlexClient`) | Sonos playback | token |
| GET    | `https://vod.provider.plex.tv/hubs` | Plex VOD hubs | token |
| GET    | `https://music.provider.plex.tv/hubs` | Tidal hubs (music) | token |
| GET    | `https://discover.provider.plex.tv/library/sections/watchlist/{filter}?…` | Watchlist read | token |
| PUT    | `https://discover.provider.plex.tv/actions/addToWatchlist?ratingKey=…` | Add to watchlist | token |
| PUT    | `https://discover.provider.plex.tv/actions/removeFromWatchlist?ratingKey=…` | Remove from watchlist | token |
| GET    | `https://discover.provider.plex.tv/library/search?…` (JSON) | Discover search | token |
| GET    | `https://metadata.provider.plex.tv/library/metadata/{ratingKey}/userState` | Per-user state on a Discover item | token |
| GET    | `https://metadata.provider.plex.tv/library/metadata/{ratingKey}/availabilities` | Streaming-service availability (from `mixins/watchlist.py`) | token |
| GET    | `https://metadata.provider.plex.tv/actions/scrobble?key=…&identifier=com.plexapp.plugins.library` | Mark Discover item played | token |
| GET    | `https://metadata.provider.plex.tv/actions/unscrobble?key=…&identifier=com.plexapp.plugins.library` | Mark Discover item unplayed | token |
| —      | `https://app.plex.tv/auth/#!?clientID=…&context[device][…]=…&code=…` | OAuth redirect target (browser-only, not a fetch) | none |

### 11.1 Host families to model in Rust

For the plex-rs port, this collapses to **six distinct hosts**:

1. `plex.tv` — the canonical account API surface (versioned v1 legacy
   under `/api/...` and v2 under `/api/v2/...`).
2. `clients.plex.tv` — JWT auth only.
3. `app.plex.tv` — OAuth redirect target (URL builder only — never
   fetched by the binding).
4. `sonos.plex.tv` — Sonos resource listing and command proxying.
5. `vod.provider.plex.tv`, `music.provider.plex.tv`,
   `discover.provider.plex.tv`, `metadata.provider.plex.tv` —
   cloud-catalogue / watchlist / discover.  All share the same auth
   scheme but disagree on Content-Type (`/library/search` is JSON,
   most others are XML).
6. *Per-resource* PMS hosts (handled by `server.py`, out of scope
   here).

This naturally suggests a Rust crate layout of one `MyPlexClient`
struct that owns a `reqwest::Client`, an account token, and a small
typed router over those six host families, with a separate
`PinLogin` / `JwtLogin` state machine and an explicit
`ResourceConnector` doing the parallel `connect()` race.
