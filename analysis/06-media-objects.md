# 06 — Media Domain Objects in `python-plexapi`

Scope: leaf metadata types (`Movie`, `Show`, `Season`, `Episode`,
`Clip`, `Artist`, `Album`, `Track`, `Photoalbum`, `Photo`), the
`Media` → `MediaPart` → `Stream` chain, and the cross-cutting
tag/marker/chapter/resource types in `plexapi.media`. Contracts are
extracted directly from `_loadData` and the mixin composition graph.

References: `plexapi/video.py` (1404 LOC), `plexapi/audio.py` (673),
`plexapi/photo.py` (332), `plexapi/media.py` (1458),
`plexapi/base.py` (`Playable`, `PlexPartialObject`, `PlexSession`,
`PlexHistory`), `plexapi/mixins/__init__.py` (composite mixin graph).

---

## 1. Inheritance tree

Wide and flat. `Playable`, `PlexSession`, `PlexHistory`, and every
`*Mixin` are siblings of the inheritance backbone, mixed into leaves
via MRO. `Video` / `Audio` / `Photoalbum` / `Photo` share
`PlexPartialObject` but diverge below it.

```
PlexObject                                                  (base.py:76)
├── PlexPartialObject                                       (base.py:588)
│   ├── Video (PlexPartialObject, PlayedUnplayedMixin)      (video.py:11)
│   │   ├── Movie     (Video, Playable, MovieMixins)        (video.py:332)
│   │   │   ├── MovieSession   (PlexSession, Movie)         (video.py:1330)
│   │   │   └── MovieHistory   (PlexHistory, Movie)         (video.py:1369)
│   │   ├── Show      (Video, ShowMixins)                   (video.py:540)
│   │   ├── Season    (Video, SeasonMixins)                 (video.py:788)
│   │   ├── Episode   (Video, Playable, EpisodeMixins)      (video.py:968)
│   │   │   ├── EpisodeSession (PlexSession, Episode)       (video.py:1343)
│   │   │   └── EpisodeHistory (PlexHistory, Episode)       (video.py:1382)
│   │   └── Clip      (Video, Playable, ClipMixins)         (video.py:1249)
│   │       ├── Extra          (Clip)                       (video.py:1313)
│   │       ├── ClipSession    (PlexSession, Clip)          (video.py:1356)
│   │       └── ClipHistory    (PlexHistory, Clip)          (video.py:1395)
│   ├── Audio (PlexPartialObject, PlayedUnplayedMixin)      (audio.py:20)
│   │   ├── Artist    (Audio, ArtistMixins)                 (audio.py:174)
│   │   ├── Album     (Audio, AlbumMixins)                  (audio.py:341)
│   │   └── Track     (Audio, Playable, TrackMixins)        (audio.py:492)
│   │       ├── TrackSession   (PlexSession, Track)         (audio.py:651)
│   │       └── TrackHistory   (PlexHistory, Track)         (audio.py:664)
│   ├── Photoalbum (PlexPartialObject, PhotoalbumMixins)    (photo.py:12)
│   └── Photo      (PlexPartialObject, Playable, PhotoMixins) (photo.py:151)
│       └── PhotoSession   (PlexSession, Photo)             (photo.py:323)
│
└── PlexObject children in plexapi.media (never PlexPartialObject):
    ├── Media           (media.py:11)
    ├── MediaPart       (media.py:105)
    ├── MediaPartStream                                     (media.py:233)
    │   ├── VideoStream    (STREAMTYPE=1)                   (media.py:279)
    │   ├── AudioStream    (STREAMTYPE=2)                   (media.py:361)
    │   ├── SubtitleStream (STREAMTYPE=3)                   (media.py:435)
    │   └── LyricStream    (STREAMTYPE=4)                   (media.py:481)
    ├── Session, TranscodeSession, TranscodeJob, Optimized, Conversion
    ├── MediaTag                                            (media.py:715)
    │   └── {Collection, Country, Director, Format, Genre, Label,
    │        Mood, Producer, Role, Similar, Style, Subformat,
    │        Tag, Writer}                                   (media.py:762–933)
    ├── Guid, Image, Rating, Review, UltraBlurColors        (media.py:937–1037)
    ├── BaseResource                                        (media.py:1039)
    │   └── {Art, Logo, Poster, SquareArt, Theme}           (media.py:1083–1105)
    ├── Chapter, Marker, Field                              (media.py:1109–1203)
    ├── SearchResult, Agent, AgentMediaType                 (media.py:1207–1288)
    ├── Availability, Level                                 (media.py:1291–1338)
    └── CommonSenseMedia, AgeRating, TalkingPoint, ParentalAdvisoryTopic
```

Two non-obvious points:

1. **`Playable` is a mixin**, not a base class. Only the *leaf*
   playable types compose it: `Movie`, `Episode`, `Clip`, `Track`,
   `Photo`. Containers (`Show`, `Season`, `Artist`, `Album`,
   `Photoalbum`) delegate to their children.
2. **`PlexSession` / `PlexHistory` are mixins too.** Five leaves
   have parallel `*Session` and (four of them) `*History` subclasses
   whose `_loadData` chains `Foo._loadData(self,data)` then
   `PlexSession._loadData(self,data)` (`video.py:1338`).

---

## 2. Per-type attribute inventory

Tables are exact: each row reflects an assignment inside `_loadData`
plus the `cached_data_property` collections following it. Format
`name : type ← @xmlAttr` or `name : List<T> ← <Child>`.
Defaults are shown when the parser supplies one. Datetimes are Unix
timestamps from `utils.toDatetime` unless `%Y-%m-%d` is noted (then
date-only).

### 2.1 `Video` (base) — `video.py:42–72`

