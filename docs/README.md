# plex-rs documentation

Idiomatic, fully-async Rust 2024 client for the Plex Media Server
HTTP API and the `plex.tv` cloud services.

This folder is the canonical design + reference documentation for
the crate. The repository root `README.md` is intentionally thin;
deep content lives here.

---

## Quick links

| Topic | Document | Purpose |
|---|---|---|
| 🏗 **Architecture** | [architecture.md](./architecture.md) | High-level design — HTTP layer, error model, trait architecture, type strategy, the six endpoint families |
| 📦 **Modules** | [modules.md](./modules.md) | Per-module deep dive — what every file does, key types, design notes |
| 🎨 **Design patterns** | [design-patterns.md](./design-patterns.md) | Rust-specific patterns used throughout — builders, redaction, DTO+domain split, forward-compat variants, etc. |
| 🌐 **API coverage** | [api-coverage.md](./api-coverage.md) | What Plex APIs are implemented, organized by endpoint family. Includes wire-format notes for the trickier bits |
| 🚫 **Out of scope** | [out-of-scope.md](./out-of-scope.md) | What's NOT implemented and why — formally categorized rationale |
| 🧪 **Testing** | [testing.md](./testing.md) | Test layout, wiremock + WebSocket replica patterns, coverage philosophy |
| 🤝 **Contributing** | [contributing.md](./contributing.md) | How to add a new endpoint / module / trait |

The release-history changelog lives at [`../CHANGELOG.md`](../CHANGELOG.md);
the milestone tracker at [`../TRACKER.md`](../TRACKER.md); the
contributor charter at [`../CLAUDE.md`](../CLAUDE.md).

---

## 60-second tour

`plex-rs` is a typed, async-first binding to every Plex API surface
a realistic client touches:

- **Authentication** — three flows (direct token, PIN/OAuth,
  password + 2FA) covering both interactive UIs and headless
  daemons.
- **Server discovery** — local LAN discovery via GDM multicast,
  and the plex.tv resources API with parallel connect-race.
- **Library browsing** — Movies / Shows / Seasons / Episodes /
  Artists / Albums / Tracks / Photos with shared `LibraryItem`
  sum type. Sections list, search, filters via typed builder,
  recently-added, on-deck, unwatched, hubs.
- **Metadata editing** — `Ratable`, `EditField`, `EditTags`, image
  URL/lock traits, `Reload`, `PlayedUnplayed`, and `EditBatch` for
  one-PUT multi-field updates.
- **Playback control** — `PlayQueue` (create/get/mutate),
  `PlexClient` (remote control of a player via /player/), direct
  play and transcoded streaming URLs.
- **Monitoring** — current sessions, paginated playback history,
  WebSocket alerts stream (`Playing`/`Timeline`/`Activity`/
  `TranscodeSession`/etc.), server admin endpoints (activities,
  butler, updater, bandwidth/resource stats).
- **Cloud services** — plex.tv watchlist (add/remove/list),
  Discover catalogue search, user state + scrobble against
  metadata.provider.plex.tv, devices list + revoke, friends
  list + remove, Home users list, webhook URL registration.
- **Webhook ingest** — typed inbound webhook payload with
  `axum::FromRequest` extractor (feature-gated).
- **Real-time alerts** — WebSocket stream from PMS, feature-gated.

---

## Status

**Pre-1.0**, public surface unstable.

**~16,500 lines of production Rust** across 58 modules and 13
top-level domains. **520 tests** (unit + wiremock integration +
doctest), all passing on the four CI gates:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo clippy --all-targets --no-default-features -- -D warnings
cargo test --all-features
RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --all-features
```

See [`../TRACKER.md`](../TRACKER.md) for milestone-by-milestone
status.

---

## Cargo features

| Feature | Default | Pulls in | Why |
|---|:---:|---|---|
| `rustls` | ✓ | (marker) | Default — rustls TLS via reqwest's bundled rustls feature |
| `native-tls` | | `reqwest/native-tls` | Use the platform TLS stack instead |
| `discovery` | | `tokio/net` | GDM local multicast discovery. Pure UDP — no `mdns-sd` dep |
| `alerts` | | `tokio-tungstenite` | WebSocket alert stream from PMS |
| `webhook-axum` | | `axum` (multipart + http1 + json + tokio) | `axum::FromRequest` extractor for inbound webhooks |

The default build pulls in only `reqwest` + `tokio` + `serde` +
the small support crates (`quick-xml`, `chrono`, `url`, etc.). The
heavyweight transport dependencies (`tokio-tungstenite`, `axum`)
are opt-in.

---

## Project layout

```
plex-rs/
├── src/                     # 16.5k LOC production + 4.8k LOC inline unit tests
│   ├── client.rs            # HttpClient — the single async I/O surface
│   ├── config.rs            # ClientConfig builder
│   ├── error.rs             # typed Error enum + Result alias
│   ├── headers.rs           # X-Plex-* identity headers
│   ├── lib.rs               # crate root + re-exports
│   ├── pagination.rs        # PageRange + header-based pagination
│   ├── uri.rs               # PlexUri enum (server://, library://, …)
│   ├── auth/                # PIN sign-in + password+2FA sign-in
│   ├── alerts/              # WebSocket alerts (feature: alerts)
│   ├── discover_gdm/        # LAN UDP discovery (feature: discovery)
│   ├── library/             # Library, sections, FilterBuilder, SmartFilter
│   ├── media/               # Movie, Show, …, Album, Track, …, Photo, Playlist, Collection
│   ├── myplex/              # MyPlexClient + resources + watchlist + discover + …
│   ├── playback/            # PlayQueue, PlexClient remote control, transcode URLs
│   ├── server/              # PlexServer + sessions + history + settings + admin
│   ├── traits/              # PlexObject + M3 trait architecture
│   ├── util/                # RatingKey, PlexToken, time, search_type, sanitize, ids
│   ├── webhook/             # Inbound webhook + axum FromRequest (feature)
│   └── xml/                 # MediaContainer<T> envelope
├── tests/                   # 5.7k LOC across 34 wiremock-driven integration tests
├── docs/                    # ← you are here
├── CHANGELOG.md             # release-by-release notes
├── TRACKER.md               # milestone tracker (status of each M0..M5 sub-item)
├── CLAUDE.md                # contributor charter — coding rules, project goals
└── Cargo.toml
```

See [`modules.md`](./modules.md) for the per-module breakdown.

---

## Where to start reading

- **Building something with the crate?** → read [`api-coverage.md`](./api-coverage.md)
  to see what's available, then jump to module docs or `cargo doc`.
- **Curious about the design?** → start with [`architecture.md`](./architecture.md)
  then [`design-patterns.md`](./design-patterns.md).
- **Adding a new endpoint?** → [`contributing.md`](./contributing.md)
  has the recipe.
- **Wondering why some Plex feature isn't there?** → check
  [`out-of-scope.md`](./out-of-scope.md) first; if it's not listed,
  open an issue.
