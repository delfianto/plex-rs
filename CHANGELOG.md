# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
once it reaches 1.0. Pre-1.0 the minor version may contain breaking changes;
each breaking change is listed under **Breaking** in its release entry.

## [Unreleased]

### Added
- Project bootstrap: `Cargo.toml`, `rust-toolchain.toml`, `deny.toml`, CI
  workflow, lint baseline in `src/lib.rs`.
- `CLAUDE.md` — contributor guide and project charter.
- `analysis/` — deep-dive notes on the `python-plexapi` reference
  implementation that this crate targets for feature parity.
- `TRACKER.md` — milestone-by-milestone implementation tracker.
- **M0 (Foundations)** — full HTTP transport layer plus shared primitives:
  - `error` — `Error` enum, `Result` alias, status-to-error mapping,
    retryability classifier.
  - `util::ids` — `RatingKey`, `MachineIdentifier`, `ClientIdentifier`,
    `PlayQueueId`, `PlexToken` (with redacted `Debug`).
  - `util::time` — Plex epoch-seconds and ISO-date helpers + serde
    adapter for stringified epoch fields.
  - `util::search_type` — full `SearchType` enum mirroring
    `python-plexapi`'s `utils.py:35` table, with forward-compat
    `Unknown(u32)`.
  - `util::sanitize` — fixture sanitiser with 14 regex rules + IPv4/IPv6
    classifier; idempotent.
  - `uri` — `PlexUri` enum covering 7 schemes (`server://`, `library://`,
    `library:///directory/`, `playlist:///`, `/playQueues/...`,
    `https://plex.tv/devices/.../sync_items`, `/security/token`),
    round-trip-stable.
  - `xml` — `MediaContainer<T>` generic envelope collapsing the 12
    `mediaContainerWith*` schemas.
  - `pagination` — `PageRange` + `advance_with()` using the header-based
    `X-Plex-Container-Start/-Size` pagination.
  - `headers` — `PlexIdentity` builder emitting the 10 `X-Plex-*`
    headers + `Accept: application/json`, with strict ASCII validation.
  - `config` — `ClientConfig` builder with timeouts and retry policy
    invariants.
  - `client` — `HttpClient`: JSON-first content negotiation, full-jitter
    exponential backoff retries, status-to-`Error` mapping, token-safe
    `Debug`.
- **M4.2 (Collection — list, items, delete)** — section-attached
  named groupings:
  - `media::Collection` — section-scoped collection with rating
    key, title, subtype (matches owning section kind), smart flag,
    leaf/child counts, collection_mode / collection_sort, composite
    image, thumb, art, timestamps, GUID, and a
    `LibrarySectionRef` back-link.
  - Unlike `Playlist`, Collection IS section-attached — so it
    composes naturally with the M3 trait suite. Implements
    `PlexObject` (metadata type 18), `Ratable`, `EditField`,
    `EditTitle`, `EditSummary`, `EditTags`, `HasGenres`,
    `HasCollections`, `HasLabels`, `HasArtUrl` + `HasArtLock`,
    `HasPosterUrl` + `HasPosterLock` — all the editing surface
    inherited from the foundational traits.
  - `LibrarySection::collections()` — `GET /library/sections/<id>/collections`
    returning `Vec<Collection>`.
  - `Collection::items()` — `GET /library/collections/<rk>/children`
    returning `Vec<LibraryItem>`.
  - `Collection::delete()` — `DELETE /library/collections/<rk>`.
  - Add / remove items, mode / sort tweaks, smart-collection
    mutation defer to follow-up iterations.
  - `tests/m4_collections.rs` — 3 wiremock integration tests
    covering list (static + smart), item walk, and DELETE.

