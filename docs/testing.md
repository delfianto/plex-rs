# Testing strategy

The crate has **520 tests** distributed across three layers, all
passing under the four CI gates. This document covers the
testing conventions in use so contributors can match the style.

```
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo clippy --all-targets --no-default-features -- -D warnings
cargo test --all-features
RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --all-features
```

All five must pass before any commit is "done".

---

## Three layers

### Layer 1 — Inline unit tests (`#[cfg(test)] mod tests` in `src/`)

For pure logic: parsers, formatters, validators, retry math,
URL builders, DTO round-trips.

Each `.rs` file under `src/` that exposes non-trivial logic ends
with a `#[cfg(test)] mod tests` block. The tests are colocated
so they're easy to find from the code they cover.

Conventions:

- One assertion per logical thing being checked. Multi-assertion
  tests are fine when they're checking facets of the same
  scenario (e.g. "parse this body, assert each extracted field").
- Test names describe the property being verified, not the input:
  `parses_settings_collection_with_all_kinds`, not `test_parse_1`.
- Tests use the public API where possible. For wire-format
  testing, `pub(crate) fn build_query()` / `pub(crate) fn
  build_url()` helpers expose the internal shape; tests assert
  on the constructed query string directly rather than mocking
  HTTP.
- Tests of `#[cfg(test)]`-only behavior go in the same module as
  the type they exercise.

Example:

```rust
// src/library/smart_filter.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operator_suffixes_dispatch_correctly() {
        let f = SmartFilter::from_uri("year>>=2010&title<=The").unwrap();
        let year = f.clauses.iter().find(|c| c.field == "year").unwrap();
        assert_eq!(year.op, FilterOp::GreaterThan);
        assert_eq!(year.values, vec!["2010"]);
    }
}
```

### Layer 2 — Integration tests (`tests/` directory)

For end-to-end behavior against a mocked PMS. Each `tests/*.rs`
file exercises one milestone's worth of public-API surface
against a `wiremock::MockServer` (or `tokio::net::TcpListener`
for the WebSocket alerts).

The 34 integration test files mirror the milestone numbers:
- `m0_http_smoke.rs` — HTTP layer smoke tests
- `m1_server_library.rs` — `PlexServer::connect` and section
  listing
- `m2_movies.rs`, `m2_tv.rs`, `m2_music.rs`, `m2_photos.rs` —
  media domain traversal
- `m2_search.rs`, `m2_filter.rs` — search + filter
- `m3_*.rs` — every M3 trait
- `m4_*.rs` — every M4 endpoint family
- `m5_*.rs` — every M5 cloud / WS / webhook endpoint

Each test:

1. Spins up a `MockServer`.
2. Registers `Mock` matchers for the exact wire shape the crate
   is expected to send.
3. Drives the public API and asserts on the resulting domain
   types.
4. The `expect(N)` matcher catches over- and under-firing — if
   the crate makes more or fewer requests than expected, the
   test fails.

Example:

```rust
#[tokio::test]
async fn set_writes_via_put_and_reloads() {
    let server = MockServer::start().await;
    // initial load + reload after set
    Mock::given(method("GET"))
        .and(path("/:/prefs"))
        .respond_with(ResponseTemplate::new(200).set_body_json(prefs_body()))
        .expect(2)
        .mount(&server).await;
    Mock::given(method("PUT"))
        .and(path("/:/prefs"))
        .and(query_param("TranscoderQuality", "80"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server).await;

    let plex = connect(&server).await;
    let settings = plex.settings().await.unwrap();
    let _ = settings.set(&plex, "TranscoderQuality", SettingValue::Int(80)).await.unwrap();
}
```

### Layer 3 — Doctests in module-level `///` comments

For the bits that are best understood through example. Doctests
serve double duty as docs and tests.

`no_run` is used liberally — most doctests show "given a
connected `PlexServer`, do X" patterns that don't need to
actually run during `cargo test --doc`. The compile-check is what
matters: the example must parse, type-check, and use the public
API correctly.