`addedAt : datetime ← @addedAt`,
`art : str ← @art`,
`artBlurHash : str ← @artBlurHash`,
`guid : str ← @guid`,
`key : str ← @key`,
`lastRatedAt : datetime ← @lastRatedAt`,
`lastViewedAt : datetime ← @lastViewedAt`,
`librarySectionID : int ← @librarySectionID`,
`librarySectionKey : str ← @librarySectionKey`,
`librarySectionTitle : str ← @librarySectionTitle`,
`listType : str = "video"` (constant),
`ratingKey : int ← @ratingKey`,
`summary : str ← @summary`,
`thumb : str ← @thumb`,
`thumbBlurHash : str ← @thumbBlurHash`,
`title : str ← @title`,
`titleSort : str ← @titleSort` (defaults to `title`),
`type : str ← @type`,
`updatedAt : datetime ← @updatedAt`,
`userRating : float ← @userRating`,
`viewCount : int ← @viewCount` (default `0`),
`fields : List<Field> ← <Field>`,
`images : List<Image> ← <Image>`.

### 2.2 `Movie` — `video.py:386–474`

Includes `Video._loadData` + `Playable._loadData`
(`playlistItemID`, `playQueueItemID`).

`audienceRating : float`, `audienceRatingImage : str`,
`chapterSource : str`, `contentRating : str`,
`duration : int (ms)`, `editionTitle : str`,
`enableCreditsMarkerGeneration : int = -1`,
`languageOverride : str`,
`originallyAvailableAt : date (%Y-%m-%d)`,
`originalTitle : str`, `primaryExtraKey : str`, `rating : float`,
`ratingImage : str`, `slug : str`,
`sourceURI : str ← @source` (remote playlist only),
`studio : str`, `tagline : str`, `theme : str`,
`useOriginalTitle : int = -1`,
`viewOffset : int = 0`, `year : int`.

Child collections:
`chapters: List<Chapter>`, `collections: List<Collection>`,
`commonSenseMedia: CommonSenseMedia?`, `countries: List<Country>`,
`directors: List<Director>`, `genres: List<Genre>`,
`guids: List<Guid>`, `labels: List<Label>`,
`markers: List<Marker>`, `media: List<Media>`,
`producers: List<Producer>`, `ratings: List<Rating>`,
`roles: List<Role>`, `similar: List<Similar>`,
`ultraBlurColors: UltraBlurColors?`, `writers: List<Writer>`.

Derived (not from XML): `actors` = alias of `roles`; `locations` =
list of `part.file` for every part; `hasCreditsMarker`,
`hasVoiceActivity`, `hasPreviewThumbnails` predicates
(`video.py:476–504`).

### 2.3 `Show` — `video.py:606–680`

Not `Playable`. `_loadData` rewrites `self.key` to strip a trailing
`/children` (FIX_BUG_50, `video.py:623` — also on `Season`,
`Artist`, `Album`, `Photoalbum`).

`audienceRating : float`, `audienceRatingImage : str`,
`audioLanguage : str = ""`,
`autoDeletionItemPolicyUnwatchedLibrary : int = 0`,
`autoDeletionItemPolicyWatchedLibrary : int = 0`,
`childCount : int`, `contentRating : str`,
`duration : int (typical episode ms)`,
`enableCreditsMarkerGeneration : int = -1`,
`episodeSort : int = -1`, `flattenSeasons : int = -1`,
`index : int`, `languageOverride : str`, `leafCount : int`,
`network : str`, `originallyAvailableAt : date`,
`originalTitle : str`, `rating : float`,
`seasonCount : int` (defaults to `childCount`),
`showOrdering : str`, `slug : str`, `studio : str`,
`subtitleLanguage : str = ""`, `subtitleMode : int = -1`,
`tagline : str`, `theme : str`,
`useOriginalTitle : int = -1`, `viewedLeafCount : int`,
`year : int`.

Child collections: `collections`, `commonSenseMedia?`, `genres`,
`guids`, `labels`, `ratings`, `roles`, `similar`,
`ultraBlurColors?`. Also `locations: List<str>` — `<Location @path>`
via `listAttrs(self._data, 'path', etag='Location')`
(`video.py:662`).

### 2.4 `Season` — `video.py:826–867`

`audienceRating : float`, `audioLanguage : str = ""`,
`index : int`, `leafCount : int`,
`parentGuid : str`, `parentIndex : int`, `parentKey : str`,
`parentRatingKey : int`, `parentSlug : str`,
`parentStudio : str`, `parentTheme : str`,
`parentThumb : str`, `parentTitle : str`,
`rating : float`, `subtitleLanguage : str = ""`,
`subtitleMode : int = -1`, `viewedLeafCount : int`,
`year : int`.

Child collections: `collections`, `guids`, `labels`, `ratings`,
`ultraBlurColors?`. `seasonNumber` aliases `index`.

### 2.5 `Episode` — `video.py:1021–1103`

`Playable`. Quirk: when seasons are hidden (`Show.flattenSeasons=1`)
the XML omits `parentKey` / `parentRatingKey` / `parentThumb`. The
parser stashes raw values into `_parentKey` / `_parentRatingKey` /
`_parentThumb` and exposes `parentKey`/`parentRatingKey`/`parentThumb`
as cached properties that fall back to a server round-trip via
`_season` (`video.py:1105–1146`). This is the *only* media-parse
path that can trigger network I/O on attribute access.

`audienceRating : float`, `audienceRatingImage : str`,
`chapterSource : str`, `contentRating : str`, `duration : int`,
`grandparentArt : str`, `grandparentGuid : str`,
`grandparentKey : str`, `grandparentRatingKey : int`,
`grandparentSlug : str`, `grandparentTheme : str`,
`grandparentThumb : str`, `grandparentTitle : str`,
`index : int`, `originallyAvailableAt : date`,
`parentGuid : str`, `parentIndex : int (season number)`,
`parentTitle : str`, `parentYear : int`,
`rating : float`, `skipParent : bool = False`,
`sourceURI : str`, `viewOffset : int = 0`, `year : int`,
plus stashed raw `_parentKey`/`_parentRatingKey`/`_parentThumb`.

Child collections: `chapters`, `collections`, `directors`, `guids`,
`labels`, `markers`, `media`, `producers`, `ratings`, `roles`,
`writers`, `ultraBlurColors?`.

