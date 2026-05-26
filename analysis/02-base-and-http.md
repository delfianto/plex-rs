# 02 — Base Layer & HTTP Foundation of `python-plexapi`

Scope: `plexapi/__init__.py`, `plexapi/const.py`, `plexapi/config.py`, `plexapi/exceptions.py`, `plexapi/utils.py`, `plexapi/base.py`. Line numbers point into `python-plexapi/plexapi/<file>:N`. Everything else in the library is built on patterns documented here.

---

## 1. The `PlexObject` model

`PlexObject` (`base.py:76`) is the root class for every server-side entity — a thin XML wrapper holding a back-ref to its `PlexServer`, the raw `xml.etree.Element`, the URL it was loaded by, and a weak ref to its parent. Concrete subclasses override `_loadData` to populate Python attributes from the XML attribute dict.

### Class attributes

```
TAG = None      # base.py:85 — XML element tag, e.g. 'Video', 'Directory'
TYPE = None     # base.py:86 — XML `type` attribute, e.g. 'movie', 'show'
key = None      # base.py:87 — Plex relative URL, set per-instance from XML
```

`TAG` and `TYPE` together form the registry key used to dispatch the XML → Python class (see §4).

### Construction sequence (`__init__`, `base.py:89-105`)

```python
def __init__(self, server, data, initpath=None, parent=None):
    self._server   = server
    self._data     = data
    self._initpath = initpath or self.key
    self._parent   = weakref.ref(parent) if parent is not None else None
    self._details_key   = None
    self._overwriteNone = True
    self._autoReload    = CONFIG.get('plexapi.autoreload', True, bool)
    self._edits         = None
    if data is not None:
        self._loadData(data)
    self._details_key = self._buildDetailsKey()
```

Key points:

- **`_server`**: every object can talk back to the server it came from. There is no detached representation; an object without a server cannot reload, fetch children, or build URLs.
- **`_data`**: the live `Element`. It is retained for the lifetime of the object so `findItems`, `cached_data_property`, and `_findAndLoadElem` can re-traverse it. (Holding the XML node is a memory cost the Rust port should weigh — see §8.)
- **`_initpath`**: the URL the data was fetched from. Used by `isFullObject()` (`base.py:676`) to decide if a follow-up reload is needed and by `_buildItem` to decide whether a child is a session/history element (`base.py:136-139`).
- **`_parent`**: a `weakref.ref` — calling `obj._parent()` either returns the parent or `None`. Used by `_isChildOf` (`base.py:199-212`) which walks up the chain via `obj = obj._parent()` until `None`. **There is no `_root()` method**; "find the root" is open-coded as `while obj._parent: obj = obj._parent()`.
- **`_overwriteNone`**: gate flag for `__setattr__`. When `False` (set during lazy reloads, `base.py:491-493`), assigning `None` to an existing attribute is a no-op. This prevents a partial-reload from blanking already-populated fields.
- **`_autoReload`**: read once from config at construction; controls whether attribute misses trigger `reload()` in `PlexPartialObject` (see §2).

### `_loadData` — abstract on `PlexObject`

```python
def _loadData(self, data):
    raise NotImplementedError('Abstract method not implemented.')
# base.py:573-575
```

Every concrete subclass overrides it. Example shape from `MediaContainer._loadData` (`base.py:1222-1234`):

```python
self.allowSync         = utils.cast(int,  data.attrib.get('allowSync'))
self.identifier        = data.attrib.get('identifier')
self.librarySectionID  = utils.cast(int,  data.attrib.get('librarySectionID'))
self.size              = utils.cast(int,  data.attrib.get('size'))
self.totalSize         = utils.cast(int,  data.attrib.get('totalSize'))
```

Pull each XML attribute by string key, cast through `utils.cast`, store on `self`. No schema; the only "type system" is the cast call.

### `_buildItem` — element-to-object factory (`base.py:127-144`)

```python
def _buildItem(self, elem, cls=None, initpath=None):
    initpath = initpath or self._initpath
    if cls is not None:
        return cls(self._server, elem, initpath, parent=self)
    etype = elem.attrib.get('streamType',
            elem.attrib.get('tagType',
            elem.attrib.get('type')))
    ehash = f'{elem.tag}.{etype}' if etype else elem.tag
    if initpath == '/status/sessions':
        ehash = f"{ehash}.session"
    elif initpath.startswith('/status/sessions/history'):
        ehash = f"{ehash}.history"
    ecls = utils.getPlexObject(ehash, default=elem.tag)
    if ecls is not None:
        return ecls(self._server, elem, initpath, parent=self)
    raise UnknownType(...)
```

