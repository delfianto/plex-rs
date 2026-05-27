# plex-rs

[![Rust](https://img.shields.io/badge/rust-2024-orange?logo=rust)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Coded by Claude](https://img.shields.io/badge/coded%20by-Claude%20Opus%204.7-7c5cff?logo=anthropic)](https://www.anthropic.com)
[![Probably works](https://img.shields.io/badge/probably-works-yellow?logo=question)](https://en.wikipedia.org/wiki/It_works_on_my_machine)
[![Salt](https://img.shields.io/badge/take%20with-a%20barn%20of%20salt-red?logo=warning)](#hold-on-a-second)

A fully-async Rust 2024 client for [Plex Media
Server](https://www.plex.tv/) and the `plex.tv` cloud, born from
a very long evening of "what if I just kept pressing accept on
the next milestone."

```toml
[dependencies]
plex-rs = "0.0"   # version reflects level of confidence
```

## Hold on, a second.

This crate is **16,500 lines of production Rust across 58
modules** with **520 passing tests**.

It is also a media-center API binding.

Those two facts coexist uneasily. Yes — `plex-rs` extensively
tests, lints, documents, retries-with-backoff, redacts tokens
in `Debug`, races concurrent connect probes, parses WebSocket
alert frames into typed events, and exposes a forward-compatible
`Unknown(String)` variant on every wire-discriminated enum. None
of this is strictly necessary to list your movies.

It was written over a marathon Claude Opus 4.7 session that
treated every "continue" as marching orders. The result is
either thorough or comically over-engineered, depending on your
priors.

**Has anyone run it against a real Plex server?** Excellent
question. Every endpoint has wiremock integration tests
asserting it sends the right shape and parses the right
response. That's not the same as "works." Filing bugs is
encouraged; expect occasional "oh, _that's_ how Plex actually
spells it" moments.

🧂 Take with appropriate seasoning.

## What's allegedly in the box

| | What it claims to do |
|---|---|
| 🔐 **Auth** | Direct token, PIN/OAuth, password + 2FA — all three flows |
| 🔭 **Discovery** | plex.tv resource list with parallel connect-race + local LAN multicast (GDM) |
| 📚 **Library** | Movie / Show / Season / Episode / Artist / Album / Track / Photoalbum / Photo / Playlist / Collection. Typed `FilterBuilder`, search, recently-added, on-deck, the works |
| ✏️ **Editing** | Rate, edit title/summary/year/tagline/studio/etc., replace tags, lock fields, swap art, all in batch if you like |
| ▶️ **Playback** | `PlayQueue` (create/get/mutate), `PlexClient` remote control (nav + playback commands), direct-play URLs, transcoded HLS/DASH URL builder |
| 👀 **Monitoring** | Current sessions, paginated playback history, server admin (activities/butler/updater/bandwidth/resources), real-time WebSocket alerts |
| ☁️ **Cloud stuff** | Watchlist + Discover search + cloud scrobble + devices + friends + Home users + webhook registration |
| 📥 **Inbound webhooks** | Axum extractor that decodes Plex's multipart payload into typed events |

## A quick taste

```rust
use plex_rs::{MyPlexPinLogin, ClientIdentifier, MyPlexClient};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), plex_rs::Error> {
    let cid = ClientIdentifier::new("my-app")?;

    // PIN sign-in. Other auth flows available; pick your poison.
    let pin = MyPlexPinLogin::start(cid.clone(), None).await?;
    println!("Visit https://plex.tv/link and enter: {}", pin.code());
    let token = pin.wait(Duration::from_secs(120), Duration::from_secs(2)).await?;

    // Race every possible connection URI Plex knows about; first to answer wins.
    let server = MyPlexClient::new(token, cid, None)?
        .resource("Living Room")
        .await?
        .expect("you named your server something else")
        .connect()
        .await?;

    // The fun part.
    for section in server.library().sections().await? {
        println!("📁 {}", section.title);
    }

    Ok(())
}
```

If that compiles and runs, you have somehow defeated the
universe's natural state of entropy and are owed a beer.

## Documentation

Detailed docs live in [`docs/`](./docs/README.md). They are
themselves about 3,000 lines because *of course* they are.

| | |
|---|---|
| 🏗 [architecture.md](./docs/architecture.md) | the "why is this 16k lines" rationale |
| 📦 [modules.md](./docs/modules.md) | per-module walkthrough |
| 🎨 [design-patterns.md](./docs/design-patterns.md) | Rust patterns used everywhere — DTO+domain splits, newtypes, redacted Debug, builders, trait architecture |
| 🌐 [api-coverage.md](./docs/api-coverage.md) | what works, what doesn't, what's "should probably work but who knows" |
| 🚫 [out-of-scope.md](./docs/out-of-scope.md) | six things we explicitly aren't shipping, with rationale |
| 🧪 [testing.md](./docs/testing.md) | the wiremock + WebSocket-replica pattern, plus what we *don't* test (live PMS, lol) |
| 🤝 [contributing.md](./docs/contributing.md) | recipes for adding new endpoints |

The tracker [`TRACKER.md`](./TRACKER.md) records what shipped in
each milestone; [`CHANGELOG.md`](./CHANGELOG.md) is the
human-readable version. [`CLAUDE.md`](./CLAUDE.md) is the
contributor charter and also the document an LLM read approximately
seven hundred times while building this.

## Cargo features

| Feature | Default? | Purpose | Risk profile |
|---|:---:|---|---|
| `rustls` | ✓ | TLS via reqwest's bundled rustls | low |
| `native-tls` | | use platform TLS stack instead | medium |
| `discovery` | | GDM local multicast — *raw UDP*, not mDNS, please don't add `mdns-sd` | low |
| `alerts` | | WebSocket alert stream, pulls in `tokio-tungstenite` | medium |
| `webhook-axum` | | axum extractor for inbound webhooks, pulls in axum | medium |

## What this isn't

- **A CLI tool.** Wrap it in your own binary. Or use python-plexapi,
  which has one and is probably more battle-tested. We won't
  judge.
- **A TUI.** See above.
- **A reimplementation of Plex's transcoder.** It builds the URL
  Plex's transcoder expects. The actual transcoding is still
  Plex's problem.
- **Synchronous.** Everything is `async fn`. Wrap with
  `tokio::runtime::Runtime::block_on` if you absolutely must,
  but you're going to feel weird about it.
- **Battle-tested.** See "salt, barn-quantities of" above.

## Status

**Pre-1.0.** The public surface is unstable. Minor version bumps
can and will break you until 1.0. We'll get to 1.0 when someone
who actually owns a Plex Pass subscription runs this against
their real server and the GitHub issues stop arriving.

If you want to know exactly which milestone shipped what, see
[`TRACKER.md`](./TRACKER.md). If you want the human-readable
release notes, see [`CHANGELOG.md`](./CHANGELOG.md). If you want
to know why on earth there are 12 traits for editing tags, read
[`docs/architecture.md`](./docs/architecture.md) and then ask
your favorite LLM "is this normal?"

## CI gates (the four green checks we're proud of)

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo clippy --all-targets --no-default-features -- -D warnings
cargo test --all-features
RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --all-features
```

(That's five. Five gates. We were trying to be funny.)

## Acknowledgements

- [`python-plexapi`](https://github.com/pkkid/python-plexapi) —
  the actual battle-tested binding this crate cribs its parity
  baseline from. If you need something that's been around since
  2015 and has actually shipped, go there. They'd appreciate the
  GitHub star more than we would.
- Claude Opus 4.7 — wrote approximately every line of this.
  Sometimes correctly.
- Plex — for an API surface so sprawling that documenting it
  required eight separate doc files.

## License

MIT — see [`LICENSE`](./LICENSE). Use it, fork it, ship it,
break it. We're not your dad.
