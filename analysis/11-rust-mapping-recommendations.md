# 11 — Rust Mapping Recommendations (Synthesis)

Status: **load-bearing**. This document is the prescriptive port plan for
`plex-rs`. It supersedes ad-hoc decisions in `CLAUDE.md` where the two
disagree (see §4 for explicit deltas). Reading this document plus
`CLAUDE.md` should be sufficient for a future contributor to know what to
build, in what order, with what trade-offs.

All file references of the form `analysis/0N-*.md§N` cite the prior ten
analyses; references of the form `python-plexapi/plexapi/<file>:<line>` cite
the python-plexapi tree at the project root.

---

## 1. Architectural overview

`plex-rs` is a six-layer stack. Each layer has a single concern, a
narrow interface to the layer above, and is independently testable.

```
┌─────────────────────────────────────────────────────────────────────┐
│ 6. Public API surface         lib.rs re-exports; doc-tour entry     │
├─────────────────────────────────────────────────────────────────────┤
│ 5. Mixin / trait layer        ~50 small capability traits           │
│                               (Ratable, Playable, EditField, …)     │
│                               drives compile-time capability matrix │
├─────────────────────────────────────────────────────────────────────┤
│ 4. Domain layer               Movie, Show, Track, Collection, …     │
│                               LibrarySection, MyPlexAccount, …      │
│                               public, owned, no parse types leak    │
├─────────────────────────────────────────────────────────────────────┤
│ 3. DTO layer                  xml::dto / json::dto                  │
│                               wire-format mirrors, serde-deser,     │
│                               crate-private, MediaContainer<T>      │
├─────────────────────────────────────────────────────────────────────┤
│ 2. Request layer              one method per Plex endpoint group    │
│                               builds URL+headers+query, decodes     │
│                               envelope, maps status → Error         │
├─────────────────────────────────────────────────────────────────────┤
│ 1. Transport layer            HttpClient: reqwest::Client +         │
│                               ClientIdentity + retry + token redact │
└─────────────────────────────────────────────────────────────────────┘
```

| Layer | Owns | Sourced from |
|---|---|---|
| 1 Transport | `reqwest::Client`, pool tuning, retry policy, `X-Plex-*` header injection, token redaction, scheme map (`http/https → ws/wss`) | `analysis/02§5`, `analysis/09§5.1` |
| 2 Request | Per-tag operation methods, pagination via `X-Plex-Container-*` headers, image/transcode URL builders, status→Error mapping | `analysis/01§4.2`, `analysis/02§3` |
| 3 DTO | `MediaContainer<T>` envelope (single generic, replaces 12 OpenAPI envelopes), tagged unions for `<Stream>`, `<Hub>`, all `_loadData` field sets as `#[derive(Deserialize)]` | `analysis/01§5`, `analysis/06§2` |
| 4 Domain | `Movie`, `Show`, `Season`, `Episode`, `Clip`, `Artist`, `Album`, `Track`, `Photoalbum`, `Photo`, `Playlist`, `Collection`, `Library`, `LibrarySection*`, `PlexServer`, `MyPlexAccount`, `PlexClient`, `PlayQueue` | `analysis/04`, `analysis/05`, `analysis/06`, `analysis/07` |
| 5 Trait | Compile-time capability matrix: `Ratable`, `Playable`, `EditField`, `EditTags`, `HasArt/Poster/Theme/Logo/SquareArt`, `PlayedUnplayed`, `Watchlistable`, `Splittable`, `Matchable`, … | `analysis/08§2`, §5 of this doc |
| 6 Public | `lib.rs` re-exports, prelude module, optional `webhook` feature, optional `live-tests` feature | `CLAUDE.md§3`, `analysis/10§3.5` |

Two cross-cutting concerns sit beside the layered stack:

- **Auth state machine** (`auth/` module) is a typestate-driven sub-API
  that owns the three sign-in flows; it produces a `PlexToken` that
  feeds layer 1. See §9.
- **Discovery & real-time** (`discover/` and `alerts/` modules) bypass
  layer 2 entirely — GDM uses raw UDP, WebSocket alerts use
  `tokio-tungstenite`. See §8 and §4.

---

## 2. Module layout (revised)

`CLAUDE.md§3`'s tentative tree is mostly right but conflates four
distinct surfaces under `myplex/`, mis-categorises GDM as mDNS, and
under-models the DTO layer. The revision below is what the crate should
look like at v1.0. `// CHANGED:` annotations mark every line that
differs from `CLAUDE.md§3`.

```
plex-rs/
├── Cargo.toml
├── CLAUDE.md
├── README.md
├── LICENSE
├── deny.toml
├── rust-toolchain.toml
├── .github/workflows/ci.yml
├── src/
│   ├── lib.rs                      # crate root: re-exports + crate docs
│   ├── prelude.rs                  # CHANGED: explicit prelude for ergonomics
│   ├── client.rs                   # HttpClient: reqwest wrapper, retries
│   ├── config.rs                   # ClientConfig builder
│   ├── error.rs                    # Error enum + Result alias
│   ├── headers.rs                  # X-Plex-* header construction
│   ├── pagination.rs               # CHANGED: header-based paginator (analysis/01§4.2)
│   ├── uri.rs                      # CHANGED: PlexUri enum + parser/display (§6)
│   ├── auth/
│   │   ├── mod.rs                  # MyPlexAccount, token sign-in
│   │   ├── pin.rs                  # PIN/OAuth flow
│   │   ├── jwt.rs                  # CHANGED: JWT flow (analysis/03§2.4)
│   │   ├── password.rs             # CHANGED: split user/pass + 2FA state machine
│   │   └── token.rs                # PlexToken newtype + redaction in Debug
│   ├── myplex/                     # plex.tv account API (NOT discover/metadata)
│   │   ├── mod.rs                  # MyPlexAccount user + subscription + ping
│   │   ├── resources.rs            # MyPlexResource + parallel connect race
│   │   ├── devices.rs              # MyPlexDevice
│   │   ├── friends.rs              # CHANGED: invites, shares, sharing settings
│   │   ├── webhooks.rs             # CHANGED: webhook list management
│   │   ├── home.rs                 # CHANGED: home users, switch, managed PINs
│   │   ├── sonos.rs                # CHANGED: PlexSonosClient relay
│   │   └── claim.rs                # CHANGED: claim/unclaim + claim token
│   ├── discover/                   # CHANGED: split out from myplex/
│   │   ├── mod.rs                  # watchlist (Discover service)
│   │   ├── search.rs               # JSON-only Discover search (analysis/03§6.4)
│   │   └── availability.rs         # streamingServices
│   ├── metadata_provider/          # CHANGED: split out from myplex/
│   │   ├── mod.rs                  # metadata.provider.plex.tv pseudo-server
│   │   ├── user_state.rs           # /library/metadata/<rk>/userState
│   │   └── scrobble.rs             # mark played/unplayed via GET (preserve wire bug)
│   ├── server/
│   │   ├── mod.rs                  # PlexServer: capabilities, identity, account
│   │   ├── sessions.rs             # current + transcode sessions, terminate
│   │   ├── history.rs              # /status/sessions/history/all
│   │   ├── system.rs               # SystemAccount, SystemDevice, agents
│   │   ├── butler.rs               # CHANGED: split — was lumped under system
│   │   ├── activities.rs           # CHANGED: + cancellation
│   │   ├── settings.rs             # PreferencesAdapter, /:/prefs
│   │   ├── updater.rs              # CHANGED: split — checkForUpdate/installUpdate
│   │   ├── statistics.rs           # CHANGED: bandwidth/resources
│   │   ├── transcode.rs            # transcode image URL builder
│   │   └── browse.rs               # CHANGED: /services/browse filesystem ops
│   ├── library/
│   │   ├── mod.rs                  # Library aggregator
│   │   ├── section.rs              # LibrarySection + 4 typed subclasses
│   │   ├── search.rs               # /hubs/search, section search
│   │   ├── filters.rs              # FilterBuilder + field discovery (§7)
│   │   ├── smart_filter.rs         # CHANGED: push/pop URI parser (analysis/05§7)
│   │   ├── hubs.rs                 # CHANGED: Hub + ManagedHub
│   │   └── tags.rs                 # CHANGED: LibraryMediaTag family
│   ├── media/
│   │   ├── mod.rs                  # Re-exports + LibraryItem enum
│   │   ├── video.rs                # Movie, Show, Season, Episode, Clip, Extra
│   │   ├── audio.rs                # Artist, Album, Track
│   │   ├── photo.rs                # Photoalbum, Photo
│   │   ├── playlist.rs             # Playlist
│   │   ├── collection.rs           # Collection
│   │   ├── media_stream.rs         # CHANGED: Media → MediaPart → Stream chain
│   │   ├── tags.rs                 # CHANGED: 14 MediaTag subclasses collapsed
│   │   ├── markers.rs              # CHANGED: Marker + Chapter
│   │   └── resources.rs            # CHANGED: image/poster/art/logo/theme/squareArt
│   ├── playback/
│   │   ├── mod.rs                  # PlayQueue
│   │   ├── client.rs               # PlexClient remote control
│   │   ├── transcode.rs            # /transcode/universal URL builder
│   │   └── sync.rs                 # CHANGED: legacy mobile sync (SyncItem)
│   ├── traits/                     # CHANGED: capability traits + capabilities! macro
│   │   ├── mod.rs                  # PlexObject, Reload
│   │   ├── editable.rs             # EditField, EditTags supertraits
│   │   ├── playable.rs             # Playable, PlayedUnplayed
│   │   ├── ratable.rs              # Ratable
│   │   ├── images.rs               # HasArt, HasPoster, HasLogo, HasSquareArt, HasTheme
│   │   ├── search.rs               # Splittable, Matchable, Watchlistable
│   │   └── capabilities.rs         # CHANGED: capabilities! macro driving impls
│   ├── alerts/                     # CHANGED: was implicit; promoted
│   │   ├── mod.rs                  # AlertStream (Stream<Item = Result<AlertEvent>>)
│   │   ├── event.rs                # AlertEvent enum (#[serde(tag="type")])
│   │   └── backoff.rs              # CHANGED: exponential reconnect (added vs Python)
│   ├── discover_gdm/               # CHANGED: was "discovery"; clarifies it's NOT mDNS
│   │   ├── mod.rs                  # GDM raw-UDP scan (analysis/09§5.2)
│   │   └── entry.rs                # GdmEntry parsed reply
│   ├── webhook/                    # CHANGED: was webhook.rs; needs separate module
│   │   ├── mod.rs                  # WebhookEvent + WebhookEventType enum
│   │   └── axum_extractor.rs       # #[cfg(feature = "webhook-axum")]
│   ├── xml/                        # CHANGED: promoted to module folder
│   │   ├── mod.rs                  # MediaContainer<T> generic envelope
│   │   └── dto/                    # CHANGED: per-surface DTOs
│   │       ├── metadata.rs         # <Video> / <Track> / <Photo> / <Directory>
│   │       ├── media.rs            # <Media> + <Part> + <Stream>
│   │       ├── library.rs          # <Directory> for sections, filters, hubs
│   │       ├── alerts.rs           # WebSocket notification frame DTOs
│   │       └── shared.rs           # tag, guid, rating, image, chapter, marker
│   ├── json/                       # CHANGED: parallel to xml/ for JSON-only paths
│   │   └── dto/
│   │       └── discover_search.rs  # JSON search results (analysis/03§6.4)
│   ├── util/
│   │   ├── mod.rs
│   │   ├── ids.rs                  # RatingKey, MachineIdentifier, ClientIdentifier
│   │   ├── time.rs                 # Plex epoch-ms <-> chrono
│   │   ├── sanitize.rs             # CHANGED: fixture sanitiser (analysis/10§6)
│   │   └── search_type.rs          # CHANGED: SearchType u8 enum + map
│   └── batch.rs                    # CHANGED: explicit batch-edit transaction
├── tests/
│   ├── support/                    # mock helpers
│   ├── fixtures/                   # captured + sanitised wire bodies
│   ├── auth.rs
│   ├── library_*.rs
│   ├── playback_*.rs
│   └── … (per analysis/10§5.1)
├── examples/
│   ├── list_libraries.rs
│   ├── search_movie.rs
│   ├── pin_signin.rs
│   ├── webhook_server.rs
│   └── dump_fixtures.rs            # CHANGED: per analysis/10§3.4
└── benches/
    └── parse_library.rs
```