```rust
//! ```no_run
//! # use plex_rs::PlexServer;
//! # use plex_rs::alerts::{Alerts, AlertEvent};
//! # use futures_util::StreamExt;
//! # async fn run(plex: PlexServer) -> Result<(), plex_rs::Error> {
//! let mut stream = Alerts::connect(&plex).await?;
//! while let Some(event) = stream.next().await {
//!     // ...
//! }
//! # Ok(()) }
//! ```
```

The hash-prefixed lines (`# use ...`, `# async fn run(...) {`,
`# Ok(()) }`) are hidden from the rendered docs but kept for
the compile check.

---

## Patterns

### Wiremock for HTTP

Every integration test that exercises an endpoint uses
`wiremock::MockServer`. The patterns:

```rust
let server = MockServer::start().await;

// Match on method + path
Mock::given(method("GET")).and(path("/library/sections")).respond_with(...).mount(&server).await;

// Match on query parameters too
Mock::given(method("PUT"))
    .and(path("/library/sections/7/all"))
    .and(query_param("id", "42"))
    .and(query_param("title.value", "Arrival"))
    .respond_with(ResponseTemplate::new(200))
    .expect(1)  // <-- fails the test if the crate doesn't fire exactly 1 matching request
    .mount(&server).await;

// Match on body content
Mock::given(method("POST"))
    .and(body_string_contains("urls%5B%5D=https%3A%2F%2Fa%2F"))
    .respond_with(ResponseTemplate::new(200))
    .mount(&server).await;
```

`expect(N)` is the secret weapon — it asserts the matcher fired
exactly N times. Without it, missing requests pass silently. We
always set it.

### `tokio::net::TcpListener` for WebSocket

The alerts integration tests can't use `wiremock` (HTTP-only).
Instead they stand up a `TcpListener` and accept one WebSocket
connection via `tokio_tungstenite::accept_hdr_async`:

```rust
async fn spawn_ws_server(frames: Vec<String>) -> (String, JoinHandle<...>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let (stream, _peer) = listener.accept().await.unwrap();
        let ws = tokio_tungstenite::accept_hdr_async(stream, |req: &Request, resp| {
            // record the request URI for handshake assertions
            *captured_uri.lock().unwrap() = Some(req.uri().to_string());
            Ok(resp)
        }).await.unwrap();
        // send each frame as a text message, then close
        let (mut writer, _) = ws.split();
        for frame in frames { writer.send(Message::Text(frame)).await.unwrap(); }
        let _ = writer.send(Message::Close(None)).await;
    });
    (format!("ws://{addr}"), handle)
}
```

This proves the full WS handshake works end-to-end including
the query-string token, then exercises the decoder against
live frames. See `tests/m5_alerts.rs` for the full pattern.

### Axum router for webhook ingest

Webhook ingest tests stand up an actual `axum::Router` with the
extractor, then POST multipart bodies via `reqwest`:

```rust
let app = Router::new().route("/plex", post(|payload: WebhookPayload| async {
    *slot.lock().await = Some(payload);
    StatusCode::NO_CONTENT
}));
let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
// ...build multipart body manually...
let resp = reqwest::Client::new()
    .post(&url)
    .header(CONTENT_TYPE, content_type)
    .body(body)
    .send().await.unwrap();
```

End-to-end: multipart construction → HTTP → axum extractor →
extracted `WebhookPayload`. Catches subtle bugs in any of the
three boundaries.

### `__test_new` for test-only constructors

`PlexServer` normally requires a `GET /` round-trip to populate
`ServerIdentity`. URL-building tests (the transcode URL builder,
for instance) don't want to mock that probe. The doc-hidden
constructor:

```rust
impl PlexServer {
    #[doc(hidden)]
    #[must_use]
    pub const fn __test_new(
        base_url: Url,
        http: HttpClient,
        identity: ServerIdentity,
    ) -> Self { Self { base_url, http, identity } }
}
```

The `__test_` prefix is a strong "don't use this" signal.
`#[doc(hidden)]` keeps it out of the rendered docs. The pattern
is sparingly used — most modules' inline tests use `pub(crate)`
constructors instead.

### Permissive fixtures, strict assertions

Test fixtures (the JSON bodies passed to `set_body_json`) carry
**every field** the corresponding production parser might want
to see, even if a particular test only asserts on a subset.
This means:

- A test that only checks `entry.rating_key` still emits a
  fixture with `viewedAt`, `accountID`, `librarySectionID`,
  etc. populated.
- If the production parser later starts requiring a field that
  was previously optional, the fixture already has it.

Assertions, on the other hand, check exactly the property
under test and nothing more. No "let's also assert on the title"
sympathy assertions — those just rot.

### Negative assertions for absence

For testing "this field should NOT appear in the wire format,"
the trick is asserting on the public-facing behavior, not on
the missing parameter:

```rust
#[tokio::test]
async fn empty_batch_makes_no_http_call() {
    let server = MockServer::start().await;
    // Mount NOTHING for PUT — if the batch fires one, wiremock
    // returns 404 and the assertion below fails.
    let plex = setup(&server).await;
    let movie = fetch_movie(&plex, &server).await;
    movie.batch().execute().await.unwrap();  // <-- must not panic
}
```

The absence of a registered `Mock` is the assertion. If the
crate fires an unexpected request, wiremock returns 404 and the
crate's `Error::NotFound` bubbles up to the `.unwrap()`.

---

## What we don't test

A few categories of test the crate intentionally doesn't carry:

- **Live PMS tests.** No integration suite runs against a real
  Plex Media Server. CLAUDE.md §9.4 documents this — live tests
  would require a token, machine identifier, and library fixtures
  that vary per dev environment. The wiremock layer covers wire
  shape; manual smoke tests against a real PMS cover the rest.
- **Snapshot tests (insta).** Considered, deferred. The DTO →
  domain conversion isn't shape-stable enough to make snapshot
  diffs informative. Field-by-field asserts catch more.
- **Property-based tests (proptest).** Considered for the URI
  parser and filter builder, deferred. The wire formats are
  too narrow to benefit from random exploration — a handful of
  hand-crafted edge cases (empty, malformed, special-char-heavy)
  covers the realistic input space.
- **Benchmarks.** A `benches/` directory was planned in
  CLAUDE.md but never instantiated. None of the hot paths are
  performance-critical enough to justify benchmark infrastructure
  yet.

---

## How to add a test

For a new endpoint:

1. **Inline DTO test** — in the same `.rs` file as the new DTO,
   add a `#[test]` that constructs a `serde_json::Value` fixture
   and asserts the `into_domain` conversion produces the expected
   domain values. Include both happy-path and edge cases
   (missing optional fields, malformed values).

2. **Inline URL/parser test** — if the module has URL or query-
   string construction logic, expose it via a `pub(crate)`
   helper and write `#[test]`s asserting on the constructed
   string. No HTTP needed.

3. **Integration test** — add a `tests/<milestone>_<feature>.rs`
   file with a `wiremock::MockServer` fixture and one
   `#[tokio::test]` per public method. Each test should:
   - Mount mocks for every endpoint the test exercises with
     `expect(N)` set
   - Drive the public API
   - Assert on the returned values
   - Assert on the request shape implicitly via mock matchers

4. **Doctest** — if the new method's usage isn't obvious, add a
   `///` example. Use `no_run` and `#`-prefixed hidden lines for
   the `let plex = ...` setup.

The test discipline is "if you wouldn't merge it without tests,
you shouldn't write it." Every public function has at least one
test exercising both its happy path and one error path.

---

## CI

`.github/workflows/ci.yml` runs the four gates on every PR and
push to main:

| Job | Command | Notes |
|---|---|---|
| `fmt` | `cargo fmt --all -- --check` | |
| `clippy` | `cargo clippy --all-targets --all-features -- -D warnings` | |
| `clippy-no-default` | `cargo clippy --all-targets --no-default-features -- -D warnings` | catches accidentally-feature-gated symbols |
| `test` | `cargo test --all-features --workspace` | full suite incl. doctests |
| `doc` | `RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --all-features` | catches broken intra-doc links, missing docs |
| `deny` | `cargo deny check` | licenses, security advisories |

Coverage (via `cargo llvm-cov`) is run informationally but not
gated — see CLAUDE.md §10. The current line coverage exceeds 90%
across the production code, but enforcing a hard threshold leads
to perverse testing incentives.