Derived: `episodeNumber` (alias `index`); `seasonNumber` (alias
`parentIndex`, falling back to `_season.index`); `seasonEpisode`
(`s00e00`); marker predicates (`hasCommercialMarker`,
`hasIntroMarker`, `hasCreditsMarker`); media predicates
(`hasVoiceActivity`, `hasPreviewThumbnails`).

### 2.6 `Clip` — `video.py:1272–1290`

Minimal `Video` subtype for trailers, extras, Live TV.

`addedAt : datetime` (re-parsed), `duration : int`,
`extraType : int`, `index : int`,
`originallyAvailableAt : date`, `skipDetails : int`,
`subtype : str` (`"trailer"`, `"behindTheScenes"`, `"sceneOrSample"`, …),
`thumbAspectRatio : str`, `viewOffset : int = 0`, `year : int`,
`media : List<Media>`.

`Extra` (`video.py:1313`) is a thin subclass that copies the
parent's `librarySection*` after load.

### 2.7 `Audio` (base) — `audio.py:55–92`

Same shape as `Video` but `listType = "audio"`, plus:
`distance : float ← @distance` (sonic-similarity queries only),
`index : int ← @index`,
`musicAnalysisVersion : int ← @musicAnalysisVersion`,
`moods : List<Mood> ← <Mood>`.

### 2.8 `Artist` — `audio.py:201–244`

Strips `/children` from `key`.
`albumSort : int = -1`, `audienceRating : float`,
`rating : float`, `theme : str`.

Child collections: `collections`, `countries`, `genres`, `guids`,
`labels`, `locations: List<str>` (via `listAttrs`), `similar`,
`styles`, `ultraBlurColors?`.

### 2.9 `Album` — `audio.py:376–425`

`audienceRating : float`, `leafCount : int`,
`loudnessAnalysisVersion : int`,
`originallyAvailableAt : date`,
`parentGuid : str`, `parentKey : str`, `parentRatingKey : int`,
`parentTheme : str`, `parentThumb : str`, `parentTitle : str`,
`rating : float`, `studio : str`,
`viewedLeafCount : int`, `year : int`.

Child collections: `collections`, `formats`, `genres`, `guids`,
`labels`, `styles`, `subformats`, `ultraBlurColors?`.

### 2.10 `Track` — `audio.py:537–588`

`Playable`. `parentIndex` is the disc number; `index` (from `Audio`)
is the track number within the disc.

`audienceRating : float`, `chapterSource : str`, `duration : int`,
`grandparentArt : str`, `grandparentGuid : str`,
`grandparentKey : str`, `grandparentRatingKey : int`,
`grandparentTheme : str`, `grandparentThumb : str`,
`grandparentTitle : str`,
`originalTitle : str` (track-level artist override),
`parentGuid : str`, `parentIndex : int (disc)`,
`parentKey : str`, `parentRatingKey : int`,
`parentThumb : str`, `parentTitle : str`,
`primaryExtraKey : str`, `rating : float`, `ratingCount : int`,
`skipCount : int`, `sourceURI : str`,
`viewOffset : int`, `year : int`.

Child collections: `chapters`, `collections`, `genres`, `guids`,
`labels`, `media`. `trackNumber` aliases `index`.

### 2.11 `Photoalbum` — `photo.py:46–74`

Does **not** call any super-class loader; standalone `_loadData`.
Strips `/children` from `key`. Not `Playable`.

`addedAt : datetime`, `art : str`, `composite : str`,
`guid : str`, `index : int`, `key : str`,
`lastRatedAt : datetime`,
`librarySectionID : int`, `librarySectionKey : str`,
`librarySectionTitle : str`,
`listType : str = "photo"`, `ratingKey : int`,
`summary : str`, `thumb : str`, `title : str`,
`titleSort : str`, `type : str`, `updatedAt : datetime`,
`userRating : float`, `fields : List<Field>`,
`images : List<Image>`.

Recursive: `/children` returns nested `Photoalbum`, `Photo`, **and
`Clip`** elements (`photo.py:104–116`). Photo libraries can hold
video.

### 2.12 `Photo` — `photo.py:197–243`

Composes `Playable` and calls `Playable._loadData` first. No
`Video`/`Audio` base.

`addedAt : datetime`, `createdAtAccuracy : str`,
`createdAtTZOffset : int`, `guid : str`, `index : int`,
`key : str`, `lastRatedAt : datetime`,
`librarySectionID : int`, `librarySectionKey : str`,
`librarySectionTitle : str`, `listType : str = "photo"`,
`originallyAvailableAt : date`, `parentGuid : str`,
`parentIndex : int`, `parentKey : str`,
`parentRatingKey : int`, `parentThumb : str`,
`parentTitle : str`, `ratingKey : int`, `sourceURI : str`,
`summary : str`, `thumb : str`, `title : str`,
`titleSort : str`, `type : str`, `updatedAt : datetime`,
`userRating : float`, `year : int`,
`fields : List<Field>`, `images : List<Image>`,
`media : List<Media>`, `tags : List<Tag>`.

---

## 3. The `Playable` surface

`Playable` (`base.py:836–1012`) is a cooperative mixin that adds
state (`playlistItemID`, `playQueueItemID`) plus behaviour. Only the
five leaf playables compose it: **Movie, Episode, Clip, Track,
Photo**. `getStreamURL` (`base.py:851–891`) explicitly rejects
anything not in `("movie","episode","track","clip")` — so `Photo`
inherits the method but can never call it successfully; downloads
go via `part.key + ?download=1` instead.

