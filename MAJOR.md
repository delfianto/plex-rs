# Held-back major dependency bumps

Written after the 2026-07-17 dependency sweep. These are **not** currently broken on `main` —
they're major-version bumps that `cargo upgrade` (compatible-only) correctly skipped. Renovate's
open PR (`renovate/non-major-dependencies` #3, mislabeled — it's not actually all non-major)
bundles all six below into one commit and fails CI. Do not merge that PR as-is; if any of these
get tackled, do it as a deliberate, separate PR per crate (or a couple of tightly-related ones
together — see grouping notes below), not as one bundled Renovate automerge.

Whatever changes here, keep to the `CLAUDE.md §5` philosophy: small tree, rustls-by-default,
column-aligned `Cargo.toml`. Don't let a migration undo that.

## reqwest 0.12 → 0.13 *(confirmed break)*

**Fails with:**
```
error: failed to select a version for `reqwest`.
package `plex-rs` depends on `reqwest` with feature `rustls-tls` but `reqwest` does not have that feature.
```

Same root cause as `stash-mcp` and `tei-proxy` hit today: reqwest 0.13 renamed `rustls-tls` to
`rustls` (plus `rustls-native-certs`/`rustls-no-provider` for finer control — see the [reqwest
0.13 release notes](https://github.com/seanmonstar/reqwest/releases) for which one matches this
project's TLS needs). Mechanical fix once you're ready:
```toml
reqwest = { version = "0.13", default-features = false, features = ["rustls", "json", "stream", "gzip", "http2", "charset"] }
```
Not otherwise audited — there may be more to 0.13 than the feature rename.

## tokio-tungstenite 0.24 → 0.30

Used for the `alerts` feature's WebSocket stream (`dep:tokio-tungstenite`, gated behind
`alerts = ["dep:tokio-tungstenite"]` in `[features]`). Six-minor-version jump; not diagnosed here
since it's currently masked by the reqwest failure (CI never got past dependency resolution to
actually compile this far). Check tokio-tungstenite's changelog for API changes to the
`connect`/`rustls-tls-webpki-roots` surface this project uses.

## axum 0.7 → 0.8

Used for the `webhook-axum` feature (`dep:axum`, gated behind `webhook-axum = ["dep:axum"]`).
Also not diagnosed — masked by the reqwest failure. axum 0.8 had router/extractor API changes in
past majors; expect to touch whatever handler code lives behind the `webhook-axum` feature flag.

## quick-xml 0.36 → 0.41

Used directly in `[dependencies]` for PMS XML response parsing — this is core to the crate, not
feature-gated. Five-minor jump, not diagnosed. Given plex-rs parses a lot of Plex's XML API
surface, this is worth doing carefully with the existing test fixtures as a safety net (`insta`
snapshot tests are already a dev-dependency here).

## thiserror 1 → 2

Only other repo in this collection still on thiserror 1.x — everywhere else already uses 2.x
cleanly (see the other repos tidied in this same sweep). thiserror 2 is generally a low-risk,
mostly-mechanical migration (mainly MSRV and some derive-macro internals) but wasn't tested here.

## rstest 0.21 → 0.26 / criterion 0.5 → 0.7

Both dev-only (`[dev-dependencies]` / the wasm-gated criterion bench target). Lower risk since
they don't touch the shipped crate's public API — a normal test/bench-suite compile-and-fix pass
should suffice. Not attempted here.

## Suggested grouping if you tackle these

- **quick-xml** alone first (core parsing, most consequential, wants its own careful pass).
- **thiserror** alone (mechanical, quick, but has all-file blast radius via `#[derive(Error)]`).
- **reqwest** alone (feature rename, need to re-verify TLS behavior in a real client run).
- **axum + tokio-tungstenite** together (both feature-gated, both about the optional
  webhook/alerts server-side surface — natural to review as one PR).
- **rstest + criterion** together (both dev/bench-only, lowest risk, fine to batch).

## Until then

Renovate will keep proposing this bundle — it doesn't know these break, only that they're
semver-compatible per its own (looser) classification. Either close each new PR as it appears, or
add a `packageRules` entry to `renovate.json` disabling major-version PRs for this repo (matching
the pattern already used in `the-bannered-mare`'s `renovate.json`) if you'd rather stop seeing
them.
