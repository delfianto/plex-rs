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