| Method | Source | On |
|---|---|---|
| state: `playlistItemID`, `playQueueItemID` | `base.py:848` | five leaves |
| `getStreamURL(**kw)` | `base.py:851` | Movie, Episode, Clip, Track (Photo raises) |
| `iterParts()` | `base.py:893` | five leaves |
| `videoStreams()` / `audioStreams()` / `subtitleStreams()` / `lyricStreams()` | `base.py:899–921` | five leaves |
| `play(client)` | `base.py:923` | five leaves |
| `download(savepath, …)` | `base.py:931` | five leaves |
| `updateProgress(time, state)` | `base.py:980` | five leaves |
| `updateTimeline(time, state, duration)` | `base.py:996` | five leaves |

`PlayedUnplayedMixin` (`mixins/played_unplayed.py:1–34`) sits one
layer up — `Video` and `Audio` inherit it (`video.py:11`,
`audio.py:20`), so **every video and audio leaf** can
`markPlayed()` / `markUnplayed()` even containers (Show, Season,
Artist, Album). `Photoalbum` and `Photo` are excluded.

`RatingMixin` (`mixins/rating.py:7`) adds `rate(value)` to every
leaf except `Clip` — Clip's composite mixin is the minimal
`ClipMixins` (`mixins/__init__.py:164`).

There is **no `setIntroMarker`** / **no `setCreditsMarker`** in this
SDK. `Marker` objects are read-only; only the policy field
`enableCreditsMarkerGeneration` can be mutated (via
`editAdvanced()`).

`PlexSession` adds `live`, `sessionKey`, `_username`, `_userId`,
`usernames`, cached `player` / `session` / `transcodeSession`, and
`stop(reason)` (`base.py:1015–1105`). `PlexHistory` adds
`accountID`, `deviceID`, `historyKey`, `viewedAt`; cannot be
reloaded; has `source()` and `delete()` (`base.py:1108–`).

---

## 4. `Media` / `MediaPart` / `Stream` chain

Every playable leaf has `media: List<Media>` — 1..N **versions**
(e.g. 4K + 1080p + Plex-optimized). Each `Media` has
`parts: List<MediaPart>` — 1..N **files**. Each `MediaPart` has a
heterogeneous `streams: List<MediaPartStream>` (Video / Audio /
Subtitle / Lyric, dispatched on the `streamType` attribute).

XML:

```xml
<Video ratingKey="..." type="movie">
  <Media id="1" videoResolution="1080" container="mkv" ...>
    <Part id="42" file="/movies/Foo.mkv" container="mkv" ...>
      <Stream id="100" streamType="1" codec="h264" .../>  <!-- Video -->
      <Stream id="101" streamType="2" codec="dts" channels="6" .../>  <!-- Audio -->
      <Stream id="102" streamType="3" codec="srt" language="English" .../>  <!-- Subtitle -->
    </Part>
  </Media>
  <Media id="2" videoResolution="sd" ...> ... </Media>
  <Genre tag="Action"/> <Director tag="..."/> <Guid id="imdb://tt..."/>
</Video>
```

Dispatch uses `(elem.tag, streamType)` looked up via
`utils.registerPlexObject` — keys `Stream.1`, `Stream.2`, `Stream.3`,
`Stream.4` (`base.py:127–144`).

**`Media`** scalars (`media.py:50–88`): `aspectRatio`,
`audioChannels`, `audioCodec`, `audioProfile`, `bitrate`,
`container`, `duration`, `height`, `id`, `has64bitOffsets`,
`hasVoiceActivity`, `optimizedForStreaming`, `proxyType`, `selected`,
`target`, `title`, `videoCodec`, `videoFrameRate`, `videoProfile`,
`videoResolution`, `width`, `uuid`. Photo-only: `aperture`,
`exposure`, `iso`, `lens`, `make`, `model`. Cached
`parts: List<MediaPart>`. Caches parent's `key` as `_parentKey` for
`delete()`. `isOptimizedVersion` = `proxyType ==
SEARCHTYPES['optimizedVersion']`; `delete()` removes *this version*.

**`MediaPart`** (`media.py:139–163`): `accessible`, `audioProfile`,
`container`, `decision`, `deepAnalysisVersion`, `duration`,
`exists`, `file`, `has64bitOffsets`, `hasThumbnail`, `id`, `indexes`,
`key`, `optimizedForStreaming`, `packetLength`, `protocol`,
`requiredBandwidths`, `selected`, `size`, `streams` (built by
`findItems(data)` — heterogeneous), `syncItemId`, `syncState`,
`videoProfile`. Filters (lazy, in-memory):
`videoStreams()`/`audioStreams()`/`subtitleStreams()`/`lyricStreams()`.
`hasPreviewThumbnails` = `indexes == "sd"`.

**`MediaPartStream`** base (`media.py:233–275`): `bitrate`, `codec`,
`decision`, `default`, `displayTitle`, `extendedDisplayTitle`,
`id`, `index = -1`, `key`, `language`, `languageCode`,
`languageTag`, `location`, `requiredBandwidths`,
`selected = False`, `streamType`, `title`, `type` (alias).

**`VideoStream`** (`media.py:279–357`) adds the full
HDR/Dolby-Vision pile: `anamorphic`, `bitDepth`, `cabac`,
`chromaLocation`, `chromaSubsampling`, `codecID`, `codedHeight`,
`codedWidth`, `colorPrimaries`, `colorRange`, `colorSpace`,
`colorTrc`, `DOVIBLCompatID`, `DOVIBLPresent`, `DOVIELPresent`,
`DOVILevel`, `DOVIPresent`, `DOVIProfile`, `DOVIRPUPresent`,
`DOVIVersion`, `duration`, `frameRate`, `frameRateMode`,
`hasScalingMatrix`, `height`, `level`, `profile`,
`pixelAspectRatio`, `pixelFormat`, `refFrames`, `scanType`,
`streamIdentifier`, `width`.

**`AudioStream`** (`media.py:361–431`) adds: `audioChannelLayout`,
`bitDepth`, `bitrateMode`, `channels`, `duration`, `profile`,
`samplingRate`, `streamIdentifier`, `visualImpaired`. Track-only:
`albumGain`, `albumPeak`, `albumRange`, `endRamp`, `gain`,
`loudness`, `lra`, `peak`, `startRamp`.

