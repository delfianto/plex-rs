# 08 — Mixins and Traits

A deep dive into `python-plexapi/plexapi/mixins/`: how Python uses multiple-inheritance "mixin" classes to compose capabilities onto leaf media objects (`Movie`, `Show`, `Episode`, `Track`, `Collection`, …), how those capabilities map onto Plex Media Server (PMS) HTTP endpoints, and — at the end — how to translate the whole arrangement into idiomatic Rust 2024 traits.

All file paths are absolute. Line citations refer to `python-plexapi/plexapi/...` unless otherwise noted.

---

## 1. What is a "mixin" in this codebase?

In `python-plexapi`, a **mixin** is a class that:

1. Has **no `__init__`** and carries **no state of its own**.
2. Assumes (without declaring) that `self` carries a `PlexObject`-shaped interface — at minimum `self._server` (a `PlexServer` capable of issuing HTTP queries), `self.ratingKey`, `self.key`, `self.TYPE`, and helpers like `self._edit(...)`, `self.fetchItems(...)`, `self.firstAttr(...)`.
3. Adds a small, cohesive **bundle of methods** that gets pulled into a leaf class via Python's multiple-inheritance MRO.

A canonical example from `mixins/resources.py:7-14`:

```python
class ArtUrlMixin:
    """ Mixin for Plex objects that can have a background artwork url. """

    @property
    def artUrl(self):
        """ Return the art url for the Plex object. """
        art = self.firstAttr('art', 'grandparentArt')
        return self._server.url(art, includeToken=True) if art else None
```

Notice: `ArtUrlMixin` references `self._server` and `self.firstAttr` but never declares them. It is *not* a standalone class — it only makes sense once mixed into a `PlexPartialObject` subclass. The composite types in `mixins/__init__.py:34-223` then string dozens of these together. For example, `Movie` (`video.py:332`) inherits from `Video, Playable, MovieMixins`, and `MovieMixins` (`mixins/__init__.py:130-136`) itself inherits from 14 sibling mixins:

```python
class MovieMixins(
    AdvancedSettingsMixin, SplitMergeMixin, UnmatchMatchMixin, ExtrasMixin, HubsMixin, RatingMixin,
    ArtMixin, LogoMixin, PosterMixin, SquareArtMixin, ThemeMixin,
    MovieEditMixins,
    WatchlistMixin
):
    pass
```

The "diamond" stacking is intentional. Each capability is independently testable and reusable across media types, and a leaf media type cherry-picks the union of capabilities it supports. There is no abstract base; the contract between a mixin and its host is implicit and runtime-enforced.

---

## 2. The mixin matrix

The composite mixins in `mixins/__init__.py` provide the authoritative declaration of which leaf type gets which capability. Two layers exist:

- **Edit composites** (`*EditMixins`) bundle field/tag edit mixins.
- **Top-level composites** (`MovieMixins`, `ShowMixins`, …) bundle the edit composite plus image/action/watchlist/etc. mixins.

The leaf class declarations live in:

| Leaf class | File:line | Composite |
|---|---|---|
| `Movie` | `video.py:332` | `MovieMixins` |
| `Show` | `video.py:540` | `ShowMixins` |
| `Season` | `video.py:788` | `SeasonMixins` |
| `Episode` | `video.py:968` | `EpisodeMixins` |
| `Clip` | `video.py:1249` | `ClipMixins` |
| `Artist` | `audio.py:174` | `ArtistMixins` |
| `Album` | `audio.py:341` | `AlbumMixins` |
| `Track` | `audio.py:492` | `TrackMixins` |
| `Photoalbum` | `photo.py:12` | `PhotoalbumMixins` |
| `Photo` | `photo.py:151` | `PhotoMixins` |
| `Collection` | `collection.py:12` | `CollectionMixins` |
| `Playlist` | `playlist.py:14` | `PlaylistMixins` |

The "action" mixin matrix (non-edit capabilities):

| Mixin | Movie | Show | Season | Episode | Clip | Artist | Album | Track | Photoalbum | Photo | Collection | Playlist |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `AdvancedSettingsMixin` | yes | yes | yes |  |  | yes |  |  |  |  | yes |  |
| `SplitMergeMixin` | yes | yes |  |  |  | yes | yes |  |  |  |  |  |
| `UnmatchMatchMixin` | yes | yes |  |  |  | yes | yes |  |  |  |  |  |
| `ExtrasMixin` | yes | yes | yes | yes |  | yes |  | yes |  |  |  |  |
| `HubsMixin` | yes | yes |  |  |  | yes |  |  |  |  | yes |  |
| `RatingMixin` | yes | yes | yes | yes |  | yes | yes | yes | yes | yes | yes |  |
| `ArtMixin` | yes | yes | yes | yes |  | yes | yes |  | yes |  | yes | yes |
| `ArtUrlMixin` (only) |  |  |  |  | yes |  |  | yes |  | yes |  |  |
| `LogoMixin` | yes | yes | yes | yes |  | yes | yes |  | yes |  | yes | yes |
| `LogoUrlMixin` (only) |  |  |  |  | yes |  |  | yes |  | yes |  |  |
| `PosterMixin` | yes | yes | yes | yes |  | yes | yes |  | yes |  | yes | yes |
| `PosterUrlMixin` (only) |  |  |  |  | yes |  |  | yes |  | yes |  |  |
| `SquareArtMixin` | yes | yes | yes | yes |  | yes | yes |  | yes |  | yes | yes |
| `SquareArtUrlMixin` (only) |  |  |  |  | yes |  |  | yes |  | yes |  |  |
| `ThemeMixin` | yes | yes |  |  |  | yes |  |  |  |  | yes |  |
| `ThemeUrlMixin` (only) |  |  | yes | yes |  |  | yes | yes |  |  |  |  |
| `WatchlistMixin` | yes | yes |  |  |  |  |  |  |  |  |  |  |
| `SmartFilterMixin` |  |  |  |  |  |  |  |  |  |  | yes | yes |
| `PlayedUnplayedMixin` | (via `Video`) | (via `Video`) | (via `Video`) | (via `Video`) | (via `Video`) | (via `Audio`) | (via `Audio`) | (via `Audio`) |  |  |  |  |

