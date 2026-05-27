# Out of scope

Six items are **formally not implemented and won't be**. This
document records each one with rationale, so future contributors
don't waste time re-litigating the decision.

For things that are merely *not yet implemented but could be*,
see the "Forward-compat gaps" section at the bottom of
[`api-coverage.md`](./api-coverage.md). The items below are
different — they've been actively considered and rejected.

---

## 🔴 M3.7 — `capabilities!` declarative macro

**What it would be:** A meta-macro that generates the impl matrix
from a single declaration site like:

```rust
capabilities!(Movie => Ratable + EditTitle + EditYear + EditTagline + HasGenres + HasDirectors + HasWriters + ...);
```

**Why not:** Zero direct user-facing value — this is an internal
refactor.

The existing `declare_edit_field_trait!` and `declare_tag_trait!`
macros already handle the per-trait expansion without
duplication. The remaining boilerplate is just `impl X for Y {}`
lines that the meta-macro would collapse, but:

1. The empty impls are scannable as documentation — a reader
   can see which capabilities Movie has by reading the impl block,
   without needing to learn yet another macro syntax.
2. Adding a third macro layer would obscure the call sites with
   no measurable payoff.
3. The maintenance cost is "write 30 empty impls" once. Future
   leaf types (none planned) would be a similar one-time cost.

The trade-off goes the wrong way. Skipped.

---

## 🔴 M3.9 — Client-side `__` filter namespace

**What it would be:** python-plexapi-style sugar:

```python
section.search(genre__exact="Sci-Fi", year__gte=2000)
```

**Why not:** Duplicates the typed `FilterBuilder` API with
stringly-typed sugar.

Rust callers reach for the typed builder, which is more
discoverable, type-safe, and self-documenting:

```rust
section.filter()
    .exact("genre", "Sci-Fi")
    .gt("year", 2000)
    .execute().await?
```

The `__` namespace would only be valuable as a porting aid for
users translating python-plexapi scripts line-by-line. That's a
real audience but a small one, and the cost of maintaining two
parallel filter APIs (and their interaction with the M3 trait
architecture) isn't justified. We have one canonical filter
surface; that's enough.

Skipped.

---

## 🔴 M5.4 — Claim tokens

**What it would be:** `MyPlexClient::generate_claim_token()` to
mint the one-shot credential that binds a fresh Plex Media Server
installation to a plex.tv account.

**Why not:** Used **exactly once per server lifetime**, almost
always through the PMS web setup UI (`http://localhost:32400/web`)
during initial installation.

Very few callers ever need to mint a claim token programmatically
— mostly automation that provisions PMS instances at scale, which
is a small audience already well-served by the simpler approach
of POSTing to `https://plex.tv/api/claim/exchange` by hand.

Excluding it keeps the auth surface focused on the flows real
users hit repeatedly. If someone files an issue with a concrete
use case, this can be revisited.

---

## 🔴 M5.4 — Sonos integration

**What it would be:** The `https://sonos.plex.tv/resources`
endpoint family that targets the specific "Plex for Sonos"
product.

**Why not:** Audience is **Plex Pass holders who also own Sonos
hardware AND want to drive it programmatically** — a tiny
minority of users.

The endpoints are also undocumented by Plex and have shifted
shape across versions. Implementing them would require careful
empirical testing against real Sonos hardware that most
contributors don't have.

Excluded to keep the crate footprint focused on broadly-useful
surfaces. Plex Sonos owners who want this can either use
python-plexapi or open an issue with the wire shapes they need.

---

## 🔴 M5.5 — Availability metadata

**What it would be:** The "available on Netflix / Disney+ / etc."
overlay endpoint that maps cloud-catalogue items to consumer
streaming services.

**Why not:** Narrow audience (recommendation apps), narrow
utility, and **better data sources exist** for callers who
actually need it.

JustWatch, MovieMeter, and other dedicated availability APIs
have richer data, more accurate price/region info, and are
designed for this specific use case. Plex's overlay is a
convenience for Plex Discover UI users, not a comprehensive
data feed.

A `plex-rs` user who wants "where can I watch this?" is better
served pointing at a real availability API. Skipped.

---

## 🔴 M5.10 — Legacy mobile sync

**What it would be:** The `/sync/items` family of endpoints that
support the old "sync library to phone" feature on legacy
mobile Plex clients.

**Why not:** **Plex itself deprecated these endpoints** in favor
of the newer "Download" feature, which uses a different endpoint
family entirely. The legacy endpoints still exist for backward
compatibility with very old mobile-app builds, but Plex's own
documentation recommends not using them, and the surface area
is gradually being removed.

Investing implementation effort in a deprecated API surface
guarantees rework when Plex eventually retires the endpoints.

If anyone genuinely needs legacy sync support — say, for an
embedded device running a five-year-old Plex client build —
they can wrap the endpoints with the existing `HttpClient`
primitives. The crate doesn't owe them a typed wrapper for a
sunsetting API.

Skipped.

---

## What about things that are partially implemented?

The tracker (`TRACKER.md`) uses `[~]` for "partial" status — see
those entries for the precise state of each module. Some
representative deferrals:

- **Image upload** (`HasArt::upload_art`, etc.) — needs `post_bytes`
  on `HttpClient`. The URL-builder and lock toggles are shipped;
  upload is a future iteration.
- **Hub-based universal search** (`/hubs/search`) — section-level
  search is shipped; cross-section hub search defers.
- **Per-stream `setStreams` mid-playback** — `PlexClient` covers
  the common playback commands but not stream-selection mutation.
- **Library section refresh / scan triggers** — read-only library
  navigation is complete; mutation defers.

These differ from the items above: they have real use cases and
would be welcome additions if someone files an issue or PR.

---

## Decision criteria

The six explicit out-of-scope items share a pattern: **the
cost-benefit goes the wrong way**.

In each case:

- **Cost**: Real implementation effort (wire-format parsing,
  testing infrastructure, doc updates) plus ongoing maintenance.
- **Benefit**: Either zero (M3.7 internal refactor), or limited
  to a tiny audience (M5.4 sonos / claim, M5.5 availability), or
  spent on a deprecated API (M5.10), or duplicated by an
  existing surface (M3.9).

Future "should we add X?" decisions should apply the same lens.
If X serves <2% of realistic users AND has no broader value, it
belongs in this document, not in the crate.