**`SubtitleStream`** (`media.py:435–477`): `canAutoSync`,
`container`, `forced = False`, `format`, `headerCompression`,
`hearingImpaired = False`, `perfectMatch`, `providerTitle`, `score`,
`sourceKey`, `transient`, `userID`.

**`LyricStream`** (`media.py:481–501`): `format`, `minLines`,
`provider`, `timed = False`.

---

## 5. `Marker`s and `Chapter`s

Both are children of the item element; they are **not**
interchangeable.

**`Chapter`** (`media.py:1109–1141`) — editorial navigation points:
`end ← @endTimeOffset (ms)`, `filter`, `id`, `index`,
`tag` (chapter name), `title` (alias), `thumb`,
`start ← @startTimeOffset (ms)`. Surface: `chapters` on `Movie`,
`Episode`, `Track`. Parent carries `chapterSource` (`"agent"` /
`"media"` / `"mixed"`).

**`Marker`** (`media.py:1144–1186`) — algorithmically-detected
intervals: `end ← @endTimeOffset`, `final : bool` (true for the last
credits marker), `id`, `type` (`"intro"` / `"credits"` /
`"commercial"`), `start ← @startTimeOffset`, `version` (parsed from
`<Attributes version="…"/>` child). Surface: `markers` on `Movie`
and `Episode`. Predicates: `Episode.hasCommercialMarker` /
`hasIntroMarker` / `hasCreditsMarker`; `Movie.hasCreditsMarker`.
`Marker.first` (`media.py:1177–1186`) returns true if this marker
is the earliest `credits` marker on the parent
(mid-credits vs. post-credits distinction).

There is **no marker-mutation surface** in this SDK. To affect
marker generation use `editAdvanced(enableCreditsMarkerGeneration=…)`
on a Movie or Show (`mixins/advanced_settings.py:29`). The Plex
server emits the marker rows; the SDK only reads them.

---

## 6. Mutating operations matrix

Writes split into three layers:

1. **Raw `edit(**kwargs)`** on every `PlexPartialObject`
   (`base.py:712`). Takes Plex keys like `title.value`,
   `title.locked`, `collection[0].tag.tag`.
2. **Per-field mixins** in `mixins/edit.py`: `editTitle`,
   `editSummary`, `editStudio`, `editAddedAt`, `editTagline`,
   `editOriginallyAvailable`, etc. Each wraps `_edit` with a
   specific field name and a `locked` flag.
3. **Per-tag mixins** wrapping `editTags`: `addCollection`,
   `removeCollection`, `addGenre`, `addLabel`, `addCountry`,
   `addDirector`, `addProducer`, `addWriter`, `addMood`, `addStyle`,
   `addTag`, `addSimilarArtist`, etc.

Matrix derived from `mixins/__init__.py:34–223`. `✓` = supported;
`—` = explicitly not composed; `url` = read-only image surface.

| Op | Movie | Show | Season | Episode | Clip | Artist | Album | Track | Photoalbum | Photo |
|---|---|---|---|---|---|---|---|---|---|---|
| `edit(**kw)` (raw) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `editTitle` / `editSortTitle` | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ | ✓ | ✓ |
| `editSummary` | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | — | ✓ | ✓ |
| `editTagline` | ✓ | ✓ | — | — | — | — | — | — | — | — |
| `editStudio` | ✓ | ✓ | — | — | — | — | ✓ | — | — | — |
| `editEditionTitle` | ✓ | — | — | — | — | — | — | — | — | — |
| `editContentRating` | ✓ | ✓ | — | ✓ | — | — | — | — | — | — |
| `editOriginalTitle` | ✓ | ✓ | — | — | — | — | — | — | — | — |
| `editTrackArtist` / `editTrackNumber` / `editDiscNumber` | — | — | — | — | — | — | — | ✓ | — | — |
| `editAudienceRating` / `editCriticRating` / `editUserRating` | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ | ✓ | ✓ |
| `editOriginallyAvailable` | ✓ | ✓ | — | ✓ | — | — | ✓ | — | — | — |
| `editAddedAt` | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ | ✓ | ✓ |
| `editCapturedTime` | — | — | — | — | — | — | — | — | — | ✓ |
| `addCollection` / `removeCollection` | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ | — | — |
| `addCountry` | ✓ | — | — | — | — | ✓ | — | — | — | — |
| `addDirector` | ✓ | — | — | ✓ | — | — | — | — | — | — |
| `addGenre` | ✓ | ✓ | — | — | — | ✓ | ✓ | ✓ | — | — |
| `addLabel` | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ | — | — |
| `addMood` | — | — | — | — | — | ✓ | ✓ | ✓ | — | — |
| `addProducer` | ✓ | — | — | — | — | — | — | — | — | — |
| `addSimilarArtist` | — | — | — | — | — | ✓ | — | — | — | — |
| `addStyle` | — | — | — | — | — | ✓ | ✓ | — | — | — |
| `addTag` | — | — | — | — | — | — | — | — | — | ✓ |
| `addWriter` | ✓ | — | — | ✓ | — | — | — | — | — | — |
| `editAdvanced` | ✓ | ✓ | ✓ | — | — | ✓ | — | — | — | — |
| `split()` / `merge(ratingKeys)` | ✓ | ✓ | — | — | — | ✓ | ✓ | — | — | — |
| `unmatch` / `matches` / `fixMatch` | ✓ | ✓ | — | — | — | ✓ | ✓ | — | — | — |
| `markPlayed` / `markUnplayed` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | — | — |
| `rate(value)` | ✓ | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ | ✓ | ✓ |
| `uploadPoster` / `setPoster` / `posters()` | ✓ | ✓ | ✓ | ✓ | url | ✓ | ✓ | url | ✓ | url |
| `uploadArt` / `setArt` / `arts()` | ✓ | ✓ | ✓ | ✓ | url | ✓ | ✓ | url | ✓ | url |
| `uploadLogo` / `logos()` | ✓ | ✓ | ✓ | ✓ | url | ✓ | ✓ | url | ✓ | url |
| `uploadSquareArt` / `squareArts()` | ✓ | ✓ | ✓ | ✓ | url | ✓ | ✓ | url | ✓ | url |
| `uploadTheme` / `themes()` | ✓ | ✓ | url | url | — | ✓ | url | url | — | — |
| `analyze()` / `refresh()` / `delete()` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `uploadSubtitles` / `searchSubtitles` / `downloadSubtitles` / `removeSubtitles` | ✓ | ✓ | ✓ | ✓ | ✓ | — | — | — | — | — |
| `addToWatchlist` / `removeFromWatchlist` | ✓ | ✓ | — | — | — | — | — | — | — | — |