`initpath` participates in dispatch: the same `<Video type="movie">` produces a different class under `/status/sessions` vs `/library/metadata/...`. This is the source of session/history sub-types.

### One object's full trace

`plex.library.section('Movies').get('Cars')`:

1. `PlexServer.__init__` (`server.py:102-110`) — `query('/')`, build `PlexServer` via `PlexObject.__init__`, `_data = <MediaContainer>`.
2. `library` property fetches `/library`, constructs `Library`.
3. `section('Movies')` → `fetchItems('/library/sections', ...)` → `query()` → `parseXMLString()` → for each `<Directory>` child, `_buildItem` resolves `('Directory', None)` to `MovieSection`.
4. `MovieSection.get('Cars')` → `fetchItem(key, title='Cars')` → `_buildItem` finds `('Video', 'movie')` → `Movie` with `_loadData` populating `title`, `ratingKey`, `key`.
5. `_buildDetailsKey` (`base.py:155-177`) appends `_INCLUDES`/`_EXCLUDES` to `self.key`, e.g. `/library/metadata/123?includeChapters=1&includeMarkers=1`. Stored in `_details_key`, used by reload.

The resulting `Movie` is *partial* (its `_initpath` is the section URL, not `_details_key`). Touching a not-yet-populated attribute triggers §2.

---

## 2. Lazy loading — `PlexPartialObject` and the `__getattribute__` trick

`PlexPartialObject` (`base.py:588`) is the lazy-reload base for every navigable entity (`Movie`, `Show`, `Episode`, `Track`, `Playlist`, `Collection`, ...). It piggybacks on Python attribute lookup to convert a missing field into a server round-trip.

### The mechanism — `__getattribute__` (`base.py:634-652`)

```python
def __getattribute__(self, attr):
    # Dragons inside.. :-/
    value = super(PlexPartialObject, self).__getattribute__(attr)
    if attr in _DONT_RELOAD_FOR_KEYS: return value           # 'key', 'sourceURI'
    if attr in USER_DONT_RELOAD_FOR_KEYS: return value
    if attr.startswith('_'): return value
    if value not in (None, []): return value
    if self.isFullObject(): return value
    if isinstance(self, (PlexSession, PlexHistory)): return value
    if self._autoReload is False: return value
    log.debug("Reloading %s for attr '%s'", objname, attr)
    self._reload(_overwriteNone=False)
    return super(PlexPartialObject, self).__getattribute__(attr)
```

Triggers reload only when **all** of these hold:

1. The attribute exists and resolved to `None` or `[]` (falsy-but-present).
2. The name is not in `_DONT_RELOAD_FOR_KEYS = {'key', 'sourceURI'}` (`base.py:19`).
3. The name is not private (`_*`).
4. `isFullObject()` returned `False` — i.e., the current `_initpath` is not a superset of `_details_key`.
5. The object is not a session/history (those have their own reload semantics).
6. `_autoReload` config is on.

The reload calls `_reload(_overwriteNone=False)` (`base.py:483-494`), which:

```python
details_key = self._buildDetailsKey(**kwargs) if kwargs else self._details_key
key         = key or details_key or self.key
self._initpath = key
data = self._server.query(key)
self._overwriteNone = _overwriteNone     # False during lazy reload
self._invalidateCacheAndLoadData(data[0])
self._overwriteNone = True
```

The `_overwriteNone=False` trick keeps already-populated attrs intact even if the more detailed response happens to omit them.

### `isFullObject` / `isPartialObject` (`base.py:676-690`)

```python
def isFullObject(self):
    parsed_key      = urlparse(self._details_key or self.key)
    parsed_initpath = urlparse(self._initpath)
    query_key  = set(parse_qsl(parsed_key.query))
    query_init = set(parse_qsl(parsed_initpath.query))
    return not self.key or (parsed_key.path == parsed_initpath.path
                            and query_key <= query_init)
```

A path-and-querystring subset check. If `_initpath` covers every include param that the canonical `_details_key` carries, the object is "full".

### Cached-property invalidation

