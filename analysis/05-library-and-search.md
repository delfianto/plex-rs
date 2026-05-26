# 05 — `library.py` and the search subsystem

`plexapi/library.py` is the largest module in python-plexapi at 3386 lines. It owns four conceptual surfaces: (1) the `Library` aggregator that lists sections on a `PlexServer`, (2) the four `LibrarySection` subclasses, (3) the **search / filter / sort DSL** with both Plex server-side filters and PlexAPI client-side `__`-suffix operators, and (4) the filter-discovery data classes (`FilteringType`, `FilteringFilter`, `FilteringSort`, `FilteringField`, `FilteringFieldType`, `FilteringOperator`, `FilterChoice`) that mirror the `/library/sections/<id>/all?includeMeta=1&includeAdvanced=1` payload. Add `Hub`, `ManagedHub`, `LibraryTimeline`, `Common`, `Folder`, `FirstCharacter`, `Path`, `File`, and 25 `LibraryMediaTag` subclasses, and you have what integrators actually touch.

---

## 1. `Library` versus `LibrarySection`

### `Library` (library.py:24-413)

`Library` is the server-level aggregator, a `PlexObject` rooted at `/library` (`Library.key = '/library'`, library.py:36). `PlexServer` constructs exactly one and exposes it as `server.library`. Attributes pulled from `/library`: `identifier` (almost always `'com.plexapp.plugins.library'`), `mediaTagVersion`, `title1`, `title2`. It does not represent a single content folder. Its job: list sections (`sections()`), look one up by title or numeric id, expose cross-section convenience endpoints (`onDeck`, `recentlyAdded`, `hubs`, `all`, `search`, `tags`, `history`), and offer server-wide mutations (`update`, `cancelUpdate`, `refresh`, `cleanBundles`, `optimize`, `emptyTrash`, `deleteMediaPreviews`, `add`).

### Section listing & caching

`Library._loadSections` (library.py:45-63) is a `cached_data_property` that issues `GET /library/sections` once and builds two dictionaries:

```python
libcls = {
    'movie': MovieSection,
    'show':  ShowSection,
    'artist': MusicSection,
    'photo':  PhotoSection,
}
for elem in self._server.query(key):
    section = libcls.get(elem.attrib.get('type'), LibrarySection)(...)
    sectionsByID[section.key] = section
    sectionsByTitle[section.title.lower().strip()].append(section)
```

The mapping is *only* by `type` attribute on each `<Directory>`. Anything not in `{movie, show, artist, photo}` (e.g. unknown future libtypes) falls back to the abstract `LibrarySection`. There is no dedicated `FilmSection` or `VideoSection` — they do not exist in this codebase.

- `Library.sections()` (library.py:75-80) returns `list(_sectionsByID.values())`.
- `Library.sectionByID(sectionID)` (library.py:106-118) is the safe lookup; it expects an **int** key (the same `key` returned in the `<Directory key="1" ...>` element).
- `Library.section(title)` (library.py:82-104) lowers/strips the title, looks it up in `_sectionsByTitle`, warns if the title is ambiguous (because multiple libraries can legally share a title), and returns the **last** entry.

There is **no public `sectionByUUID()`** helper. UUIDs (`32258d7c-3e6c-4ac5-98ad-bad7a3b78c63`) are loaded onto each `LibrarySection` as `self.uuid` (library.py:454) and are used internally for `sync_item.location = f'library://{self.uuid}/directory/{quote_plus(key)}'` (library.py:1620) but not as a lookup index — callers must filter `library.sections()` manually if they need it.

### `LibrarySection` base class (library.py:416-1799)

`LibrarySection` is a `PlexObject` that represents a single library folder. Its attributes from `<Directory>` (library.py:438-454):

| attribute | source | type |
|---|---|---|
| `agent` | `agent=` | str (e.g. `com.plexapp.agents.imdb`, `tv.plex.agents.movie`) |
| `allowSync` | `allowSync=` | bool |
| `art`, `composite`, `thumb` | XML | str (relative image paths) |
| `createdAt`, `updatedAt` | XML | `datetime` via `utils.toDatetime` |
| `filters` | XML | bool |
| `key` | `key=` | **int** (the section id) |
| `language` | XML | str |
| `refreshing` | XML | bool |
| `scanner` | XML | str |
| `title` | XML | str |
| `type` | XML | str — one of `movie`, `show`, `artist`, `photo` |
| `uuid` | XML | str |
| `locations` | `<Location path=…>` children, cached | `list[str]` (library.py:456-458) |
| `totalSize` | computed via `totalViewSize()` | int |
| `totalDuration`, `totalStorage` | `/media/providers?includeStorage=1` | int |

`totalViewSize(libtype=None, includeCollections=True)` (library.py:505-529) issues a peek query with `X-Plex-Container-Start=0&X-Plex-Container-Size=0` and reads the `totalSize` attribute off the container — no items are fetched.

### `__getattribute__` interceptor (library.py:475-487)

`LibrarySection` intercepts attribute lookups to forward Edit-Mixin method calls to its `_edits['items']` when `batchMultiEdit()` is in flight. This is how `MovieSection.editTitle(...)`, `editGenre(...)`, etc. work *as section-level batch operations*: while `_edits` is a dict (set by `batchMultiEdits()`, library.py:1761-1787), any Mixin method called on the section is captured into the batch and finally flushed by `saveMultiEdits()` (library.py:1789-1799).

---

## 2. Section subclasses

All four subclasses share `LibrarySection`'s base; they differ only in `TYPE`, `METADATA_TYPE`, `CONTENT_TYPE`, the EditMixins they pull in, and a few thin per-libtype helpers.

| class | line | TYPE | METADATA_TYPE | CONTENT_TYPE | Mixins | Extras |
|---|---|---|---|---|---|---|
| `MovieSection` | 1802 | `movie` | `movie` | `video` | `MovieEditMixins` | `searchMovies`, `recentlyAddedMovies` |
| `ShowSection` | 1862 | `show` | `episode` | `video` | `Show/Season/EpisodeEditMixins` | `searchShows/Seasons/Episodes`, three `recentlyAdded*` |
| `MusicSection` | 1946 | `artist` | `track` | `audio` | `Artist/Album/TrackEditMixins` | `albums()`, `stations()`, three `search*`, three `recentlyAdded*`, `sonicAdventure()` |
| `PhotoSection` | 2063 | `photo` | `photo` | `photo` | `PhotoalbumEditMixins, PhotoEditMixins` | `searchAlbums/Photos`, `recentlyAddedAlbums`; **disables `collections()`** |

Notes:

- `ShowSection.METADATA_TYPE = 'episode'` — when a show library is *synced*, the leaves are episodes.
- `MusicSection.stations()` (library.py:1963-1965) picks the first hub whose `context == 'hub.music.stations'`.
- `MusicSection.sonicAdventure(start, end)` (library.py:2037-2060) hits `GET /library/sections/<key>/computePath?startID=<id>&endID=<id>` — Plex's "compute a track path between two tracks" feature.
- `PhotoSection.all()` defaults `libtype='photoalbum'`; `PhotoSection.collections()` raises `NotImplementedError`.
- Each subclass overrides `sync()` to wire the right `MediaSettings.create*()` + `Policy.create()`.

The four classes are picked up in `Library._loadSections` (library.py:51-56). Abstract `LibrarySection` is the fallback for unknown `type`. There is no `FilmSection`, `VideoSection`, or `MixedSection`.

---

## 3. The search surface

### Per-section `search()` (library.py:1291-1549)

This is the canonical entry point. Signature:

```python
def search(self, title=None, sort=None, maxresults=None, libtype=None,
           container_start=None, container_size=None, limit=None,
           filters=None, **kwargs)
```

| param | role |
|---|---|
| `title` | free-text title contains (becomes `?title=…`) — *or* list (`title__in`) when promoted to a filter field |
| `sort` | `FilteringSort` instance, `"field:dir"` string, comma list, or list of either — see `_validateSortFields` |
| `maxresults` | client-side limit; controls `fetchItems` pagination |
| `libtype` | filters & casts result objects — `movie`, `show`, `season`, `episode`, `artist`, `album`, `track`, `photoalbum`, `photo`, `collection` |
| `container_start`, `container_size` | server-side pagination (`X-Plex-Container-Start`/`X-Plex-Container-Size`) — applied by `fetchItems` |
| `limit` | server-side `limit=` parameter (different from `maxresults`) |
| `filters` | dict — the advanced `{'and': [...], 'or': [...]}` tree |
| `**kwargs` | mixed: anything whose suffix is in `OPERATORS` (e.g. `summary__icontains`) is a *client-side* filter; everything else (`year=2024`, `genre='Action'`) is validated as a *server-side* Plex filter field |

The mechanics live in `_buildSearchKey` (library.py:1250-1283):

```python
args['includeGuids'] = int(bool(kwargs.pop('includeGuids', True)))
for field, values in list(kwargs.items()):
    if field.split('__')[-1] not in OPERATORS:
        filter_args.append(self._validateFilterField(field, values, libtype))
        del kwargs[field]
if title is not None:
    if isinstance(title, (list, tuple)):
        filter_args.append(self._validateFilterField('title', title, libtype))
    else:
        args['title'] = title
if filters is not None:
    filter_args.extend(self._validateAdvancedSearch(filters, libtype))
if sort is not None:
    args['sort'] = self._validateSortFields(sort, libtype)
if libtype is not None:
    args['type'] = utils.searchType(libtype)
if limit is not None:
    args['limit'] = limit
```

The remaining `**kwargs` (those still ending in a known `__op`) are forwarded to `fetchItems(...)` for client-side post-filtering.

**Example constructed URLs:**

```
# library.search(title='Inception', libtype='movie', sort='year:desc', limit=20)
/library/sections/1/all?includeGuids=1&title=Inception&sort=movie.year:desc&type=1&limit=20

# library.search(genre='Action', year=2024)         (server-side filters)
/library/sections/1/all?includeGuids=1&genre=23&year=2024

# library.search(filters={'and': [{'genre':'Action'}, {'year>>': 1990}]})
/library/sections/1/all?includeGuids=1&push=1&genre=23&and=1&year>=1990&pop=1
```

`_buildSearchKey` returns `(key, kwargs)`; `search()` then calls:

```python
return self.fetchItems(key, container_start=..., container_size=...,
                       maxresults=..., **kwargs)
```

with whatever `__`-suffixed kwargs survived.

### Convenience wrappers

All section subclasses ship narrow wrappers that pin `libtype`:

| method | section | call |
|---|---|---|
| `searchMovies` | `MovieSection` | `search(libtype='movie', **kw)` |
| `searchShows` | `ShowSection` | `search(libtype='show', **kw)` |
| `searchSeasons` | `ShowSection` | `search(libtype='season', **kw)` |
| `searchEpisodes` | `ShowSection` | `search(libtype='episode', **kw)` |
| `searchArtists` | `MusicSection` | `search(libtype='artist', **kw)` |
| `searchAlbums` | `MusicSection`, `PhotoSection` | `search(libtype='album', **kw)` / `'photoalbum'` |
| `searchTracks` | `MusicSection` | `search(libtype='track', **kw)` |
| `searchPhotos` | `PhotoSection` | `search(libtype='photo', **kw)` |

### Hub search

**Per-section** `LibrarySection.hubSearch(query, mediatype=None, limit=None)` (library.py:1285-1289) just delegates to `PlexServer.search(query, mediatype, limit, sectionId=self.key)`. The server-level `PlexServer.search()` (server.py:761-797) issues:

```
/hubs/search?query=<q>&includeCollections=1&includeExternalMedia=1[&limit=<n>][&sectionId=<id>]
```

The response is a list of `<Hub>` containers, each holding heterogeneous matches grouped by media type (movies, actors, directors, episodes, ...). When `mediatype` is supplied, only that hub's `_partialItems` are returned; otherwise items from all hubs are concatenated.

There is **no `searchDiscover()` on `LibrarySection`** — that method lives on `MyPlexAccount.searchDiscover()` (myplex.py:1072) and queries the global Plex Discover service, not the local PMS library. It is out of scope for this module.

### Library-level "easy" search

`Library.search(title, libtype, **kwargs)` (library.py:162-179) is a very thin convenience that targets `/library/all` (across all sections) and only supports a few raw query params — no advanced filters, no `_validateFilterField`. The docstring itself warns: *"Use library section search when you can."*

### Server-wide entry points exposed on `Library`

- `Library.onDeck()` → `/library/onDeck` (library.py:152-155)
- `Library.recentlyAdded()` → `/library/recentlyAdded` (library.py:157-160)
- `Library.hubs(sectionID, identifier, **kw)` → `/hubs[?contentDirectoryID=&identifier=]` (library.py:120-140)
- `Library.all(**kw)` → iterates every section and concatenates `section.all(**kw)` (library.py:142-150)
- `Library.tags(tag)` → `/library/tags?type=<TAGTYPE>` (library.py:405-413)

### Section-level convenience

