# plex-rs

[![Rust](https://img.shields.io/badge/rust-2024-orange?logo=rust)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Idiomatic, fully-async Rust 2024 client for the
[Plex Media Server](https://www.plex.tv/) HTTP API and the `plex.tv`
cloud services.

```toml
[dependencies]
plex-rs = "0.0"
```

## What's in the box

- **Three auth flows** — direct token, PIN / OAuth, password + 2FA
- **Server discovery** — plex.tv resources with parallel connect-race,
  plus local LAN multicast (GDM)
- **Full media domain** — Movie / Show / Season / Episode / Artist /
  Album / Track / Photoalbum / Photo / Playlist / Collection, with
  shared `LibraryItem` sum type, typed `FilterBuilder`, search,
  recently-added, on-deck
- **Metadata editing** — `Ratable`, `EditField`, `EditTags`, image
  URL + lock traits, `Reload`, `PlayedUnplayed`, `EditBatch` for
  one-PUT multi-field updates
- **Playback control** — `PlayQueue` (create / get / mutate),
  `PlexClient` (player remote control), direct-play URLs,
  transcoded streaming URL builder
- **Monitoring** — current sessions, paginated playback history,
  server admin endpoints (activities / butler / updater /
  bandwidth + resource stats), real-time WebSocket alerts
- **Cloud services** — plex.tv watchlist + Discover catalogue
  search + user state + scrobble, devices list + revoke, friends
  list + remove, Home users list, webhook URL registration
- **Webhook ingest** — typed payload with `axum::FromRequest`
  extractor (feature-gated)

## Quick start

```rust
use plex_rs::{MyPlexPinLogin, ClientIdentifier, MyPlexClient};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), plex_rs::Error> {
    let cid = ClientIdentifier::new("my-app")?;

    // Sign in via PIN
    let pin = MyPlexPinLogin::start(cid.clone(), None).await?;
    println!("Visit https://plex.tv/link and enter: {}", pin.code());
    let token = pin.wait(Duration::from_secs(120), Duration::from_secs(2)).await?;

    // Find a server and connect
    let server = MyPlexClient::new(token, cid, None)?
        .resource("Living Room").await?
        .unwrap()
        .connect().await?;

    // Browse
    for section in server.library().sections().await? {
        println!("Section: {}", section.title);
    }

    Ok(())
}
```

## Documentation

📚 **Detailed documentation lives in [`docs/`](./docs/README.md).**

Quick links:

| | |
|---|---|
| 🏗 [architecture.md](./docs/architecture.md) | high-level design |
| 📦 [modules.md](./docs/modules.md) | per-module deep dive |
| 🎨 [design-patterns.md](./docs/design-patterns.md) | Rust patterns used throughout |
| 🌐 [api-coverage.md](./docs/api-coverage.md) | what's implemented |
| 🚫 [out-of-scope.md](./docs/out-of-scope.md) | what's not, and why |
| 🧪 [testing.md](./docs/testing.md) | test layout + conventions |
| 🤝 [contributing.md](./docs/contributing.md) | how to extend the crate |

Also: [`CHANGELOG.md`](./CHANGELOG.md), [`TRACKER.md`](./TRACKER.md)
(milestone status), [`CLAUDE.md`](./CLAUDE.md) (contributor charter).

## Cargo features

| Feature | Default | Purpose |
|---|:---:|---|
| `rustls` | ✓ | rustls TLS via reqwest (default) |
| `native-tls` | | platform TLS stack instead |
| `discovery` | | GDM local multicast discovery |
| `alerts` | | WebSocket alert stream |
| `webhook-axum` | | axum extractor for inbound webhooks |

## Status

**Pre-1.0.** Public surface unstable; expect breaking changes in
minor versions until 1.0. See [`TRACKER.md`](./TRACKER.md) for
milestone-by-milestone status and [`CHANGELOG.md`](./CHANGELOG.md)
for release notes.

**~16,500 lines of production Rust across 58 modules** with
**520 passing tests** under five CI gates:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo clippy --all-targets --no-default-features -- -D warnings
cargo test --all-features
RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --all-features
```

## License

MIT — see [`LICENSE`](./LICENSE).