- **M4.1 (Playlist — list, items, delete)** — first piece of the
  playback layer:
  - `media::Playlist` — server-level (not section-attached) ordered
    item collection. Holds the `HttpClient` and base URL directly
    so it can hit `/playlists/<rk>` endpoints. Carries the rating
    key, title, kind, smart flag, content URI (for smart
    playlists), duration, leaf counts, composite image,
    timestamps, GUID.
  - `media::PlaylistKind` enum (`Audio | Video | Photo | Other`)
    discriminating on Plex's `playlistType` wire field.
  - `PlexServer::playlists()` — `GET /playlists` listing all
    playlists on the server.
  - `Playlist::items()` — `GET /playlists/<rk>/items` returning
    `Vec<LibraryItem>` (mixed kinds dispatched on wire `type`).
    The `librarySectionID` Plex emits on each playlist item is
    wired into the per-item `LibrarySectionRef` so future edits
    can route through the right section.
  - `Playlist::delete()` — `DELETE /playlists/<rk>`, consumes
    `self`.
  - Smart playlist creation/mutation, item add/remove/move, and
    rename defer to follow-up iterations — they need
    server-URI construction for the `?uri=` parameter and the
    `playlistItemID` shadow keys (analysis/07 §4).
  - `tests/m4_playlists.rs` — 3 wiremock integration tests
    covering list (mixed static + smart), item walk with section
    back-link, and the DELETE endpoint shape.

- **M3.4/M3.5 expansion (macro-driven trait suite)** — fills out
  the field- and tag-family ergonomic-trait surface using two
  small declarative macros:
  - `declare_edit_field_trait!(TraitName, method_name, wire_field)`
    emits a `trait TraitName: EditField` with a single
    string-typed `method_name(value, locked)` method bound to the
    given wire field name. Used to land `EditTagline`,
    `EditStudio`, `EditContentRating`, `EditSortTitle` (wire form
    `titleSort` — Plex schema inconsistency preserved),
    `EditOriginalTitle`.
  - `EditYear` — hand-written numeric variant (the macro is
    string-only).
  - `declare_tag_trait!(TraitName, replace_fn, remove_fn, wire_field)`
    emits a `trait TraitName: EditTags` with `replace_*` /
    `remove_*` method pair. Used to land `HasDirectors`,
    `HasWriters`, `HasCountries`, `HasProducers`, `HasRoles`,
    `HasLabels`, `HasMoods`, `HasStyles`.
  - Both macros are `#[macro_export]` so downstream crates and
    examples can declare additional traits the same way.
  - Implementor coverage per leaf is now extensive:
    - Movie/Show/Episode: Tagline, Studio, ContentRating,
      SortTitle, OriginalTitle, Year, plus all 8 tag families
      (Genre, Collection, Director, Writer, Country, Producer,
      Role, Label).
    - Album: SortTitle, Studio, Year, Genre, Collection, Label,
      Mood, Style.
    - Artist: SortTitle, Genre, Collection, Label, Mood, Style.
    - Track: SortTitle, OriginalTitle, Genre, Collection.
    - Season: SortTitle only (limited edit surface on the wire).
  - No new tests — the wire-form correctness is already proven
    by `m3_edit_field.rs` and `m3_edit_tags.rs`; macro expansion
    just multiplies the surface.

