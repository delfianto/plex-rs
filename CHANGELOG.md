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
