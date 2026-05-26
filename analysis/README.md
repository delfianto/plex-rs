# `analysis/` — Reference notes for the `plex-rs` Rust port

This directory holds the **reading notes** that informed the architecture of
`plex-rs`. The source material is two-fold:

1. The **official Plex Media Server OpenAPI 3.1 spec** at the repo root
   (`/openapi.json`, ~1.3 MB).
2. The **`python-plexapi`** reference implementation cloned at
   `/python-plexapi/` (~16 000 lines of Python across 23 modules), which
   `plex-rs` targets for feature parity.

The notes are intentionally dense and prescriptive. Future contributors —
human or Claude — should be able to start a new feature PR with **only**
[`CLAUDE.md`](../CLAUDE.md) plus the relevant doc(s) below in context.

> **Status:** captured 2026-05-27 against `python-plexapi` master and Plex
> Media Server OpenAPI spec version 1.2.2. Both upstreams change; if a
> finding here contradicts current code, trust the code and update the
> note in the same PR.

---

## Reading order

If you're new, read them in this order. Each builds on the previous one.

| # | Doc | Words | What it answers |
| - | --- | ----: | --- |
| 01 | [`01-openapi-overview.md`](./01-openapi-overview.md)         | 4 360 | What does the official OpenAPI spec actually cover? Which tags, paths, schemas, gaps, and what's a sane Rust codegen strategy? |
| 02 | [`02-base-and-http.md`](./02-base-and-http.md)               | 3 218 | How does `python-plexapi` model an HTTP response as a typed object? `PlexObject`, lazy reload, the `**kwargs` filter DSL, the HTTP session, the exception hierarchy. |
| 03 | [`03-myplex-and-auth.md`](./03-myplex-and-auth.md)           | 4 496 | How do the three sign-in flows (token, password+2FA, PIN/OAuth) actually work on the wire? Resource discovery, sharing, watchlist, sonos, webhook management. ~50-endpoint plex.tv inventory. |
| 04 | [`04-plexserver.md`](./04-plexserver.md)                     | 4 468 | What does `PlexServer` do? Identity, sessions, history, system, settings, hubs, clients, transcoding. 40-endpoint PMS inventory + a "naming mismatches" appendix flagging methods that don't exist verbatim. |
| 05 | [`05-library-and-search.md`](./05-library-and-search.md)     | 5 570 | The 3 386-LOC monster. `Library`, `LibrarySection` subclasses, the dual filter operator namespaces (PlexAPI `__suffix` vs Plex `!/<</>>`), advanced filter dict grammar, smart filters, hubs. |
| 06 | [`06-media-objects.md`](./06-media-objects.md)               | 5 298 | The full media class hierarchy across `video.py`/`audio.py`/`photo.py`/`media.py`. Per-leaf attribute tables, `Media`/`Part`/`Stream` chain, mutating-ops matrix, "Rust modelling notes" closing section. |
| 07 | [`07-playback-and-playlists.md`](./07-playback-and-playlists.md) | 3 782 | `PlexClient` remote control (14 nav + 19 playback + mirror commands), `PlayQueue`, `Playlist`/`Collection` (regular vs smart), `SyncItem`, `PlexSonosClient`. 7-scheme URI inventory + 37-endpoint reference. |
| 08 | [`08-mixins-and-traits.md`](./08-mixins-and-traits.md)       | 5 844 | The Python mixin multi-inheritance pattern and how to express it in Rust 2024. Two large capability matrices, four candidate Rust strategies, a hybrid recommendation. |
| 09 | [`09-realtime-and-discovery.md`](./09-realtime-and-discovery.md) | 2 645 | WebSocket alert stream (URL, all 13+ message types, Python's HTTPS bug), GDM local discovery (raw UDP M-SEARCH on 32412/32414 — **not mDNS**), server `Settings`/`Setting`, webhook payload management. |
| 10 | [`10-testing-strategy.md`](./10-testing-strategy.md)         | 3 634 | Why we cannot copy `python-plexapi`'s "live Docker PMS" test rig. Three-tier Rust strategy: pure unit / parser snapshot via `insta` / integration via `wiremock`. 15-row sanitiser regex table. |
| 11 | [`11-rust-mapping-recommendations.md`](./11-rust-mapping-recommendations.md) | 9 261 | **Synthesis.** Pulls the load-bearing decisions out of docs 01–10 into a single prescriptive Rust port plan: revised module layout (with `// CHANGED:` annotations vs `CLAUDE.md` §3), pattern translation table, 11 corrections to `CLAUDE.md`, trait architecture with concrete signatures for 10 load-bearing traits, `PlexUri` enum, typestate auth state machine, 6-milestone (`M0..M5`) implementation order with rationale, top-10 risk register, definition of done. **Read this before starting any feature PR — if it disagrees with `CLAUDE.md`, this doc wins.** |

Total: **~52 600 words** across docs 01–11, indexed by this README.

---

## How to use these notes

- **Before writing new code**: re-read the doc covering that surface
  (e.g. opening movie search → read `05-library-and-search.md`). Cite
  the doc + section in the PR description so reviewers have the same
  context.
- **When porting a Python feature**: open the equivalent
  `python-plexapi/plexapi/<file>.py` and use the analysis doc as your
  index. The docs cite source lines (`base.py:634`) wherever possible.
- **When the wire format surprises you**: check the analysis first —
  several non-obvious quirks (HTTP `GET` used for mutations, edit
  endpoint on the *section* rather than the *item*, smart-filter
  `push=1/pop=1` wire tokens, etc.) are documented as findings.

## How to maintain these notes

- Updates should happen in the same PR as the code change that
  invalidated them. Don't let the docs drift.
- Each doc opens with a date and pins the upstream commit / spec
  version it was written against.
- If a Python idiom maps to a Rust idiom in a non-obvious way, add a
  row to the pattern translation table in
  [`11-rust-mapping-recommendations.md`](./11-rust-mapping-recommendations.md).
- New analysis topics get a new file (`12-...md`, `13-...md`, …) and a
  row in the table above. Keep each file focused on one surface area.

## What's intentionally NOT here

- A re-explanation of code that's clear on its own. The python-plexapi
  source is on disk at `/python-plexapi/` — open it.
- Per-version changelogs of either upstream. Use `git log` on the cloned
  repo for that.
- Rust crate API documentation. That belongs in `///` doc comments on
  the public items themselves.

---

_Findings captured 2026-05-27 by Claude (`opus-4-7-1m`), supervised by
Dwi Elfianto._