- `LibrarySection.onDeck()` → `/library/sections/<id>/onDeck` (library.py:803-806)
- `LibrarySection.continueWatching()` → `/hubs/sections/<id>/continueWatching/items` (library.py:808-811)
- `LibrarySection.recentlyAdded(maxresults=50, libtype=None)` → `search(sort='addedAt:desc', maxresults=..., libtype=...)` (library.py:813-822). There is **no separate `unwatched()` helper** on `LibrarySection`; the unwatched filter is invoked as `search(unwatched=True)`. `firstCharacter()` (library.py:824-826) hits `/library/sections/<id>/firstCharacter` and returns a list of `FirstCharacter` objects.
- `LibrarySection.getGuid(guid)` (library.py:643-686): for `plex://` GUIDs, just `self.search(guid=guid)`. For external GUIDs (`imdb://`, `tmdb://`, `tvdb://`), it fetches a dummy item to call `dummy.matches(agent=self.agent, title=guid.replace('://','-'))` and then re-searches with the resolved Plex GUID. Only works on `Plex Movie`/`Plex TV Series` agents.

---

## 4. `fetchItems` filter language — the `__`-suffix DSL

The `__`-operator table lives in `plexapi/base.py:20-38` (it's imported as `OPERATORS` at the top of `library.py`):

```python
OPERATORS = {
    'exact':     lambda v, q: v == q,
    'iexact':    lambda v, q: v.lower() == q.lower(),
    'contains':  lambda v, q: q in v,
    'icontains': lambda v, q: q.lower() in v.lower(),
    'ne':        lambda v, q: v != q,
    'in':        lambda v, q: v in q,
    'gt':        lambda v, q: v > q,
    'gte':       lambda v, q: v >= q,
    'lt':        lambda v, q: v < q,
    'lte':       lambda v, q: v <= q,
    'startswith':lambda v, q: v.startswith(q),
    'istartswith':lambda v, q: v.lower().startswith(q.lower()),
    'endswith':  lambda v, q: v.endswith(q),
    'iendswith': lambda v, q: v.lower().endswith(q.lower()),
    'exists':    lambda v, q: v is not None if q else v is None,
    'regex':     lambda v, q: bool(re.search(q, v)),
    'iregex':    lambda v, q: bool(re.search(q, v, flags=re.IGNORECASE)),
}
```

These operators run **client-side** in `PlexObject._checkAttrs` (base.py:496-512) after `PlexObject.fetchItems` has fetched a page of results. They are *not* sent to the Plex server. The detection logic is the literal suffix check inside `_buildSearchKey` (library.py:1258-1261):

```python
for field, values in list(kwargs.items()):
    if field.split('__')[-1] not in OPERATORS:
        filter_args.append(self._validateFilterField(field, values, libtype))
        del kwargs[field]
```

So any `kwarg` whose final `__token` is one of the 17 suffixes above is *left in `kwargs`* and forwarded to `fetchItems`; everything else is shipped to Plex as an `?attr=val` query string.

`fetchItems` (base.py:227) recurses XML children: `media__videoCodec__exact='h265'` walks into the `<Media>` child, then `<Video?> videoCodec` attribute, then exact compares. The recursion is implemented in `_getAttrValue` (base.py:522-539), and value casting (so `query=8` vs `value='8.0'` interoperate) is in `_castAttrValue` (base.py:541-552).

### Operator dispatch (`_getAttrOperator`, base.py:514-520)

```python
def _getAttrOperator(self, attr):
    for op, operator in OPERATORS.items():
        if attr.endswith(f'__{op}'):
            attr = attr.rsplit('__', 1)[0]
            return attr, op, operator
    return attr, 'exact', OPERATORS['exact']
```

Important consequence: nested traversal uses `__` *inside* the path (`media__videoCodec`), and the operator suffix is always the **last** `__` segment. `media__videoCodec__exact` means: descend to `<Media><Part videoCodec="…"/></Media>` and `==` compare. `media__videoCodec` alone defaults to `exact`.

### Special exception: `boolean` query

If `query` is `bool`, `_castAttrValue` casts the XML value via `bool(int(value))` — so `viewed__exact=True` works because Plex stores `"1"`/`"0"`.

### Examples

```python
library.search(summary__icontains="Christmas")
library.search(duration__gt=7200000)
library.search(media__videoCodec__exact="h265")
library.search(genre="holiday", viewCount__gte=3)   # genre → server, viewCount__gte → client
```

The last example is the mixed case: `genre="holiday"` is converted to `?genre=<key>` server-side, then the returned list is post-filtered client-side by `viewCount >= 3`.

---

## 5. The `search(filters=...)` advanced dictionary

### Flat dict (single `and` group)

```python
library.search(filters={"genre": "Action", "year": 2024})
```

Each `(field, value)` pair feeds `_validateFilterField` (library.py:1062-1094) which:

1. Regex-splits `(libtype.)?field(operator)?` with `r'(?:([a-zA-Z]*)\.)?([a-zA-Z]+)([!<>=&]*)'` (library.py:1066). The `libtype.` prefix is optional, e.g. `episode.title`, `show.collection`.
2. Resolves the field in `listFields(libtype)`, falling back to other filter types if not found (library.py:1075-1083). This is what enables `library.search(unwatched=True)` on a `ShowSection` to fall through `show.unwatched` (does not exist) to `episode.unwatched`.
3. Validates the operator with `_validateFieldOperator` (library.py:1096-1117).
4. Validates the value with `_validateFieldValue` (library.py:1119-1146) — bools become `0/1`, dates become Unix epochs or `-30d` relative strings, tags get resolved to numeric `key` IDs via `_validateFieldValueTag` (library.py:1159-1172).
5. Returns a URL-encoded slice. For `&=` (AND-of-tags) operator the values are emitted as repeated params; otherwise comma-joined.

### Nested `and`/`or` tree

```python
filters = {
    'and': [
        {'or': [{'title': 'elephant'}, {'title': 'bunny'}]},
        {'year>>': 1990},
        {'unwatched': True}
    ]
}
library.search(filters=filters)
```

`_validateAdvancedSearch` (library.py:1220-1248):

```python
for field, values in filters.items():
    if field.lower() in {'and', 'or'}:
        if len(filters.items()) > 1:
            raise BadRequest('Multiple keys in the same dictionary with and/or is not allowed.')
        validatedFilters.append('push=1')
        for value in values:
            validatedFilters.extend(self._validateAdvancedSearch(value, libtype))
            validatedFilters.append(f'{field.lower()}=1')
        del validatedFilters[-1]
        validatedFilters.append('pop=1')
    else:
        validatedFilters.append(self._validateFilterField(field, values, libtype))
```

This means the URL encoding for `and`/`or` is a flat sequence of `push=1 … <op>=1 … pop=1` tokens. The example above produces approximately:

```
/library/sections/1/all?
  includeGuids=1&
  push=1&
    push=1&title=elephant&or=1&title=bunny&pop=1&and=1&
    year>=1990&and=1&
    show.unwatched=1&
  pop=1
```

A single push starts a sub-expression; each child is delimited by the literal `and=1` or `or=1` operator marker; `pop=1` closes the group. The very last `<op>=1` before the closing `pop=1` is trimmed (`del validatedFilters[-1]`) — Plex's parser requires that the operator only appears *between* operands. Round-tripping back into a dict is what `SmartFilterMixin._parseFilterGroups` does (see §7).

### Composition with `sort`, `libtype`, `includeAdvanced`

`filters=` is merged into the same query string as `sort`, `libtype`, `limit`, `title` (library.py:1276-1283):

```python
joined_args = utils.joinArgs(args).lstrip('?')
joined_filter_args = '&'.join(filter_args) if filter_args else ''
params = '&'.join([joined_args, joined_filter_args]).strip('&')
key = f'/library/sections/{self.key}/all?{params}'
```

`includeAdvanced=1` is **not** set here for normal searches; it appears only when *discovering* filters in `_loadFilters` (library.py:881). The server has its own internal handling for the `push/pop` sequences regardless.

---

## 6. Filter discovery

Discovery is rooted in the cached `LibrarySection._loadFilters` (library.py:876-898):

```python
_key = ('/library/sections/{key}/{filter}?includeMeta=1&includeAdvanced=1'
        '&X-Plex-Container-Start=0&X-Plex-Container-Size=0')

key = _key.format(key=self.key, filter='all')
data = self._server.query(key)
filterTypes = self.findItems(data, FilteringType, rtag='Meta')
fieldTypes  = self.findItems(data, FilteringFieldType, rtag='Meta')

if self.TYPE != 'photo':
    key = _key.format(key=self.key, filter='collections')
    data = self._server.query(key)
    filterTypes.extend(self.findItems(data, FilteringType, rtag='Meta'))

# manually inject "guid" field with only the "=" operator
guidFieldType = '<FieldType type="guid"><Operator key="=" title="is"/></FieldType>'
fieldTypes.append(self._manuallyLoadXML(guidFieldType, FilteringFieldType))

return filterTypes, fieldTypes
```

Two endpoints are touched: `/library/sections/<id>/all?includeMeta=1&includeAdvanced=1&…` and `/library/sections/<id>/collections?…`. The `<Meta>` element of the response contains nested `<Type>` and `<FieldType>` definitions. The guid `FieldType` is *manually synthesized*: Plex doesn't expose it but PlexAPI needs it so consumers can filter by GUID with the `=` operator only.

### Public discovery API

| method | endpoint touched | returns |
|---|---|---|
| `filterTypes()` (library.py:900-902) | (cached) | `list[FilteringType]` |
| `getFilterType(libtype=None)` (library.py:904-920) | (cached) | one `FilteringType` |
| `fieldTypes()` (library.py:922-924) | (cached) | `list[FilteringFieldType]` |
| `getFieldType(fieldType)` (library.py:926-941) | (cached) | one `FilteringFieldType` |
| `listFilters(libtype=None)` (library.py:943-960) | (cached) | `list[FilteringFilter]` from `filterType.filters` |
| `listSorts(libtype=None)` (library.py:962-979) | (cached) | `list[FilteringSort]` from `filterType.sorts` |
| `listFields(libtype=None)` (library.py:981-998) | (cached) | `list[FilteringField]` from `filterType.fields` |
| `listOperators(fieldType)` (library.py:1000-1019) | (cached) | `list[FilteringOperator]` per data type |
| `listFilterChoices(field, libtype=None)` (library.py:1021-1060) | `<filter.key>` (e.g. `/library/sections/1/genre`) | `list[FilterChoice]` |

A typical chain:

```
section.listFilters()             -> [genre, country, year, decade, contentRating, …]
section.listSorts()               -> [titleSort, addedAt, originallyAvailableAt, rating, …]
section.listFields()              -> [Field(key='movie.title',     type='string'),
                                      Field(key='movie.year',      type='integer'),
                                      Field(key='movie.genre',     type='tag'), …]
section.listOperators('integer')  -> [is, is not, is greater than, is less than]
section.listFilterChoices('genre')-> [FilterChoice(key='23', title='Action'), …]
```

### Manual augmentation (`FilteringType._manualFilters`, `_manualSorts`, `_manualFields`)

`FilteringType` (library.py:2665-2867) injects extra fields/filters/sorts the Plex server does not advertise but which work in practice. For example, every libtype gets a `guid`, `id`, `index`, `lastRatedAt`, `updatedAt`, `group`, and `having` field appended. Movies additionally get `audienceRating`, `rating`, `viewOffset`. Episodes get `audienceRating`, `duration`, `rating`, `viewOffset`, `label`. The `group` and `having` fields are special — they are **not** prefixed with the libtype because they map to literal SQL group/having clauses on the server side (library.py:2856-2857).

The `_manualSorts` injection adds `guid:asc`, `id:asc`, `index:asc`, `summary:asc`, `tagline:asc`, `updatedAt:asc` to every libtype, plus libtype-specific bonuses (`titleSort` for seasons, `absoluteIndex` for tracks, `viewUpdatedAt` for photos, `addedAt` for collections).

`_manualFilters` only adds `label` for `season`/`episode`/`artist`/`track`/`collection` types — Plex normally exposes label as a filter only on the top-level libtype, but consumers want it everywhere.

---

## 7. Smart filters / smart collections

`plexapi/mixins/smart_filter.py` (98 lines) hosts `SmartFilterMixin`. It is consumed by `Collection` and `Playlist` so they can deserialize a smart-filter URL back into the same `filters={...}` dictionary you would pass to `search()`.

The full input is a URL like:

```
?type=1&sort=addedAt%3Adesc&push=1&genre=23&and=1&year%3E%3E=1990&pop=1
```

`SmartFilterMixin._parseFilters(content)` (smart_filter.py:86-98):

1. URL-decodes the query string.
2. Walks each `(key, value)` pair. If `value` starts with `=` (the `==` case where Plex flips the equals into the value), moves it back onto the key so `field` becomes `field=`.
3. Feeds the pairs into `_parseQueryFeed` (smart_filter.py:55-84) which extracts the special keys:
   - `type` → `libtype` (via `utils.reverseSearchType`)
   - `sort` → split by `,`
   - `includeGuids`, `limit` → int
   - `group`, `having` → kept as raw strings
   - everything else → handed to `_parseFilterGroups`

4. `_parseFilterGroups` (smart_filter.py:11-53) is the recursive `push/pop` parser. It maintains a stack and a single logical operator per group:

```python
while feed:
    key, value = feed.popleft()
    if key == "push":
        currentFiltersStack.append(self._parseFilterGroups(feed, returnOn))
    elif key in returnOn:    # "pop" or one of the reserved top-level keys
        if not key == "pop":
            feed.appendleft((key, value))
        break
    elif key in ["and", "or"]:
        if operatorForStack and operatorForStack != key:
            raise ValueError(...)
        operatorForStack = key
    else:
        currentFiltersStack.append({key: value})
```

The mixing of `and` and `or` inside the same group throws `ValueError` — Plex's parser also rejects this, so the symmetry is intentional. The final return shape exactly matches what `_validateAdvancedSearch` can re-emit, so a smart collection's URL can be round-tripped: parse → edit → re-emit.

**Why this matters for integrators:** `Collection.filters` returns the parsed `{filters: {...}, sort: [...], libtype: ...}` dict directly. To update a smart collection, you mutate this dict and pass it back to `Collection.updateFilters(...)` (defined in `plexapi/collection.py`), which round-trips through `_buildSearchKey`. The smart filter URL stored on the server side is a `library://<uuid>/directory/<urlencoded>` path; see library.py:1620.

---

## 8. Hub objects

### `Hub` (library.py:2193-2265)

A `Hub` is one row of `/hubs` or `/hubs/sections/<id>` output. Attributes (library.py:2213-2224):

| attr | meaning |
|---|---|
| `context` | e.g. `'hub.movie.recentlyAdded'`, `'hub.music.stations'`, `'home.continue'`, `'home.ondeck'` |
| `hubKey` | API URL for the full item list (when `more=True`) |
| `hubIdentifier` | stable identifier used to address this hub |
| `key` | sub-URL of this hub |
| `more` | bool — true if items are paginated; calling `.items()` fetches them |
| `random` | bool — items returned in randomized order |
| `size` | item count |
| `style` | UI hint |
| `title` | display title |
| `type` | media libtype |

`Hub.items()` (library.py:2246-2248) returns `self._items`, which is a `cached_data_property` (library.py:2234-2244) that fetches `self.key` if `more=True`, otherwise returns the inline `_partialItems` parsed at construction time.

`Hub.section()` (library.py:2255-2258) looks up the parent `LibrarySection` by `librarySectionID` — this attribute is dynamically populated when the hub is parsed (it doesn't appear in `_loadData` but is set by `_findAndLoadElem` from the parent context).

### Where hubs come from

- `Library.hubs(sectionID=None, identifier=None, **kw)` → `/hubs[?contentDirectoryID=<id>&identifier=<i>]` (library.py:120-140)
- `LibrarySection.hubs()` → `/hubs/sections/<key>?includeStations=1` (library.py:713-717)
- `LibrarySection.managedHubs()` → `/hubs/sections/<key>/manage` (library.py:701-705)
- `LibrarySection.continueWatching()` → `/hubs/sections/<key>/continueWatching/items` (library.py:808-811)
- `PlexServer.search()` returns a flat list of items but underlying URL is `/hubs/search` (server.py:790)

### `ManagedHub` (library.py:3008-3136)

A `ManagedHub` represents a *promoted* recommendation row on a section. Attributes include `deletable`, `homeVisibility`, `promotedToOwnHome`, `promotedToRecommended`, `promotedToSharedHome`, `recommendationsVisibility`. Mutations:

| method | endpoint |
|---|---|
| `move(after=None)` | `PUT /hubs/sections/<id>/manage/<identifier>/move[?after=<id>]` |
| `remove()` | `DELETE /hubs/sections/<id>/manage/<identifier>` |
| `updateVisibility(...)` | `POST /hubs/sections/<id>/manage` (new promotion) or `PUT /hubs/sections/<id>/manage/<identifier>` (existing) |
| `promote/demote{Recommended, Home, Shared}` | thin aliases over `updateVisibility` |

`resetManagedHubs()` on the section (library.py:707-711) is `DELETE /hubs/sections/<id>/manage`.

---

## 9. Refresh / scan / empty trash / analyze

### Section-level

| method | endpoint | verb | purpose |
|---|---|---|---|
| `update(path=None)` (library.py:842-852) | `/library/sections/<id>/refresh[?path=…]` | GET | trigger an incremental scan; optional sub-path to scan only one folder |
| `cancelUpdate()` (library.py:854-858) | `/library/sections/<id>/refresh` | DELETE | abort an in-progress scan |
| `refresh()` (library.py:860-866) | `/library/sections/<id>/refresh?force=1` | GET | full re-pull from metadata agents (locked fields are preserved) |
| `analyze()` (library.py:828-834) | `/library/sections/<id>/analyze` | PUT | re-analyze (intro detection, bitrate inspection, chapter thumbs, etc.) |
| `emptyTrash()` (library.py:836-840) | `/library/sections/<id>/emptyTrash` | PUT | purge soft-deleted items |
| `deleteMediaPreviews()` (library.py:868-874) | `/library/sections/<id>/indexes` | DELETE | remove preview thumbnails (expensive to regenerate) |
| `delete()` (library.py:531-541) | `/library/sections/<id>` | DELETE | drop the entire library |
| `timeline()` (library.py:797-801) | `/library/sections/<id>/timeline` | GET | latest activity snapshot |

### Library-wide

| method | endpoint | verb |
|---|---|---|
| `Library.update()` | `/library/sections/all/refresh` | GET |
| `Library.cancelUpdate()` | `/library/sections/all/refresh` | DELETE |
| `Library.refresh()` | `/library/sections/all/refresh?force=1` | GET |
| `Library.cleanBundles()` | `/library/clean/bundles?async=1` | PUT |
| `Library.optimize()` | `/library/optimize?async=1` | PUT |
| `Library.emptyTrash()` | iterates `section.emptyTrash()` for each section | — |
| `Library.deleteMediaPreviews()` | iterates `section.deleteMediaPreviews()` | — |

---

## 10. Edit operations on sections

### `LibrarySection.edit(agent=None, **kwargs)` (library.py:550-573)

```python
part = f'/library/sections/{self.key}?agent={agent}&{urlencode(params, doseq=True)}'
self._server.query(part, method=self._server._session.put)
```

Anything passed as `kwargs` becomes a query parameter. `location` is special — it can be a single path or a list, and each path is validated against `_server.isBrowsable(path)` first (library.py:560-568). For Plex advanced settings, the `kwargs` keys are encoded as `prefs[<settingID>]=<value>` — but `edit()` doesn't do that conversion itself; you should use `editAdvanced` (library.py:730-752) which queries `/library/sections/<id>/prefs` to discover all valid settings (`Setting` objects), validates that `value in enums`, and then calls `edit(prefs[<id>]=<value>, ...)`.

`defaultAdvanced()` (library.py:754-764) resets every advanced setting to its `default` value.

### `addLocations` / `removeLocations` (library.py:575-622)

Both read `self.locations`, mutate the list, and call `edit(location=<list>)`. `removeLocations` refuses to leave a section with zero paths.

### `lockAllField` / `unlockAllField` (library.py:766-795)

```python
args = {'type': utils.searchType(libtype), f'{field}.locked': int(locked)}
self._server.query(f'/library/sections/{self.key}/all{utils.joinArgs(args)}', method=PUT)
```

Locks every item in the library against future agent edits to that field. Useful when scripting a global override.

### Multi-edit

- `multiEdit(items, **kwargs)` (library.py:1748-1759) — single API call PUT to `/library/sections/<id>/all?id=<r1>,<r2>,…&type=<t>&<field>=<value>…`.
- `batchMultiEdits(items)` / `saveMultiEdits()` (library.py:1761-1799) — defer edits in `self._edits` and flush at the end. The `__getattribute__` interceptor (library.py:475-487) is what routes per-item Mixin methods (`editTitle`, `addCollection`, `removeGenre`, etc.) into this batch.

### Adding a new library

`Library.add(name, type, agent, scanner, location, language='en-US', **kwargs)` (library.py:231-392) is a *very* long docstring listing every `prefs[<id>]` for every (agent × type) combination. Implementation:

```python
part = (f'/library/sections?name={quote_plus(name)}&type={type}&agent={agent}'
        f'&scanner={quote_plus(scanner)}&language={language}&{urlencode(locations, doseq=True)}')
if kwargs:
    prefs_params = {f'prefs[{k}]': v for k, v in kwargs.items()}
    part += f'&{urlencode(prefs_params)}'
data = self._server.query(part, method=POST)
self._invalidateCachedProperties()
```

Note that section creation invalidates the cache so the next `sections()` call rebuilds from `/library/sections`.

---

## 11. `recentlyAdded`, `onDeck`, `continueWatching`, `unwatched`, `getGuid`

These are the entry points integrators reach for most often.

| call | endpoint | scope |
|---|---|---|
| `server.library.recentlyAdded()` | `/library/recentlyAdded` | all sections |
| `server.library.onDeck()` | `/library/onDeck` | all sections |
| `section.recentlyAdded(maxresults=50, libtype=None)` | `/library/sections/<id>/all?sort=addedAt:desc&type=<t>` | one section |
| `MovieSection.recentlyAddedMovies(maxresults=50)` | same, `libtype='movie'` | one section |
| `ShowSection.recentlyAddedShows / Seasons / Episodes` | same, libtype pinned | one section |
| `MusicSection.recentlyAddedArtists / Albums / Tracks` | same | one section |
| `PhotoSection.recentlyAddedAlbums` | `/library/sections/<id>/all?sort=addedAt:desc` (libtype=None) | one section |
| `section.onDeck()` | `/library/sections/<id>/onDeck` | one section |
| `section.continueWatching()` | `/hubs/sections/<id>/continueWatching/items` | one section |
| `section.search(unwatched=True)` | `/library/sections/<id>/all?…&unwatched=1` (with fallback to `episode.unwatched` for show libs) | one section |
| `section.getGuid('plex://show/…')` | `/library/sections/<id>/all?…&guid=plex%3A%2F%2F…` | one section |
| `section.getGuid('imdb://tt…')` | uses `Video.matches()` to map agent GUID → Plex GUID, then re-search | one section |

There is no `section.unwatched()` helper — it is purely a filter field. The docstring notes that for `ShowSection`, `unwatched=True` *transparently* falls back to `episode.unwatched=1` because `show.unwatched` doesn't exist as a server-side filter; `_validateFilterField` walks `filterTypes()` in reverse to find the most specific libtype that exposes the field (library.py:1075-1083).

---

## 12. Small data classes for filter capability

### `Common` (library.py:3245-3386)

Result of `LibrarySection.common(items)` — describes which metadata fields are *identical* across a batch. Endpoint: `GET /library/sections/<id>/common?id=<r1>,<r2>,…&type=<t>`. Attributes: `contentRating, editionTitle, grandparentRatingKey/Title, guid, index, key, mixedFields (list[str] — fields that differ), originallyAvailableAt, parentRatingKey/Title, ratingKey, studio, summary, tagline, title, titleSort, type, year`, plus `cached_data_property` collections of `media.Collection/Country/Director/Field/Genre/Guid/Label/Mood/Producer/Rating/Role/Style/Tag/Writer`.

`Common.commonType` parses `type=` from `_initpath` via `utils.reverseSearchType`. `Common.ratingKeys` parses `id=`. `Common.items()` re-fetches via `server.fetchItems(self.ratingKeys)`. Drives the editor UI's "edit all selected" dialog: identical fields shown once, `mixedFields` shown indeterminate.

### `FilteringOperator` (library.py:2962-2975)

```
TAG = 'Operator'
key:    e.g. '=', '!=', '<<', '>>', '<', '>', '&='
title:  e.g. 'is', 'is not', 'is less than', 'is greater than',
              'begins with', 'ends with', 'is and'
```

Each `FilteringFieldType` carries a list of these.

### `FilterChoice` (library.py:2978-3005)

```
TAG = 'Directory'
fastKey: e.g. '/library/sections/1/all?genre=23'
key:     e.g. '23'  (the value to put on the URL)
thumb:   image URL (genre/country/studio thumbnails)
title:   display name e.g. 'Action'
type:    'genre' / 'contentRating' / ...
```

`FilterChoice.items()` (library.py:3002-3005) follows `fastKey` to get the items with that choice applied.

### `FilteringField` (library.py:2919-2936)

```
TAG = 'Field'
key:    e.g. 'movie.year', 'episode.unwatched', 'guid'
title:  e.g. 'Year', 'Unplayed'
type:   the FilteringFieldType key — 'string', 'integer', 'boolean',
        'date', 'tag', 'subtitleLanguage', 'audioLanguage',
        'resolution', 'guid'
subType:e.g. 'decade', 'rating' (optional finer classification)
```

### `FilteringSort` (library.py:2891-2916)

```
TAG = 'Sort'
active:           bool — currently selected
activeDirection:  current sort direction
default:          server's default direction
defaultDirection: e.g. 'asc' / 'desc' (used when sort dir is omitted)
descKey:          URL key for descending variant
firstCharacterKey:URL for the /firstCharacter index (sort-specific)
key:              field key (e.g. 'titleSort', 'addedAt', 'year')
title:            display name
```

### `FilteringFilter` (library.py:2869-2888)

```
TAG = 'Filter'
filter:    field token (e.g. 'genre', 'year', 'unwatched')
filterType:e.g. 'tag', 'integer', 'boolean'
key:       /library/sections/<id>/<filter>?type=<t>
title:     display name (e.g. 'Genre')
type:      always 'filter'
```

### `FilteringFieldType` (library.py:2939-2959)

```
TAG = 'FieldType'
type:       e.g. 'string', 'integer', 'tag', ...
operators:  cached list[FilteringOperator]
```

### `FilteringType` (library.py:2665-2867)

The top-level grouping by libtype.

```
TAG = 'Type'
active:  bool — currently selected libtype
key:     e.g. /library/sections/1/all?type=1
title:   e.g. 'Movies', 'Episodes'
type:    e.g. 'movie', 'show', 'season', 'episode', 'artist', 'album',
              'track', 'photoalbum', 'photo', 'collection'

fields:  list[FilteringField]  (= server XML + _manualFields())
filters: list[FilteringFilter] (= server XML + _manualFilters())
sorts:   list[FilteringSort]   (= server XML + _manualSorts())
```

`active`, `key`, `title`, `type` come directly from the `<Type>` XML; the three lists are *augmented* via the manual injections described in §6.

### `LibraryMediaTag` (library.py:2268-2320) and its 25 subclasses

The base wraps `<Directory>` rows returned by `/library/tags?type=<TAGTYPE>`. Subclasses (library.py:2323-2662) only set `TAGTYPE`: `Tag=0, Genre=1, Collection=2, Director=4, Writer=5, Role=6, Producer=7, Country=8, Chapter=9, Review=10, Label=11, Marker=12, MediaProcessingTarget=42 (TAG='Tag'), Make=200, Model=201, Aperture=202, Exposure=203, ISO=204, Lens=205, Device=206, Autotag=207, Mood=300, Style=301, Format=302, Similar=305, Concert=306, Poster=312, Art=313, Guid=314, RatingImage=316, Theme=317, Studio=318, Network=319, Place=400`.

Base attributes: `count, filter, id, key, librarySectionID/Key/Title/Type, reason, reasonID, reasonTitle, score, type, tag, tagKey, tagType, tagValue, thumb`. `.items()` follows `self.key` to fetch tagged media. `Library.tags(tag)` (library.py:405-413) maps a name (`'genre'`, `'director'`) to TAGTYPE via `utils.tagType(tag)` and queries `/library/tags?type=<n>`.

### `Folder` (library.py:3139-3175)

`section.folders()` returns `/library/sections/<id>/folder` (a `<Directory>` per top-level folder). `Folder.subfolders()` recurses; once `key` starts with `/library/metadata` we have hit a media item and switch off the `Folder` cast.

### `FirstCharacter` (library.py:3178-3191)

`section.firstCharacter()` returns the alphabet index (`#`, `A`, `B`, …) with per-letter `size` and a `key` pointing at all items beginning with that character. Used for fast jump-scroll in the UI.

### `Path` / `File` (library.py:3194-3242)

These represent filesystem entries returned by `_server.browse()` and `_server.walk()`. `Path.browse(includeFiles=True)` and `Path.walk()` are pass-through aliases to the server methods. Both classes carry `key`, `path`, `title`; `Path` additionally has `home` (is it the home directory?) and `network` (is it a network mount?).

### `LibraryTimeline` (library.py:2138-2173)

Returned from `section.timeline()`. Carries `latestEntryTime` (epoch), `updateQueueSize` (items pending scan), and a handful of UI hints. Most useful as a polling oracle: when `updateQueueSize` drops to zero and `latestEntryTime` advances, you know a scan finished.

### `Location` (library.py:2177-2190)

Wraps a `<Location id="…" path="…"/>` child of `<Directory>` for a `LibrarySection`. Two attributes: `id` and `path`. `LibrarySection._locations()` returns them, but `LibrarySection.locations` (cached_data_property at library.py:456-458) flattens to a `list[str]` for convenience.

---

## Appendix A — full operator-suffix table

These are the suffixes recognised by `OPERATORS` in `plexapi/base.py:20-38`. They run client-side via `_checkAttrs` (base.py:496-512). If a kwarg's final `__token` is one of these, the kwarg is **stripped from the server query** and forwarded to `fetchItems` for in-process filtering.

| Python suffix | semantics | example kwarg | client expression |
|---|---|---|---|
| `__exact` *(default)* | `==` | `year__exact=1999` / `year=1999` | `v == q` |
| `__iexact` | case-insensitive `==` | `title__iexact='foo'` | `v.lower() == q.lower()` |
| `__contains` | substring | `summary__contains='hero'` | `q in v` |
| `__icontains` | case-insensitive substring | `summary__icontains='christmas'` | `q.lower() in v.lower()` |
| `__ne` | not equal | `year__ne=2020` | `v != q` |
| `__in` | membership | `year__in=(2020,2021,2022)` | `v in q` |
| `__gt` | `>` | `duration__gt=7_200_000` | `v > q` |
| `__gte` | `>=` | `viewCount__gte=3` | `v >= q` |
| `__lt` | `<` | `audienceRating__lt=5.0` | `v < q` |
| `__lte` | `<=` | `audienceRating__lte=6.0` | `v <= q` |
| `__startswith` | prefix | `audienceRatingImage__startswith='rottentomatoes://'` | `v.startswith(q)` |
| `__istartswith` | case-insensitive prefix | `title__istartswith='the '` | `v.lower().startswith(q.lower())` |
| `__endswith` | suffix | `title__endswith='Part II'` | `v.endswith(q)` |
| `__iendswith` | case-insensitive suffix | `title__iendswith='part ii'` | `v.lower().endswith(q.lower())` |
| `__exists` | attr presence test | `trailerURL__exists=True` | `v is not None if q else v is None` |
| `__regex` | regex match | `title__regex=r'S\d+E\d+'` | `bool(re.search(q, v))` |
| `__iregex` | case-insensitive regex | `summary__iregex=r'christmas'` | `bool(re.search(q, v, re.IGNORECASE))` |

For comparison, the **server-side** Plex operators (recognised by `_validateFilterField` library.py:1066-1094) are appended to the *field name* rather than as a `__suffix`:

| Plex operator | semantics | applicable types | URL form |
|---|---|---|---|
| *(none)* | depends on type: `is` for tag/int/bool/resolution/guid, `contains` for string | all | `genre=23`, `year=2024`, `title=hero` |
| `!` | `is not` / `does not contain` | tag, int, bool, str | `genre!=23`, `title!=hero` |
| `=` | exact string match | str only | `title==hero` (Plex URL: `title==hero` becomes `title=` + `=hero` after the `==` flip) |
| `!=` | exact string ≠ | str only | `title!==hero` |
| `<` | `begins with` | str | `title<=marvel` |
| `>` | `ends with` | str | `title>=ave` |
| `<<` | `is before` / `is less than` | datetime, int | `addedAt<<=2021-01-01`, `userRating<<=8` |
| `>>` | `is after` / `is greater than` | datetime, int | `addedAt>>=2021-01-01`, `userRating>>=8` |
| `&` / `&=` | AND-of-tags (default is OR) | tag | `genre&=horror&genre=thriller` (emitted as repeated params) |

The two namespaces are independent: you can mix them in a single call (`library.search(genre='Action', summary__icontains='heist')`).

---

## Appendix B — full list of PMS endpoints touched by `library.py`

Grouped by category. `<id>` = section key; `<r>` = ratingKey; `<g>` = guid; `<i>` = managed hub identifier.

### Listing & section metadata

| method | verb | path | source |
|---|---|---|---|
| List sections | GET | `/library/sections` | library.py:48 |
| Section storage/duration | GET | `/media/providers?includeStorage=1` | library.py:492 |
| Section folders | GET | `/library/sections/<id>/folder` | library.py:698 |
| Section settings (`prefs`) | GET | `/library/sections/<id>/prefs` | library.py:726 |
| Section timeline | GET | `/library/sections/<id>/timeline` | library.py:799 |
| First-character index | GET | `/library/sections/<id>/firstCharacter` | library.py:825 |
| Filter discovery (items) | GET | `/library/sections/<id>/all?includeMeta=1&includeAdvanced=1&X-Plex-Container-Start=0&X-Plex-Container-Size=0` | library.py:881-884 |
| Filter discovery (collections) | GET | `/library/sections/<id>/collections?includeMeta=1&includeAdvanced=1&X-Plex-Container-Start=0&X-Plex-Container-Size=0` | library.py:890 |
| Filter-choice values | GET | `<FilteringFilter.key>` e.g. `/library/sections/<id>/genre` | library.py:1059 |
| Library-wide tags | GET | `/library/tags?type=<TAGTYPE>` | library.py:412 |
| Library-wide all | GET | `/library/all?…` | library.py:178 |
| `Common` element for a batch | GET | `/library/sections/<id>/common?id=<r1>,<r2>,…&type=<t>` | library.py:1731 |

### Search & browse

| method | verb | path | source |
|---|---|---|---|
| Section search (the main one) | GET | `/library/sections/<id>/all?<params>` | library.py:1279 |
| Section albums (music) | GET | `/library/sections/<id>/albums` | library.py:1960 |
| Section onDeck | GET | `/library/sections/<id>/onDeck` | library.py:805 |
| Library onDeck | GET | `/library/onDeck` | library.py:154 |
| Library recentlyAdded | GET | `/library/recentlyAdded` | library.py:159 |
| Total view size peek | GET | `/library/sections/<id>/all?X-Plex-Container-Start=0&X-Plex-Container-Size=0[&type=<t>]` | library.py:527 |
| Music sonic adventure | GET | `/library/sections/<id>/computePath?startID=<r>&endID=<r>` | library.py:2059 |

### Hubs

| method | verb | path | source |
|---|---|---|---|
| Library-wide hubs | GET | `/hubs[?contentDirectoryID=<id>][&identifier=<i>]` | library.py:139 |
| Section hubs | GET | `/hubs/sections/<id>?includeStations=1` | library.py:716 |
| Continue watching (section) | GET | `/hubs/sections/<id>/continueWatching/items` | library.py:810 |
| Managed hubs list | GET | `/hubs/sections/<id>/manage` | library.py:704 |
| Managed hub reload | GET | `/hubs/sections/<id>/manage` | library.py:3041 |
| Managed hub move | PUT | `/hubs/sections/<id>/manage/<i>/move[?after=<i>]` | library.py:3057 |
| Managed hub remove | DELETE | `/hubs/sections/<id>/manage/<i>` | library.py:3073 |
| Managed hub create promotion | POST | `/hubs/sections/<id>/manage` (body params) | library.py:3107 |
| Managed hub update visibility | PUT | `/hubs/sections/<id>/manage/<i>` | library.py:3110 |
| Reset managed hubs | DELETE | `/hubs/sections/<id>/manage` | library.py:710 |

### Mutations on sections

| method | verb | path | source |
|---|---|---|---|
| Add new library | POST | `/library/sections?name=&type=&agent=&scanner=&language=&location=…&prefs[…]=` | library.py:390 |
| Edit library | PUT | `/library/sections/<id>?agent=…&<params>` | library.py:572 |
| Delete library | DELETE | `/library/sections/<id>` | library.py:534 |
| Section scan | GET | `/library/sections/<id>/refresh[?path=…]` | library.py:851 |
| Cancel section scan | DELETE | `/library/sections/<id>/refresh` | library.py:857 |
| Force refresh section | GET | `/library/sections/<id>/refresh?force=1` | library.py:865 |
| Analyze section | PUT | `/library/sections/<id>/analyze` | library.py:833 |
| Empty trash (section) | PUT | `/library/sections/<id>/emptyTrash` | library.py:839 |
| Delete media previews | DELETE | `/library/sections/<id>/indexes` | library.py:873 |
| Lock/unlock field across libtype | PUT | `/library/sections/<id>/all?type=<t>&<field>.locked=<0|1>` | library.py:773 |
| Multi-edit items | PUT | `/library/sections/<id>/all?id=<r1>,<r2>,…&type=<t>&<field>=<value>…` | library.py:1744 |

### Library-wide mutations

| method | verb | path | source |
|---|---|---|---|
| Update all | GET | `/library/sections/all/refresh` | library.py:207 |
| Cancel update all | DELETE | `/library/sections/all/refresh` | library.py:213 |
| Refresh all (force) | GET | `/library/sections/all/refresh?force=1` | library.py:220 |
| Clean bundles | PUT | `/library/clean/bundles?async=1` | library.py:188 |
| Optimize DB | PUT | `/library/optimize?async=1` | library.py:202 |
