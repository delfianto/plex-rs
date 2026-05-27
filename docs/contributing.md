# Contributing

This document covers how to add things to the crate — new
endpoints, new domain types, new traits. For high-level
architecture, start with [`architecture.md`](./architecture.md);
for testing conventions, [`testing.md`](./testing.md).

The project charter and coding rules live in
[`../CLAUDE.md`](../CLAUDE.md). Read it before opening a PR.

---

## Adding a new endpoint

The walk-through. Use existing modules as templates — every
new endpoint lands in one of three shapes:

### Shape A: a simple GET that returns a list

Pattern modeled on `MyPlexClient::devices()`.

1. **Create the module.** `src/<area>/<thing>.rs` for whatever
   it is. If it hangs off `MyPlexClient`, add to `src/myplex/`.
   If off `PlexServer`, add to `src/server/`. If off `Library`,
   `src/library/`.

2. **Define the DTO.** A `pub(crate)` struct with serde derive,
   matching Plex's exact field names. Use `#[serde(rename_all = "camelCase")]`
   for the easy fields; explicit `#[serde(rename)]` for the
   `*ID` / `*URI` / `IPv6` exceptions
   ([`design-patterns.md`](./design-patterns.md#wire-spelling-exceptions-via-explicit-rename)).

3. **Define the domain type.** `pub`, `#[non_exhaustive]`,
   `#[derive(Debug, Clone)]`. Use typed fields where the DTO
   has stringly-typed wire values — `RatingKey` instead of
   `String`, `DateTime<Utc>` instead of epoch-string.

4. **Implement the conversion.** `impl Dto { fn into_domain(self) -> Result<Domain> }`
   or `impl From<Dto> for Domain`. Do validation here.

5. **Add the accessor method.** `impl MyPlexClient { pub async fn things(&self) -> Result<Vec<Thing>> }`.
   Inside, construct the URL, GET via `self.http().get_json` (or
   `get_bytes` for XML endpoints), and convert each DTO to a
   domain object.

6. **Wire-up.** Add `pub mod things;` to `src/<area>/mod.rs` and
   `pub use things::Thing;` to the same. Then add the same
   re-export at `src/lib.rs` so it appears in the crate's top-
   level surface.

7. **Tests.** See [`testing.md`](./testing.md#how-to-add-a-test).

### Shape B: an endpoint with options / filters

Pattern modeled on `WatchlistOptions` /
`MyPlexClient::watchlist_with(&opts)`.

Everything from Shape A, plus:

1. **Define an Options struct.** `pub struct ThingOptions { ... }`
   with `Default` impl. Use `Option<T>` for fields that have
   no default.

2. **Add builder methods.** Each takes `mut self` and returns
   `Self`. Naming: `.with_<field>(value)` for setting,
   `.without_<field>()` for clearing.

3. **Provide a default-options accessor too.** `pub async fn things(&self)`
   calls `things_with(&ThingOptions::default()).await`. The
   verbose version takes `&ThingOptions`.

### Shape C: a mutation endpoint

Pattern modeled on the `PlayQueue` self-consuming methods.

Everything from Shape A, plus:

1. **Decide on the return shape.** If the mutation returns a
   useful new state (refreshed snapshot, server-mutated id),
   consume `self` and return `Result<Self>`. If it just emits
   a side effect, take `&self` and return `Result<()>`.

2. **Implement client-side validation where possible.** Reject
   obvious mistakes (unknown id, wrong-kind value, out-of-range
   numeric) before issuing the request. See `Settings::validate`
   for the canonical pattern.

3. **Build the URL via `url::Url::query_pairs_mut`**, not
   `format!`. Percent-encoding happens for free that way.

---

## Adding a new domain leaf type

If Plex introduces a new top-level media kind (it shouldn't,
but…), the recipe:

1. **Define the leaf type** in `src/media/<kind>.rs` with all
   the scalar fields it has, plus `media: Vec<Media>`,
   `tags: Vec<Tag>`, `markers: Vec<Marker>` (where applicable),
   and the mandatory `section_ref: LibrarySectionRef` back-link.

2. **Add a variant to `LibraryItem`** in `src/media/mod.rs`.
   Update the `match` arms in `LibraryItem::title()`,
   `LibraryItem::rating_key()`, `LibraryItem::key()`,
   `LibraryItem::list_type()`.

3. **Add a `MetadataDto::into_<kind>()` conversion**. Most fields
   come straight from the shared DTO; only kind-specific ones
   need carving out.

4. **Wire dispatch.** Add the new wire `type` string to the
   match in `MetadataDto::into_library_item`.

5. **Implement applicable M3 traits.** Use the trait architecture
   in [`architecture.md`](./architecture.md#trait-architecture-m3).
   Most leaves implement `PlexObject`, `Reload`, `Ratable`,
   `EditField`, `EditTags`, `Playable` (if media-bearing), and
   the per-family edit / tag traits that apply.

6. **Tests** — wiremock integration covering happy-path listing
   and one of the M3 traits per the testing guide.

---

## Adding a new M3 capability trait

For a one-off capability (not field- or tag-specific), define
directly:

```rust
pub trait Splittable: PlexObject {
    fn split(&self) -> impl Future<Output = Result<()>> + Send
    where
        Self: Sync,
    {
        async move {
            let path = format!(
                "/library/metadata/{rk}/split",
                rk = self.rating_key(),
            );
            let url = self.base_url().join(&path)?;
            self.http().put_no_body(url.as_str()).await
        }
    }
}

impl Splittable for Movie {}
impl Splittable for Show {}
```

For a field-specific edit trait, use the existing macro:

```rust
// In src/traits/edit_field.rs, alongside the other declare_edit_field_trait! calls.
declare_edit_field_trait!(EditNewField, edit_new_field, "newField");
```

That single line generates the trait, the default-body method,
and the doc string. Then add `impl EditNewField for Movie {}`
in the leaf module.

For a tag-family trait, use `declare_tag_trait!`:

```rust
declare_tag_trait!(HasNewFamily, replace_new_family, "newFamily");
```

The `EditBatch` builder picks up new field/tag traits
automatically via the generic `set_field` / `replace_tags`
primitives. Add a convenience shortcut to `EditBatch` if the
trait is going to see a lot of use (`replace_genres` etc.).

---

## Adding a feature-gated module

Three modules are gated: `alerts`, `discovery`, `webhook-axum`.
The pattern:

1. **Declare the feature in `Cargo.toml`:**

   ```toml
   [features]
   my-feature = ["dep:heavyweight-crate"]

   [dependencies]
   heavyweight-crate = { version = "X", optional = true, default-features = false, features = [...] }
   ```

2. **Gate the module in `src/lib.rs`:**

   ```rust
   #[cfg(feature = "my-feature")]
   pub mod my_module;
   ```

3. **Gate the re-exports too:**

   ```rust
   #[cfg(feature = "my-feature")]
   pub use crate::my_module::{Type1, Type2};
   ```

   (Re-exports inside `lib.rs` need their own `cfg` because
   the module itself is gated.)

4. **Gate the integration tests:**

   ```rust
   // tests/my_feature_tests.rs
   #![cfg(feature = "my-feature")]
   // ...
   ```

5. **Verify under `--no-default-features`:**

   ```bash
   cargo clippy --all-targets --no-default-features -- -D warnings
   ```

   This catches the common bug of forgetting a `cfg` somewhere.
   The fourth CI gate runs this.

---

## Coding conventions

Strict ones (enforced by clippy / rustdoc):

- `#![forbid(unsafe_code)]` — no unsafe anywhere, period.
- `missing_docs` is `deny` — every public item has `///` docs.
- `clippy::all` + `clippy::correctness` + `clippy::suspicious` +
  `clippy::perf` + `clippy::style` are all `deny`.
- `clippy::pedantic` + `clippy::nursery` + `clippy::cargo` are
  `warn`. Most warnings still fail CI (because `-D warnings`),
  but a few specific lints are crate-globally allowed with
  rationale at the allow site.

Project-specific ones (from CLAUDE.md):

- **No `.unwrap()` / `.expect()`** in library code outside
  `const` contexts. Tests can use `.unwrap()` freely.
- **No `println!` / `eprintln!`** in library code. Use `tracing`.
- **No `tokio::spawn`** in library code. Callers own their
  runtime.
- **No `lazy_static`, no `once_cell`** — use `std::sync::OnceLock`
  or `LazyLock` (stable since 1.80).
- **No `async-trait` macro** for native AFIT. The one exception
  is `webhook::WebhookPayload`'s `FromRequest` impl which has
  to match axum's `#[async_trait]`-using trait.
- **No mocking the database / network at unit-test level.**
  Pure-logic unit tests live in `src/`, network tests use
  `wiremock` in `tests/`.
- **Tokens never logged.** `PlexToken` has the redacted `Debug`;
  use it.

Style:

- Module headers use `//!` doc comments that describe the
  module's purpose, wire endpoint(s), and any non-obvious
  design choices. Refer to architecture / design-pattern docs
  with intra-doc links.
- Function `///` comments include `# Errors` listing the
  variants of `Error` the function can return.
- Inline comments explain *why*, not *what*. Don't reference
  the current task or recent fixes — that belongs in commits.
- Prefer match statements for fixed sets; if/let chains for
  early-return logic.
- Builders chain `.with_<field>(value)`. Self-consuming
  mutators take `mut self` and return `Self` (or `Result<Self>`).

---

## Commit hygiene

- One milestone per commit. Mixing two milestones is fine when
  they're tightly coupled (a new module + the lib re-export);
  not when they're independent (a bug fix + a new feature).
- Commit messages: imperative present, first line under 70
  chars, body wraps at 72 chars and explains the *why*. The
  changelog gets the user-facing summary; the commit message
  gets the technical rationale.
- The CI gates must pass on every commit (not just the tip of
  a PR). If you're mid-feature, squash or rebase before pushing.

---

## Adding a dependency

Justify it in the PR description. The crate is intentionally
small-dep:

- `reqwest`, `tokio`, `serde`, `serde_json` — non-negotiable.
- `quick-xml`, `chrono`, `url`, `uuid`, `tracing`, `bytes`,
  `futures-util`, `thiserror`, `regex` — small, well-maintained,
  earn their weight.
- `tokio-tungstenite`, `axum` — opt-in only, behind a feature.

Before adding a new crate:

1. Is there a stdlib alternative? `OnceLock`, `LazyLock`,
   `std::time`, `std::net::UdpSocket` (we use the tokio variant
   but std works for sync code) — these often suffice.
2. Is the crate well-maintained? Last commit, advisory history,
   dependency tree depth. Run `cargo deny check` to see what it
   pulls in.
3. Is it widely-used? Avoid niche / one-author crates if a more
   mainstream alternative exists.

The current dep tree is reviewed under `cargo deny check`
which fails CI on GPL/AGPL transitive deps and on RUSTSEC
advisories.

---

## Releasing

(Pre-1.0; mostly informational.)

1. Update `CHANGELOG.md` with the new release section. Move
   the contents of `[Unreleased]` to `[X.Y.Z] - YYYY-MM-DD`.
2. Bump version in `Cargo.toml`.
3. `cargo publish --dry-run` to sanity-check.
4. Tag the commit (`git tag vX.Y.Z`).
5. `cargo publish`.
6. Push the tag (`git push origin vX.Y.Z`).

Pre-1.0 every minor bump can have breaking changes; document
them under a `### Breaking` heading in the changelog entry.