`cached_data_property` (`base.py:41-53`) is `functools.cached_property` plus a class-side registry. `PlexObjectMeta` (`base.py:56-73`) collects each `cached_data_property` into `cls._cached_data_properties`. On reload, `_invalidateCacheAndLoadData` (`base.py:554-563`) drops them from `self.__dict__` so they re-compute against the new `_data`.

---

## 3. `fetchItem` / `fetchItems` / `findItems`

These are the search/iteration primitives every higher-level method ultimately calls.

### `fetchItems` (`base.py:227-363`)

Signature:

```python
def fetchItems(self, ekey, cls=None,
               container_start=None, container_size=None, maxresults=None,
               params=None, **kwargs)
```

1. `ekey` may be a string URL or `List[int]` ratingKeys → rewritten to `/library/metadata/<k1,k2,k3>` (`base.py:318-319`).
2. Pagination uses **request headers** `X-Plex-Container-Start` and `X-Plex-Container-Size` (`base.py:332-333`), set per iteration in `while True`. Default `container_size = X_PLEX_CONTAINER_SIZE = 100` (`__init__.py:23`).
3. Each iteration calls `self._server.query(ekey, headers=headers, params=params)`, runs `findItems`, accumulates into a `MediaContainer`. Loop stops when `container_start > totalSize` or `maxresults` is reached (`base.py:352-361`).
4. Returns a `MediaContainer[cls]` — both a `list` subclass *and* a `PlexObject`, so callers iterate and slice but also get `.totalSize`, `.offset`.

### `fetchItem` (`base.py:365-390`)

A thin convenience wrapper: returns `fetchItems(...)[0]` or raises `NotFound` on empty result. Accepts an `int` ratingKey shorthand that becomes `/library/metadata/<k>`.

### `findItems` (`base.py:392-412`)

Operates on already-fetched XML (no I/O):

```python
if cls and cls.TAG and 'tag' not in kwargs:
    kwargs['etag'] = cls.TAG          # ← `etag` is *element tag*, not HTTP ETag
if cls and cls.TYPE and 'type' not in kwargs:
    kwargs['type'] = cls.TYPE
if rtag:
    data = next(utils.iterXMLBFS(data, rtag), Element('Empty'))
items = MediaContainer[cls](...) if data.tag == 'MediaContainer' else []
for elem in data:
    if self._checkAttrs(elem, **kwargs):
        item = self._buildItemOrNone(elem, cls, initpath)
        if item is not None:
            items.append(item)
return items
```

Note `etag` as a kwarg name is overloaded: it means "match the XML element tag", not the HTTP cache header. (Pitfall — see §8.)

### Kwarg filter grammar — `_checkAttrs` (`base.py:496-552`)

Django-style filtering in three helpers:

- `_getAttrOperator(attr)` (`base.py:514-520`) — strips trailing `__op` (one of `OPERATORS`, `base.py:20-38`), returns `(attr, op_name, op_callable)`. Falls back to `exact`.
- `_getAttrValue(elem, attrstr)` (`base.py:522-539`) — splits on `__`. Each part except the last walks into a child element by tag (case-insensitive); the last is an attribute. `'etag'` returns `[elem.tag]`. Returns a list — `Genre__tag="Animation"` matches when *any* `<Genre tag="Animation"/>` child exists.
- `_castAttrValue(op, query, value)` (`base.py:541-552`) — coerces the raw XML string to `type(query)` so `viewCount__gte=0` works.

Operators registered at `base.py:20-38`:

```
exact, iexact, contains, icontains, ne, in, gt, gte, lt, lte,
startswith, istartswith, endswith, iendswith, exists, regex, iregex
```

`CamelCase__field` walks into a child element; `field__op` applies an operator; they compose: `Media__Part__file__startswith="D:\\Movies"`. Examples from `base.py:264-313`: `viewCount=0`, `Genre__tag="Animation"`, `viewCount__gte=0`, `Media__container__in=["mp4","mkv"]`, `guid__regex=r"..."`.

### `MediaContainer` (`base.py:1140-1234`)

`class MediaContainer(Generic[PlexObjectT], List[PlexObjectT], PlexObject)` — a `list` that also carries `size / totalSize / offset / librarySectionID / identifier / ...`. `fetchItems` extends across pagination pages, merging `size`/`totalSize` (`base.py:1176-1220`).

