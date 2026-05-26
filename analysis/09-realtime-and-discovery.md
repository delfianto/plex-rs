# 09 — Real-time, Discovery, and Settings Surfaces

This document covers four loosely related surfaces in python-plexapi that all
sit outside the normal "fetch XML by URL" request/response loop:

1. `AlertListener` — a WebSocket consumer for the Plex Media Server (PMS)
   notification channel.
2. `GDM` — Plex's "Good Day Mate" UDP multicast/broadcast discovery protocol
   for local servers and clients.
3. `Settings` / `Setting` — the strongly-typed wrapper around PMS preferences
   (`/:/prefs`).
4. **HTTP webhooks** — the user-registered HTTPS callbacks PMS POSTs to when
   media or admin events fire. python-plexapi only *manages the URL list*;
   it does not parse the inbound payload.

All file:line citations are against this checkout of
`python-plexapi/plexapi/`.

---

## 1. `AlertListener` — WebSocket notification stream

### 1.1 Class shape

`alert.py:9` defines `class AlertListener(threading.Thread)`. The class is
deliberately small (~96 LOC). Construction (`alert.py:40-47`):

```python
AlertListener(server, callback=None, callbackError=None, ws_socket=None)
```

- `server` — a `PlexServer` instance (used only to derive the URL and token).
- `callback(data: dict)` — invoked for every successfully-parsed message.
- `callbackError(error)` — invoked when the underlying websocket raises.
- `ws_socket` — optional pre-existing socket (pass-through into
  `websocket.WebSocketApp(... socket=...)`).

The thread is marked `self.daemon = True` (`alert.py:42`) so it does not block
process exit. There is no internal queue; the callback is invoked synchronously
on the websocket reader thread.

### 1.2 URL construction

`alert.py:38` defines the key path:

```python
key = '/:/websockets/notifications'
```

`run()` builds the full URL by reusing `PlexServer.url(...)` and swapping the
scheme (`alert.py:56`):

```python
url = self._server.url(self.key, includeToken=True).replace('http', 'ws')
```

Concretely, against a server at `http://10.0.0.20:32400` with token
`xxxxxxxxxxxxxxxxxxxx`, this yields:

```
ws://10.0.0.20:32400/:/websockets/notifications?X-Plex-Token=xxxxxxxxxxxxxxxxxxxx
```

Note: the naive `replace('http', 'ws')` replaces only the first occurrence,
so `http://...` → `ws://...` works but `https://...` → `wssps://...` is
broken. python-plexapi assumes plaintext HTTP/WS to the local PMS. A Rust
port should explicitly map `http`→`ws` and `https`→`wss`.

The query string includes `X-Plex-Token` (and any other plex-headers folded in
by `PlexServer.url(..., includeToken=True)`). PMS does **not** accept the token
via `Sec-WebSocket-Protocol` or `Authorization` headers on this endpoint — it
must be in the URL.

### 1.3 Driving the loop

`alert.py:51-61`:

```python
import websocket  # the `websocket-client` PyPI package
...
self._ws = websocket.WebSocketApp(
    url,
    on_message=self._onMessage,
    on_error=self._onError,
    socket=self._socket,
)
self._ws.run_forever()
```

`websocket-client`'s `run_forever()` blocks until the connection is closed.
python-plexapi does **not** install reconnection logic, ping/pong keepalives,
or backoff. If the TCP connection drops or PMS restarts, the listener simply
ends; `server.py:811-833`'s `PlexServer.startAlertListener()` does not
re-spawn it. The caller must detect end-of-thread (e.g. via `is_alive()`) and
restart.

`stop()` (`alert.py:63-69`) calls `self._ws.close()`. Once stopped the thread
cannot be re-started — `threading.Thread.start()` raises `RuntimeError` on a
second call. The docstring at `alert.py:64-66` says so explicitly:

> Once the notifier is stopped, it cannot be directly started again. You must
> call `PlexServer.startAlertListener` from a PlexServer instance.