### Key separation decisions

1. **`myplex/` vs `discover/` vs `metadata_provider/`** *(analysis/03§11.1)*
   `python-plexapi` lumps six distinct hosts under one `myplex.py`. They
   have different auth requirements, different content-types, and
   different semantics:
   - `myplex/` covers `plex.tv` and `clients.plex.tv` (account state,
     auth, devices, friends, webhooks, claim, Sonos relay).
   - `discover/` covers `discover.provider.plex.tv` (watchlist,
     Discover-only search returning JSON, streaming-service availability).
     The XML round-trip wart documented in `analysis/03§6.4` is fixed
     here — we parse the JSON directly.
   - `metadata_provider/` covers `metadata.provider.plex.tv`
     (per-user state on Discover items, GET-method scrobble for
     watchlist items). This host is a *pseudo-server* in
     python-plexapi — see `analysis/03§6.1`.

2. **`discover_gdm/` is NOT mDNS** *(analysis/09§2)*
   Plex's local discovery is HTTP-over-UDP using `M-SEARCH * HTTP/1.0`
   on `239.0.0.250:32414` (multicast) for servers and
   `255.255.255.255:32412` (broadcast) for clients. mDNS uses different
   ports, different multicast groups, and DNS message format. **Drop
   the `mdns-sd` dependency from `Cargo.toml`.** Use raw
   `tokio::net::UdpSocket` with `set_multicast_ttl_v4(1)` /
   `set_broadcast(true)`. Module name disambiguates from `discover/`
   (which is the cloud catalogue).

3. **DTOs live in a `xml::dto` module, not a separate crate**
   *(analysis/01§9)*. The hybrid recommendation in `analysis/01§9` is
   "hand-write the operation surface, codegen the schemas only". We
   refine that further: we do *not* run `progenitor` over the spec at
   all. The 22 uppercase `$ref` aliases, the 5 undeclared tags, the
   non-standard path syntax, the `text/html` errors, and the ~25
   missing endpoints all conspire to make pure codegen
   counter-productive (`analysis/01§8`). Instead, the DTO layer is
   hand-written under `src/xml/dto/` and `src/json/dto/`, deserialized
   with `serde` + `quick-xml`, and converted to public domain types via
   `From`/`TryFrom`. The `openapi.json` becomes documentation, not a
   build input.

4. **Parent-section back-reference for edits** *(analysis/08§3.2)*
   The PMS edit endpoint is
   `PUT /library/sections/<sectionKey>/all?id=<ratingKey>…`, not the
   intuitive `/library/metadata/<ratingKey>?…`. Every editable leaf
   therefore needs to reach a `LibrarySection`. Python does this by
   holding a `weakref` to the parent and walking up. Rust holds a
   `LibrarySectionRef { id: u32, server: Arc<HttpClient> }` directly on
   every leaf. `LibrarySectionRef` lives in `src/library/section.rs`
   and is constructed at parse time from the
   `librarySectionID`/`librarySectionKey`/`librarySectionTitle`
   attributes that every leaf XML element carries.

5. **`traits/capabilities.rs` macro** *(analysis/08§13, Option D)*
   The mixin matrix has ~50 traits × ~12 leaves and ~0.4 density. We
   express it via a `capabilities!` declarative macro whose input is
   essentially the matrix in `analysis/08§2`. The macro emits the
   `impl CapTrait for Leaf {}` lines and nothing else; method bodies
   live in trait `default fn`s. This is the only macro in the crate.

---

## 3. Pattern translation table

Every load-bearing python idiom and its mandated Rust translation.