---

## 4. Class registry — `registerPlexObject` and dispatch

The registry is the single module-level dict `PLEXOBJECTS` in `utils.py:105`. Subclasses opt in via the `@registerPlexObject` decorator (`utils.py:132-147`):

```python
def registerPlexObject(cls):
    etype = getattr(cls, 'STREAMTYPE',
            getattr(cls, 'TAGTYPE', cls.TYPE))
    ehash = f'{cls.TAG}.{etype}' if etype else cls.TAG
    if getattr(cls, '_SESSIONTYPE', None):
        ehash = f"{ehash}.session"
    elif getattr(cls, '_HISTORYTYPE', None):
        ehash = f"{ehash}.history"
    if ehash in PLEXOBJECTS:
        raise Exception(f'Ambiguous PlexObject definition ...')
    PLEXOBJECTS[ehash] = cls
    return cls
```

Key composition: `TAG[.TYPE|.STREAMTYPE|.TAGTYPE][.session|.history]`. E.g. `<Video type="movie">` → `"Video.movie"` → `Movie`; same under `/status/sessions` → `"Video.movie.session"` → `MovieSession`; `<Stream streamType="2">` → `"Stream.2"` → `AudioStream`; `<Genre>` → `"Genre"`.

Lookup is `utils.getPlexObject(ehash, default)` (`utils.py:150-160`) — walks back down the dotted hash on miss: `"Video.movie.session"` → `"Video.movie"` → `"Video"` → `default` (the raw `elem.tag` passed by `_buildItem`), so an unknown subtype still resolves to the base class.

`PLEXOBJECTS` is populated at import time as a side-effect of decorating each subclass. No init step.

Element-to-class dispatch is `_buildItem` (`base.py:127-144`), composing the same hash from `elem.attrib`. The only contract between server XML and Python classes is the dotted string `TAG.TYPE`.

---

## 5. HTTP layer

The library uses `requests.Session` directly — no adapter customization, no retry policy, no pool tuning. `PlexServer` and `MyPlexAccount` reimplement nearly the same `query()` method.

### `PlexServer.__init__` (`server.py:102-110`)

```python
self._baseurl  = baseurl or CONFIG.get('auth.server_baseurl', 'http://localhost:32400')
self._baseurl  = self._baseurl.rstrip('/')
self._token    = logfilter.add_secret(token or CONFIG.get('auth.server_token'))
self._session  = session or requests.Session()
self._timeout  = timeout or TIMEOUT          # TIMEOUT defaults to 30s (__init__.py:20)
data = self.query(self.key, timeout=self._timeout)
super().__init__(self, data, self.key)
```

No `HTTPAdapter`, no `urllib3.Retry`, no SSL config — bare `requests` defaults.

### `PlexServer._headers` (`server.py:156-162`)

```python
def _headers(self, **kwargs):
    headers = BASE_HEADERS.copy()
    if self._token:
        headers['X-Plex-Token'] = self._token
    headers.update(kwargs)
    return headers
```

### `BASE_HEADERS` (`config.py:53-68`)

Static dict assembled once at module import. Keys: `X-Plex-Platform`, `X-Plex-Platform-Version`, `X-Plex-Provides='controller'`, `X-Plex-Product='PlexAPI'`, `X-Plex-Version`, `X-Plex-Device`, `X-Plex-Device-Name`, `X-Plex-Client-Identifier` (`hex(getnode())` — MAC-derived UUID), `X-Plex-Language='en'`, `X-Plex-Sync-Version='2'`, `X-Plex-Features='external-media'`. All overridable via `~/.config/plexapi/config.ini` or `PLEXAPI_HEADER_*` env vars (`config.py:23-41`).

### `PlexServer.query` (`server.py:738-759`)

```python
def query(self, key, method=None, headers=None, params=None, timeout=None, **kwargs):
    url = self.url(key)
    method   = method or self._session.get
    timeout  = timeout or self._timeout
    log.debug('%s %s', method.__name__.upper(), url)
    headers  = self._headers(**headers or {})
    response = method(url, headers=headers, params=params, timeout=timeout, **kwargs)
    if response.status_code not in (200, 201, 204):
        codename = codes.get(response.status_code)[0]
        errtext  = response.text.replace('\n', ' ')
        message  = f'({response.status_code}) {codename}; {response.url} {errtext}'
        if   response.status_code == 401: raise Unauthorized(message)
        elif response.status_code == 404: raise NotFound(message)
        else:                              raise BadRequest(message)
    return utils.parseXMLString(response.text)
```

