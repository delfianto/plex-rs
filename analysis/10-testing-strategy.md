# 10. Testing strategy for `plex-rs`

How `python-plexapi` tests itself today, why we cannot mirror that approach
verbatim in a Rust crate that lives or dies by CI, and the concrete
mocked-HTTP + snapshot strategy we will use instead.

Sources read for this analysis:

- `python-plexapi/tests/conftest.py`
- `python-plexapi/tests/payloads.py`
- `python-plexapi/tests/__init__.py`
- `python-plexapi/tests/test__prepare.py`
- `python-plexapi/tests/test_fetch_items.py`
- `python-plexapi/tests/test_misc.py`
- `python-plexapi/tests/test_audio.py` (skim, representative)
- `python-plexapi/tools/plex-bootstraptest.py`
- `python-plexapi/.github/workflows/ci.yaml`
- `CLAUDE.md` §9 / §10 (coverage targets and CI matrix)

---

## 1. What python-plexapi actually does

`python-plexapi`'s test suite runs **against a live Plex Media Server**. There
is no HTTP mocking layer for the bulk of the suite — `requests_mock` shows up
in exactly one fixture (`mocked_account`, used to test MyPlex XML parsing
against `tests/payloads.py::ACCOUNT_XML`). Everything else issues real HTTP
to `http://127.0.0.1:32400` and asserts on responses from a server that has
been bootstrapped with a known media corpus.

### 1.1 Bootstrap entry points

`conftest.py` reads four config keys via `plexapi.CONFIG` (a wrapper around
env vars + `~/.config/plexapi/config.ini`):

| Config key            | Env var equivalent                  | Purpose                                          |
| --------------------- | ----------------------------------- | ------------------------------------------------ |
| `auth.server_baseurl` | `PLEXAPI_AUTH_SERVER_BASEURL`       | Base URL of the live PMS (CI: `http://127.0.0.1:32400`) |
| `auth.myplex_username`| `PLEXAPI_AUTH_MYPLEX_USERNAME`      | plex.tv username for authenticated runs          |
| `auth.myplex_password`| `PLEXAPI_AUTH_MYPLEX_PASSWORD`      | plex.tv password                                 |
| `auth.server_token`   | `PLEXAPI_AUTH_SERVER_TOKEN`         | Pre-issued X-Plex-Token (alternative to creds)   |
| `auth.client_baseurl` | `PLEXAPI_AUTH_CLIENT_BASEURL`       | Base URL of a Plex *client* (for `--client`)     |
| `auth.client_token`   | `PLEXAPI_AUTH_CLIENT_TOKEN`         | Token for that client                            |

The hook `pytest_runtest_setup` skips the entire test if the required combo
is missing, e.g. `@pytest.mark.authenticated` tests skip unless username +
password or a server token is present.

### 1.2 Fixtures defined in `conftest.py`

Session-scoped:

- **`sess`** — a shared `requests.Session` with 120-s timeout.
- **`account`** — `MyPlexAccount(session=sess)`; uses `SERVER_TOKEN` when
  set, otherwise credentials.
- **`account_once`** — same `account`, but skipped under CI unless
  `TEST_ACCOUNT_ONCE=1` (avoids hammering plex.tv from parallel jobs).
- **`account_plexpass`** — `account` if `subscriptionActive`, else skip.
  Gates Plex Pass–only assertions.
- **`account_synctarget`** — `account_plexpass` plus a check that the iOS
  sync headers are wired up; required for sync tests.
- **`plex`** — `PlexServer(SERVER_BASEURL, token, session=sess)`. The
  `pytest_generate_tests` hook parametrises this fixture across two
  variants (`anonymously` / `authenticated`) depending on whether the test
  also asks for `account` or carries the `@authenticated` mark.
- **`sync_device`** — looks up or creates a fake iOS sync device via
  `createMyPlexDevice`.

Function-scoped (assume a live server with the bootstrapped corpus exists):

- **`mocked_account`** — the lone HTTP mock fixture; wires
  `requests_mock` to return `ACCOUNT_XML` from `tests/payloads.py` for
  `https://plex.tv/api/v2/user`, then constructs `MyPlexAccount(token="faketoken")`.