- **M3.6 (Image URL + lock traits)** — six new traits across three
  image families:
  - `HasArtUrl` / `HasArtLock` — background-art (`art` wire field).
  - `HasPosterUrl` / `HasPosterLock` — poster (`thumb` wire field —
    Plex's confusing wire name for the full poster).
  - `HasThemeUrl` / `HasThemeLock` — theme song (`theme` wire field,
    Show-only).
  - `*Url` traits expose `*_url() -> Result<Option<Url>>` builders
    that resolve against the server base URL.
  - `*Lock` traits add `lock_*()` / `unlock_*()`. These emit just
    `<field>.locked=<0|1>` (no `.value` pair) — Plex's lock-toggle
    wire path differs from regular value edits. Implemented by a
    new `EditField::lock_field(field, locked)` primitive added to
    the foundational edit trait.
  - Implementors:
    - Movie / Show / Season / Episode: art + poster + (Show-only) theme.
    - Artist / Album: art + poster.
    - Track: poster only (Tracks inherit album art on the wire).
  - Full `HasArt` / `HasPoster` CRUD (`set_*` / `upload_*` /
    `delete_*`) needs Plex's `POST /library/metadata/<rk>/<kind>`
    endpoints + `post_bytes()` on the HTTP client; deferred to a
    follow-up iteration.
  - `tests/m3_images.rs` — 4 wiremock integration tests covering
    URL resolution for art + poster and the lock/unlock toggles
    on art. The lock test exposed (and the fix corrected) the
    distinction between value-edit (`<field>.value=` + `<field>.locked=`)
    and lock-only (`<field>.locked=`) wire forms.

- **M3.5 (EditTags + HasGenres + HasCollections)** — tag-family
  mutations:
  - `traits::EditTags` — two low-level primitives:
    - `replace_tags(field, items, locked)` — emits
      `<field>[i].tag.tag=v` per item plus `<field>.locked=<0|1>`,
      replacing the entire list.
    - `remove_tags(field, items, locked)` — emits the magic
      `<field>[].tag.tag-=csv` remove sigil (analysis/08 §3.4),
      stripping the named tags.
  - `traits::HasGenres` / `traits::HasCollections` — first
    per-family ergonomic traits, default-bodied via `EditTags`
    with the right field string baked in. The remaining tag
    families (Director, Writer, Country, Producer, Role, Label,
    Mood, Style) follow the same one-line pattern.
  - Implementors: Movie / Show / Episode (Genre + Collection),
    Album / Track / Artist (Genre + Collection where the wire
    schema supports it).
  - "Add" semantics (read-modify-write — fetch current list,
    prepend new) deferred to the EditBatch transaction in a
    future iteration. For now, callers can compose
    `replace_tags(field, [&existing..., &new..], …)` themselves.
  - `tests/m3_edit_tags.rs` — 3 wiremock integration tests
    covering replace, remove (with the trailing `-` sigil), and
    the collection-family alias.

- **M3.4 (EditField + EditTitle + EditSummary)** — the universal
  metadata-edit primitive:
  - `traits::EditField` — single low-level `edit_field(field,
    value, locked)` method that emits the wire-format URL Plex
    actually expects:
    `PUT /library/sections/<section_id>/all?id=<rating_key>&type=<N>&<field>.value=<v>&<field>.locked=<0|1>`.
    The endpoint is on the *section*, not the metadata item, even
    though the item is what's being edited — see analysis/11 §2.4.
    The `LibrarySectionRef` back-link on every leaf carries the
    `section_id` and (via the `metadata_type_id()` accessor added
    in this commit) the `type` discriminator.
  - `traits::FieldValue` — typed enum (`Str | Int | Float |
    Bool`) with `From` impls for `&str`, `String`, `i64`, `i32`,
    `u32`, `u16`, `f32`, `bool`. Display renders the wire form
    (e.g. `Bool(true)` → `"1"`).
  - `traits::EditTitle` / `traits::EditSummary` — first
    field-specific traits, default-bodied via `EditField`.
    `impl EditTitle for Movie {}` is all a leaf type needs. The
    remaining ~30 field-specific traits (`EditTagline`,
    `EditContentRating`, `EditStudio`, `EditYear`, …) follow the
    same one-line pattern; they land in a follow-up iteration.
  - `PlexObject` gains `section_ref()` (returning
    `&LibrarySectionRef`) and `metadata_type_id()` (returning the
    `?type=N` integer) as required methods; `http()` and
    `base_url()` become default-derived. Every leaf type's
    `impl_plex_object*!` macro invocation now also threads the
    type discriminator.
  - Implementors of `EditField` / `EditTitle` / `EditSummary`:
    Movie / Show / Season / Episode / Artist / Album / Track.
    Photoalbum and Photo are excluded for now — their edit
    surface differs slightly and lands with the photo-specific
    traits.
  - `tests/m3_edit_field.rs` — 3 wiremock integration tests
    proving the section-keyed wire shape, the lock-flag round-trip,
    and percent-encoding of special characters.

- **M3.3 (Ratable trait)** — set / clear the user's personal rating
  on an item:
  - `traits::Ratable` — single `rate(Option<f32>)` method. `None`
    clears (wire sentinel `-1`); `Some(v)` requires
    `v ∈ [0.0, 10.0]` (Plex's 0-to-5-stars × 2 scale).
    Out-of-range values surface as `Error::Config` before any
    HTTP traffic.
  - Wire endpoint: `PUT /:/rate?key=<rating_key>&identifier=com.plexapp.plugins.library&rating=<value>`.
  - Implemented on Movie / Show / Episode / Album / Track.
    Season's rating field is rarely user-set on the wire so the
    impl is intentionally omitted.
  - `tests/m3_ratable.rs` — 3 wiremock integration tests covering
    the happy path, the `None` clear, and client-side range
    validation.

- **M3.1 (Foundational traits + PlayedUnplayed)** — first mutation
  surface and the trait architecture it rides on:
  - `traits::PlexObject` — supertrait every capability trait
    builds on. Three accessors: `http()` → `&HttpClient`,
    `base_url()` → `&Url`, `rating_key()` → `RatingKey`. Implemented
    on Movie / Show / Season / Episode / Artist / Album / Track via
    two small `impl_plex_object*!` macros.
  - `traits::PlayedUnplayed` — `view_count()` reader plus default
    bodies for `is_played()`, `mark_played()`, `mark_unplayed()`.
    The two `mark_*` methods issue `GET /:/scrobble` and
    `/:/unscrobble` respectively with
    `key=<rating_key>&identifier=com.plexapp.plugins.library`. Plex
    requires GET for these despite them being mutations; preserved
    on the wire (analysis/11 §4.10) but exposed as `mark_*` verbs
    on the public surface.
  - Implemented on Movie / Episode / Show / Season / Album /
    Artist / Track — every type that carries a `view_count` field
    on the wire. The trait uses Rust 2024 AFIT (async fn in
    traits) so callers don't need the `async_trait` macro.
  - Inherent `is_played()` methods preserved on Movie/Episode/Track
    alongside the trait so callers don't have to import the trait
    just to read the boolean.
  - `tests/m3_played_unplayed.rs` — 3 wiremock integration tests
    covering the scrobble + unscrobble endpoint shapes plus the
    inherent/trait `is_played` agreement.

- **M2.9 (FilterBuilder)** — typed search expression builder for the
  section-listing surface:
  - `library::FilterBuilder` — fluent, named-op API:
    `.equal()` / `.not_equal()` / `.exact()` / `.not_exact()` /
    `.starts_with()` / `.ends_with()` / `.gt()` / `.lt()` /
    `.and_values()` / `.clause(field, FilterOp, value)`. Plus
    `.sort_by()` / `.sort_by_desc()` / `.limit()` / `.offset()` /
    `.page_size()` / `.libtype()`.
  - `library::FilterOp` enum maps every named op to the canonical
    Plex wire suffix per python-plexapi `library.py:1442-1460`
    (`=`, `!=`, `==`, `!==`, `<=`, `>=`, `>>=`, `<<=`, `&=`).
  - `library::SortDirection` (`Asc | Desc`) renders as
    `field:asc` / `field:desc`.
  - `FilterBuilder::build_query()` emits the URL query string
    suffix with RFC 3986 percent-encoding.
  - `LibrarySection::filter(&builder)` executes the filter
    against `GET /library/sections/<id>/all?<query>` and parses
    the response as `Vec<LibraryItem>`.
  - `src/library.rs` → `src/library/mod.rs`; `filters` is the
    first sub-module.
  - Client-side `__icontains`/`__gte` Python-style suffixes
    deferred to M3.
  - `tests/m2_filter.rs` — 2 wiremock integration tests covering
    the full chain wire form and the empty-builder fallback.

- **M2.8 (LibraryItem + mixed-content listings)** — search and
  curated-list surfaces:
  - `media::LibraryItem` — sum type discriminating on Plex's wire
    `type` field. Nine variants: Movie / Show / Season / Episode /
    Artist / Album / Track / Photoalbum / Photo.
    `LibraryItem::title()` / `rating_key()` hide the variant.
  - `MetadataDto::into_library_item()` performs the dispatch;
    unknown `type` values surface as `Error::Config`.
  - `LibrarySection::search(title)` — `GET /library/sections/<id>/all?title=<q>`
    using a hand-written RFC 3986 percent-encoder (no `url`-crate
    dependency for query construction).
  - `LibrarySection::recently_added()` —
    `GET /library/sections/<id>/recentlyAdded`.
  - `LibrarySection::on_deck()` —
    `GET /library/sections/<id>/onDeck`.
  - `LibrarySection::unwatched()` —
    `GET /library/sections/<id>/unwatched`.
  - All four return `Vec<LibraryItem>` so callers can pattern-match
    on the variant.
  - `tests/m2_search.rs` — 4 wiremock integration tests covering
    title search, mixed-type recently-added, empty on-deck, and
    unknown-`type` error propagation.

- **M2.7 (Read-only media — Markers + Chapters)** — playable-video
  navigation surfaces:
  - `media::Marker` — auto-detected intro/credits/commercial range
    with `start_ms`, `end_ms`, and a `final_credits` flag for the
    end-of-show credits (Plex's post-credits-scene detection).
    `Marker::duration_ms()` and `Marker::contains(time_ms)`
    convenience helpers.
  - `media::MarkerKind` enum (`Intro | Credits | Commercial |
    Other(String)`) — `Other` preserves wire-format strings Plex
    adds later.
  - `media::Chapter` — embedded DVD-style scene index entry with
    optional title, index, end time, and per-chapter thumb.
  - `Movie` and `Episode` gain `markers: Vec<Marker>` and
    `chapters: Vec<Chapter>`. Music and photos don't carry these
    on the wire — left off.

- **M2.6 (Read-only media — Tags)** — `Genre`/`Director`/`Writer`/
  `Country`/`Producer`/`Role`/`Collection`/`Label`/`Mood`/`Style`
  child elements collapsed into a unified `Tag` type:
  - `media::Tag` carries `kind: TagKind`, `value`, optional `id`
    (numeric Plex tag id used by edit operations), `role` and
    `thumb` (for actor `<Role>` entries), and `filter` (the
    smart-filter URI Plex uses for "find more like this").
  - `media::TagKind` enum with all 10 known families plus
    `Other(String)` forward-compat.
  - `Movie`, `Show`, `Episode`, `Album`, `Track` gain
    `tags: Vec<Tag>` populated by `MetadataDto::collect_tags()`.
    `Artist`, `Photo`, `Photoalbum`, `Season` don't carry tags on
    the wire — left out by design.
  - `Field` (per-field edit-lock indicator) intentionally not
    modelled as a `Tag` — different shape, lands with the edit
    traits in M3.

- **M2.5 (Read-only media — Media/Part/Stream chain)** — file-level
  metadata for every playable type:
  - `media::Media` — one re-encode of a playable item (quality /
    container variant). Holds duration, bitrate, dimensions,
    aspect ratio, audio channels + codec, video codec, container,
    frame rate + resolution buckets, optimised-for-streaming flag,
    and a `Vec<MediaPart>` of the underlying files.
  - `media::MediaPart` — one file on disk. Carries the download
    key, filesystem path, size, container, duration,
    has-thumbnail / optimised-for-streaming flags, and a
    `Vec<Stream>` of contained tracks.
  - `media::Stream` — sum type
    `Video(VideoStream) | Audio(AudioStream) | Subtitle(SubtitleStream) | Lyric(LyricStream) | Unknown(UnknownStream)`
    dispatched on Plex's `streamType` discriminator. Per-variant
    fields cover codec, language, dimensions, frame rate, channel
    layout, bitrate, sampling rate, bit depth, default/selected/
    forced flags, display titles, and external-track keys.
  - `Movie`, `Episode`, `Track`, `Photo` gain a
    `media: Vec<Media>` field populated when the source endpoint
    emits `Media[]` (always empty for plain `?type=N` listings;
    populated on `/library/metadata/<rk>` direct fetches).
  - Shared `MetadataDto` learns the wire-format `Media[]` →
    `MediaDto[]` mapping; conversion methods now pre-compute the
    typed chain.

- **M2.4 (Read-only media — Photos)** — Photoalbum / Photo:
  - `media::Photoalbum` — top-level photo container, supports
    nesting. `children()` returns a `PhotoEntry` sum type mixing
    sub-albums and photos; `sub_albums()` / `photos()` filter
    convenience helpers built on top.
  - `media::Photo` — single photo (or video clip in a photo
    section) with parent-album back-reference, EXIF caption,
    capture year, position index, GUID. Width/height land with
    Media/Part/Stream in M2.5.
  - `media::PhotoEntry` — `Album(Photoalbum) | Photo(Photo)` sum
    type for mixed listings.
  - `MetadataDto` gains a `metadata_type` field (renamed from the
    wire `type`) so the photo path can dispatch on
    `photoalbum`/`photo`/`clip` discriminators.
  - `LibrarySection::photoalbums()` — `?type=14` dispatch on
    `SectionKind::Photo`.
  - `tests/m2_photos.rs` — 1 wiremock integration test covering
    the full mixed-children walk and both convenience filters.

- **M2.3 (Read-only media — Music hierarchy)** — Artist / Album / Track:
  - `media::Artist` — top-level music entity with `child_count`
    (number of albums), bio summary, image surface, and
    `Artist::albums()` listing helper.
  - `media::Album` — parent (artist) typed back-reference, year +
    release date, label/studio, leaf_count + viewed_leaf_count,
    rating. `Album::tracks()` lists tracks.
  - `media::Track` — leaf playable with parent (album) +
    grandparent (artist) back-references, index (track within
    disc), `disc_number` (mapped from Plex's `parentIndex`, which
    is intentionally counterintuitive), duration, view count + offset,
    `original_title` for compilation per-track artist, GUID.
    `Track::is_played()` helper.
  - `LibrarySection::artists()` — `?type=8` dispatch on
    `SectionKind::Music`. Shared `list_typed()` boilerplate-eliminator
    from M2.2 reused.
  - `tests/m2_music.rs` — 2 wiremock integration tests covering
    Artist → Album → Track walk and kind-mismatch error path.

- **M2.2 (Read-only media — TV hierarchy)** — Show / Season / Episode:
  - `media::Show` — 24 scalar fields including child_count (seasons),
    leaf_count (total episodes), viewed_leaf_count (played episodes),
    theme path, network/studio. `Show::seasons()` lists seasons via
    `GET /library/metadata/<rk>/children`. `Show::watch_progress()`
    returns `viewed_leaf_count / leaf_count`.
  - `media::Season` — parent_rating_key (typed `RatingKey`),
    parent/show metadata back-link, index (season number),
    leaf_count / child_count / viewed_leaf_count.
    `Season::episodes()` lists episodes via the same `/children`
    endpoint.
  - `media::Episode` — parent (season) and grandparent (show) typed
    back-references, index (episode number), parent_index (season
    number), summary, duration, view-count/offset, full image and
    GUID surface. `Episode::season_episode_label()` returns the
    `S01E03`-style display label.
  - `LibrarySection::shows()` — analogous to `movies()`, dispatches
    on `SectionKind::Show` and queries with `?type=2`. Internal
    `list_typed()` helper eliminates the listing-method
    boilerplate.
  - `tests/m2_tv.rs` — 4 wiremock integration tests covering the
    full Show → Season → Episode walk plus the kind-mismatch error
    path.

- **M2.1 (Read-only media — Movie)** — first content type:
  - `media::Movie` — 24 scalar fields covering Plex's `<Video type="movie">`
    payload (rating_key, title, year, summary, rating triple, duration,
    view count + offset, GUIDs, thumb/art paths, timestamps).
  - `LibrarySection::movies()` lists every movie in a movie section via
    `GET /library/sections/<id>/all?type=1`; returns `Error::Config`
    when called on a non-movie section.
  - `Movie::is_played()`, `Movie::thumb_url()` convenience accessors.
  - `tests/m2_movies.rs` — 2 wiremock integration tests.

- **M1 (Minimum viable client)** — first wire I/O surface, token sign-in
  only:
  - `HttpClient` is now `Clone` (reqwest's underlying client is
    `Arc`-shared, so cloning is cheap).
  - `server::PlexServer` — `connect(url, token)`,
    `connect_with_config()`, `from_http()`, `identity()`, `library()`,
    `ping()`. Eagerly parses `GET /` into `ServerIdentity`.
  - `server::ServerIdentity` — captures machine identifier, version,
    friendly name, platform, MyPlex linkage flags, capabilities. Parses
    Plex's flexible boolean encoding (`"0"`/`"1"`/`0`/`1`/`true`).
  - `library::Library` — bound to a PMS, exposes `sections()`.
  - `library::LibrarySection` — typed section with `SectionKind` enum
    (`Movie | Show | Music | Photo | Other`) and a `LibrarySectionRef`
    back-link for future edit-trait URL construction.
  - `tests/m1_server_library.rs` — 5 wiremock-driven end-to-end tests
    covering identity parsing, 401 surfacing, section listing, and
    edit-URL construction.

[Unreleased]: https://github.com/justdewey/plex-rs/compare/HEAD...HEAD