### 1.4 Message envelope and types

`_onMessage` (`alert.py:71-83`):

```python
data = json.loads(message)['NotificationContainer']
log.debug('Alert: %s %s %s', *data)
if self._callback:
    self._callback(data)
```

The wire payload is JSON (PMS sends only text frames on this endpoint). Every
message has a top-level wrapper `{"NotificationContainer": {...}}`, and the
callback receives the inner dict. The `log.debug('Alert: %s %s %s', *data)`
line iterates the dict's *keys* (a side effect of Python `*dict`), which
incidentally tells us PMS messages always have exactly three top-level keys,
typically:

- `type` — string discriminator
- `size` — message item count (matches the length of the body array)
- a third key whose name matches `type` (e.g. `PlayState`, `ActivityNotification`)
  and whose value is the array of items.

A representative `playing` message:

```json
{"NotificationContainer": {
  "type": "playing", "size": 1,
  "PlaySessionStateNotification": [{
    "sessionKey": "12", "ratingKey": "31425",
    "key": "/library/metadata/31425",
    "viewOffset": 12345, "state": "playing"
  }]
}}
```

python-plexapi itself does **not** enumerate the message types it expects —
the entire dispatch is the caller's responsibility. The only enumeration in
the file is the **timeline state values** documented in the class docstring
(`alert.py:16-26`), which apply to messages of type `timeline` with
`identifier=com.plexapp.plugins.library`:

| `state` | Meaning                            |
|---------|------------------------------------|
| 0       | The item was created               |
| 1       | Reporting progress on item processing |
| 2       | Matching the item                  |
| 3       | Downloading the metadata           |
| 4       | Processing downloaded metadata     |
| 5       | The item processed                 |
| 9       | The item deleted                   |