`PlayedUnplayedMixin` is special: it's not pulled in through `*Mixins` composites. Instead, `Video` and `Audio` themselves inherit it directly (`video.py:11`, `audio.py:20`), so it propagates to every subclass.

The edit-mixin matrix (which fields each leaf can edit) — each row is `mixins/edit.py`:

| Edit mixin (file:line) | Field name | Movie | Show | Season | Episode | Artist | Album | Track | Photoalbum | Photo | Collection | Playlist |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `AddedAtMixin` (`edit.py:33`) | `addedAt` | yes | yes | yes | yes | yes | yes | yes | yes | yes | yes |  |
| `AudienceRatingMixin` (`edit.py:51`) | `audienceRating` | yes | yes | yes | yes | yes | yes | yes |  |  | yes |  |
| `ContentRatingMixin` (`edit.py:64`) | `contentRating` | yes | yes |  | yes |  |  |  |  |  | yes |  |
| `CriticRatingMixin` (`edit.py:77`) | `rating` | yes | yes | yes | yes | yes | yes | yes |  |  | yes |  |
| `EditionTitleMixin` (`edit.py:90`) | `editionTitle` | yes |  |  |  |  |  |  |  |  |  |  |
| `OriginallyAvailableMixin` (`edit.py:103`) | `originallyAvailableAt` | yes | yes |  | yes |  | yes |  |  |  |  |  |
| `OriginalTitleMixin` (`edit.py:118`) | `originalTitle` | yes | yes |  |  |  |  |  |  |  |  |  |
| `SortTitleMixin` (`edit.py:131`) | `titleSort` | yes | yes |  | yes | yes | yes |  | yes | yes | yes | yes |
| `StudioMixin` (`edit.py:144`) | `studio` | yes | yes |  |  |  | yes |  |  |  |  |  |
| `SummaryMixin` (`edit.py:157`) | `summary` | yes | yes | yes | yes | yes | yes |  | yes | yes | yes | yes |
| `TaglineMixin` (`edit.py:170`) | `tagline` | yes | yes |  |  |  |  |  |  |  |  |  |
| `TitleMixin` (`edit.py:183`) | `title` (album also sends `artist.id.value`) | yes | yes | yes | yes | yes | yes | yes | yes | yes | yes | yes |
| `TrackArtistMixin` (`edit.py:200`) | `originalTitle` |  |  |  |  |  |  | yes |  |  |  |  |
| `TrackNumberMixin` (`edit.py:213`) | `index` |  |  |  |  |  |  | yes |  |  |  |  |
| `TrackDiscNumberMixin` (`edit.py:226`) | `parentIndex` |  |  |  |  |  |  | yes |  |  |  |  |
| `PhotoCapturedTimeMixin` (`edit.py:239`) | `originallyAvailableAt` (formatted) |  |  |  |  |  |  |  |  | yes |  |  |
| `UserRatingMixin` (`edit.py:254`) | `userRating` | yes | yes | yes | yes | yes | yes | yes | yes | yes | yes |  |
| `CollectionMixin` (tag) (`edit.py:343`) | `collection[]` | yes | yes | yes | yes | yes | yes | yes |  |  |  |  |
| `CountryMixin` (tag) (`edit.py:365`) | `country[]` | yes |  |  |  | yes |  |  |  |  |  |  |
| `DirectorMixin` (tag) (`edit.py:387`) | `director[]` | yes |  |  | yes |  |  |  |  |  |  |  |
| `GenreMixin` (tag) (`edit.py:409`) | `genre[]` | yes | yes |  |  | yes | yes | yes |  |  |  |  |
| `LabelMixin` (tag) (`edit.py:431`) | `label[]` | yes | yes | yes | yes | yes | yes | yes |  |  | yes |  |
| `MoodMixin` (tag) (`edit.py:453`) | `mood[]` |  |  |  |  | yes | yes | yes |  |  |  |  |
| `ProducerMixin` (tag) (`edit.py:475`) | `producer[]` | yes |  |  |  |  |  |  |  |  |  |  |
| `SimilarArtistMixin` (tag) (`edit.py:497`) | `similar[]` |  |  |  |  | yes |  |  |  |  |  |  |
| `StyleMixin` (tag) (`edit.py:519`) | `style[]` |  |  |  |  | yes | yes |  |  |  |  |  |
| `TagMixin` (tag) (`edit.py:541`) | `tag[]` |  |  |  |  |  |  |  |  | yes |  |  |
| `WriterMixin` (tag) (`edit.py:563`) | `writer[]` | yes |  |  | yes |  |  |  |  |  |  |  |
| `ArtLockMixin` (lock) (`resources.py:17`) | `art.locked` | yes | yes | yes | yes | yes | yes |  | yes |  | yes | yes |
| `PosterLockMixin` (lock) (`resources.py:148`) | `thumb.locked` | yes | yes | yes | yes | yes | yes |  | yes |  | yes | yes |
| `ThemeLockMixin` (lock) (`resources.py:274`) | `theme.locked` | yes | yes | yes | yes | yes | yes |  |  |  | yes |  |

(`LogoLockMixin` and `SquareArtLockMixin` exist in `resources.py:82` and `resources.py:213` but are bundled inside `LogoMixin` / `SquareArtMixin` rather than wired into the edit composites.)

These tables are the **single source of truth** for the trait surface you'd need to implement in `plex-rs`.

---

## 3. `edit.py` deep dive — `EditFieldsMixin`, `_edit`, `editTags`, field-locking

### 3.1 The pivot point: `PlexPartialObject._edit`

Every field/tag mutation funnels through `PlexPartialObject._edit` (`base.py:700-710`):