`split` / `merge` and `unmatch` / `fixMatch` only apply to types
Plex's agents identify: Movie, Show, Artist, Album.
`uploadSubtitles` lives on `Video` so technically every video class
exposes it; in practice it is meaningful only for Movie and Episode.

---

## 7. Stream selection

No top-level `selectStream`. Selection lives on `MediaPart`:

- `MediaPart.setSelectedAudioStream(stream)` — `media.py:190`. PUTs
  `/library/parts/{id}?allParts=1&audioStreamID=…`.
- `MediaPart.setSelectedSubtitleStream(stream)` — `media.py:207`.
- `MediaPart.resetSelectedSubtitleStream()` — `media.py:224`
  (PUTs `subtitleStreamID=0`).
- `AudioStream.setSelected()` / `SubtitleStream.setSelected()` —
  `media.py:416,473`. Convenience wrappers that call the parent.

The `allParts=1` flag means "apply across every Part in this
version". There is no separate `setDefault*` — Plex collapses the
concept into the `selected` attribute on each stream.

**Language picking** is not first-class. The pattern is:

```python
en = next(s for s in part.audioStreams() if s.languageCode == "eng")
part.setSelectedAudioStream(en)
```

Per-item **preferences** live on the container (`Show.audioLanguage`,
`Show.subtitleLanguage`, `Show.subtitleMode`; same on `Season`) and
are mutated via `editAdvanced` / `edit(**kw)`, not the stream API.

**Subtitle search/download/upload** on `Video` (`video.py:100–182`):

- `searchSubtitles(language, hearingImpaired, forced)` → list of
  on-demand `SubtitleStream` candidates.
- `downloadSubtitles(subtitleStream)` → PUTs
  `{video.key}/subtitles?key={stream.key}`.
- `uploadSubtitles(filepath)` → multipart POST.
- `removeSubtitles(stream | streamID | streamTitle)` → DELETE.

---

## 8. External providers / GUIDs

Two distinct guid concepts:

1. **Item-level `guid`** — the `@guid` string on every item.
   New-style: `plex://movie/<hash>`, `plex://show/<hash>`,
   `plex://artist/<hash>`. Legacy: `com.plexapp.agents.imdb://tt…`.
   `Movie.editions()` (`video.py:515`) uses this guid to find other
   library entries pointing at the same canonical work
   (`filters = {'guid': self.guid, 'id!': self.ratingKey}`).
2. **External provider GUIDs** — `<Guid id="…"/>` children. Parsed
   into `media.Guid` objects (`media.py:937–948`) carrying only
   `id`. Typical ids: `imdb://tt0944947`, `tmdb://1399`,
   `tvdb://121361`, `mbid://…`. Exposed as `guids` on Movie, Show,
   Season, Episode, Artist, Album, Track.

The library-level helper is `LibrarySection.getGuid(guid)`
(`library.py:643`): searches by `guid=…` first, then falls back to
calling `matches(...)` to derive a Plex GUID via the agent and
searching by that. Examples
(`library.py:659–662`):

```text
library.getGuid('plex://show/5d9c086c46115600200aa2fe')
library.getGuid('imdb://tt0944947')
library.getGuid('tmdb://1399')
library.getGuid('tvdb://121361')
```

The SDK does not parse the `scheme://id` split of provider GUIDs;
that is left to the caller.

---

## 9. Image surfaces (posters / arts / logos / square arts / themes)

Five resources, identical shape, each backed by a `BaseResource`
subclass (`media.py:1039–1105`).

| Resource | Class | List endpoint | Upload |
|---|---|---|---|
| Poster | `media.Poster` | `/library/metadata/{rk}/posters` | `POST .../posters[?url=…]` |
| Art | `media.Art` | `/library/metadata/{rk}/arts` | `POST .../arts[?url=…]` |
| Logo (clearLogo) | `media.Logo` | `/library/metadata/{rk}/clearLogos` | `POST .../clearLogos[?url=…]` |
| Square art | `media.SquareArt` | `/library/metadata/{rk}/squareArts` | `POST .../squareArts[?url=…]` |
| Theme | `media.Theme` | `/library/metadata/{rk}/themes` | `POST .../themes[?url=…]` |

`BaseResource._loadData` parses `key`, `provider` (`"local"` /
`None` / agent id), `ratingKey` (a `media://` / `metadata://` /
`upload://` URL), `selected`, `thumb`. `BaseResource.select()` PUTs
to the listing endpoint with `?url=<ratingKey>` (`media.py:1060`).

Mixins (`mixins/resources.py`) give each resource the same quartet:
`posters()` / `uploadPoster()` / `setPoster()` / `deletePoster()`,
plus `lockPoster()` / `unlockPoster()` that do
`edit(thumb.locked=1)`-style updates. The thin `*UrlMixin` variants
(`PosterUrlMixin`, `ArtUrlMixin`, etc.) expose only
`posterUrl`/`artUrl`/`logoUrl`/`squareArtUrl`/`themeUrl` properties
— no list, no upload. These are what `Clip`, `Track`, `Photo` get.

Note: `logoUrl` is computed from `self.images` (looks for an
`<Image type="clearLogo"/>`) (`mixins/resources.py:71–79`); the
others read the `thumb` / `art` / `theme` attributes directly.
Uploaded posters get a `ratingKey` prefixed with `upload://`;
agent-supplied ones have a non-null `provider`.