Always returns parsed XML or `None`. The HTTP verb is a callable (`self._session.post/put/delete`), not a string.

### `MyPlexAccount.query` (`myplex.py:250-274`)

Same shape, but adds:

```python
elif response.status_code == 422 and "Invalid token" in response.text:
    raise Unauthorized(message)
...
if "verification code" in response.text:
    raise TwoFactorRequired(message)
...
# content negotiation:
if 'application/json' in response.headers.get('Content-Type', ''):
    return response.json()
elif 'text/plain' in response.headers.get('Content-Type', ''):
    return response.text.strip()
return utils.parseXMLString(response.text)
```

Content negotiation only happens at the **plex.tv** layer. The local PMS at `server.py:759` *always* parses XML — it never inspects `Content-Type`. If PMS started returning JSON the server-layer `query` would break.

### Retries, timeouts, ETag

- **Retries**: none. `grep -rn "HTTPAdapter\|max_retries\|Retry"` over `plexapi/` returns nothing. 5xx or network errors propagate from `requests`.
- **Timeout**: single value per request, default 30s (`__init__.py:20`). Connect and read timeouts not separated.
- **ETag**: no HTTP ETag support. The token `etag` in this codebase always means "element tag" (`base.py:399`, `base.py:533`).

---

## 6. Exception hierarchy

`exceptions.py:1-33` defines five classes plus two leaves:

```
PlexApiException                 (base.py:1)
├── BadRequest                   non-2xx fallback; also raised by helpers on invalid args
│   ├── Unauthorized             401, or 422 + "Invalid token" body
│   │   └── TwoFactorRequired    401 with body containing "verification code"
├── NotFound                     404, or `fetchItem` with empty result
├── UnknownType                  `_buildItem` could not resolve TAG.TYPE in PLEXOBJECTS
└── Unsupported                  operation valid in API but not supported for this object
                                 (e.g., `_reload` with no key; non-playable stream URL)
```

HTTP-status → exception mapping is duplicated (`server.py:749-758`, `myplex.py:256-269`). Every other 4xx/5xx becomes `BadRequest` — no `RateLimited`, `ServerError`, or `Timeout`. `requests.exceptions.*` propagate unwrapped.

---

## 7. Utility patterns worth porting

For each utility, one line of Python behavior, one line of suggested Rust shape.