Other message `type` values observed in the wild (not enumerated by
python-plexapi but documented by PMS and referenced by downstream consumers
like Home Assistant's `pms` integration and `plexwebsocket`):

- `playing` — `PlaySessionStateNotification[]`. Emitted on play/pause/stop
  with `state ∈ {"playing", "paused", "stopped", "buffering"}`.
- `progress` — `ProgressNotification[]` for scrobble-style ticks.
- `activity` — `ActivityNotification[]` with `event ∈ {"started", "updated", "ended"}`
  describing scans, optimizes, refreshes.
- `timeline` — `TimelineEntry[]`; library item lifecycle (see state table).
- `transcodeSession.start|update|end` — `TranscodeSession[]`.
- `update.statechange` — server self-update lifecycle.
- `reachability` — connectivity changes.
- `status` — generic status notifications.
- `setting` / `preference` — server setting/preference changes.
- `backgroundProcessingQueue` — queued background tasks.
- `account` — myplex account changes pushed down.

A Rust port should treat these as an open enum (deserialize-as-string + match
known variants, surface `Unknown(String)` for forward-compatibility).

### 1.5 Threading model

- `AlertListener` *is* the thread (subclass of `threading.Thread`).
- `daemon=True` → exits with the process.
- `run()` blocks on `websocket.run_forever()`.
- Callbacks run on this thread. Long-running callbacks block message
  consumption; PMS will eventually disconnect if the buffer fills.
- No mutex/lock is held; the listener owns no shared state besides the
  reference to `_server` (read-only) and `_ws` (only touched by `stop()`).

---

## 2. `GDM` — Good Day Mate discovery (`gdm.py`)

### 2.1 Provenance

`gdm.py:1-10` notes the file was lifted from
`home-assistant/netdisco/gdm.py` (Apache 2.0) and traces lineage back to
hippojay's `plexGDM` and iBaa's `PlexConnect`. The protocol is plaintext over
UDP; nothing about it is documented officially by Plex.

### 2.2 The two scan modes

`GDM.update(scan_for_clients)` (`gdm.py:50-132`) drives the entire scan. There
is one constant message payload (`gdm.py:84`):

```
M-SEARCH * HTTP/1.0
```

(yes, HTTP/1.0, not 1.1 — Plex deviates from SSDP here; SSDP uses HTTP/1.1
`M-SEARCH *`). Encoded as ASCII bytes and sent as a single UDP datagram.

Mode selection (`gdm.py:99-108`):

| Mode                       | Destination                | Port    | Socket flags |
|----------------------------|----------------------------|---------|--------------|
| `scan_for_clients=False` (servers) | `239.0.0.250` (multicast)  | `32414` | `IP_MULTICAST_TTL=1` |
| `scan_for_clients=True` (clients)  | `255.255.255.255` (broadcast) | `32412` | `SO_REUSEADDR`, `SO_BROADCAST` |

Note these match the ports inverted from what you'd expect from naive
reading: **servers respond on 32414, clients on 32412**. The `from` tuples in
the response dicts in the docstring confirm this (`gdm.py:64-81`):

- Server reply: `'from': ('10.10.10.100', 32414)`
- Client reply: `'from': ('10.10.10.101', 32412)`

TTL is hard-coded to 1 (`gdm.py:97`), keeping the multicast strictly local.

### 2.3 Timeout

`gdm.py:85-92`:

```python
gdm_timeout = 1                   # seconds
sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sock.settimeout(gdm_timeout)
```

Total scan time is bounded by the recv timeout; the loop (`gdm.py:115-130`)
keeps `recvfrom()`-ing until `socket.timeout` fires, then returns. There is no
configurable timeout exposed on the class. With `gdm_timeout=1` you have
roughly a one-second wall-clock budget.

(There is a copy-paste oddity at `gdm.py:97`: the multicast TTL is set to
`struct.pack("B", gdm_timeout)`, reusing the timeout variable as TTL. They
happen to both equal 1, but conceptually they are different things. A Rust
port should split them.)

### 2.4 Response parsing

Each reply is a tiny HTTP-shaped text blob like:

```
HTTP/1.0 200 OK
Content-Type: plex/media-server
Resource-Identifier: 646ab0aa8a01c543e94ba975f6fd6efadc36b7
Name: myfirstplexserver
Port: 32400
Version: 1.18.8.2527-740d4c206
Host: 53f4b5b6023d41182fe88a99b0e714ba.plex.direct
```

Parser (`gdm.py:117-128`) checks for `'200 OK'` on the status line, then
splits each remaining line on `':'`. Caveats: `line.split(':')` (not
`splitn(2)`) will explode on any value containing a colon (e.g. IPv6 in a
future `Host:`); header names are case-sensitive; dedup is by
`Resource-Identifier`. Returned shape: `{'data': {<headers>}, 'from':
(ip, port)}`.

### 2.5 Helper methods on `GDM`

- `scan(scan_for_clients=False)` (`gdm.py:25-27`) — invokes `update()`.
  No caching: every call re-issues UDP traffic.
- `all(scan_for_clients=False)` (`gdm.py:29-35`) — returns a copy of
  `entries`.
- `find_by_content_type(value)` (`gdm.py:37-41`) — substring match on
  `entry['data']['Content-Type']` (so `"server"` matches `"plex/media-server"`).
- `find_by_data(values: dict)` (`gdm.py:43-48`) — strict subset match.

### 2.6 PMS toggle

The PMS has a server-level preference `GdmEnabled` (boolean). When disabled,
the server simply does not reply, so `entries` stays empty. The shipped test
(`tests/test_gdm.py`) keys on exactly this:

```python
gdm_enabled = plex.settings.get("GdmEnabled")
gdm.scan()
if gdm_enabled:
    assert len(gdm.entries)
else:
    assert not len(gdm.entries)
```

---

## 3. `settings.py` — PMS preferences

### 3.1 Class layout

Two real classes (`settings.py:9`, `:88`) plus a registered subclass
(`settings.py:170`):

- `Settings(PlexObject)` — the container, anchored at `key = '/:/prefs'`
  (`settings.py:15`).
- `Setting(PlexObject)` — a single preference. Not registered with
  `@registerPlexObject` directly; `Settings` builds the dict itself.
- `Preferences(Setting)` (`settings.py:170-184`) — registered with
  `TAG='Setting'` and `FILTER='preferences'` so generic `fetchItems` calls
  that walk a `<Preferences><Setting .../></Preferences>` blob (e.g. a
  client's player preferences) deserialize each `<Setting>` element to a
  `Preferences` instance. Adds `_default()` which PUTs back the `default`
  value.

### 3.2 The type system

`settings.py:103-110`:

```python
_bool_cast = lambda x: bool(x == 'true' or x == '1')
_bool_str  = lambda x: str(x).lower()
TYPES = {
    'bool':   {'type': bool,  'cast': _bool_cast, 'tostr': _bool_str},
    'double': {'type': float, 'cast': float,      'tostr': str},
    'int':    {'type': int,   'cast': int,        'tostr': str},
    'text':   {'type': str,   'cast': str,        'tostr': str},
}
```

PMS additionally sends `type='enum'` settings, but the table has no `enum`
row. The cast path (`settings.py:128-132`) skips conversion for enums (raw
string), while `set()` at `:155` does `isinstance(value, self.TYPES[self.type]['type'])` —
which `KeyError`s for `type='enum'`. A Rust port should explicitly model
enum as a fifth variant with allowed values.

`enumValues` parsing (`settings.py:134-148`):

- PMS sends either `enumValues="a|b|c"` (plain pipe-separated) or
  `enumValues="0:Off|1:On|2:Auto"` (colon-keyed labels).
- If any `:` appears, returns `dict[cast_key -> label]`.
- Otherwise returns `list[str]`.

### 3.3 Reading values

Container `__getattr__` (`settings.py:21-27`) forwards attribute access:

```python
plex.settings.AcceptedEULA   #  ↔  plex.settings.get('AcceptedEULA').value
```

`get(id)` lowercases the first character (`utils.lowerFirst`) before lookup,
so `Settings.get('AcceptedEULA')` and `Settings.get('acceptedEULA')` resolve
to the same `Setting`. The internal dict `self._settings` is keyed by the
lowerFirst form (`settings.py:37-41`).

`groups()` / `group(name)` (`settings.py:54-69`) bucket settings by the
server-supplied `group` attribute (`general`, `network`, `transcoder`, etc.).

### 3.4 Writing values

Two-phase commit:

1. `setting.set(value)` (`settings.py:150-162`):
   - Type-checks against `self.TYPES[self.type]['type']`.
   - Validates against `enumValues` membership if present.
   - Stores the stringified value into `self._setValue` (a *staging slot*) —
     `self.value` (the read-side) is unchanged.

2. `settings.save()` (`settings.py:71-85`):
   - Iterates all settings; collects any with non-None `_setValue` into a
     params dict.
   - Each value is `urllib.parse.quote(...)`-encoded (`:79`).
   - Builds `url = '/:/prefs?key1=v1&key2=v2'` (`:82-83`).
   - Issues `self._server.query(url, self._server._session.put)` — i.e. **PUT
     /:/prefs?…** with no body.
   - Calls `self.reload()` to pull fresh state and clear the staging slots
     (because `_loadData` resets `_setValue=None` via the
     `_invalidateCacheAndLoadData` path at `:38-39, :126`).
   - If no settings were staged, raises `BadRequest('No setting have been
     modified.')`.

### 3.5 `Setting` fields

From `_loadData` (`settings.py:112-126`): `id`, `label`, `summary`, `type`
(`bool|int|double|text|enum`), `default`, `value`, `hidden`, `advanced`,
`secure` (passwords/tokens), `group`, `option`, `enumValues` (list or dict),
plus the staging `_setValue`.

### 3.6 `Preferences._default()`

`settings.py:180-184`:

```python
def _default(self):
    key = f'{self._initpath}/prefs?'
    url = key + f'{self.id}={self.default}'
    self._server.query(url, method=self._server._session.put)
```

Used to reset a player's preference to its server-declared default. Bypasses
the staging slot — it PUTs immediately. The URL is `<initpath>/prefs?<id>=<default>`,
which for a client looks like `/player/setting/prefs?<id>=<default>`.

---

## 4. HTTP Webhooks (cross-reference to `myplex.py`)

### 4.1 What python-plexapi manages

The only webhook surface in the library is **URL list management** on a
`MyPlexAccount` (requires a Plex Pass subscription server-side). The endpoint
(`myplex.py:120`):

```
WEBHOOKS = 'https://plex.tv/api/v2/user/webhooks'
```

Four methods (`myplex.py:765-787`):

| Method                | HTTP                                  | Effect                       |
|-----------------------|---------------------------------------|------------------------------|
| `webhooks()`          | GET `/api/v2/user/webhooks`           | List configured URLs         |
| `addWebhook(url)`     | POST with full new `urls[]=...` list  | Add one URL                  |
| `deleteWebhook(url)`  | POST with full reduced `urls[]=...`   | Remove (raises `BadRequest` if absent at `:773`) |
| `setWebhooks(urls)`   | POST with `urls[]=u1&urls[]=u2` or `urls=` (empty) | Replace whole list |

The empty-list case at `:779` is notable — sending `data = {'urls': ''}`
(string, no brackets) is how you clear all webhooks; PMS distinguishes empty
array (`urls[]` absent) from "intentionally empty" (`urls=`).

Response parsing uses `listAttrs(data, 'url', etag='webhook')` (`:781, :786`)
to flatten the returned `<webhook url="..."/>` XML into a `list[str]`.

### 4.2 What PMS posts to webhooks

PMS POSTs to user-registered URLs with `Content-Type: multipart/form-data`
containing a `payload` JSON part and, for some events, a second `thumb` part
with a JPEG.

python-plexapi **does not parse this payload at all** — no `Webhook` or
`WebhookPayload` class exists in the package (grep confirms zero `multipart`
hits; the only `payload` matches are in `tests/payloads.py`, which holds
static XML fixtures, not webhook JSON). Consumers must implement parsing
themselves.

For completeness — the documented (and observed) event types in the JSON
`event` field are:

| `event`                  | Trigger                                                      |
|--------------------------|--------------------------------------------------------------|
| `media.play`             | Playback started                                             |
| `media.pause`            | Playback paused                                              |
| `media.resume`           | Playback resumed                                             |
| `media.stop`             | Playback stopped manually                                    |
| `media.scrobble`         | Item watched (passed 90% threshold)                          |
| `media.rate`             | User rated an item                                           |
| `library.on.deck`        | New item appears on a user's "On Deck"                       |
| `library.new`            | New item added to a library                                  |
| `admin.database.backup`  | Scheduled DB backup completed                                |
| `admin.database.corrupted` | DB integrity check failed                                  |
| `device.new`             | A new device first appeared on the account                   |
| `playback.started`       | Playback started (broader than `media.play`)                 |

The JSON envelope has the rough shape:

```json
{
  "event": "media.play",
  "user": true,
  "owner": true,
  "Account": {"id": 1, "thumb": "https://...", "title": "user"},
  "Server":  {"title": "myserver", "uuid": "..."},
  "Player":  {"local": true, "publicAddress": "...", "title": "...", "uuid": "..."},
  "Metadata": { ...same shape as /library/metadata response... }
}
```

A Rust port should ship this struct (with `serde(rename_all="camelCase")` and
optional fields) as part of an optional `webhook` feature, gated behind a
server framework choice.

---

## 5. Rust port notes

### 5.1 `AlertListener` → `tokio-tungstenite`

- Use `tokio_tungstenite::connect_async(url)` with a correct scheme map
  (`http`→`ws`, `https`→`wss`); do not port Python's `.replace('http','ws')`
  bug. Token must be in the query string (`?X-Plex-Token=...`).
- Define `enum Notification` with `#[serde(tag = "type")]` over the known
  variants (`playing`, `progress`, `activity`, `timeline`,
  `transcodeSession.start|update|end`, `update.statechange`, `reachability`,
  `status`, `setting`, `preference`, `account`, `backgroundProcessingQueue`),
  plus `#[serde(other)] Unknown` for forward compatibility.
- Wrap the envelope: `struct NotificationContainer { size, #[serde(flatten)] body: Notification }`.
- Expose a `Stream<Item = Result<Notification, AlertError>>` so consumers can
  drive it via `tokio::select!` / `StreamExt::next()`; avoid the Python
  callback-on-the-reader-thread pattern.
- Add what Python lacks: reconnect with exponential backoff (configurable
  cap), periodic `Message::Ping` to keep NAT mappings alive, and graceful
  shutdown via `CancellationToken`.

### 5.2 `GDM` → `tokio::net::UdpSocket`

- `mdns-sd` is *not* the right crate — GDM is **not mDNS**, it's bespoke
  HTTP-over-UDP. Use raw `tokio::net::UdpSocket` with
  `set_multicast_ttl_v4(1)` (servers) or `set_broadcast(true)` (clients).
- Two scan modes:
  - Servers: send `M-SEARCH * HTTP/1.0` to `239.0.0.250:32414`.
  - Clients: send `M-SEARCH * HTTP/1.0` to `255.255.255.255:32412`.
- Optionally `join_multicast_v4(239.0.0.250, INADDR_ANY)` to receive
  multicast replies on platforms where unicast replies to the source port
  aren't enough.
- Parse replies with `splitn(2, ':')` (not `split(':')`), dedupe by
  `Resource-Identifier`.
- Surface a `Stream<GdmEntry>` with a caller-controlled `Duration` budget;
  default 1s for parity. Model `GdmEntry { headers: HashMap<String,String>,
  from: SocketAddr }` with typed accessors (`content_type`,
  `resource_identifier`, `name`, `port: u16`, `version`).

### 5.3 Settings

- Model `SettingValue` as `enum { Bool(bool), Int(i64), Double(f64),
  Text(String), Enum { value: String, allowed: EnumAllowed } }` where
  `EnumAllowed` is `List(Vec<String>)` or `Map(IndexMap<String,String>)`.
- Builder-style setter that stages writes in a `HashMap<String,String>` on
  the parent `Settings`, mirroring `_setValue`.
- `save(&mut self, server)` issues a single `PUT /:/prefs?...` with
  URL-encoded params, then `reload()`s. Match Python's behavior of erroring
  when nothing is staged.

### 5.4 Webhook ingest

- Match python-plexapi's scope in the core SDK: provide
  `MyPlexAccount::{webhooks, set_webhooks, add_webhook, remove_webhook}`
  against `https://plex.tv/api/v2/user/webhooks`.
- Ship inbound parsing as a **separate, opt-in crate/feature**
  (`plex-rs-webhook` with an `axum` feature) providing an
  `axum::extract::FromRequest` extractor: parses `multipart/form-data` via
  `axum::extract::Multipart`, reads `payload` as JSON into a typed
  `WebhookEvent { event: WebhookEventType, user, owner, account, server,
  player, metadata }`, exposes the optional `thumb` part as `Bytes`. PMS does
  not sign these payloads — consumers must add URL-path secrets or
  reverse-proxy auth.
- `WebhookEventType` uses explicit `#[serde(rename = "media.play")]` per
  dotted variant + `#[serde(other)] Unknown`. Full taxonomy lives in this
  crate's docs, keeping the core SDK in lockstep with python-plexapi's scope.