---

## 10. Edge cases

**Multi-edition movies.** `Movie.editionTitle` carries strings like
"Director's Cut". Two `Movie` entries with the same `guid` but
different `ratingKey` are two editions — each is its own top-level
`Movie`. `Movie.editions()` (`video.py:515`) finds them via
`{'guid': self.guid, 'id!': self.ratingKey}`. This is a genuinely
multi-rooted graph.

**Hidden seasons.** With `Show.flattenSeasons=1`, Plex omits
`parentKey` / `parentRatingKey` / `parentThumb` from Episode XML.
`Episode._loadData` stashes raw values in `_parentKey` etc. and
overrides the public properties to back-fill via `_season`, which
queries the server (`video.py:1105–1146`). This is the only
attribute access in the media parser that can trigger network I/O.

**FIX_BUG_50.** Plex returns container keys with a trailing
`/children`. Stripped inside `_loadData` for `Show`
(`video.py:623`), `Season` (`video.py:832`), `Artist`
(`audio.py:206`), `Album` (`audio.py:380`), `Photoalbum`
(`photo.py:53`). Failing to mirror this normalisation in Rust will
produce doubled-up URL segments on reload/edit calls.

**Clips (Live TV / DVR).** `Clip` is the workhorse for Live TV,
recordings, and trailers. Very thin: has `media` but no `markers`,
no `chapters`, no `guids`, no `genres`, no `roles`. `subtype`
distinguishes (`"trailer"`, `"behindTheScenes"`, `"sceneOrSample"`,
…); `extraType` is the same idea as int. `Extra` (`video.py:1313`)
subclasses `Clip` and back-fills `librarySection*` from the parent
because trailer XML doesn't include them.

**Multi-disc albums.** `Track.parentIndex` = disc number;
`Track.index` (from `Audio`) = track number within the disc.
`Album.tracks()` returns the flat list in disc-major / track-minor
order; no grouping.

**Photoalbum recursion.** A `Photoalbum`'s `/children` listing
returns a mixed container of `Photoalbum`, `Photo`, **and `Clip`**
(`photo.py:104–116`). Photo libraries can hold video.

**Session / History variants.** Five leaves have a `*Session`
shape, four have `*History`:
Movie/Episode/Clip/Track/Photo (sessions); Movie/Episode/Clip/Track
(history). Each `*Session._loadData` chains
`Leaf._loadData(self,data)` then `PlexSession._loadData(self,data)`
(e.g. `video.py:1338`). Same for history.

**`CommonSenseMedia`.** Separate metadata pile keyed by Plex GUID
(`media.py:1342–1400`). Two-stage: small `ageRatings` payload ships
with the Movie/Show; `.reload()` fetches the full report
(`anyGood`, `oneLiner`, `parentsNeedToKnow`,
`parentalAdvisoryTopics`, `talkingPoints`) from the user's account
metadata service (Plex Pass required). Only Movie and Show parse it.

**`UltraBlurColors`.** Four hex strings (`topLeft`, `topRight`,
`bottomLeft`, `bottomRight`) used by Plex clients for the "ultra
blur" background gradient. Present on every top-level item type.

**`distance` on `Audio`.** Populated only when the item comes from
`sonicallySimilar()` (`audio.py:143`); `None` otherwise. Don't model
as required.

---

## Rust modelling notes

The SDK's dynamic-Python shape is fine for the source language but
hostile to an idiomatic Rust port. The decisions below are the
non-trivial ones — places where straight translation produces bad
ergonomics or unsound types.

### A. Library item: sum type, not trait object

Ten leaves share a broad common surface (title, summary,
`ratingKey`, …) but each carries fields the others don't. A
`Box<dyn LibraryItem>` forces callers into downcast acrobatics. Use:

```rust
enum LibraryItem {
    Movie(Movie),      Show(Show),       Season(Season),
    Episode(Episode),  Clip(Clip),
    Artist(Artist),    Album(Album),     Track(Track),
    Photoalbum(Photoalbum),               Photo(Photo),
}
```

Discriminator: the XML `(TAG, TYPE)` pair, which already drives
`registerPlexObject` (e.g. `Movie.TAG='Video'`, `Movie.TYPE='movie'`
— `video.py:382–383`). For session/history variants, prefer
composition over an enum explosion:

```rust
struct Session { common: SessionCommon, item: LibraryItem }
struct History { common: HistoryCommon, item: LibraryItem }
```

### B. Container vs. playable: two traits, not one

`Show`, `Season`, `Artist`, `Album`, `Photoalbum` have no
`media`/`parts`/`streams` but can still be rated, played-marked,
edited, refreshed. Don't unify them with playable leaves under a
single trait that includes `get_stream_url`. Split:

- `trait LibraryItemRef`: `rating_key`, `key`, `title`,
  `section_id`, `refresh`, `delete`, `mark_played`, `mark_unplayed`,
  `rate`, `edit`, `posters`, `arts`.
- `trait Playable: LibraryItemRef`: `get_stream_url`, `play`,
  `download`, `update_progress`, `iter_parts`, stream getters.

`Photo` is `Playable` in the SDK but `getStreamURL` rejects at
runtime (`base.py:862`). In Rust, *don't* implement `Playable` on
`Photo`; give it `download()` separately. Turn the Python runtime
error into a compile-time absence.

### C. Optional everywhere; default-back where the SDK does

Nearly every scalar is nullable in XML. `Option<T>` is unavoidable.
Mitigations:

- Match the SDK's defaulted ints (`viewCount=0`,
  `enableCreditsMarkerGeneration=-1`, `useOriginalTitle=-1`,
  `albumSort=-1`, `subtitleMode=-1`) at parse time so callers see
  `i32` not `Option<i32>` for tri-state flags.
- `originallyAvailableAt` is date-only (`%Y-%m-%d`) — use
  `NaiveDate`, not `DateTime`.