| Helper | Location | Python behavior | Rust equivalent |
|---|---|---|---|
| `cast(func, value)` | `utils.py:163-185` | Coerce string XML attr to `bool`/`int`/`float`; `int`/`float` failures return `nan`. | `fn parse_attr<T: FromStr>(v: Option<&str>) -> Option<T>` returning `Option<T>` — no NaN sentinel; reserve `None` for absent. |
| `lowerFirst(s)` | `utils.py:204-205` | Lowercase first char only. | `inline fn lower_first(s: &str) -> String`; trivial. |
| `joinArgs(args: dict)` | `utils.py:188-201` | Sorted key=urlencoded_value, `?a=b&c=d`. URL-encodes only the value with `safe=''`. | Use `serde_urlencoded` but pre-sort keys (BTreeMap) to match Plex's deterministic ordering; tests compare full URLs. |
| `searchType(libtype)` | `utils.py:239-254` | Map `"movie"` ↔ `1`, accept either direction; raise `NotFound`. | A static `&[(str, u8)]` plus two lookup functions returning `Result<u8, PlexError>`. Or a `#[repr(u8)]` enum with `FromStr`/`Display`. |
| `tagType(tag)` | `utils.py:274-289` | Same idea for tag types (0–500). | Same as above; second enum. |
| `threaded(callback, listargs)` | `utils.py:309-330` | Fan-out N daemon threads, each writes its result into a shared `results` list at a pre-allocated index; poll until done. | Replace with `tokio::task::JoinSet` or `futures::future::join_all`. Drop the spinning poll and the "pre-allocate slots" pattern (it exists only to keep ordering in a non-thread-safe list). |
| `toDatetime(value, format=None)` | `utils.py:393-415` | If `format`, `strptime`; else integer-seconds timestamp; falls back to `epoch + timedelta(seconds=v)` for out-of-bounds values; can attach a configurable `tzinfo`. | `chrono::DateTime<Utc>` (or `Tz`). Two parse paths: `chrono::NaiveDateTime::parse_from_str` vs `from_timestamp`. Configurable TZ via `Tz` field on the client, not a global. |
| `toJson(obj)` | `utils.py:766-777` | Walks `obj.__dict__`, skips private attrs, ISO-formats datetimes. | `serde::Serialize` derive on every model. Free for nothing. |
| `download(url, token, ...)` | `utils.py:487-563` | Streaming `requests.get` with `X-Plex-Token` header, optional `tqdm` bar, optional zip unpack, optional mocked mode for tests. | `reqwest::get` → `bytes_stream()` → `tokio::fs::File`. Optional `indicatif::ProgressBar`. Skip the mocked branch; use trait-based injection for tests. |
| `getMyPlexAccount(opts)` | `utils.py:566-594` | Resolves credentials via CLI args → env/INI → interactive prompt. | A `MyPlexAccountBuilder` with `from_env`, `from_cli_args`, and `prompt` (gated behind a `cli` feature). Keep prompts out of the library core. |
| `registerPlexObject(cls)` | `utils.py:132-147` | Decorator that inserts `cls` into the global `PLEXOBJECTS` dict keyed by `TAG.TYPE[.session/.history]`. | Replace with a static dispatch: parse XML into a tagged enum (`PlexElement::Movie(MovieData)`), use `#[derive(Deserialize)]` + a `serde` tag-and-content discriminator on `tag`+`type`, or a hand-rolled `match` in a `from_element` constructor. Avoid the runtime registry. |
| `cached_property` (re-exported as `cached_data_property`) | `base.py:41-53` | Cache property value on instance; metaclass tracks which props to invalidate on reload. | `OnceCell<T>` field; invalidate by reassigning to a fresh `OnceCell::new()` (or by holding `Option<OnceCell<T>>`). |
| `deprecated(message)` | `utils.py:739-751` | Decorator that emits `DeprecationWarning` + logs. | `#[deprecated(note = "...")]` attribute. Built-in to rustc. |
| `iterXMLBFS(root, tag)` | `utils.py:754-763` | BFS iterator over an ElementTree, optionally filtering by tag. | Trivial helper over `roxmltree::Node::descendants()`; the standard tree types already support BFS via `Children`. |
| `parseXMLString(s)` | `utils.py:836-844` | Try `ElementTree.fromstring`; on `ParseError`, strip illegal Unicode ranges via a precomputed regex (`utils.py:795-833`) and retry. | `roxmltree::Document::parse_with_options`; fall back to a sanitizing pre-pass only if needed. The illegal-Unicode set is worth preserving verbatim (`utils.py:795-803`). |
| `SecretsFilter` | `utils.py:111-129` | Logging filter that replaces known secrets (tokens) in log args with `<hidden>`. | A `tracing` layer that walks `Event` fields; tokens registered at `PlexServer::new` time. |

---

## 8. Pitfalls — Python-isms not to replicate

Concrete patterns that work in Python and would be wrong (or impossible) in idiomatic Rust.

1. **`__getattribute__` magic for lazy reload** (`base.py:634-652`). Every attribute access checks "is this `None` or `[]`? → maybe issue an HTTP request." In Rust, make it explicit: `movie.summary() -> Result<&str, PlexError>` that reloads on miss, or a `Lazy<T>` field. Never hide I/O behind field access.

2. **Mutable global state** — `PLEXOBJECTS = {}` (`utils.py:105`), `DATETIME_TIMEZONE = None` (`utils.py:108`), `USER_DONT_RELOAD_FOR_KEYS = set()` (`base.py:18`), `CONFIG` (`__init__.py:15`), `BASE_HEADERS` (`__init__.py:36`). Move *all* of this onto a `PlexClient` struct. Config is per-instance.

3. **Custom `__setattr__` that silently drops writes** (`base.py:112-116`). `self.title = None` may or may not actually set the attr depending on `_overwriteNone`. Works only because assignments inside `_loadData` happen in a specific order. In Rust, model partial loads with `Option<T>` and merge explicitly.