- **`clear_sync_device`** — deletes all sync items.
- **`fresh_plex`**, **`plex2`** — uninitialised / re-initialised servers.
- **`client`** — `PlexClient` against `CLIENT_BASEURL` (skipped without `--client`).
- **Library section fixtures** — `movies`, `tvshows`, `music`, `photos`,
  each calling `plex.library.section("...")`.
- **Media item fixtures** — `movie` ("Elephants Dream"), `show` ("Game
  of Thrones"), `season` (s1), `episode` ("Winter Is Coming"), `artist`
  ("Broke For Free"), `album` ("Layers"), `track` ("As Colourful as
  Ever"), `photoalbum` ("Cats"), `photo` ("photo1"). All hardcoded to
  the names provisioned by `plex-bootstraptest.py`.
- **`collection`**, **`playlist`** — get-or-create test artefacts.
- **`subtitle`** — `mock_open`-backed file handle.
- **`m3ufile`** — walks the music library for a real `playlist.m3u`,
  falling back to writing one in `tmp_path`.
- **`shared_username`** — looks up a shared user (env: `SHARED_USERNAME`,
  default `PKKid`).
- **`monkeydownload`** — monkeypatches `plexapi.utils.download` to
  `mocked=True` so downloads no-op.
- **`empty_response`**, **`patched_http_call`** — `mocker.MagicMock`-based
  blanket HTTP stubs used by a handful of misc tests.

`payloads.py` provides four canned XML blobs: `ACCOUNT_XML`,
`SONOS_RESOURCES`, `SERVER_RESOURCES`, `SERVER_TRANSCODE_SESSIONS`,
`MYPLEX_INVITE`. These already use placeholder values (`testuser`,
`192.168.1.x`, `RINCON_…`, `xxxxxxxxxx`) — they are the seed of the
sanitised-fixture pattern we want to adopt wholesale.

### 1.3 Marks and the parametrise hook

`pytest_generate_tests` injects the right `plex` parametrisation for every
test that names `plex` as a fixture, splitting between `TEST_ANONYMOUSLY`
and `TEST_AUTHENTICATED`. Marks used across the suite:

- `@pytest.mark.authenticated` — needs a logged-in account
- `@pytest.mark.anonymous`     — must be an unclaimed server
- `@pytest.mark.client`        — needs `--client` flag and `CLIENT_BASEURL`
- `@pytest.mark.req_client`    — secondary client requirement
- `@pytest.mark.xfail`         — expected to fail (Plex regressions, flaky
  features). Reasons in the wild: "Plex's OpenSubtitles times out
  occasionally", "Changing images fails randomly", "Plex regression
  `playQueueTotalCount` value incorrect", etc. ~10 distinct `xfail` cases.
- `@pytest.mark.skip(reason="broken test?")` — one case only.

### 1.4 Bootstrap: `tools/plex-bootstraptest.py`

This 642-line script is the entire reason the live-server approach is even
tractable. It:

1. Pulls `plexinc/pms-docker:<tag>` and runs it with a 30-line `docker run`
   command that publishes ports 32400/3005/8324/32469 TCP and 1900/32410-14
   UDP, mounts `<dest>/db`, `<dest>/transcode`, `<dest>/media`, and passes
   `PLEX_CLAIM=<token>` if not running unclaimed.
2. Generates the test media tree by copying three stub files —
   `tests/data/video_stub.mp4` (~17 MB), `tests/data/audio_stub.mp3`
   (~3.6 MB), `tests/data/cute_cat.jpg` (~30 KB) — into a hierarchy whose
   *filenames and folder names* match the assertions in the test suite:
   - Movies: `Elephants Dream (2006).mp4`, `Sita Sings the Blues (2008).mp4`,
     `Big Buck Bunny (2008).mp4`, `Sintel (2010).mp4`.
   - Shows: `Game of Thrones` S01-S10 E01-E10, `The 100` S01-S13 plus E01-E16.
   - Music: `Broke for free/Layers/1 - As Colorful As Ever.mp3` plus a
     generated `playlist.m3u`.
   - Photos: `Cats/photo*.jpg`, plus `Cats in bed`, `Cats not in bed`,
     `Not cats in bed` subfolders.
3. Adds each as a library section via the PlexAPI itself, then starts an
   `AlertListener` and waits (up to 120 s/section) for `state=5` timeline
   events to confirm metadata processing.
4. Disables butler tasks, BIF/chapter thumb generation, intro markers,
   credits markers, ad markers, VAD, music analysis — anything that would
   re-touch the corpus and produce nondeterminism.
5. Shares the server with `SHARED_USERNAME` (default `PKKid`) so the
   sharing tests have something to look at.

The CI matrix in `python-plexapi/.github/workflows/ci.yaml` runs this
twice — `unclaimed` and `claimed` — with `PLEXAPI_AUTH_SERVER_BASEURL=http://127.0.0.1:32400`
and `PLEXAPI_PLEXAPI_TIMEOUT=60`.

---

## 2. Why we cannot copy this in `plex-rs`

The python-plexapi approach is genuinely good for what it is — it catches
real wire-format drift and exercises the whole stack — but adopting it as
the *primary* test harness for `plex-rs` is wrong for our context:

1. **CI flake.** A Docker-hosted PMS plus async metadata scanning plus
   timeline polling produces non-deterministic timing failures. The python
   suite tolerates this with `xfail(strict=False)`; we cannot afford that
   noise against a 90 %-line / 85 %-branch gate (CLAUDE.md §9).
2. **Cost.** Bootstrap adds ~3 minutes per matrix leg before a single
   assertion runs. Multiplied across `fmt`, `clippy`, `test`,
   `test-no-default`, `doc`, `deny`, `coverage` (CLAUDE.md §10) the CI
   wall-clock would balloon.
3. **Media licensing.** `video_stub.mp4`, `audio_stub.mp3`, `cute_cat.jpg`
   together are ~22 MB checked into the python repo. We do **not** want
   to vendor binary media into `plex-rs`; we want fixtures to be plain
   text (XML/JSON) for diff-ability and for `insta` snapshot review.
4. **plex.tv coupling.** The `account` / `account_plexpass` fixtures hit
   real plex.tv. Anyone forking the crate would need to mint a Plex token
   to run the full suite. That's a contributor papercut we refuse to ship.
5. **Coverage attribution.** With a live server, coverage drops in
   parsers/builders are hidden behind successful end-to-end calls. We want
   parser failures to surface as parser test failures, not as
   `assert_eq!` mismatches three layers up.

Therefore: **fixtures-from-recording + mocked HTTP** is the default. Live
PMS testing exists only as an opt-in escape hatch (§6.5 below).

---

## 3. Recommended Rust strategy

The dev-dependency set in `Cargo.toml` already supports everything below
(`wiremock`, `insta`, `rstest`, `tokio` with `test-util`,
`pretty_assertions`). We just need to wire them into the layout below.

### 3.1 Three test tiers

1. **Pure unit tests** — `#[cfg(test)] mod tests` next to the code. No
   HTTP, no fixtures, no async runtime unless the function under test is
   genuinely async. This is where the *bulk* of coverage comes from
   (§10 below).
2. **Parser / snapshot tests** — under `tests/parsers/` or as
   `#[cfg(test)]` modules in `src/xml/`. Load a fixture from
   `tests/fixtures/`, run the DTO parser and the DTO → domain conversion,
   compare the result with `insta::assert_yaml_snapshot!`.
3. **Integration tests** — under `tests/`, one file per surface area
   (`auth.rs`, `library_movie.rs`, …). Spin up `wiremock::MockServer`,
   register `Mock::given(...).respond_with(ResponseTemplate::new(200).set_body_string(fixture))`,
   point a `PlexServer::builder().base_url(server.uri())` at it, drive
   one full user flow, assert on both `MockServer::received_requests()`
   (headers/path/query) **and** the returned domain type.

### 3.2 `wiremock` patterns

- One `MockServer` per `#[tokio::test]`. Spawn cost is ~1 ms; don't
  share.
- Register every request the test expects, even ones whose body the test
  doesn't care about — `.expect(1)` catches accidental retry storms.
- For "MyPlex needs to call plex.tv *and* the discovered server":
  start *two* `MockServer`s, mount account discovery on the first, return
  a `<Connection uri="http://<port-2>"/>` blob that points at the second.
- Build a small helper, `tests/support/mock.rs`:
  ```rust
  pub async fn pms() -> MockServer { MockServer::start().await }
  pub fn xml(path: &str) -> String {
      std::fs::read_to_string(format!("tests/fixtures/{path}")).unwrap()
  }
  ```
  Keeps individual tests one-liner-ish.

### 3.3 `insta` snapshot conventions

- Use `assert_yaml_snapshot!` (YAML > JSON for human review of nested
  structs).
- Configure redactions for any field that legitimately changes
  request-to-request even on a deterministic fixture (`view_count`, a
  generated `ratingKey` on POST responses, etc.).
- Run `cargo insta review` locally; commit `.snap` files. CI runs `cargo
  insta test --check` to fail on uncommitted changes.

### 3.4 Fixture capture, once

`examples/dump_fixtures.rs` (per CLAUDE.md §9.5):

1. Reads `PLEX_TEST_BASEURL` + `PLEX_TEST_TOKEN` from env.
2. Hits each documented endpoint, writes raw response bodies to
   `tests/fixtures/<surface>/<endpoint>.{xml,json}`.
3. Pipes each through the sanitiser (§9) before writing.
4. Emits a `tests/fixtures/MANIFEST.toml` recording PMS version, capture
   timestamp, and SHA-256 of each body so we can detect drift later.

The dumper is run **manually**, never in CI. Re-capturing happens when we
intentionally upgrade the parity baseline.

### 3.5 `--features live-tests` (off by default, off in CI)

In `Cargo.toml`:

```toml
[features]
live-tests = []
```

Live-server tests use `#[cfg(feature = "live-tests")]` and additionally
`#[ignore]`-fence themselves so a stray `cargo test --features
live-tests` without `PLEX_TEST_TOKEN` exits cleanly. The runner reads
`PLEX_TEST_BASEURL` + `PLEX_TEST_TOKEN` and skips with `eprintln!` +
`return` if either is absent. CI never sets this flag.

### 3.6 `rstest` for matrix cases

Use `#[rstest]` + `#[case]` for parser-input matrices, e.g.:

```rust
#[rstest]
#[case::movie("movie.xml", LibraryKind::Movie, 4)]
#[case::show("show.xml",   LibraryKind::Show,  2)]
#[case::music("music.xml", LibraryKind::Music, 1)]
fn library_section_parses(#[case] fixture: &str, #[case] kind: LibraryKind, #[case] expected_count: usize) { ... }
```

This is the Rust analogue of the `ANON_PARAM`/`AUTH_PARAM` pattern.

### 3.7 WebSocket / AlertListener tests

The `tools/plex-alertlistener.py` script and the python `AlertListener`
consume `ws(s)://<server>/:/websockets/notifications`. For our equivalent:

- Use `tokio-tungstenite` directly as both client (library code) and
  server (test fixture).
- A tiny fixture in `tests/support/ws.rs`:
  ```rust
  pub async fn ws_fixture(events: Vec<serde_json::Value>) -> (SocketAddr, JoinHandle<()>) {
      let listener = TcpListener::bind("127.0.0.1:0").await?;
      let addr = listener.local_addr()?;
      let handle = tokio::spawn(async move { /* accept, upgrade, send each event, close */ });
      (addr, handle)
  }
  ```
- Tests assert that the parsed `Event` enum matches the published
  payload, that the reader survives a `Ping`/`Pong`, and that a server-
  initiated close propagates to the caller as `Error::Internal("ws
  closed")`.

`wiremock` does not handle WebSockets, so this small handwritten harness
is unavoidable. Keep it in `tests/support/` and reuse across files.

---

## 4. Coverage gate

CLAUDE.md §9 mandates **≥90 % line, ≥85 % branch** and §10 wires it into
CI as:

```
cargo llvm-cov --all-features --fail-under-lines 90
```

Enforcement details:

- Run `cargo llvm-cov --workspace --all-features --fail-under-lines 90
  --fail-under-functions 90 --branch --fail-under-branches 85` in the
  `coverage` job.
- Upload `lcov.info` as an artefact + (optionally) push to Codecov so
  per-file coverage shows up in the PR.
- The 90/85 gate is the floor. PRs that move coverage down by >1 % require
  a justification line in the description; PRs that move it up by adding
  parser fixtures are the norm.

Branch coverage on a parser is mostly XML element-presence branches.
Make sure every `Option<T>` field has at least one fixture with the field
present and one without (`rstest` cases are perfect for this).

---

## 5. Naming and layout

### 5.1 Test files

Already partially listed in CLAUDE.md §3. Concretely:

```
tests/
├── auth.rs                       # token sign-in, password+2FA, PIN/OAuth
├── myplex_account.rs             # user, subscription, profile
├── myplex_resources.rs           # resources, connection picking
├── myplex_friends.rs             # invites, shared sources, updateFriend
├── server_identity.rs            # capabilities, system endpoint
├── server_sessions.rs            # current + transcode sessions, stop session
├── server_history.rs
├── server_settings.rs
├── library.rs                    # listing, refresh, scan, delete
├── library_movie.rs              # MovieSection all/search/recentlyAdded/hubs
├── library_show.rs               # show/season/episode traversal
├── library_music.rs
├── library_photo.rs
├── library_filters.rs            # FilterBuilder + field discovery
├── search.rs                     # universal search + searchV2 hubs
├── media_edit.rs                 # title/summary/poster upload, mark watched
├── playlist.rs                   # audio/video/photo playlists CRUD
├── collection.rs                 # collections CRUD + smart mode
├── playqueue.rs
├── transcode.rs                  # transcode URL builder + decision endpoint
├── playback_client.rs            # PlexClient remote control
├── sync.rs                       # legacy mobile sync (best-effort)
├── webhook.rs                    # payload deser + axum extractor
├── alertlistener.rs              # ws notifications
└── support/
    ├── mod.rs
    ├── mock.rs                   # wiremock helpers
    ├── ws.rs                     # tokio-tungstenite fixture
    ├── fixtures.rs               # load_xml / load_json helpers
    └── redactions.rs             # shared insta redaction sets
```

`tests/support/mod.rs` is exposed via `#[path]` from each test file:

```rust
#[path = "support/mod.rs"]
mod support;
```

— or via a `tests/support/lib.rs` if we want it as an actual crate-style
module (latter is cleaner; pick one and stick with it).

### 5.2 Fixture directory

One folder per surface area, one file per endpoint:

```
tests/fixtures/
├── README.md                     # capture + sanitization rules (this doc, condensed)
├── MANIFEST.toml                 # PMS version, capture date, body sha256 per file
├── myplex/
│   ├── user.xml                  # GET /api/v2/user
│   ├── resources.xml             # GET /api/v2/resources
│   ├── friends.xml               # GET /api/users
│   ├── invite.xml                # POST /api/v2/shared_servers (response)
│   ├── pin_create.json           # POST /api/v2/pins (json variant)
│   └── pin_poll.json             # GET /api/v2/pins/<id>
├── server/
│   ├── identity.xml              # GET /identity
│   ├── capabilities.xml          # GET /
│   ├── sessions.xml              # GET /status/sessions
│   ├── transcode_sessions.xml    # GET /transcode/sessions
│   ├── history.xml               # GET /status/sessions/history/all
│   ├── settings.xml              # GET /:/prefs
│   └── system_accounts.xml       # GET /accounts
├── library/
│   ├── sections.xml              # GET /library/sections
│   ├── movie_section_all.xml
│   ├── movie_section_search.xml
│   ├── movie_section_hubs.xml
│   ├── show_section_all.xml
│   ├── show_episodes.xml
│   ├── music_section_all.xml
│   ├── music_album_tracks.xml
│   ├── photo_section_all.xml
│   ├── filter_fields_movie.xml
│   └── search_v2.json
├── media/
│   ├── movie_detail.xml
│   ├── episode_detail.xml
│   ├── track_detail.xml
│   └── photo_detail.xml
├── playlist/
│   ├── playlist_audio.xml
│   ├── playlist_video.xml
│   └── playlist_smart.xml
├── collection/
│   ├── collection_static.xml
│   └── collection_smart.xml
├── playqueue/
│   ├── create.xml
│   └── advance.xml
├── transcode/
│   └── decision.xml
├── webhook/
│   ├── play.json
│   ├── pause.json
│   ├── resume.json
│   ├── stop.json
│   ├── scrobble.json
│   ├── rate.json
│   ├── library_new.json
│   └── library_on_deck.json
└── errors/
    ├── 401_unauthorized.xml      # for error-mapping tests
    ├── 404_not_found.xml
    └── 500_internal.xml
```

Naming rule: `<endpoint-shape>[_<variant>].{xml,json}`. The variant suffix
distinguishes e.g. "with images present" / "without images" / "with
guids" cases that `rstest` will iterate over.

---

## 6. Sanitisation rules

Every fixture passes through the sanitiser before being written to disk.
The sanitiser is one Rust function in `examples/dump_fixtures.rs` (or
behind a `--bin sanitize` if we promote it later) and is itself
unit-tested in `src/util/sanitize.rs::tests`.

| What to scrub                              | Regex (Rust `regex` crate syntax)                                  | Replacement                            |
| ------------------------------------------ | ------------------------------------------------------------------ | -------------------------------------- |
| X-Plex-Token query param                   | `(?i)([?&])(X-Plex-Token)=([^&"\s]+)`                              | `${1}${2}=REDACTED_TOKEN`              |
| X-Plex-Token attribute / header value      | `(?i)(token|authToken|accessToken)="([^"]+)"`                      | `${1}="REDACTED_TOKEN"`                |
| `X-Plex-Client-Identifier` value           | `(?i)(X-Plex-Client-Identifier)["=:\s]+["']?([0-9a-f-]{8,})`       | `${1}="REDACTED_CLIENT_ID"`            |
| Server `machineIdentifier`                 | `machineIdentifier="([0-9a-f]{40})"`                               | `machineIdentifier="REDACTED_MACHINE_ID"` |
| Sonos `RINCON_...` identifiers             | `RINCON_[0-9A-F]{16,}:\d+`                                         | `RINCON_PLACEHOLDER:0000000000`        |
| Generic UUIDs (server uuid, session uuid)  | `\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b` | `00000000-0000-0000-0000-000000000000` |
| RFC1918 IPv4 (private LAN)                 | `\b(10\.(?:\d{1,3}\.){2}\d{1,3}|192\.168\.\d{1,3}\.\d{1,3}|172\.(?:1[6-9]|2\d|3[0-1])\.\d{1,3}\.\d{1,3})\b` | `192.0.2.1` (TEST-NET-1) |
| Public IPv4 (likely external/relay IP)     | `\b(?:\d{1,3}\.){3}\d{1,3}\b` (applied **after** RFC1918 pass)     | `198.51.100.1` (TEST-NET-2)            |
| IPv6 globally-routable addresses           | `\b([0-9a-fA-F]{1,4}:){7}[0-9a-fA-F]{1,4}\b`                       | `2001:db8::1`                          |
| Email addresses                            | `[A-Za-z0-9_.+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+`               | `testuser@example.com`                 |
| plex.tv usernames in `username="..."`      | `username="([^"]+)"`                                               | `username="testuser"`                  |
| plex.tv friendly names                     | `friendlyName="([^"]+)"`                                           | `friendlyName="Test User"`             |
| Avatar / thumb URLs with user-id segments  | `https://plex\.tv/users/[0-9a-f]+/avatar\?c=\d+`                   | `https://plex.tv/users/REDACTED/avatar?c=0` |
| Numeric user IDs                           | ` id="\d{6,}"` *(scoped to `<User`/`<Account` elements; do not blast across the file)* | ` id="12345"` |
| Plex Pass entitlement device IDs           | `\bdeviceId="[0-9a-f-]{16,}"`                                      | `deviceId="REDACTED_DEVICE_ID"`        |
| Filesystem paths in `<Part file="...">`    | `file="(/[^"]+)"`                                                  | `file="/data/redacted/path"`           |
| `Server name="..."` (server display name)  | `<Server\s+name="([^"]+)"`                                         | `<Server name="testserver"`            |

Ordering matters: run the *more specific* patterns first (token, machine
identifier, UUID) before the *more general* ones (IPv4-any, email), and
strip RFC1918 before stripping arbitrary IPv4 so we keep some semantic
distinction between LAN and WAN addresses in fixtures.

The sanitiser must be **idempotent**: running it twice on the same input
produces the same output. Unit test:

```rust
#[test]
fn sanitize_is_idempotent() {
    let input = include_str!("../tests/fixtures-raw/library/sections.xml");
    let once = sanitize(input);
    let twice = sanitize(&once);
    assert_eq!(once, twice);
}
```

Plus one test per regex row that asserts the row matches at least one
real-world example and produces the expected replacement.

---

## 7. What can be unit-tested without any HTTP

This is where 60-70 % of our coverage will come from. None of the
following requires `wiremock`:

1. **Header builder** (`src/headers.rs`)
   - Required `X-Plex-*` headers always present.
   - `X-Plex-Token` only emitted when a token is configured.
   - `X-Plex-Client-Identifier` round-trips a configured identifier
     verbatim.
   - `Debug` impl on `PlexToken` prints `"***redacted***"`.
   - Header map merge with caller-supplied extras preserves required
     ones.

2. **URL builder** (`src/client.rs::Url::join` wrappers)
   - Base URL + path → absolute URL.
   - Query string composition: `_buildQueryKey` equivalent that
     appends `includeGuids=1` and arbitrary kwargs in stable order
     (see python `test_build_query_key`).
   - Preserves existing query string (`?foo=bar`) and uses `&` instead
     of `?` on the appended segment.
   - URL-encodes values containing `&`, `=`, `/`, spaces, unicode.

3. **Filter builder** (`src/library/filters.rs`)
   - Typestate transitions: cannot call `.execute()` without a field set.
   - Operator validation per field type (string vs int vs boolean vs
     tag vs date).
   - Sort direction parsing.
   - Round-trip with the wire format (`title__icontains=foo` →
     query-string equivalent).

4. **Error mapping** (`src/client.rs::map_response`)
   - 401 → `Error::Unauthorized`.
   - 404 → `Error::NotFound(path)`.
   - 4xx → `Error::Api { status, message }` with the body parsed as
     either `<Response code message/>` XML or plain text fallback.
   - 5xx + transient network → `Error::Transport`.
   - Timeout future → `Error::Timeout(duration)`.

5. **Parser DTOs** (`src/xml/dto/*` and `src/json/dto/*`)
   - One unit test per DTO struct that loads a small inline literal
     string and checks every public field. This is the meat of branch
     coverage.
   - Optional fields: present + absent fixtures (`rstest` matrix).
   - Enum dispatch via `type=` attribute (`movie`, `show`, `season`,
     `episode`, `artist`, `album`, `track`, `photo`, `collection`,
     `playlist`).

6. **DTO → domain conversion** (`From`/`TryFrom` impls)
   - Field renames are correct.
   - Plex epoch-milliseconds → `chrono::DateTime<Utc>` round-trip,
     including the `0` / `null` cases (return `None`).
   - `RatingKey`/`MachineIdentifier` newtype construction rejects empty
     strings.

7. **Time utilities** (`src/util/time.rs`)
   - `from_plex_epoch_ms` and inverse.
   - DST-insensitive (UTC only).

8. **ID newtypes** (`src/util/ids.rs`)
   - `Display`, `FromStr`, serde round-trip.
   - `PlexToken` redaction in `Debug` *and* in `Display` (yes, both).

9. **Sanitiser** (above) — every regex row.

10. **Config builder** (`src/config.rs`)
    - Required fields enforced.
    - Defaults sensible (timeout, user agent).
    - `ClientConfig::generated()` produces a v4 UUID identifier.

Only after these are green do we move to fixture-backed parser tests,
and only then to `wiremock` integration tests. Per CLAUDE.md §9.4 we
**never** rely on a real PMS in CI.

---

## 8. Equivalences cheat-sheet (python ↔ rust)

| python-plexapi                                       | plex-rs equivalent                                          |
| ---------------------------------------------------- | ----------------------------------------------------------- |
| `conftest.py` session fixtures                       | `tests/support/mock.rs` helpers + `OnceLock` for shared state |
| `plex` fixture (live PMS)                            | `wiremock::MockServer` per test                             |
| `account` / `mocked_account`                         | `wiremock` + `tests/fixtures/myplex/user.xml`               |
| `payloads.py::ACCOUNT_XML`                           | `tests/fixtures/myplex/user.xml`                            |
| `pytest_runtest_setup` skip-by-env                   | `#[cfg(feature = "live-tests")]` + early `return`           |
| `@pytest.mark.authenticated` parametrise             | `#[rstest] #[case::with_token(...)] #[case::anon(...)]`     |
| `@pytest.mark.xfail`                                 | **forbidden** — fix the underlying issue or `#[ignore]` with a tracked issue link (CLAUDE.md §14.7) |
| `tools/plex-bootstraptest.py` Docker bootstrap       | `examples/dump_fixtures.rs` capture (run once, manually)    |
| `tests/data/*.{mp3,mp4,jpg}` binary stubs            | **none** — we do not test binary upload paths in CI; cover those under `--features live-tests` only |
| `is_datetime`, `is_int`, `is_string` assertion helpers | direct `assert_eq!` on typed fields (the type system replaces these) |
| `wait_until` polling helper                          | `tokio::time::timeout(...).await` against a future          |
| `requests_mock`                                      | `wiremock`                                                  |
| `mocker.patch` (`pytest-mock`)                       | not needed — pass collaborators in via constructors         |

---

## 9. Operational checklist (per PR)

Every PR that touches an endpoint must:

1. Capture or update the fixture under `tests/fixtures/<surface>/...`,
   sanitised.
2. Add or extend the parser DTO + domain conversion.
3. Add at least one unit test for each new public function.
4. Add at least one `wiremock` integration test (happy path) and one
   failure-path test (404 / 401 / malformed body).
5. Update `tests/fixtures/MANIFEST.toml` if the fixture is new.
6. Run `cargo llvm-cov --all-features --fail-under-lines 90
   --fail-under-branches 85` locally before pushing.
7. Tick the parity checklist in `docs/parity.md` (per CLAUDE.md §13).

Test files that cannot honour these without a live server (sync,
mDNS/GDM discovery) must live behind `--features live-tests` and ship
with a "what this would cover" doc comment so reviewers can see the
intended scope.

---

## 10. Summary

- python-plexapi tests by provisioning a real PMS in Docker, populating
  it with named stub media, and asserting against the live HTTP surface.
  This is great as a behavioural spec but a bad fit for our CI gate.
- We adopt the *fixture data* from python-plexapi's `payloads.py` and the
  *fixture-naming convention* from its directory structure, but execute
  every test through `wiremock` against sanitised captures.
- The strict 90 % line / 85 % branch coverage gate (CLAUDE.md §9, §10)
  forces us to push coverage into pure unit tests on parsers, builders,
  and error mapping — exactly where python-plexapi is *thinnest* because
  it gets that coverage incidentally via live calls.
- A `--features live-tests` escape hatch exists for the handful of
  capabilities (sync, mDNS discovery, real-world WS) that genuinely
  cannot be mocked end-to-end. CI never enables it.
- Sanitisation is a hard invariant: tokens, machine identifiers, IPs,
  emails, usernames, server UUIDs never land in a fixture. The
  sanitiser is itself unit-tested for idempotency and per-regex
  correctness.