| Python idiom | Source citation | Rust translation |
|---|---|---|
| `__getattribute__` checks "is this `None` or `[]`? → maybe issue HTTP" (`base.py:634-652`) | `analysis/02§2` | **Explicit `Partial<T>` / `Loaded<T>` typestate.** `Movie<Partial>` exposes only fields that came on the first fetch; `Movie<Loaded>` exposes the full set. Promote with `.fetch().await -> Movie<Loaded>`. Never hide I/O behind field access. Eliminates the runtime "is this partial?" check. |
| `_loadData(elem)` dispatching on `TAG.TYPE.session.history` keys (`base.py:127-144`, `utils.py:132-160`) | `analysis/02§4` | **Tagged-enum serde deserialize.** `#[serde(tag = "type")] enum MetadataItem { #[serde(rename="movie")] Movie(MovieDto), … }`. Session/history are *not* a parallel enum — they're a struct composition: `Session<T> { common: SessionCommon, item: T }`. Photo libraries can contain `Clip` (`analysis/06§2.11`); make children of `Photoalbum` deserialize into `enum PhotoalbumChild { Photoalbum, Photo, Clip }`. |
| `**kwargs` filter DSL (`Genre__tag="Action"`, `viewCount__gte=0`) (`base.py:227-313`, `library.py:1062-1146`) | `analysis/02§3`, `analysis/05§4-§5` | **`FilterBuilder` typestate.** See §7. Distinguishes the two operator namespaces statically: `.client(|q| q.summary().icontains("hero"))` for client-side, `.server(|q| q.genre().eq("Action").year().gt(1990))` for server-side. Composable in one call. Smart-filter `push=/pop=` parser is a separate parser-only module. |
| Multiple inheritance mixins (`mixins/__init__.py:34-223`) | `analysis/08` | **Extension traits + `capabilities!` macro** (Option B+D hybrid). ~50 capability traits with `default fn` bodies. Leaf types get a generated block of impl markers. Compile-time matrix; `clip.rate()` is a compile error, not an HTTP 400. |
| `requests.Session` as a per-object/global handle (`server.py:107`, `myplex.py:134`) | `analysis/02§5` | **Single injected `HttpClient` per `PlexServer`/`MyPlexAccount`.** Built once in `ClientConfig::build()` with retry/backoff (added — Python has none), connect/read timeouts split, `rustls` by default. `HttpClient` is `Clone` (cheap — wraps `reqwest::Client`'s internal `Arc`). Cross-server playlists *share* the client across `PlexServer` instances (`analysis/07§4.6`). |
| `PLEXOBJECTS = {}` mutable global registry (`utils.py:105`) | `analysis/02§4` | **Compile-time `enum MetadataItem`.** No runtime dispatch. The four `_loadData` registry keys (`TAG`, `TAG.TYPE`, `TAG.TYPE.session`, `TAG.TYPE.history`) become serde tag/content discriminators on three small enums: `MetadataItem`, `Session`, `History`. |
| `utils.threaded(callback, listargs)` daemon-thread fan-out (`utils.py:309-330`) | `analysis/02§7`, `analysis/03§3.3` | **`futures::future::join_all` for the simple case, `tokio::task::JoinSet` for the resource-connect race** (which needs early-exit on first success when `X_PLEX_ENABLE_FAST_CONNECT` is set). No daemon threads, no shared-mutable result list, no `time.sleep(0.05)` polling. |
| ETree XML scrape via `_loadData` field-by-field assignment (`server.py:114-154`, every leaf) | `analysis/02§1`, `analysis/06§2` | **`quick-xml` serde-deserialize into DTO**, then `From::<Dto> for Domain`. The DOM is dropped after parse — no equivalent to Python's `_data` retention (`analysis/06§D`). Trade-off: lose the `cached_data_property` lazy-rebuild trick; pay it back by parsing everything upfront. Wire bytes are typically <100 KB; upfront cost is in the microseconds. |
| `etag` kwarg meaning "element tag, not HTTP cache header" (`base.py:399, :533`) | `analysis/02§8.6` | **Rename to `element_tag` in every Rust API.** The `etag` overload is documented but a footgun; it does not exist in Rust. HTTP `ETag` is not used by python-plexapi at all (`analysis/02§5`) — the field name is therefore unambiguous in Rust. |
| Magic implicit reload key built from `key` + `_INCLUDES`/`_EXCLUDES` (`base.py:155-177`) | `analysis/02§1`, `analysis/02§2` | **Explicit `reload_url: String` field on every domain object.** Set at construction time from the response that built the object. The `Partial<T>` → `Loaded<T>` transition uses this URL; there is no magic merge. |
| `weakref.ref` parent for "walk up to find section" (`base.py:93, :199-212`) | `analysis/02§8.4`, `analysis/08§3.2` | **`LibrarySectionRef` owned by every leaf**, populated at parse time. No back-references. Editable types additionally hold `Arc<HttpClient>` directly (via the `LibrarySectionRef`). `MediaTag`, `Marker`, `Chapter`, `BaseResource` get a `ParentCtx<'_>` *at parse time only* (`analysis/06§J`) — they don't retain it. |
| Module-level mutable sets (`USER_DONT_RELOAD_FOR_KEYS`, `BASE_HEADERS`, `CONFIG`, `DATETIME_TIMEZONE`) (`base.py:18`, `__init__.py:13-55`, `utils.py:105-108`) | `analysis/02§8.2, §8.11` | **Per-instance state.** `ClientConfig` carries every previously-global flag; no `OnceLock<...>` mutable state. The only `OnceLock` in the crate is the static `searchType` lookup table (`analysis/02§7`, `utils.py:239-254`) — a strictly-immutable `phf` map of ~13 entries. |
| Bare `except:` clauses (`config.py:40`, `utils.py:235`) | `analysis/02§8.10` | **Typed `Error` enum with `#[from]`** (already in `CLAUDE.md§6.5`). No catch-all. Specifically: `reqwest::Error` never bubbles unwrapped; `quick_xml::DeError` and `serde_json::Error` are distinct variants; HTTP 401 maps to `Unauthorized`, 401+`"verification code"` maps to `Auth("2fa_required")`, 404 maps to `NotFound`, 422+`"Invalid token"` maps to `Unauthorized`. See `analysis/02§6`. |
| `cast(func, value)` returning `float('nan')` on parse failure (`utils.py:163-185`) | `analysis/02§7, §8.7` | **`Option<T>` for absent, `Result<T, ParseError>` for malformed.** Never sentinel values. The tri-state ints Python defaults (`viewCount=0`, `enableCreditsMarkerGeneration=-1`) become `i32` (not `Option<i32>`) at parse time with the same defaults (`analysis/06§C`). |
| `joinArgs(args: dict)` sorted key=urlencoded_value (`utils.py:188-201`) | `analysis/02§7` | **`BTreeMap<&str, Cow<'_, str>>` + `serde_urlencoded`.** BTreeMap preserves Plex's deterministic ordering so wiremock URL assertions are stable. URL-encodes values only, not keys — matches python-plexapi `safe=''` behaviour. |
| `cached_data_property` + metaclass invalidation (`base.py:41-73`) | `analysis/02§2` | **Don't replicate.** Eager-parse everything. The Python design exists to delay walking a retained `Element`; Rust drops the XML buffer immediately and stores owned data. There is no field that costs more to parse than it costs to store. |
| `MediaContainer` as both `list` and `PlexObject` subclass (`base.py:1140-1144`) | `analysis/02§8.14` | **Plain struct `MediaContainer<T> { items: Vec<T>, offset: u32, size: u32, total_size: u32, identifier: String, … }`** implementing `IntoIterator`, `Index`, `Deref<Target=[T]>`. Generic over the inner item type. **Single envelope replaces the 12 `mediaContainerWith*` OpenAPI variants** (`analysis/01§5.1, §7`). |
| `_overwriteNone` flag controlling partial-reload merge (`base.py:112-116, :491-494`) | `analysis/02§8.3` | **Drop entirely.** `Partial<T>` → `Loaded<T>` is a `From` conversion that replaces the value; there's no in-place merge. The partial type only exposes the subset of fields that XML in a list-of-items context is guaranteed to carry. |
| `fetchItems(maxresults=...)` header-paginated loop (`base.py:227-363`) | `analysis/02§3` | **`impl Stream<Item = Result<T>>` paginator.** Streams pages via `X-Plex-Container-Start`/`-Size` request headers; reads `totalSize` from `X-Plex-Container-Total-Size` response header *and* envelope (Plex sends both; envelope wins on conflict). Provide `try_collect()` for the eager case. |
| `requests.Session` no retry / 30 s timeout / single value (`server.py:107`, `__init__.py:20`) | `analysis/02§5, §8.9` | **`reqwest::ClientBuilder` with:** separate connect/read/total timeouts (defaults 5/30/60 s), pool size 16, idle timeout 90 s, `tower::retry::Retry` on idempotent verbs + 5xx + transport errors, exponential backoff with jitter (`50 ms..30 s`, max 5 retries). Python has none of this; we add it. |

---

## 4. Critical corrections to CLAUDE.md

Each item lists the contradiction, its evidence, and the fix.

### 4.1 `mdns-sd` is wrong for GDM

**Contradiction.** `CLAUDE.md§5` declares `discovery = ["dep:mdns-sd"]`.
**Evidence** (`analysis/09§2.1-§2.3`). Plex's GDM protocol uses
`M-SEARCH * HTTP/1.0` (note: HTTP/1.0, not even SSDP's HTTP/1.1) on UDP
ports `32412` (clients via broadcast) and `32414` (servers via
multicast `239.0.0.250`). This is not DNS-format; it's a tiny HTTP
preamble with header lines, and the response is plain text headers
parsed line-by-line. `mdns-sd` will not interoperate.
**Fix.** Drop `mdns-sd` from `Cargo.toml`. Replace with raw
`tokio::net::UdpSocket`. Rename the feature `discovery = []` (no
dependency needed — `tokio`'s UDP is already pulled in by the `net`
feature, which we should add to `tokio`'s feature list). Module path
becomes `src/discover_gdm/`.

### 4.2 Edit endpoint is on the section, not the metadata item

**Contradiction.** `CLAUDE.md§13` lists "Media metadata edit" as a
checkbox; doesn't specify the wire endpoint.
**Evidence** (`analysis/08§3.2`, `library.py:1734-1746`).
`PUT /library/metadata/<ratingKey>?title.value=X` does **not** work
generally; the canonical edit endpoint is
`PUT /library/sections/<sectionKey>/all?id=<ratingKey>(,…)&type=<int>&<field>.value=…&<field>.locked=0|1`.
The section is part of the URL, not just metadata context.
**Fix.** Every editable leaf must carry a `LibrarySectionRef`
(§2 above). `EditField::edit_field()` builds the URL from the leaf's
section, not its own key. `LibrarySectionRef` carries `id` and a
clone of `HttpClient` — sufficient for editing without traversing
back up.

### 4.3 Pagination is via headers, not query params

**Contradiction.** `CLAUDE.md§6.2` says "Stream paginated endpoints
with `impl Stream<Item = Result<T>>`" but doesn't specify mechanism.
**Evidence** (`analysis/01§4.2`, `analysis/02§3`). Pagination uses the
request headers `X-Plex-Container-Start` and `X-Plex-Container-Size`,
and the response headers `X-Plex-Container-Start` and
`X-Plex-Container-Total-Size`. The query-string `limit` and `offset`
are *different* mechanisms (`limit` does not return `totalSize` and
short-circuits the server's pagination). Many community OpenAPI
generators get this wrong.
**Fix.** `src/pagination.rs` exposes `Paginator<T>` taking the request
URL and a `page_size` (default 100, configurable on `ClientConfig`).
Internally sets `X-Plex-Container-Start: N`, `X-Plex-Container-Size: K`
headers per page; reads `X-Plex-Container-Total-Size` from response
headers (falling back to envelope `totalSize` field). Provides
`try_collect()`, `into_stream()`, `with_page_size()`, `take(n)`.

### 4.4 Errors are HTML stubs, not JSON

**Contradiction.** `CLAUDE.md§6.5` shows
`Error::Api { status, message }` with `message: String` — but does
not say where the message comes from. Naive readers will try to
deserialize a structured body.
**Evidence** (`analysis/01§5.2`). Every reusable 400/403/404 response
in the OpenAPI spec declares `text/html` with literal stub HTML:
`<html><head><title>Bad Request</title></head><body><h1>400 Bad
Request</h1></body></html>`. There is no `application/problem+json`,
no JSON error body, no structured error model anywhere in Plex.
**Fix.** `client.rs::map_response` builds `Error::Api.message` from
`{status_code_phrase}; {url}; {first 200 chars of body}` exactly as
python-plexapi does (`server.py:749-758`, `myplex.py:256-274`). Never
attempt to parse the body as JSON or XML on non-2xx. Document this
explicitly in `Error::Api`'s rustdoc.

### 4.5 ~25 PMS endpoints used by python-plexapi are missing from the spec

**Contradiction.** `CLAUDE.md§2` calls out plexapi.dev as
"best machine-readable description" — implying we could codegen from
it.
**Evidence** (`analysis/01§8`). The spec is missing:
`/accounts`, `/accounts/{id}`, `/clients`, `/devices` (PMS-side, not
the grabber `devices`), `/diagnostics/databases`, `/diagnostics/logs`,
`/library/onDeck`, `/library/recentlyAdded`,
`/library/sections/{id}/onDeck`, `/library/sections/{id}/timeline`,
`/library/sections/{id}/folder`, `/library/sections/{id}/firstCharacter`,
`/library/metadata/{rk}/posters` (and `art`/`arts`/`theme`/`themes`/
`thumb`/`clearLogo`/`clearLogos`/`squareArt`/`squareArts`),
`/library/metadata/{id}/children`, `/myplex/account`, `/myplex/claim`,
`/resources`, `/services/browse`, `/services/browse/{base64path}`,
`/status/sessions/history`, the `/sync/*` family, `/system/agents`,
`/transcode/sessions`, `/actions/removeFromContinueWatching`. These
are *used* by python-plexapi and we need them.
**Fix.** Treat the spec as **advisory, not authoritative.** Hand-write
the operation surface (`analysis/01§9`). Each operation method
references python-plexapi's source for behavioural correctness. The
spec is pinned at v1.2.2 in-repo for inspection but never code-genned.

### 4.6 `mediaContainerWith*` collapse to one generic envelope

**Contradiction.** `CLAUDE.md` doesn't address the envelope explosion;
`CLAUDE.md§7` says "Wrap the envelope in a generic `MediaContainer<T>`"
but doesn't note this collapses 12 spec schemas.
**Evidence** (`analysis/01§5.1, §7`). The spec defines 11
`mediaContainerWith*` variants plus the base `mediaContainer`, all
because the spec models the JSON envelope literally rather than by
composition. 22 of the 64 component schemas are uppercase `$ref`
aliases pointing at the lowercase canonical schemas.
**Fix.** `src/xml/mod.rs::MediaContainer<T>` is a single generic
struct. Inner item type is parameterised. The 22 alias schemas are
ignored. The 12 envelope variants collapse to one. Document in
rustdoc.

### 4.7 WebSocket scheme bug

**Contradiction.** Implicit: `CLAUDE.md` mentions WebSockets only in
passing (§13).
**Evidence** (`analysis/09§1.2`, `alert.py:56`).
python-plexapi builds the WebSocket URL by
`url.replace('http', 'ws')` which produces `wssps://...` for HTTPS
servers (replaces first `http` substring only).
**Fix.** `alerts/mod.rs` explicitly maps `http → ws`, `https → wss`
via `url::Url::set_scheme()`. Document the bug we are *not*
reproducing. Token goes in the query string (PMS does not accept
`Sec-WebSocket-Protocol` or `Authorization` headers on the
notifications endpoint).

### 4.8 No retries in python-plexapi — we add them

**Contradiction.** `CLAUDE.md§1` mentions "retry/backoff for transient
failures"; `CLAUDE.md§13` makes it a checkbox.
**Evidence** (`analysis/02§5`). python-plexapi uses a bare
`requests.Session` with no `HTTPAdapter`, no `urllib3.Retry`, no pool
tuning. 5xx and network errors propagate unwrapped.
**Fix.** `client.rs` wraps `reqwest::Client` with a `tower::retry::Retry`
middleware that retries on:
- transport errors (connection refused, reset, DNS),
- HTTP 5xx,
- HTTP 408 (request timeout),
- HTTP 503 (service unavailable, with `Retry-After` honoured).
Strictly idempotent verbs only (GET, PUT, DELETE — but **not POST**;
two of Plex's POST endpoints are non-idempotent: playlist creation
returns the new `ratingKey`). Exponential backoff with jitter,
configurable cap. Disable per-call via
`ClientConfig::retries(None)`.

### 4.9 22 uppercase `$ref` aliases in OpenAPI components

**Contradiction.** None directly — but a naive codegen attempt would
produce duplicated Rust types.
**Evidence** (`analysis/01§7`). 22 of 64 components are uppercase
camelCase `$ref` aliases to lowercase canonical schemas
(`Hub` → `hub`, `Metadata` → `metadata`, etc.).
**Fix.** Since we hand-write the DTO layer (§4.5 above), this is
moot — but for any future spec-parsing tooling (e.g. a script that
generates a coverage report against the spec), filter these out
first. Document the filter list in `analysis/01§9.2`.

### 4.10 `markPlayed`/`markUnplayed` are HTTP GET despite being mutations

**Contradiction.** `CLAUDE.md§14.4` says "No `.unwrap()` … in library
code outside genuinely-infallible situations" and `§14.7` says
"assume every `.await` may be dropped." Neither covers the issue.
**Evidence** (`analysis/08§7`, `played_unplayed.py:1-34`).
`GET /:/scrobble?key=<rk>&identifier=com.plexapp.plugins.library` and
`GET /:/unscrobble?...` are mutations issued via HTTP GET. Same for
the Discover scrobble endpoints (`analysis/03§6.3`).
**Fix.** Keep GET on the wire for compatibility. **Expose via mutating
methods** in Rust: `Movie::mark_played(&self) -> Result<()>`, etc.
The retry middleware (§4.8) considers GET retryable; that's
intentional — PMS treats repeated scrobble GETs as idempotent
(`viewCount` only ticks up once per session anyway). Document the
quirk in rustdoc. Do not add a `_post` variant.

### 4.11 `addWebhook`/`removeWebhook` are read-modify-write

**Contradiction.** None addressed directly.
**Evidence** (`analysis/03§8`, `analysis/09§4.1`).
`addWebhook(url)` reads `self._webhooks` (a *local* cached list),
appends, and POSTs the entire list. If you didn't call `webhooks()`
first, you clobber existing entries. The Plex API has no per-webhook
DELETE — you always POST the whole list.
**Fix.** `MyPlexAccount::add_webhook(&self, url)` always does a `GET`
first (no caching at the public API), then POSTs the modified list.
Document the inherent race: two concurrent `add_webhook` calls can
lose one entry. Wrap in an `add_webhook_with_retry(url, max_attempts)`
that does optimistic-concurrency: GET, modify, POST, GET again,
compare, retry if drift detected. Default `max_attempts = 3`. Same
treatment for `remove_webhook`. Document the race explicitly in
rustdoc with `# Cancel safety` and `# Concurrency` sections.

---

## 5. Trait architecture decision

Final choice from `analysis/08§13`: **Option B (extension traits per
capability) as the primary pattern, augmented by Option D
(`capabilities!` macro) to remove boilerplate, with Option C
(`MetadataItem` enum) reserved for heterogeneous result sets.**

The trait surface is small (~10 load-bearing traits + ~30 thin field
traits). Below, concrete signatures for the load-bearing ones. All
methods are `async fn` (Rust 2024 AFIT). Where a trait needs a
heterogeneous bound, it uses a supertrait rather than an associated
type — keeps trait-object compatibility minimal but not lost.

### 5.1 `PlexObject` — the foundation

```rust
pub trait PlexObject: Send + Sync {
    /// HTTP client to talk back to the server this object came from.
    fn http(&self) -> &HttpClient;

    /// Relative key, e.g. "/library/metadata/12345".
    fn key(&self) -> &str;

    /// Full reload URL with includes appended (analysis/02§2).
    fn reload_url(&self) -> &str;

    /// The XML <Tag> + type that identifies this item ("Video.movie", etc.).
    fn item_kind(&self) -> ItemKind;
}
```

Implementors: every concrete domain leaf (Movie, Show, Season, …,
Playlist, Collection, MyPlexUser, …). The `ItemKind` enum is the
compile-time replacement for the `PLEXOBJECTS` registry.

### 5.2 `Reload` — partial → full

```rust
pub trait Reload: PlexObject + Sized {
    /// The "fully-loaded" version of this type.
    type Full;

    /// Re-fetch this object using `reload_url()` and return the loaded form.
    async fn reload(self) -> Result<Self::Full>;
}
```

Implementors:
- `Movie<Partial>::Full = Movie<Loaded>` (and same for every leaf with
  a partial form);
- `Movie<Loaded>::Full = Movie<Loaded>` (idempotent — re-fetch the
  same object);
- `Playlist`, `Collection`, `MyPlexAccount` (no `Partial<T>` —
  always fully loaded).

Associated type avoids the typestate-leakage problem (a `Reload`
trait that takes `&self` cannot transmute its receiver).

### 5.3 `Playable` — leaf playables only

```rust
pub trait Playable: PlexObject {
    /// Build the universal-transcode stream URL.
    fn stream_url(&self, params: &TranscodeParams) -> Url;

    /// Begin playback on a client.
    async fn play(&self, client: &PlexClient) -> Result<()>;

    /// Iterate file parts.
    fn parts(&self) -> &[MediaPart];

    fn video_streams(&self) -> Vec<&VideoStream>;
    fn audio_streams(&self) -> Vec<&AudioStream>;
    fn subtitle_streams(&self) -> Vec<&SubtitleStream>;
    fn lyric_streams(&self) -> Vec<&LyricStream>;

    /// Send a /:/timeline ping with optional view offset.
    async fn update_progress(&self, ms: u64, state: PlayState) -> Result<()>;

    async fn download(&self, dest: &Path) -> Result<u64>;
}
```

Implementors: `Movie`, `Episode`, `Clip`, `Track`. **Not `Photo`** —
python's `Playable` mixin technically implements it but `getStreamURL`
rejects at runtime (`base.py:862`). Rust does not implement `Playable`
on `Photo`; `Photo` gets a separate `Photo::download(&self, dest)`
method (`analysis/06§B`).

### 5.4 `Ratable` — leaf rate

```rust
pub trait Ratable: PlexObject {
    /// 0.0..=10.0 over the wire; -1 clears.
    async fn rate(&self, rating: Option<f32>) -> Result<()> {
        let r = rating.unwrap_or(-1.0);
        if r != -1.0 && !(0.0..=10.0).contains(&r) {
            return Err(Error::Auth("rating must be 0..=10 or None".into()));
        }
        let url = format!(
            "/:/rate?key={}&identifier=com.plexapp.plugins.library&rating={r}",
            self.key().trim_start_matches('/'),
        );
        self.http().put(&url).await
    }
}
```

Implementors: Movie, Show, Season, Episode, Artist, Album, Track,
Photoalbum, Photo, Collection. **Not** Clip, Playlist (matrix in
`analysis/08§2`).

### 5.5 `EditField` — universal single-field edit primitive

```rust
pub trait EditField: PlexObject + HasSection {
    /// Issue PUT /library/sections/{section}/all?id={key}&{field}.value=&{field}.locked=
    async fn edit_field(
        &self,
        field: &str,
        value: impl Into<FieldValue> + Send,
        locked: bool,
    ) -> Result<()>;

    /// Open a batch transaction; multiple edits flush as one request.
    fn batch(&self) -> EditBatch<'_, Self>;
}
```

`FieldValue` is an enum (`String(String)`, `Bool(bool)`, `Int(i64)`,
`F32(f32)`, `Date(NaiveDate)`, `DateTime(DateTime<Utc>)`). `HasSection`
is a tiny supertrait `fn section(&self) -> &LibrarySectionRef`.

Field-specific traits (`EditTitle`, `EditSummary`, `EditTagline`,
`EditCriticRating`, …) supertrait `EditField` with default-body
methods, e.g.:

```rust
pub trait EditTitle: EditField {
    async fn edit_title(&self, title: &str, locked: bool) -> Result<()> {
        self.edit_field("title", title, locked).await
    }
}
```

The full set is given by the matrix in `analysis/08§2`. The Plex
field-name quirks (`CriticRatingMixin` edits `rating` not
`criticRating`; `TrackArtistMixin` edits `originalTitle`;
`SortTitleMixin` edits `titleSort`) live in the trait's default body,
not on the call site.

### 5.6 `EditTags` — collection/genre/director/…

```rust
pub trait EditTags: PlexObject + HasSection {
    /// Adds: `<tag>[i].tag.tag=value` per element, prepended to current list.
    /// Removes: `<tag>[].tag.tag-=csv` (note trailing `-`).
    /// Always emits `<tag>.locked=0|1`.
    async fn edit_tags(
        &self,
        tag: &str,
        items: &[&str],
        locked: bool,
        remove: bool,
    ) -> Result<()>;
}
```

Per-tag traits (`HasCollections`, `HasGenres`, `HasLabels`, …) provide
both the read accessor and the `add_*`/`remove_*` default methods:

```rust
pub trait HasGenres: EditTags {
    fn genres(&self) -> &[Tag];   // singular: Tag::Genre kind

    async fn add_genres(&self, items: &[&str], locked: bool) -> Result<()> {
        self.edit_tags("genre", items, locked, false).await
    }
    async fn remove_genres(&self, items: &[&str], locked: bool) -> Result<()> {
        self.edit_tags("genre", items, locked, true).await
    }
}
```

Note: python's add semantics are merge-with-existing
(`analysis/08§3.4`). The Rust default body **must** read
`self.genres()` and prepend before sending — replicate exactly, with
an explicit doc note about the semantics.

### 5.7 Image traits: `HasArt`, `HasPoster`, `HasTheme`, `HasLogo`, `HasSquareArt`

Three layers per family, matching `analysis/08§6`. Sketch for art:

```rust
pub trait HasArtUrl: PlexObject {
    fn art_url(&self) -> Option<Url> { /* default: from self.art via thumb_path() */ }
}

pub trait HasArtLock: HasArtUrl + EditField {
    async fn lock_art(&self) -> Result<()>   { self.edit_field("art.locked", true,  true).await }
    async fn unlock_art(&self) -> Result<()> { self.edit_field("art.locked", false, true).await }
}

pub trait HasArt: HasArtLock {
    async fn arts(&self) -> Result<Vec<Resource>>;       // GET /library/metadata/{rk}/arts
    async fn upload_art_url(&self, url: &str) -> Result<()>;
    async fn upload_art_bytes(&self, bytes: Bytes) -> Result<()>;
    async fn set_art(&self, art: &Resource) -> Result<()>;
    async fn delete_art(&self) -> Result<()>;            // DELETE /library/metadata/{rk}/art
}
```

Leaves with only the URL aspect (`Clip`, `Track`, `Photo`) implement
just `HasArtUrl`. Theme has no `set_theme` (PMS exposes no selection
endpoint — `analysis/08§6` notes `ThemeMixin.setTheme` raises
`NotImplementedError`); the Rust trait omits the method entirely.

### 5.8 `PlayedUnplayed`

```rust
pub trait PlayedUnplayed: PlexObject {
    async fn mark_played(&self) -> Result<()> {
        let url = format!(
            "/:/scrobble?key={}&identifier=com.plexapp.plugins.library",
            self.key().trim_start_matches('/'),
        );
        self.http().get_no_body(&url).await  // GET despite being a mutation; §4.10
    }
    async fn mark_unplayed(&self) -> Result<()> { /* /:/unscrobble */ }

    fn is_played(&self) -> bool;  // implementor reads view_count > 0
}
```

Implementors: every concrete `Video` subclass and every `Audio`
subclass (i.e. Movie, Show, Season, Episode, Clip, Artist, Album,
Track). **Not** Photo, Photoalbum (matrix).

### 5.9 Search/agent traits

```rust
pub trait Splittable: PlexObject {
    async fn split(&self) -> Result<()>;             // PUT {key}/split
    async fn merge(&self, others: &[RatingKey]) -> Result<()>;  // PUT {key}/merge?ids=
}

pub trait Matchable: PlexObject {
    async fn unmatch(&self) -> Result<()>;
    async fn matches(&self, opts: MatchOpts) -> Result<Vec<SearchResult>>;
    async fn fix_match(&self, sr: &SearchResult, auto: bool) -> Result<()>;
}

pub trait Watchlistable: PlexObject + HasGuid {
    async fn on_watchlist(&self, account: &MyPlexAccount) -> Result<bool>;
    async fn add_to_watchlist(&self, account: &MyPlexAccount) -> Result<()>;
    async fn remove_from_watchlist(&self, account: &MyPlexAccount) -> Result<()>;
    async fn streaming_services(&self, account: &MyPlexAccount)
        -> Result<Vec<Availability>>;
}
```

`HasGuid` is a tiny supertrait `fn guid(&self) -> &Guid` because
watchlist uses GUID-derived rating keys, not the local ratingKey
(`analysis/08§8`).

### 5.10 Summary table (per-leaf, derived from analysis/08§2)

| Leaf | PlexObject | Reload | Playable | Ratable | PlayedUnplayed | EditField suite | EditTags suite | HasArt/Poster/SquareArt | HasTheme | HasLogo | Watchlistable | Splittable | Matchable |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Movie | yes | yes | yes | yes | yes | full | full | yes | yes | yes | yes | yes | yes |
| Show | yes | yes |  | yes | yes | full | most | yes | yes | yes | yes | yes | yes |
| Season | yes | yes |  | yes | yes | partial | partial | yes | url | yes |  |  |  |
| Episode | yes | yes | yes | yes | yes | partial | partial | yes | url | yes |  |  |  |
| Clip | yes | yes | yes |  | yes |  |  | url only |  | url only |  |  |  |
| Artist | yes | yes |  | yes | yes | partial | partial | yes | yes | yes |  | yes | yes |
| Album | yes | yes |  | yes | yes | partial | partial | yes | url | yes |  | yes | yes |
| Track | yes | yes | yes | yes | yes | partial | partial | url only | url only | url only |  |  |  |
| Photoalbum | yes | yes |  | yes |  | partial |  | yes |  | yes |  |  |  |
| Photo | yes | yes |  | yes |  | partial | partial (Tag only) | url only |  | url only |  |  |  |
| Collection | yes | yes |  | yes |  | partial | partial (Label only) | yes | yes | yes |  |  |  |
| Playlist | yes | yes |  |  |  | partial |  | yes |  | yes |  |  |  |

(`partial` means the leaf implements a subset of the suite; concrete
fields per leaf are in `analysis/08§2`.)

---

## 6. Concrete URI / scheme handling

Plex uses URI strings as opaque references in query parameters and as
stored playlist/collection `content` fields. From the inventory in
`analysis/07§8`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlexUri {
    /// `server://<machineId>/com.plexapp.plugins.library<key>`
    /// — most common; references any library item on a specific PMS.
    /// Cites: playqueue.py:185, playlist.py:360, collection.py:435.
    Server {
        machine_id: MachineIdentifier,
        key: String,  // typically "/library/metadata/<rk>" or comma-joined
    },

    /// `library://<sectionUUID>/item/library/metadata/<rk>`
    /// — single item by section UUID; used for PlayQueue mutations.
    /// Cites: playqueue.py:251-252.
    LibraryItem {
        section_uuid: Uuid,
        rating_key: RatingKey,
    },

    /// `library:///directory/<urlencoded(path)>`  (three-slash form)
    /// — a directory; comma-joined metadata list or collection children.
    /// Cites: playqueue.py:176-177, collection.py:536-537.
    LibraryDirectory {
        path: String,  // already URL-decoded on the inside
    },

    /// `playlist:///<urlencoded(guid)>`
    /// — playlist GUID for sync purposes.
    /// Cites: playlist.py:495.
    Playlist {
        guid: String,
    },

    /// `/playQueues/<id>?own=0|1&window=N` — used as `containerKey`.
    /// Cites: client.py:520, sonos.py:100.
    PlayQueueContainer {
        play_queue_id: PlayQueueId,
        own: bool,
        window: Option<u32>,
    },

    /// `https://plex.tv/devices/<clientId>/sync_items[/<id>]`
    /// — plex.tv sync queue.
    /// Cites: sync.py:116-128, sync.py:103-105.
    Device {
        client_id: ClientIdentifier,
        item: Option<u64>,
    },

    /// Server's `/security/token?type=delegation&scope=all` minted token.
    /// Not a URL scheme per se; modelled as a typed wrapper around the
    /// scoped token string for embedding into player commands.
    /// Cites: server.py:229-235.
    SecurityToken(String),
}

impl PlexUri {
    pub fn parse(s: &str) -> Result<Self, ParseError>;
    /// Round-trip-stable: parse(uri.to_string()) == Ok(uri).
}

impl std::fmt::Display for PlexUri { /* ... */ }
```

The parser is `nom`-based (or `winnow` — pick one; `winnow` is the
modern successor and the build cost is similar). The grammar is
tiny — six discriminating prefixes. The parser is unit-tested via
`rstest` round-tripping every example from the citation list above
plus the ones discovered in fixture captures.

**Constructor helpers** for the common cases:

```rust
impl PlexUri {
    pub fn server_item(server: &PlexServer, key: &str) -> Self;
    pub fn server_items<I: IntoIterator<Item = RatingKey>>(server: &PlexServer, keys: I) -> Self;
    pub fn library_item(section: &LibrarySection, rk: RatingKey) -> Self;
    pub fn library_directory(path: &str) -> Self;
    pub fn play_queue(id: PlayQueueId, own: bool, window: Option<u32>) -> Self;
}
```

The "type" parameter naming chaos (`video`/`audio`/`photo`/`music` in
some places, `listType` in others, numeric `searchType` 1/2/8/9/10
elsewhere — `analysis/07§8`) is **not** absorbed into `PlexUri`. It's
carried separately by `MediaKind`/`SearchType` newtypes and added as a
query parameter alongside the URI. Mixing them would forfeit
round-trip stability.

---

## 7. Filter builder design

From `analysis/05§4-§5`. Two operator namespaces are mixable in a
single call. The Rust builder makes the namespace **statically
visible** without forbidding the mix.

### 7.1 Operator inventory

```rust
/// Client-side operator suffixes (analysis/05§4 / Appendix A).
/// Applied to results after fetch, by walking parsed structs.
#[derive(Debug, Clone, Copy)]
pub enum ClientOp {
    Exact,
    Iexact,
    Contains,
    Icontains,
    Ne,
    In,
    Gt, Gte, Lt, Lte,
    Startswith, Istartswith,
    Endswith, Iendswith,
    Exists,
    Regex, Iregex,
}

/// Server-side operators recognised by Plex's `/library/sections/<id>/all`.
/// Emitted onto the wire as field-suffix tokens (analysis/05§4 / Appendix A).
#[derive(Debug, Clone, Copy)]
pub enum ServerOp {
    /// Default: `is` for tag/int/bool/guid; `contains` for string.
    Default,
    /// `!`  — is not / does not contain
    Not,
    /// `=`  — exact string match (only on str fields)
    ExactStr,
    /// `!=` — exact string ≠
    NotExactStr,
    /// `<`  — string begins with
    BeginsWith,
    /// `>`  — string ends with
    EndsWith,
    /// `<<` — datetime/int "less than" / "before"
    Before,
    /// `>>` — datetime/int "greater than" / "after"
    After,
    /// `&=` — AND-of-tags (default tag op is OR)
    AndTag,
}
```

### 7.2 The builder

```rust
pub struct FilterBuilder<S = NoLibType> {
    libtype: Option<LibType>,
    server_terms: Vec<ServerTerm>,
    client_terms: Vec<ClientTerm>,
    sort: Vec<SortField>,
    limit: Option<u32>,
    title_search: Option<TitleSpec>,
    advanced: Option<AdvancedTree>,  // for `filters={'and': [...], 'or': [...]}` form
    _marker: PhantomData<S>,
}

// Typestate: `.libtype()` transitions NoLibType → WithLibType.
// `.execute()` is only available on WithLibType.

impl FilterBuilder<NoLibType> {
    pub fn new() -> Self;
    pub fn libtype(self, lt: LibType) -> FilterBuilder<WithLibType>;
}

impl<S> FilterBuilder<S> {
    /// Server-side terms — go on the URL.
    pub fn server(self, f: impl FnOnce(ServerFilterCtx) -> ServerFilterCtx) -> Self;

    /// Client-side terms — post-filter the result stream.
    pub fn client(self, f: impl FnOnce(ClientFilterCtx) -> ClientFilterCtx) -> Self;

    pub fn sort(self, field: &str, direction: SortDir) -> Self;
    pub fn limit(self, n: u32) -> Self;
    pub fn title(self, t: &str) -> Self;             // contains
    pub fn title_in(self, ts: &[&str]) -> Self;      // promoted to filter field

    /// Build a `{'and'|'or': [...]}` tree (analysis/05§5).
    pub fn advanced(self, tree: AdvancedTree) -> Self;
}

impl FilterBuilder<WithLibType> {
    pub fn execute(self, section: &LibrarySection)
        -> Result<impl Stream<Item = Result<MetadataItem>>>;
}

/// Closure-passed builder context: typed per field family.
pub struct ServerFilterCtx { /* … */ }
impl ServerFilterCtx {
    pub fn genre(self) -> TagField<Self>;          // .eq, .ne, .and_eq
    pub fn year(self) -> IntField<Self>;           // .eq, .ne, .gt, .lt, .gte, .lte
    pub fn title(self) -> StringField<Self>;       // .eq, .ne, .contains, .ne_contains, .begins_with, .ends_with
    pub fn unwatched(self) -> BoolField<Self>;     // .eq(true) shorthand
    pub fn added_at(self) -> DateField<Self>;
    pub fn guid(self) -> GuidField<Self>;
    pub fn group(self) -> RawField<Self>;          // SQL "group by" (analysis/05§6)
    pub fn having(self) -> RawField<Self>;
    // ... discovered fields per libtype from LibrarySection::list_fields()
}
```

`TagField`/`IntField`/`StringField`/etc. only expose operators valid
for that field type (per `FilteringFieldType`'s allowed operators —
`analysis/05§12`). `BoolField` only has `.eq(bool)`; etc. This is
where the type system buys us correctness: you cannot call
`.genre().gt("Action")` because tag fields don't support `>`.

### 7.3 Mixing namespaces

```rust
let items = section.search()
    .libtype(LibType::Movie)
    .server(|q| q.genre().eq("Action").year().gt(1990))
    .client(|q| q.summary().icontains("heist"))
    .sort("addedAt", SortDir::Desc)
    .limit(50)
    .execute(&section)
    .await?
    .try_collect::<Vec<_>>()
    .await?;
```

This produces (genre/year on the URL; summary post-filter in-process):

```
GET /library/sections/1/all
  ?type=1
  &genre=23
  &year>=1990
  &sort=addedAt:desc
  &limit=50
  X-Plex-Container-Start: 0
  X-Plex-Container-Size: 100
```

And the returned stream applies the `icontains("heist")` predicate to
each `summary` field client-side.

### 7.4 Smart-filter round-trip — out of scope for v1

`analysis/05§7` describes the smart-filter URI grammar:
`push=1`/`pop=1`-delimited groups with `and=1`/`or=1` operators between
operands. The python `SmartFilterMixin._parseFilters` parses these
into the same `{filters: {...}, sort, libtype}` dict that
`_validateAdvancedSearch` emits.

For v1 we ship:
- The **parser** (read-side): `SmartFilter::parse(url)` returns an
  `AdvancedTree` that can be inspected.
- The **flat-dict serializer** (write-side, for new collections):
  `FilterBuilder::advanced(tree).execute(...)` builds the wire URI.
- We do **not** ship full round-trip with mutation of an existing
  smart collection's filter URL until v1.1. The round-trip is brittle
  (URL encoding ambiguities; the trailing-operator trim at
  `smart_filter.py:50`) and rarely needed; callers who need it can
  call the parser, mutate the tree, and pass it back to
  `Collection::update_filters(tree)`.

Documented as a known limitation in `Collection::update_filters`'s
rustdoc.

---

## 8. WebSocket alerts design

From `analysis/09§1`. Python's `AlertListener` has three notable
flaws we explicitly fix:

1. naive `replace('http','ws')` (`https → wssps`),
2. no reconnect or backoff,
3. callback-on-reader-thread blocking model.

### 8.1 Dependency choice

`tokio-tungstenite = { version = "0.23", default-features = false,
features = ["rustls-tls-webpki-roots"] }`. We use webpki-roots (not
native-tls) to keep our TLS story consistent with `reqwest`'s
default. `tokio-tungstenite` does not pull in `tokio`'s `net`
feature implicitly; we add it to our `tokio` features.

### 8.2 Scheme map

```rust
fn ws_url_from_http(base: &Url, key: &str, token: &PlexToken) -> Result<Url> {
    let mut u = base.clone();
    let scheme = match u.scheme() {
        "http"  => "ws",
        "https" => "wss",
        other   => return Err(Error::Internal("base URL not http(s)")),
    };
    u.set_scheme(scheme).map_err(|_| Error::Internal("set_scheme failed"))?;
    u.set_path(key);
    u.query_pairs_mut().append_pair("X-Plex-Token", token.as_str());
    Ok(u)
}
```

Token goes in the query string (`analysis/09§1.2`: PMS does not
accept it via headers on this endpoint).

### 8.3 Typed events

```rust
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AlertEvent {
    #[serde(rename = "playing")]
    Playing { play_session_state_notification: Vec<PlaySessionState> },
    #[serde(rename = "progress")]
    Progress { progress_notification: Vec<ProgressEntry> },
    #[serde(rename = "activity")]
    Activity { activity_notification: Vec<ActivityEntry> },
    #[serde(rename = "timeline")]
    Timeline { timeline_entry: Vec<TimelineEntry> },
    #[serde(rename = "transcodeSession.start")]
    TranscodeSessionStart { transcode_session: Vec<TranscodeSession> },
    #[serde(rename = "transcodeSession.update")]
    TranscodeSessionUpdate { transcode_session: Vec<TranscodeSession> },
    #[serde(rename = "transcodeSession.end")]
    TranscodeSessionEnd { transcode_session: Vec<TranscodeSession> },
    #[serde(rename = "update.statechange")]
    UpdateStateChange(serde_json::Value),
    #[serde(rename = "reachability")]
    Reachability { reachability_notification: Vec<ReachabilityEntry> },
    #[serde(rename = "status")]
    Status(serde_json::Value),
    #[serde(rename = "setting")]
    Setting(serde_json::Value),
    #[serde(rename = "preference")]
    Preference(serde_json::Value),
    #[serde(rename = "account")]
    Account(serde_json::Value),
    #[serde(rename = "backgroundProcessingQueue")]
    BackgroundProcessingQueue(serde_json::Value),

    /// Forward-compat: unknown discriminator preserved as raw JSON.
    #[serde(other)]
    Other,
}
```

(`Other` uses `serde(other)` which loses the body. For full
forward-compat, use a custom deserializer that captures the raw value
into `Other(serde_json::Value)`. The variant enumeration is sourced
from `analysis/09§1.4`.)

The envelope wrapper `{"NotificationContainer": {...}}` is stripped
by an outer `Deserialize` impl on `AlertFrame`.

### 8.4 Reconnect with backoff

```rust
pub struct AlertConfig {
    pub backoff: ExponentialBackoff,  // 1s..120s, 1.5x, jittered
    pub max_attempts: Option<usize>,  // None = retry forever
    pub ping_interval: Duration,      // 30s default — added; Python has no pings
}

pub fn alerts(
    server: &PlexServer,
    config: AlertConfig,
) -> impl Stream<Item = Result<AlertEvent>> + Send + 'static;
```

`alerts()` returns a cold `Stream` that, when polled, connects, reads
frames, decodes them, and yields events. On disconnect (server close,
ping timeout, transport error) it sleeps per backoff, reconnects, and
resumes. Polling cancels cleanly — the caller can drop the stream at
any await point; the inner task is owned by the stream's `Drop`.

### 8.5 Cancel safety

`Stream::next().await` is cancel-safe in the standard sense: dropping
the future leaves the underlying WebSocket either drained-up-to-this-
point or closed (depending on which way the cancel arrives). The
reconnect logic is in `Stream::poll_next`, not in a `tokio::spawn`'d
background task, so dropping the stream actually stops the
reconnections.

Document this in `# Cancel safety` of `alerts()`'s rustdoc. A
follow-up task (post-v1.0) can add a background-task version for use
cases that want resilient reconnects independent of caller polling.

---

## 9. Authentication state machine

From `analysis/03§2`. Three sign-in flows, modelled as typestates so
`PlexToken` only exists once it's been obtained.

```rust
pub struct MyPlexAuth<S = Anonymous>(AuthState<S>);

pub struct Anonymous;
pub struct WithToken     { token: PlexToken }
pub struct Password      { username: String, password: String }
pub struct RequiresTwoFactor { username: String, password: String }
pub struct PinPending    { id: u64, code: String, created_at: Instant }
pub struct PinExpired;

impl MyPlexAuth<Anonymous> {
    pub fn new(config: ClientConfig) -> Self;

    /// Direct token (analysis/03§2.1) — fastest, no plex.tv round-trip beyond GET /user.
    pub async fn with_token(self, token: PlexToken)
        -> Result<MyPlexAccount, AuthError>;

    /// Username + password (analysis/03§2.2).
    pub fn with_password(self, user: &str, pass: &str) -> MyPlexAuth<Password>;

    /// PIN flow (analysis/03§2.3).
    pub async fn start_pin(self, strong: bool) -> Result<MyPlexAuth<PinPending>, AuthError>;
}

impl MyPlexAuth<Password> {
    /// POST /api/v2/users/signin. On 200 → MyPlexAccount.
    /// On 401 + "verification code" → transitions to RequiresTwoFactor.
    pub async fn submit(self) -> Result<MyPlexAccount, TwoFactorOr<AuthError>>;
}

pub enum TwoFactorOr<E> {
    NeedsCode(MyPlexAuth<RequiresTwoFactor>),
    Err(E),
}

impl MyPlexAuth<RequiresTwoFactor> {
    pub async fn submit_code(self, code: &str)
        -> Result<MyPlexAccount, AuthError>;
}

impl MyPlexAuth<PinPending> {
    pub fn id(&self) -> u64;
    pub fn code(&self) -> &str;

    /// Single poll. Returns:
    ///  - `Ok(None)` if not yet claimed,
    ///  - `Ok(Some(acct))` once claimed,
    ///  - `Err(_)` on transport / expiry.
    pub async fn poll_once(&self) -> Result<Option<MyPlexAccount>, AuthError>;

    /// Convenience: poll every `interval` until claimed or `timeout` elapses.
    pub async fn wait_for_claim(
        self,
        interval: Duration,   // default 1s — Python's POLLINTERVAL
        timeout: Duration,    // default 120s — Python's default
    ) -> Result<MyPlexAccount, AuthError>;

    /// Browser-side OAuth redirect URL (analysis/03§2.3).
    pub fn oauth_url(&self, forward: Option<&Url>) -> Url;
}
```

The JWT flow (`analysis/03§2.4`) is **out of scope for v1** —
documented in §10 (M5). It requires Ed25519 key generation/storage
and is currently a Plex-internal API. Adding it would land as
`MyPlexAuth<Anonymous>::with_jwt() -> MyPlexAuth<JwtPending>` once we
have it.

The state machine forces correct sequencing at the type level:
`PinPending` cannot poll without the PIN id, `Password::submit()`
must be called before `RequiresTwoFactor::submit_code()`, and
`MyPlexAccount` is only ever constructed via a path that produces a
valid `PlexToken`. The previous-step state values flow through; no
stringly-typed "have I called this yet?" check.

---

## 10. Implementation order

`CLAUDE.md§13` lists items in roughly the right order but doesn't
group by milestone or call out dependencies between items. The
implementation order below is strict — each milestone unblocks the
next.

### M0 — Bootstrap (DONE) + foundations

**Already in tree:**
- `Cargo.toml`, `rust-toolchain.toml`, `deny.toml`, `rustfmt.toml`
- analysis docs in `analysis/`
- the `openapi.json` spec snapshot

**Land in M0:**
1. `src/error.rs` — full `Error` enum + `Result` alias (per
   `analysis/02§6`).
2. `src/util/ids.rs` — `RatingKey`, `MachineIdentifier`,
   `ClientIdentifier`, `PlexToken` with redacted `Debug`.
3. `src/headers.rs` — `X-Plex-*` header construction; static defaults
   from `analysis/02§5` (env var / config-file overrides per
   `analysis/02§7`).
4. `src/config.rs` — `ClientConfig` builder.
5. `src/client.rs` — `HttpClient`: `reqwest::Client` wrapper, retry
   middleware (§4.8), token redaction, `Accept: application/json`
   default. Includes the `map_response` status→Error mapper.
6. `src/uri.rs` — `PlexUri` enum + parser + display (§6).
7. `src/pagination.rs` — `Paginator<T>` (§4.3).
8. `src/xml/mod.rs` — `MediaContainer<T>` envelope (§4.6).
9. `src/util/time.rs` — Plex epoch-ms / `NaiveDate` / `DateTime<Utc>`
   helpers.
10. `src/util/search_type.rs` — static `SearchType` enum + maps.
11. Sanitiser unit-tested in `src/util/sanitize.rs`.
12. Apply CLAUDE.md§4 corrections to `Cargo.toml`: drop `mdns-sd`, add
    `tokio` `net` feature, add `tokio-tungstenite` (gated behind a
    new feature or default-on — pick one; recommend default-on since
    real-time alerts are core).

**Rationale:** Nothing higher in the stack can be tested without
these. Coverage on this layer alone should be ~95% (pure
functions/parsers/builders).

### M1 — Minimum viable client

**Land in M1:**
1. `src/auth/` — token sign-in (`MyPlexAuth<Anonymous>::with_token`).
2. `src/auth/token.rs` — `PlexToken` already in M0; this lands the
   integration with `MyPlexAuth`.
3. `src/myplex/mod.rs` — `MyPlexAccount` construction from `with_token`,
   `_loadData` equivalent from `<user>` XML.
4. `src/server/mod.rs` — `PlexServer::connect(base_url, token)` —
   `GET /` + `_loadData` from the root document.
5. `src/server/system.rs` (partial) — `PlexServer::identity()`
   (`GET /identity`).
6. `src/library/mod.rs` — `Library::sections()` listing.
7. `src/library/section.rs` — `LibrarySection` enum: `Movie`, `Show`,
   `Music`, `Photo` (+ fallback `Other`).
8. `src/xml/dto/library.rs` — `<Directory>` DTO + `From` conversion.
9. First integration tests: list sections, fetch identity, error
   mapping.

**Rationale:** This is the smoke-test client. A user can authenticate
and discover what's on their server. Everything else hangs off this.

### M2 — Read-only media domain

**Land in M2:**
1. `src/media/video.rs` — `Movie`, `Show`, `Season`, `Episode`,
   `Clip`, `Extra` (+ `*Session`, `*History` composition types).
2. `src/media/audio.rs` — `Artist`, `Album`, `Track`.
3. `src/media/photo.rs` — `Photoalbum`, `Photo`.
4. `src/media/media_stream.rs` — `Media`, `MediaPart`, `Stream` enum
   (`Video`/`Audio`/`Subtitle`/`Lyric`).
5. `src/media/tags.rs` — `Tag` struct + `TagKind` enum (14 kinds
   collapsed per `analysis/06§F`).
6. `src/media/markers.rs` — `Marker { kind: MarkerKind, ... }`,
   `Chapter`.
7. `src/xml/dto/metadata.rs` — DTO for every `<Video>`/`<Track>`/etc.
   element.
8. `src/library/search.rs` — `LibrarySection::all()`,
   `LibrarySection::search(title)`, `LibrarySection::recently_added()`,
   `LibrarySection::on_deck()`, hub search.
9. `src/library/filters.rs` — `FilterBuilder` (§7) with server-side
   namespace only. Client-side `__` operator support lands in M3.

**Rationale:** Read-side parity with python-plexapi. Every fixture
under `tests/fixtures/media/` and `tests/fixtures/library/` becomes a
parser snapshot test. Coverage of the DTO layer drives most of the
crate-level coverage gate.

### M3 — Edit / tag / lock mixins as traits

**Land in M3:**
1. `src/traits/` — `PlexObject`, `Reload`, `Ratable`, `PlayedUnplayed`,
   `HasArtUrl`/`HasPosterUrl`/etc.
2. `src/traits/editable.rs` — `EditField` + every field-specific trait
   (`EditTitle`, `EditSummary`, …). Default-body methods.
3. `src/traits/capabilities.rs` — `capabilities!` macro.
4. `src/batch.rs` — `EditBatch` transaction (per Python's
   `batchEdits`/`saveEdits`, `analysis/08§3.1`).
5. `src/traits/images.rs` — full image traits + upload/set/delete.
6. `src/library/filters.rs` — client-side namespace
   (`__icontains`/`__gt`/etc.) via a `client(|q|…)` closure.
7. `src/library/smart_filter.rs` — smart-filter parser (read-only;
   write deferred).

**Rationale:** Edit support is the largest single trait-architecture
investment. Doing it after M2 lets the trait surface be designed
against real types rather than speculatively.

### M4 — Playback / playlists / collections / play queues / sessions / history

**Land in M4:**
1. `src/playback/mod.rs` — `PlayQueue` create/get/mutate.
2. `src/media/playlist.rs` — regular + smart + M3U creation, mutations.
3. `src/media/collection.rs` — regular + smart, mutations,
   `ManagedHub` visibility (read).
4. `src/playback/client.rs` — `PlexClient`, command protocol,
   `playMedia` start-playback dance.
5. `src/playback/transcode.rs` — `/transcode/universal` URL builder
   with decision endpoint support.
6. `src/server/sessions.rs` — `sessions()`, `transcode_sessions()`,
   `PlexSession::stop()`.
7. `src/server/history.rs` — `history()` with the operator-suffix
   query DSL.
8. `src/server/settings.rs` — `Settings` + `Setting` with two-phase
   commit via the staging slot.
9. `src/server/butler.rs`, `src/server/activities.rs`,
   `src/server/updater.rs`, `src/server/statistics.rs`,
   `src/server/browse.rs`, `src/server/transcode.rs`.

**Rationale:** This is the action layer — the moment users can drive
playback and mutate state at the playback/queue level. Depends on
M3's traits because playlists/collections compose `EditField`.

### M5 — Real-time, discovery, cloud catalogue, webhooks

**Land in M5:**
1. `src/auth/pin.rs` — PIN flow (state machine in §9).
2. `src/auth/password.rs` — username + password + 2FA.
3. `src/myplex/resources.rs` — `MyPlexResource`, parallel connect
   race with proper TLS error surfacing.
4. `src/myplex/devices.rs`, `src/myplex/friends.rs`,
   `src/myplex/home.rs`, `src/myplex/webhooks.rs`,
   `src/myplex/claim.rs`, `src/myplex/sonos.rs`.
5. `src/discover/` — watchlist read + mutate, JSON Discover search,
   streaming-service availability.
6. `src/metadata_provider/` — Discover item user-state, scrobble.
7. `src/alerts/` — WebSocket stream + reconnect + typed events (§8).
8. `src/discover_gdm/` — raw-UDP GDM scan (§4.1).
9. `src/webhook/` — payload deser + Axum extractor under feature.
10. `src/playback/sync.rs` — legacy mobile sync (best-effort,
    documented gaps).

**Rationale:** Last because each item is independently optional from
the perspective of a basic Plex client. Real-time alerts and webhooks
are both opt-in surfaces; PIN/auth flow is only needed for users not
pre-issued a token; GDM is purely a convenience. Shipping these
together as M5 lets us focus the v1.0 release announcement on a
complete optional-features story.

---

## 11. Risk register

Top 10 risks ranked by likelihood × impact, with mitigations.

| # | Risk | L | I | Mitigation |
|---|---|---|---|---|
| 1 | **Plex.tv endpoint deprecation mid-development** — the legacy `/api/users/` and `/api/v2/sharings/` family have been around forever but are not in any spec; Plex could change them. | M | H | Wiremock fixtures + `--features live-tests` opt-in CI run once a week against a real PMS to detect drift. Failure → spec issue, not blocker. (`analysis/03§5`, `analysis/10§3.5`) |
| 2 | **OpenAPI drift mid-development** — the spec we pinned (`1.2.2`) ages; new endpoints appear. | H | M | Treat spec as advisory (`analysis/01§9.3`). Re-capture fixtures monthly with `examples/dump_fixtures.rs` against latest PMS. Diff manifest hash → review. |
| 3 | **Smart-filter URI round-trip complexity** — full grammar with `push=/pop=` plus the trailing-operator trim is brittle. | M | M | Ship parser only in v1; defer write-side round-trip to v1.1. Document as known limitation. (`analysis/05§7`) |
| 4 | **Live-test coverage gaps** — `wiremock` cannot exercise real WebSocket protocol details, real-server pagination edge cases, or real transcode decisions. | H | M | `--features live-tests` integration suite plus benchmarks. CI runs the live suite on a weekly schedule; PRs may opt in. (`analysis/10§3.5`) |
| 5 | **XML parsing failure modes** — Plex returns malformed Unicode occasionally (`utils.py:795-833`); python-plexapi has a sanitizer regex pass. | M | M | Mirror the regex sanitizer in `xml::parser`. Unit tests with the known-bad bytes. (`analysis/02§7` `parseXMLString`) |
| 6 | **Trait surface explosion** — ~50 traits is a lot to navigate. | H | L | `capabilities!` macro is the single source of truth, mirrored from `analysis/08§2`. Crate-level `prelude` module re-exports the common ones. Doc index page lists every trait + leaf compatibility table. |
| 7 | **Cancellation correctness** — `Stream`-based pagination and WebSocket reconnect both have non-trivial cancel-safety stories. | M | M | Every public `async fn` has a `# Cancel safety` rustdoc section. Test cancellations explicitly with `tokio::select! { ... = ... => {}, _ = tokio::time::sleep(short) => {} }` patterns. (`CLAUDE.md§14.7`) |
| 8 | **Plex Pass subscription gating** — sonos, view-state-sync, opt-out, edition titles, CommonSenseMedia all require Plex Pass and silently fail or return 403 otherwise. | M | L | Document gating per-method in rustdoc. Error message surfaces 403 unambiguously. Live-test suite gated by `account_plexpass` parity with python-plexapi. (`analysis/03§9.4-§9.9`, `analysis/06§10`) |
| 9 | **Webhook ingest scope creep** — caller wants signed payload verification, retries, deduping. PMS provides none of that. | L | M | Document Plex's lack of signing. Webhook crate provides only deser + Axum extractor; rest is the application's. (`analysis/09§4.2`) |
| 10 | **Photo libraries can contain video** — `Photoalbum.children` returns mixed `Photoalbum`/`Photo`/`Clip`. Easy to miss when modelling. | L | M | The child enum `PhotoalbumChild` explicitly carries all three variants. Test fixture under `tests/fixtures/library/photo_section_with_video.xml`. (`analysis/06§2.11`) |

---

## 12. Definition of done (v1.0)

Restating `CLAUDE.md§12` and `CLAUDE.md§13` with criteria pulled
from the analyses.

**v1.0 ships when all of the following are true:**

### Code completeness
- [ ] Every leaf class in `analysis/06` (Movie, Show, Season,
      Episode, Clip, Extra, Artist, Album, Track, Photoalbum, Photo,
      Playlist, Collection) has a Rust equivalent with `Deserialize`
      coverage on every documented field per §2.X of `analysis/06`.
- [ ] Every endpoint in the appendix of `analysis/04` (PlexServer),
      `analysis/05§B` (library), `analysis/03§11` (MyPlex),
      `analysis/07§9` (playback) has a hand-written request method.
- [ ] The mixin matrix in `analysis/08§2` is fully expressed via
      `capabilities!` macro invocations. No leaf is missing a trait
      it has in python-plexapi.
- [ ] All three auth flows (`analysis/03§2.1-§2.3`) work end-to-end.
      JWT (`§2.4`) is **explicitly out of scope** for v1.0 — note in
      `MyPlexAuth`'s rustdoc.

### Wire-level parity
- [ ] Every URI scheme variant in `analysis/07§8` is in
      `PlexUri::parse()` and round-trips losslessly (one test per
      citation).
- [ ] Pagination uses `X-Plex-Container-*` headers, verified against
      real-server fixture (`analysis/01§4.2`).
- [ ] Error mapping covers `analysis/02§6`'s five exception classes
      (mapped to four `Error` variants since `TwoFactorRequired` is
      surfaced via the auth state machine, not as a top-level error).
- [ ] Retry policy retries only idempotent verbs + 5xx + transport
      errors; never 401, never 404, never POST.

### Tests
- [ ] `cargo llvm-cov --all-features --fail-under-lines 90
      --fail-under-branches 85` passes (`CLAUDE.md§9, §10`).
- [ ] Every fixture in `tests/fixtures/` is sanitised — sanitiser
      idempotency test passes (`analysis/10§6`).
- [ ] At least one happy-path + one failure-path integration test per
      surface area listed in `analysis/10§5.1`.
- [ ] WebSocket reconnect logic has explicit test coverage with a
      custom `tokio-tungstenite` server fixture (`analysis/10§3.7`).

### Docs
- [ ] Every public item has a `///` doc comment; `missing_docs` is
      `deny`-level.
- [ ] Every fallible function lists `# Errors`.
- [ ] Every cancellable async function has `# Cancel safety`.
- [ ] `lib.rs` crate-doc opens with the auth → server → library →
      media → playback tour (`CLAUDE.md§11`).
- [ ] `analysis/11` (this doc) and `CLAUDE.md` agree on every
      load-bearing decision; the deltas in §4 above have been
      reconciled.

### Hygiene
- [ ] `cargo fmt --all --check` clean.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`
      clean.
- [ ] `cargo deny check` clean (`CLAUDE.md§4`).
- [ ] No `unsafe`. No `unwrap()`/`expect()` in non-test library code.
      No `println!`/`eprintln!`. No `tokio::spawn` except where
      explicitly documented (currently: zero locations).
- [ ] `CHANGELOG.md` records every breaking change up to 1.0
      (`CLAUDE.md§12`).

### Explicit non-goals at v1.0
- JWT auth flow (deferred to v1.1).
- Smart-filter write-side round-trip (deferred to v1.1; read-side
  parser ships in v1.0).
- DLNA, transcoder reimpl, direct media bytes (per `CLAUDE.md§16`).
- CLI binary (per `CLAUDE.md§16`).

---

## Closing note

This synthesis is the single load-bearing document for the port. If
this doc disagrees with `CLAUDE.md`, this doc wins; raise a PR
updating `CLAUDE.md` rather than working around the conflict. If a
new finding lands in `analysis/`, it should either confirm an
existing recommendation here or trigger a documented amendment
section at the end of this file.

The order of operations for any future contributor is:

1. Read `CLAUDE.md` §1–§3, §6, §13.
2. Read this document, end to end.
3. Read the specific `analysis/0N` referenced by the milestone you're
   working on.
4. Open a draft PR against the next unchecked item in §10 of this
   doc.
5. The PR description points at the §10 milestone, §11 risks
   touched, and §12 DoD items moved from `[ ]` to `[x]`.

That's the loop.