```python
def _edit(self, **kwargs):
    """ Actually edit an object. """
    if isinstance(self._edits, dict):
        self._edits.update(kwargs)
        return self

    if 'type' not in kwargs:
        kwargs['type'] = utils.searchType(self._searchType)

    self.section()._edit(items=self, **kwargs)
    return self
```

Two behaviors:

- If `batchEdits()` was called (which sets `self._edits = {}` per `base.py:740-758`), it just **accumulates** kwargs into a dict and returns. `saveEdits()` (`base.py:760-771`) then flushes that dict through `_edit` with `_edits = None`.
- Otherwise it delegates to `LibrarySection._edit(items=self, **kwargs)` which actually issues the HTTP PUT.

### 3.2 The wire format

`LibrarySection._edit` (`library.py:1734-1746`) builds the PMS endpoint:

```python
def _edit(self, items=None, **kwargs):
    if isinstance(self._edits, dict) and items is None:
        self._edits.update(kwargs)
        return self
    kwargs['id'] = ','.join(str(item.ratingKey) for item in self._validateItems(items))
    if 'type' not in kwargs:
        kwargs['type'] = utils.searchType(items[0].type)
    part = f'/library/sections/{self.key}/all{utils.joinArgs(kwargs)}'
    self._server.query(part, method=self._server._session.put)
    return self
```

The real PMS request is therefore:

```
PUT /library/sections/<sectionKey>/all
    ?id=<ratingKey>(,<ratingKey>…)
    &type=<searchType>
    &<field>.value=<urlEncodedValue>
    &<field>.locked=0|1
    &<tag>[<idx>].tag.tag=<value>
    &<tag>.locked=0|1
    &<tag>[].tag.tag-=<csvOfQuoted>     # for removals
```

Note that despite the docstring promise of "PUT /library/metadata/&lt;rk&gt;?…", the canonical edit endpoint is actually `/library/sections/<sectionKey>/all` with `id=<ratingKey>` — this matters when porting to Rust, because it requires the parent `LibrarySection` to be reachable from the leaf item.

### 3.3 `EditFieldMixin.editField` — single-field edit

`mixins/edit.py:5-30`:

```python
class EditFieldMixin:
    def editField(self, field, value, locked=True, **kwargs):
        edits = {
            f'{field}.value': value or '',
            f'{field}.locked': 1 if locked else 0
        }
        edits.update(kwargs)
        return self._edit(**edits)
```

Each field mixin (`AddedAtMixin`, `SummaryMixin`, `TitleMixin`, …) is a thin wrapper:

```python
class SummaryMixin(EditFieldMixin):
    def editSummary(self, summary, locked=True):
        return self.editField('summary', summary, locked=locked)
```

The only non-trivial wrappers:

- `AddedAtMixin.editAddedAt` (`edit.py:33-48`) coerces `str|datetime|int` → Unix timestamp.
- `OriginallyAvailableMixin` (`edit.py:103-115`) and `PhotoCapturedTimeMixin` (`edit.py:239-251`) format `datetime` → `"%Y-%m-%d"` and `"%Y-%m-%d %H:%M:%S"` respectively.
- `CriticRatingMixin.editCriticRating` (`edit.py:77-87`) — note: edits the field named **`rating`**, not `criticRating`. Subtle PMS naming wart.
- `TrackArtistMixin.editTrackArtist` (`edit.py:200-210`) edits **`originalTitle`** (Plex repurposes that field for the "track artist" override).
- `SortTitleMixin.editSortTitle` (`edit.py:131-141`) edits **`titleSort`** (Plex's actual field name).
- `TrackNumberMixin` edits `index`, `TrackDiscNumberMixin` edits `parentIndex`.
- `TitleMixin.editTitle` (`edit.py:183-197`) has special-case logic for albums: it also sends `artist.id.value = parentRatingKey` so PMS doesn't lose the parent link.

### 3.4 `EditTagsMixin.editTags` — tag adds/removes/locks

`edit.py:267-340`. This is the more intricate machinery:

```python
def editTags(self, tag, items, locked=True, remove=False, **kwargs):
    if not isinstance(items, list):
        items = [items]
    if not remove:
        tags = getattr(self, self._tagPlural(tag), [])
        if isinstance(tags, list):
            items = tags + items
    edits = self._tagHelper(self._tagSingular(tag), items, locked, remove)
    edits.update(kwargs)
    return self._edit(**edits)
```

Three semantically important behaviors:

1. **Adds are merge-with-existing.** When `remove=False`, `editTags` reads the current `self.<tagPlural>` list (e.g. `self.collections`) and **prepends** it to the new items before sending. This is because the PMS edit endpoint replaces the tag set — it doesn't merge. So to "add", you must resend the full list.

2. **Removes use a magic suffix.** `_tagHelper` (`edit.py:322-340`):

   ```python
   if remove:
       tagname = f'{tag}[].tag.tag-'
       data[tagname] = ','.join(quote(str(t)) for t in items)
   else:
       for i, item in enumerate(items):
           tagname = f'{str(tag)}[{i}].tag.tag'
           data[tagname] = item
   ```

   So a remove sends `genre[].tag.tag- = Action,Comedy` (the trailing `-` is the PMS removal sigil), while an add sends `genre[0].tag.tag=Action&genre[1].tag.tag=Comedy`.

3. **Locked toggling is always sent.** Every tag mutation always carries `<tag>.locked=0|1`. There's no way to mutate the value without also asserting the lock state.

`_tagSingular` / `_tagPlural` (`edit.py:300-320`) handle two irregular nouns: `country↔countries` and `similar↔similar` (uncountable). Everything else is `s`-suffixed.

### 3.5 `locked` semantics

The `locked` parameter is a Plex agent-fight signal:

- `locked=1` tells Plex's metadata agents (TMDB, TVDB, MusicBrainz, …) "the user changed this; do not overwrite on next refresh."
- `locked=0` returns the field to "managed by agent" status. The next library refresh may replace whatever value is there.

Every edit method defaults `locked=True`. There's also a pair of pure-lock methods in `resources.py` (`lockArt`, `unlockArt`, `lockPoster`, `unlockPoster`, `lockTheme`, `unlockTheme`, `lockLogo`, `unlockLogo`, `lockSquareArt`, `unlockSquareArt`) that flip the lock without touching the value — they send only `<field>.locked = 0|1`.

The current lock state of any field on a fetched object is read via `PlexPartialObject.isLocked(field)` (`base.py:692-698`), which inspects the `<Field>` children of the metadata XML.

---

## 4. The auto-generated field mixins

The "auto-generated" framing is a slight overstatement — there's no code generator; each mixin is hand-written, but they all follow the same shape (single method, calls `self.editField(<name>, value, locked=...)`).

| Mixin (`mixins/edit.py:line`) | Method | PMS field |
|---|---|---|
| `AddedAtMixin` (33) | `editAddedAt(addedAt, locked=True)` | `addedAt` (Unix ts) |
| `AudienceRatingMixin` (51) | `editAudienceRating(audienceRating, locked)` | `audienceRating` |
| `ContentRatingMixin` (64) | `editContentRating(contentRating, locked)` | `contentRating` |
| `CriticRatingMixin` (77) | `editCriticRating(criticRating, locked)` | `rating` |
| `EditionTitleMixin` (90) | `editEditionTitle(...)` | `editionTitle` (Plex Pass only) |
| `OriginallyAvailableMixin` (103) | `editOriginallyAvailable(...)` | `originallyAvailableAt` |
| `OriginalTitleMixin` (118) | `editOriginalTitle(...)` | `originalTitle` |
| `SortTitleMixin` (131) | `editSortTitle(...)` | `titleSort` |
| `StudioMixin` (144) | `editStudio(...)` | `studio` |
| `SummaryMixin` (157) | `editSummary(...)` | `summary` |
| `TaglineMixin` (170) | `editTagline(...)` | `tagline` |
| `TitleMixin` (183) | `editTitle(...)` | `title` (+ `artist.id.value` for albums) |
| `TrackArtistMixin` (200) | `editTrackArtist(...)` | `originalTitle` (Track override) |
| `TrackNumberMixin` (213) | `editTrackNumber(...)` | `index` |
| `TrackDiscNumberMixin` (226) | `editDiscNumber(...)` | `parentIndex` |
| `PhotoCapturedTimeMixin` (239) | `editCapturedTime(...)` | `originallyAvailableAt` (datetime fmt) |
| `UserRatingMixin` (254) | `editUserRating(...)` | `userRating` |

Several PMS warts to highlight when porting:

- Three mixins (`CriticRatingMixin`, `TrackArtistMixin`, `PhotoCapturedTimeMixin`) edit a field with a **different name** than the method suggests. The Plex schema reuses fields.
- `SortTitleMixin` is `titleSort` not `sortTitle`.

---

## 5. Tag mixins

All inherit from `EditTagsMixin` and follow the same template (`addXxx` / `removeXxx`):

| Mixin (`mixins/edit.py:line`) | Singular tag (sent over wire) | Pluralized attr (read for merge) |
|---|---|---|
| `CollectionMixin` (343) | `collection` | `collections` |
| `CountryMixin` (365) | `country` | `countries` (irregular) |
| `DirectorMixin` (387) | `director` | `directors` |
| `GenreMixin` (409) | `genre` | `genres` |
| `LabelMixin` (431) | `label` | `labels` |
| `MoodMixin` (453) | `mood` | `moods` |
| `ProducerMixin` (475) | `producer` | `producers` |
| `SimilarArtistMixin` (497) | `similar` | `similar` (uncountable) |
| `StyleMixin` (519) | `style` | `styles` |
| `TagMixin` (541) | `tag` | `tags` (Photo only) |
| `WriterMixin` (563) | `writer` | `writers` |

`items` can be either a list of `str` or a list of `media.MediaTag`. The helper just stringifies — there's no validation against existing tags on the server.

### Mental model

Adds: GET current list → append new → PUT entire list with `<tag>[<i>].tag.tag=`.
Removes: PUT just the deltas with `<tag>[].tag.tag-=` (CSV, URL-encoded).
Both always assert `<tag>.locked = 0|1`.

There's no "replace" primitive in the mixin layer; if you want a pure replace, you call `editTags` with the desired set and then issue a remove for what's left over (or use the low-level `edit()` and craft kwargs yourself — see `base.py:712-738`).

---

## 6. Image mixins (`mixins/resources.py`)

Despite the filename, this module deals with **image/theme resources attached to library items** — not MyPlex resources. The naming overlap with `MyPlexResource` in `myplex.py` is unfortunate; that's covered in analysis 03.

Five image families, each in three layers:

| Family | URL mixin | Lock mixin | Full mixin (UrlMixin + LockMixin + actions) |
|---|---|---|---|
| Art (background) | `ArtUrlMixin` (7) | `ArtLockMixin` (17) | `ArtMixin` (29) |
| Logo (clearLogo) | `LogoUrlMixin` (68) | `LogoLockMixin` (82) | `LogoMixin` (94) |
| Poster (thumb) | `PosterUrlMixin` (133) | `PosterLockMixin` (148) | `PosterMixin` (160) |
| Square art (backgroundSquare) | `SquareArtUrlMixin` (199) | `SquareArtLockMixin` (213) | `SquareArtMixin` (225) |
| Theme (audio) | `ThemeUrlMixin` (264) | `ThemeLockMixin` (274) | `ThemeMixin` (286) |

The `*Url` mixins provide read-only computed URLs. The `*Lock` mixins toggle `<field>.locked` via `self._edit(**{'<field>.locked': 0|1})`. The full `*Mixin` adds:

- `xxxs()` — list available alternatives. `PosterMixin.posters()` (`resources.py:163-165`):

  ```python
  def posters(self):
      return self.fetchItems(f'/library/metadata/{self.ratingKey}/posters', cls=media.Poster)
  ```

  Hits **`GET /library/metadata/<rk>/posters`**, returns `media.Poster[]`.

- `uploadXxx(url=None, filepath=None)` — `PosterMixin.uploadPoster` (`resources.py:167-181`):

  ```python
  if url:
      key = f'/library/metadata/{self.ratingKey}/posters?url={quote_plus(url)}'
      self._server.query(key, method=self._server._session.post)
  elif filepath:
      key = f'/library/metadata/{self.ratingKey}/posters'
      data = openOrRead(filepath)
      self._server.query(key, method=self._server._session.post, data=data)
  ```

  POST to the same endpoint with either `?url=…` (Plex pulls the URL) or the raw bytes as the body.

- `setXxx(obj)` — calls `obj.select()` (on `media.Poster` etc.), which itself issues a `PUT` against the resource's selection key.

- `deleteXxx()` — `DELETE /library/metadata/<rk>/poster` (singular path for delete, plural for list — note inconsistency).

`ThemeMixin` is the odd one out: `setTheme()` raises `NotImplementedError` (`resources.py:316-322`) because PMS exposes no selection endpoint for themes. You can only re-upload. Delete works.

The URL endpoints with their methods:

| Family | List | Upload | Set | Delete | Lock toggle field |
|---|---|---|---|---|---|
| art | `GET /library/metadata/{rk}/arts` | `POST /library/metadata/{rk}/arts[?url=…]` | `obj.select()` (`PUT`) | `DELETE /library/metadata/{rk}/art` | `art.locked` |
| logo | `GET /library/metadata/{rk}/clearLogos` | `POST /library/metadata/{rk}/clearLogos[?url=…]` | `obj.select()` | `DELETE /library/metadata/{rk}/clearLogo` | `clearLogo.locked` |
| poster | `GET /library/metadata/{rk}/posters` | `POST /library/metadata/{rk}/posters[?url=…]` | `obj.select()` | `DELETE /library/metadata/{rk}/thumb` | `thumb.locked` |
| squareArt | `GET /library/metadata/{rk}/squareArts` | `POST /library/metadata/{rk}/squareArts[?url=…]` | `obj.select()` | `DELETE /library/metadata/{rk}/squareArt` | `squareArt.locked` |
| theme | `GET /library/metadata/{rk}/themes` | `POST /library/metadata/{rk}/themes[?url=…]` | unsupported | `DELETE /library/metadata/{rk}/theme` | `theme.locked` |

---

## 7. `played_unplayed.py` and `rating.py`

Two of the smallest mixins in the file, and two of the most-used.

### `PlayedUnplayedMixin` (`played_unplayed.py:1-34`)

Not declared in any `*Mixins` composite — instead it's mixed directly into `Video` (`video.py:11`) and `Audio` (`audio.py:20`), so it propagates to every video/audio leaf:

```python
class Video(PlexPartialObject, PlayedUnplayedMixin):
    ...
class Audio(PlexPartialObject, PlayedUnplayedMixin):
    ...
```

Surface:

- `isPlayed` (property): `bool(self.viewCount > 0)`.
- `markPlayed()`: `GET /:/scrobble?key=<ratingKey>&identifier=com.plexapp.plugins.library`.
- `markUnplayed()`: `GET /:/unscrobble?key=<ratingKey>&identifier=com.plexapp.plugins.library`.
- Aliases: `isWatched`, `markWatched()`, `markUnwatched()` (no semantic difference).

The `key` is a `ratingKey` (numeric), and the `identifier` is a constant — Plex carries this from the days when alternative scrobble providers existed. Note these are `GET` requests despite being mutations; the PMS API is unusual that way.

### `RatingMixin` (`rating.py:1-22`)

Single method `rate(rating=None)`:

```python
def rate(self, rating=None):
    if rating is None:
        rating = -1
    elif not isinstance(rating, (int, float)) or rating < 0 or rating > 10:
        raise BadRequest('Rating must be between 0 to 10.')
    key = f'/:/rate?key={self.ratingKey}&identifier=com.plexapp.plugins.library&rating={rating}'
    self._server.query(key, method=self._server._session.put)
    return self
```

`PUT /:/rate?key=<rk>&identifier=com.plexapp.plugins.library&rating=<0..10|-1>`. The scale is 0–10 over the wire, displayed as 0–5 stars in UI (each star = 2 wire-points). `rating=-1` resets the rating.

`RatingMixin` is widely applied — it shows up in every `*Mixins` composite for video, audio, photo, and collection types (see matrix above).

---

## 8. `watchlist.py` — MyPlex-routed, not local-server

`mixins/watchlist.py` is unusual: it's a per-item mixin attached to `Movie` and `Show`, but its methods do not talk to your local PMS at all. They route through `MyPlexAccount` against `https://discover.provider.plex.tv`.

Methods (`watchlist.py:1-62`):

- `onWatchlist(account=None) → bool`
- `addToWatchlist(account=None) → self`
- `removeFromWatchlist(account=None) → self` (note: the task description's `removeToWatchlist` typo doesn't exist — the real method is `removeFromWatchlist`)
- `streamingServices(account=None) → list[media.Availability]`

All follow the same pattern:

```python
try:
    account = account or self._server.myPlexAccount()
except AttributeError:
    account = self._server
return account.onWatchlist(self)
```

If you fetched the item from a regular PMS (`PlexServer`), `self._server.myPlexAccount()` returns the linked `MyPlexAccount`. If you fetched the item from MyPlex directly (Discover endpoints), `self._server` *is* already a `MyPlexAccount`, so the `AttributeError` fallback handles that.

The real endpoints (in `myplex.py:996, :1017`):

- `PUT https://discover.provider.plex.tv/actions/addToWatchlist?ratingKey=<rk>`
- `PUT https://discover.provider.plex.tv/actions/removeFromWatchlist?ratingKey=<rk>`
- The `ratingKey` here is the **Discover/Metadata GUID-derived key**, not the local PMS ratingKey. `streamingServices` (`watchlist.py:60-62`) extracts it explicitly:

  ```python
  ratingKey = self.guid.rsplit('/', 1)[-1]
  ```

Implication for Rust: watchlist operations must be modeled against MyPlex (Discover), not against a `PlexServer` connection. They probably belong on a `MyPlexAccount` trait/impl rather than as a per-item method, with a small ergonomic extension trait `Watchlistable for Movie | Show` that delegates.

---

## 9. `split_merge.py` and `unmatch_match.py` — agent-side operations

These two mixins manage the **metadata agent's view** of an item — they're entirely server-side; no MyPlex round-trips.

### `SplitMergeMixin` (`split_merge.py:1-21`)

Applied to `Movie`, `Show`, `Artist`, `Album` (see matrix). Two methods:

- `split()` — `PUT {self.key}/split`. Splits a duplicate/merged metadata record back into its underlying components.
- `merge(ratingKeys)` — `PUT {self.key}/merge?ids=<csv>`. Merges other rating keys into this object's metadata record.

`self.key` here is the relative path like `/library/metadata/12345`, so the full endpoints are:

- `PUT /library/metadata/<rk>/split`
- `PUT /library/metadata/<rk>/merge?ids=<rk2>,<rk3>,...`

### `UnmatchMatchMixin` (`unmatch_match.py:1-97`)

Applied to `Movie`, `Show`, `Artist`, `Album`. Three methods:

- `unmatch()` — `PUT {self.key}/unmatch`. Breaks the agent match.
- `matches(agent=None, title=None, year=None, language=None) → list[media.SearchResult]` — `GET {self.key}/matches?manual=1&…`. Returns candidate matches from the configured (or specified) agent. The body of the method (`unmatch_match.py:42-69`) is a sequence of fallbacks: if only `agent` is provided, use the section's `language` and the agent's identifier; otherwise default `title` to the item's `title`, `year` to its `year`, etc.
- `fixMatch(searchResult=None, auto=False, agent=None)` — `PUT {self.key}/match?guid=<…>&name=<…>`. Either accepts a specific `SearchResult` from `matches()`, or auto-picks the first match when `auto=True`.

`utils.getAgentIdentifier(section, agent)` (in `plexapi/utils.py`) is the bridge that maps human-friendly names (`"imdb"`, `"thetvdb"`) to the section-installed agent's identifier (e.g. `com.plexapp.agents.imdb`).

For `plex-rs` these are pure URL builders against the item's metadata key — straightforward to translate.

---

## 10. `resources.py` — image resources only

As noted in §6, the file named `mixins/resources.py` contains **image/theme resource mixins** (`ArtMixin`, `PosterMixin`, `LogoMixin`, `SquareArtMixin`, `ThemeMixin` plus their `Url` and `Lock` halves). It has nothing to do with MyPlex resources (servers/clients reachable through plex.tv).

The MyPlex parallel-connect algorithm — where `MyPlexResource.connect()` races all advertised connection URLs and picks the first that responds — lives in `myplex.py` and is documented in `analysis/03-myplex-and-auth.md`. There is no overlap or duplication.

If you were searching for the "resources mixin" in the architectural sense (a mixin that wraps a remote-resource concept), it does not exist in this codebase; `MyPlexResource` is a regular `PlexObject` subclass with its own `connect()` method, not a mixin.

---

## 11. `smart_filter.py` — parsing smart-filter URIs

`SmartFilterMixin` (`mixins/smart_filter.py:8-98`) is applied to `Collection` and `Playlist`. Both can be "smart" — backed by a filter URI like `?type=1&genre=Action&push=&year>>=2010&year<<=2020&pop=` — and this mixin parses that URI into a structured dict.

The mixin doesn't *build* a URI from a dict (that direction lives elsewhere, in `LibrarySection.search`-related machinery). It only **parses**.

Three internal methods:

- `_parseFilters(content)` (`smart_filter.py:86-98`) — entry point. Takes the raw `content` URL on a smart collection/playlist, splits it, parses the query string into a `deque[(key, value)]`, and dispatches to `_parseQueryFeed`. A small trick at line 92-95 handles the `==` operator by promoting the `=` into the key (so `key==value` becomes `key= → value`).

- `_parseQueryFeed(feed)` (`smart_filter.py:55-84`) — pulls out top-level structural keys:
  - `type` → `libtype` (via `utils.reverseSearchType` — `1 → "movie"`, etc.).
  - `sort` → split on `,` into a list.
  - `includeGuids`, `limit` → integers.
  - `group`, `having` → kept as-is.
  - Everything else is delegated to `_parseFilterGroups` and merged into a top-level `filters` key.

- `_parseFilterGroups(feed, returnOn=None)` (`smart_filter.py:11-53`) — the recursive descent. Plex's smart filter URIs use `push=` and `pop=` as parenthesis markers; this method recurses on `push`, returns on `pop`, and accumulates a stack of single-pair dicts. If it encounters `and` or `or`, that becomes the group operator. Mismatched operators in one stack raise `ValueError`. Default operator is `and`.

The output is a nested dict like:

```python
{
    "libtype": "movie",
    "sort": ["addedAt:desc"],
    "filters": {
        "and": [
            {"genre": "Action"},
            {"or": [{"year>>": "2010"}, {"year<<": "2020"}]},
        ]
    },
}
```

For Rust this is a parser-only concern. A `pest`/`nom`-based parser of the same Plex filter grammar, returning a `serde_json::Value` or a typed enum tree, would map well. No HTTP I/O is involved.

---

## 12. `advanced_settings.py` — `LibrarySection.prefs` / `editAdvanced`

`AdvancedSettingsMixin` (`mixins/advanced_settings.py:7-57`) is applied to `Movie`, `Show`, `Season`, `Artist`, `Collection` (see matrix). It exposes the per-item **agent/refresh preferences** that PMS stores per-metadata-record.

Surface:

- `preferences() → list[settings.Preferences]` (`advanced_settings.py:10-13`):
  ```python
  key = f'{self.key}?includePreferences=1'
  return self.fetchItems(key, cls=settings.Preferences, rtag='Preferences')
  ```
  `GET /library/metadata/<rk>?includePreferences=1`, extract `<Preferences>` child.

- `preference(pref)` — looks up a specific `Preferences` by `id`; raises `NotFound` with the full list of available IDs.

- `editAdvanced(**kwargs)` (`advanced_settings.py:29-47`) — validates each kwarg against the loaded preferences' `enumValues`, then issues:
  ```
  PUT /library/metadata/<rk>/prefs?<settingID>=<value>&...
  ```
  This is different from the field/tag edit endpoint (`/library/sections/<key>/all`).

- `defaultAdvanced()` — pulls every preference's `default` and PUTs them all back. A "reset to defaults" operation.

For `plex-rs`: this is a separate sub-resource per item with its own PUT endpoint and validation against an enum schema fetched from the server. The validation is non-trivial — the server tells you what the enum values are, and the client refuses to send anything outside them.

---

## Mapping to Rust traits

Python's mixin pattern relies on three features that Rust 2024 does not provide directly:

1. **Multiple inheritance with a deterministic MRO.** A Python class can inherit from N siblings and Python composes them. Rust has only single inheritance through `Deref` (anti-pattern) and trait composition.
2. **Untyped duck-typed `self`.** A mixin like `ArtMixin` freely calls `self._server`, `self.ratingKey`, `self.fetchItems(...)`, `self.firstAttr(...)`. In Rust, every one of those access patterns must be expressed as a trait bound.
3. **No-state mixins.** Mixins don't declare fields; they're pure behavior. In Rust, behavior-only types ARE traits — that's actually a clean mapping.

The Plex mixin set has ~50 mixin classes, ~12 leaf classes, and a roughly 0.4-density matrix between them. The wire formats are uniform and largely just URL templating. So the question is which Rust idiom maps the matrix cleanly without combinatorial pain.

### Option A — Blanket trait impls on a `PlexObject` marker

```rust
pub trait PlexObject {
    fn server(&self) -> &PlexServer;
    fn rating_key(&self) -> &str;
    fn key(&self) -> &str;
    fn item_type(&self) -> ItemType;
}

pub trait Ratable: PlexObject {
    async fn rate(&self, rating: Option<f32>) -> Result<&Self> { /* default impl */ }
}

// Blanket: every PlexObject is automatically Ratable.
impl<T: PlexObject> Ratable for T {}
```

**Pros:** Zero per-leaf boilerplate. Adding a new leaf type automatically gets all "universal" capabilities.

**Cons:** The matrix in §2 isn't actually universal — `RatingMixin` isn't on `Playlist`, `Clip`, `Track` (`Track` only via the base trait pattern). `WatchlistMixin` is *only* on `Movie` and `Show`. A blanket impl forces you to either (a) give every type every capability (wrong — `clip.rate()` should not compile) or (b) push the "is this allowed?" check to runtime as `Err(BadRequest)`. Both lose Rust's type-safety benefit.

**Verdict:** Useful for the small universal subset (`ArtUrl`, `PosterUrl` — any item with a `thumb` field can return a URL). Bad fit for the broader matrix.

### Option B — Extension traits per capability, with explicit `impl` lines per leaf

```rust
pub trait Ratable: PlexObject {
    async fn rate(&self, rating: Option<f32>) -> Result<&Self>;
}

impl Ratable for Movie { /* one-line forward, or default in trait */ }
impl Ratable for Show { /* … */ }
impl Ratable for Season { /* … */ }
// … no impl for Clip, Playlist
```

**Pros:** Mirror's Python's matrix exactly. Compile-time refusal of `clip.rate()`. Behavior lives in the trait (via `fn rate(&self, ...) -> ... { default body }`), so leaf impls are usually empty markers. Excellent for IDE discoverability and rustdoc — `Movie` page lists every implemented capability trait.

**Cons:** ~50 traits × ~12 leaves = up to ~600 trivial `impl` lines. Some pain at write time, but those lines are pure mechanical boilerplate and easy to grep/audit. Adding a new trait requires touching every leaf you want it on.

**Verdict:** Strongest correctness. The boilerplate is a one-time cost and *truthfully reflects* Plex's actual capability matrix.

### Option C — Single trait with enum dispatch

```rust
pub enum Item { Movie(Movie), Show(Show), Season(Season), ... }

impl Item {
    pub async fn rate(&self, r: Option<f32>) -> Result<&Self> {
        match self {
            Item::Movie(m) => m.rate(r).await,
            Item::Show(s) => s.rate(r).await,
            _ => Err(Error::Unsupported("rate")),
        }
    }
}
```

**Pros:** Simple ergonomics if you primarily handle heterogeneous lists (`Vec<Item>`). One type to import. Easy `match` dispatch.

**Cons:** Errors become runtime errors (`Err(Unsupported)`), defeating the type system. Adding a new leaf is one place but every method needs an arm. The "trait" surface devolves into a giant method bag on the enum. Plex client code in practice almost always operates on a *known* leaf type (`section.search() -> Vec<Movie>`), so enum dispatch is overkill.

**Verdict:** Worth keeping as a `Item::*` enum for `Vec<Item>` results (heterogeneous searches, hubs), but not as the primary trait architecture.

### Option D — Macro-generated trait impls

```rust
declare_capabilities! {
    Movie:    [Ratable, Splittable, MatchFixable, Watchlistable, ArtMixin, PosterMixin, ThemeMixin, ...],
    Show:     [Ratable, Splittable, MatchFixable, Watchlistable, ArtMixin, PosterMixin, ThemeMixin, ...],
    Season:   [Ratable, ArtMixin, PosterMixin, ThemeUrl, ...],
    Episode:  [Ratable, ArtMixin, PosterMixin, ThemeUrl, ...],
    Clip:     [ArtUrl, LogoUrl, PosterUrl, SquareArtUrl],
    ...
}
```

A declarative `macro_rules!` that takes a matrix declaration and expands to the right `impl` lines.

**Pros:** Combines option B's correctness with much less typing. The macro input is essentially a transliteration of the matrix in §2 — it's *the spec*, not boilerplate. Easy to keep in sync with Python's `mixins/__init__.py` composites. Trait method bodies stay in the trait `default fn` so the macro only emits `impl`.

**Cons:** Macro reading/debugging is slightly worse than plain `impl` blocks. The macro itself is a piece of code to maintain. IDE jump-to-definition can be flaky inside macro expansions.

**Verdict:** Excellent if the matrix gets large or churns; equivalent to B otherwise.

### Recommendation

**Hybrid: Option B as the primary pattern, Option D for high-fan-in capabilities, Option C only for `Vec<Item>` heterogeneity.**

Concretely:

1. **Define a small foundation trait `PlexObject`** with `server()`, `rating_key()`, `key()`, `item_type()`, `section()`, and the partial-vs-full reload mechanics (`is_full_object`, `reload`). Every leaf implements it directly.

2. **Express each Plex capability as a separate trait** with all methods provided as `default fn` bodies in the trait. The default body uses only the `PlexObject` foundation (and capability-specific super-traits where needed). Examples:
   - `trait Ratable: PlexObject { async fn rate(&self, rating: Option<f32>) -> Result<&Self> { ... default ... } }`
   - `trait Watchlistable: PlexObject + HasGuid { async fn on_watchlist(...) ... }` — `HasGuid` is a small extra trait because watchlist operations need `self.guid`, not `self.rating_key`.
   - `trait EditField: PlexObject + HasSection { async fn edit_field(&self, field: &str, value: impl Into<Value>, locked: bool) -> Result<&Self> { ... } }` — this is the universal edit primitive; all field-mixin traits supertrait it.
   - `trait EditTitle: EditField { async fn edit_title(&self, title: impl AsRef<str>, locked: bool) -> Result<&Self> { ... default ... } }`
   - Same for `EditSummary`, `EditTagline`, `EditSortTitle`, etc.

3. **For tag mixins**, define one supertrait `EditTags: PlexObject + HasSection` carrying the `edit_tags` core method, plus a per-tag trait (`HasCollections`, `HasGenres`, `HasLabels`, …) with `add_*` and `remove_*` default methods. The `HasCollections` trait *also* exposes a `fn collections(&self) -> &[MediaTag]` accessor so the add-merge logic from §3.4 can read the current list.

4. **For image mixins**, split exactly as Python does: `ArtUrl: PlexObject`, `ArtLock: PlexObject + HasSection`, `Art: ArtUrl + ArtLock` adding `arts()`, `upload_art(...)`, `set_art(...)`, `delete_art()`. Leaves that only have URL semantics (`Clip`, `Track`, `Photo`) implement just `ArtUrl`.

5. **Use a `capabilities!` macro** to emit the per-leaf `impl CapTrait for Leaf {}` lines from a single matrix declaration. This declaration is the Rust analogue of `mixins/__init__.py:34-223` and should be commented to point back to it. ~12 lines of input, ~600 lines of generated impls, easily auditable.

6. **Provide an `Item` enum** (`Item::Movie(Movie) | Item::Show(Show) | …`) for heterogeneous result sets, with thin method-forwarding via a `match`. Don't make it the primary surface — most callers will work with concrete types.

7. **Foundational invariants:**
   - The trait foundation needs an `async fn _edit(&self, params: HashMap<&str, Value>)` — analogous to Python's `_edit` — that routes either to a batch accumulator (`Cell<Option<HashMap<...>>>` on the item, or a `&BatchSession` argument) or to the section's PUT endpoint. The batch-mode design needs careful thought: in Rust you probably want an explicit `let mut tx = movie.batch(); tx.edit_title(...).edit_summary(...).commit().await?;` builder rather than Python's stateful `_edits` attr.
   - `WatchlistMixin` goes on a `MyPlexAccount` API surface, not on the leaf type directly. Per-item `addToWatchlist()` is a convenience extension trait that delegates.
   - `PlayedUnplayedMixin` becomes a `Playable` trait (already aligns with Python's `Playable` mixin in `base.py:836`).

The end-state in Rust: ~50 small capability traits, each ~5–20 lines, with default bodies and a tight set of supertrait bounds. ~12 leaf structs, each with `impl PlexObject` + a generated block of marker impls. The Python matrix becomes the macro input and the source of truth. Type errors at compile time tell you "you can't call `rate()` on `Clip`", just as Python's `AttributeError` does at runtime — but earlier.

This preserves the cohesion and discoverability of the Python design while gaining real type-level guarantees and zero-cost dispatch.

---

## File reference summary

- `mixins/__init__.py` — composite mixin definitions; the matrix in §2 derives from `mixins/__init__.py:34-223`.
- `mixins/edit.py` — `EditFieldMixin` (5), `EditTagsMixin` (267), all field and tag mixins.
- `mixins/resources.py` — image/theme mixins (art, poster, logo, squareArt, theme).
- `mixins/advanced_settings.py` — per-item agent prefs.
- `mixins/objects.py` — `ExtrasMixin`, `HubsMixin`.
- `mixins/played_unplayed.py` — `markPlayed`/`markUnplayed` on `Video` / `Audio`.
- `mixins/rating.py` — single `rate()` method.
- `mixins/smart_filter.py` — Plex filter-URI parser.
- `mixins/split_merge.py` — agent-side split/merge.
- `mixins/unmatch_match.py` — agent-side unmatch/match/fixMatch.
- `mixins/watchlist.py` — Discover-routed watchlist operations.
- `base.py:700-771` — `_edit`, `edit`, `batchEdits`, `saveEdits`.
- `base.py:692-698` — `isLocked`.
- `base.py:836` — `Playable`.
- `library.py:1734-1746` — section-level `_edit` (the actual HTTP issuer).
- `myplex.py:128-129, :969-1017` — MyPlex Discover/Metadata endpoints used by `WatchlistMixin`.