- All Unix-second timestamps (`addedAt`, `updatedAt`, `viewedAt`,
  `lastViewedAt`, `lastRatedAt`) → `Option<DateTime<Utc>>`.
- All `*Offset`, `duration`, `start`, `end`, `viewOffset` are
  milliseconds. Either `std::time::Duration` (lossy to ms) or a
  newtype `Ms(u64)` for unit clarity.

### D. Discard the DOM after parse

Python retains `self._data` (the `xml.etree` element) to lazily
build child collections. Rust should not. Walk the XML once with
`quick-xml` streaming, build owned structs, drop the buffer. The
partial-vs-full dichotomy is real but should be modelled with an
explicit `MaybePartial<T>` wrapper or `ensure_full(&mut self)`
method — not via the silent `__getattribute__` reload hack
(`base.py:634–652`), which is a footgun we don't have to reproduce.

### E. `Media`/`Part`/`Stream` — clean nested structs

The simplest sub-model:

```rust
struct Media   { id: i64, parts: Vec<MediaPart>, /* §4 scalars */ }
struct MediaPart { id: i64, streams: Vec<Stream>, /* §4 scalars */ }
enum Stream {
    Video(VideoStream), Audio(AudioStream),
    Subtitle(SubtitleStream), Lyric(LyricStream),
}
```

`streamType` (1/2/3/4) is the discriminator. Heterogeneous
`Vec<Box<dyn Stream>>` is unambiguously worse than the enum here.

### F. Collapse the 14 `MediaTag` subclasses

All `MediaTag` children share `_loadData` (`media.py:735–759`) and
differ only in `TAG`/`FILTER` constants. Don't replicate the
explosion. Either:

```rust
struct Tag { kind: TagKind, id: Option<i64>, tag: String,
             role: Option<String>, tag_key: Option<String>,
             thumb: Option<String>, filter: Option<String>,
             key: Option<String> }
enum TagKind { Collection, Country, Director, Format, Genre, Label,
               Mood, Producer, Role, Similar, Style, Subformat,
               Tag, Writer }
```

…and expose `Movie::genres()` as
`self.tags.iter().filter(|t| t.kind == TagKind::Genre)`. Or, if
type-level enforcement matters, use thin newtype wrappers
(`struct Genre(Tag)`) — but pick one strategy. Same logic collapses
the five `BaseResource` subclasses (Art / Logo / Poster / SquareArt
/ Theme) into one `Resource { kind: ResourceKind, … }`. The
list-endpoint URL substring (`posters`, `arts`, `clearLogos`,
`squareArts`, `themes`) becomes a method on `ResourceKind`.

### G. Promote `Guid` to a parsed enum

`Guid.id` is always `scheme://id`. Parse at decode time:

```rust
enum GuidRef {
    Plex(String),       // plex://movie/...
    Imdb(String),       // imdb://tt...
    Tmdb(String),       // tmdb://12345
    Tvdb(String),       // tvdb://12345
    MusicBrainz(String),// mbid://...
    Other { scheme: String, id: String },
}
```

…and store `guids: Vec<GuidRef>`. Makes `getGuid(&GuidRef::Imdb(…))`
ergonomic; avoids string-bashing on call sites.

### H. Marker: enum the type field, keep `Chapter` simple

`Marker.type` is a string in XML. Promote:

```rust
enum MarkerKind { Intro, Credits, Commercial, Other(String) }
```

…and keep the `Other(String)` arm for forward-compat. `Chapter`
stays as a plain struct. `Marker.first` (the
"is this the earliest credits marker?" property) becomes a free
function `is_first_credits(&marker, &all_markers)`.

### I. Mixin matrix → trait composition

Don't put the union of all editor methods on every type. Reflect
the §6 matrix with marker traits:

```rust
trait CanRate         { fn rate(&self, value: f32) -> Result<()>; }
trait CanEditTitle    { fn edit_title(&self, t: &str, locked: bool) -> Result<()>; }
trait CanAddCollection{ fn add_collection(&self, names: &[&str]) -> Result<()>; }
trait CanSplit        { fn split(&self) -> Result<()>;
                        fn merge(&self, rks: &[i64]) -> Result<()>; }
trait CanUploadPoster { fn upload_poster_url(&self, url: &str) -> Result<()>; … }
```

Implement per concrete leaf. This is what `MovieMixins` /
`ShowMixins` / etc. do via MRO; trait impls give better IDE
completion and stop you e.g. calling `addProducer` on a `Track`.

### J. Parent context, not parent backrefs

Several types (`Media`, `MediaTag`, `Marker`, `BaseResource`) reach
back to their parent in `_loadData` via a `weakref` to copy
`librarySectionID` / compute `key` / read `_parentType`. Rust
shouldn't have parent backrefs. Pass parent context through at
parse time:

```rust
fn parse_marker(elem: &Element, ctx: &ParentCtx) -> Marker { … }
struct ParentCtx<'a> {
    library_section_id: Option<i64>,
    parent_key: &'a str,
    parent_type: &'a str,
}
```

This keeps `Marker`, `Tag`, `Media`, `BaseResource` owned and
self-contained.

### K. Error surface: model `Unsupported` as `impl` absence

The SDK raises `Unsupported` at runtime when you call e.g.
`Photo.getStreamURL()` (`base.py:862`). Use trait splits from (B)
to make this a compile error: `get_stream_url` only exists on
`impl Playable for {Movie, Episode, Clip, Track}`. Define
`PlexError` with variants for HTTP non-2xx, not-found, XML parse
failure, and a residual `Unsupported(&'static str)` for the few
runtime-only checks (e.g. attempting to upload posters when the
library doesn't allow it).

### L. Locked semantics

Almost every editor takes a `locked: bool` controlling whether the
field is locked against agent overwrites. This is not "is set"; it
is a separate piece of state. Keep it on the editor method
signature, not the data struct. The read-side `fields: Vec<Field>`
list (each `Field` carries `{name, locked}`, `media.py:1190–1203`)
remains as-is for inspection.