4. **`weakref.ref` parents** (`base.py:93`). Exists to avoid cycles. Rust ownership models this naturally (`Arc<Server>` for the server back-ref; children don't need to hold parents — drop `_parent` and pass context downward).

5. **The kwarg DSL** (`fetchItems(viewCount__gte=0, Genre__tag="Animation", ...)`, `base.py:227-313`). Impossible to type-check in Rust. Replace with a fluent filter builder: `q.attr("viewCount").gte(0).child("Genre").attr("tag").eq("Animation")`. Or a tiny query AST that compiles to the same `_checkAttrs` checks.

6. **`etag` overloading** (`base.py:399`, `base.py:533`). `"etag"` as a kwarg means "match XML element tag", not the HTTP cache header. In Rust call it `element_tag`.

7. **`cast` returning `float('nan')` on failure** (`utils.py:181-184`). NaN-as-sentinel propagates silently. Use `Option<T>` / `Result<T, ParseError>`.

8. **Daemon threads + shared mutable list + `time.sleep(0.05)` polling** in `threaded` (`utils.py:317-329`). Replace with `tokio::join!` or `JoinSet`; never poll.

9. **`requests.Session` without retry/pool tuning** (`server.py:107`, `myplex.py:134`). Rust port should ship sane defaults: pool size, idle timeout, retry on idempotent verbs, separate connect/read timeouts (`reqwest::ClientBuilder` + `tower::retry::Retry`).

10. **Bare `except:` clauses** — `config.py:40` and `utils.py:235`. Both swallow `KeyboardInterrupt`, `SystemExit`, programmer errors. Never make "tried, returning default" the universal fallback.

11. **Module-level side effects on import** (`__init__.py:13-55`): reads `~/.config/plexapi/config.ini`, mutates `BASE_HEADERS`, sets up rotating file handler. The Rust crate must avoid `lazy_static!` doing I/O — config loading is an explicit `Config::load(...)`.

12. **`__init__` orders `_loadData` before `_buildDetailsKey`** (`base.py:103-105`); the latter depends on `self.key` set by the former. Subclasses overriding either must know the ordering. Rust `new` should build a `PlexObjectData` struct first, then wrap.

13. **Filter walks the XML tree on every `findItems` call** (`base.py:407-411`, `base.py:522-539`). For a 10k-entry `MediaContainer` with a 4-level filter (`Media__Part__file__startswith`), it's O(n × tree-depth). Consider an indexed/streaming filter at the parser level.

14. **`MediaContainer` as both `list` and `PlexObject`** (`base.py:1140-1144`). Rust cannot subclass `Vec`. Use a struct `MediaContainer { items: Vec<T>, metadata: ContainerMeta }` implementing `IntoIterator` / `Index`.

15. **`USER_DONT_RELOAD_FOR_KEYS`** (`base.py:18`) is a mutable module-level set. Two `PlexServer` instances against different servers share it. Move per-instance.

16. **Implicit `cls.TAG` / `cls.TYPE` reflection in `findItems`** (`base.py:398-401`): passing `cls=Movie` auto-injects `etag="Video"` and `type="movie"` filters. In Rust, model as type-based dispatch from §4 — `find_items::<Movie>(...)` using associated constants, no runtime lookup.

---

## Quick reference — file map

```
plexapi/__init__.py    62 LOC  module-level config bootstrap, BASE_HEADERS, logging
plexapi/const.py        8 LOC  version constants (4.18.1)
plexapi/config.py      68 LOC  PlexConfig(ConfigParser) with env-var override + reset_base_headers()
plexapi/exceptions.py  33 LOC  5 exception classes, 2-level hierarchy
plexapi/utils.py      848 LOC  cast/joinArgs/searchType/threaded/toDatetime/download/registerPlexObject/parseXMLString
plexapi/base.py      1234 LOC  PlexObject / PlexPartialObject / Playable / PlexSession / PlexHistory / MediaContainer
plexapi/server.py    (HTTP)   PlexServer.query at server.py:738-759; _headers at server.py:156-162
plexapi/myplex.py    (HTTP)   MyPlexAccount.query at myplex.py:250-274; content-neg JSON/text/XML
```

Everything else in `plexapi/` is a concrete `PlexObject` subclass — `@registerPlexObject`, override `_loadData`, add domain methods that ultimately call `self._server.query(...)` / `self.fetchItems(...)`. The foundation documented here is what the Rust port needs to nail first.
