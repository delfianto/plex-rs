//! Tag domain — `<Genre>` / `<Director>` / `<Writer>` / `<Country>` /
//! `<Producer>` / `<Role>` / `<Collection>` / `<Label>` / `<Mood>` /
//! `<Style>` children that Plex attaches to metadata items.
//!
//! Plex emits each family under its own JSON field name (`Genre`,
//! `Director`, …) but every element shares the same shape: an
//! integer `id`, the text `tag` value, plus optional fields specific
//! to the kind (`role` and `thumb` for `<Role>`, `filter` URI for
//! several others). Modelled as a single [`Tag`] carrying a
//! [`TagKind`] discriminator so consumers can keep them all in one
//! `Vec<Tag>` without losing the per-kind context.
//!
//! Field-locks (`<Field>` children) are intentionally **not** modelled
//! as `Tag`s — they don't carry a value, they carry a `name` +
//! `locked` flag. They land in M3 alongside the edit traits that
//! consume them.

use serde::Deserialize;

// -----------------------------------------------------------------------------
// TagKind.
// -----------------------------------------------------------------------------

/// Which Plex tag family this tag belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TagKind {
    /// `<Genre>` — e.g. "Action", "Sci-Fi".
    Genre,
    /// `<Director>` — director names.
    Director,
    /// `<Writer>` — writer / screenwriter names.
    Writer,
    /// `<Country>` — production country.
    Country,
    /// `<Producer>` — producer names.
    Producer,
    /// `<Role>` — actor names. Carries `role` (character name) and
    /// `thumb` (actor photo) in addition to the base fields.
    Role,
    /// `<Collection>` — owning collection title.
    Collection,
    /// `<Label>` — record label / publisher.
    Label,
    /// `<Mood>` — music mood descriptor.
    Mood,
    /// `<Style>` — music style descriptor.
    Style,
    /// Plex tag family this build doesn't yet model. Preserves the
    /// raw JSON field name so callers can pattern-match.
    Other(String),
}

impl TagKind {
    /// Canonical JSON / XML field name for this kind.
    #[must_use]
    pub fn as_wire(&self) -> &str {
        match self {
            Self::Genre => "Genre",
            Self::Director => "Director",
            Self::Writer => "Writer",
            Self::Country => "Country",
            Self::Producer => "Producer",
            Self::Role => "Role",
            Self::Collection => "Collection",
            Self::Label => "Label",
            Self::Mood => "Mood",
            Self::Style => "Style",
            Self::Other(s) => s,
        }
    }
}

// -----------------------------------------------------------------------------
// Tag.
// -----------------------------------------------------------------------------

/// One tag attached to a metadata item.
///
/// `kind` tells you which family the tag belongs to; `value` is the
/// human-readable label (`"Action"`, `"Denis Villeneuve"`, …);
/// optional fields are populated only when Plex emits them.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Tag {
    /// Tag family.
    pub kind: TagKind,
    /// Tag value (the `tag` attribute on the wire).
    pub value: String,
    /// Numeric Plex identifier for this tag (used by `editTags`).
    pub id: Option<u64>,
    /// Character name — only populated for [`TagKind::Role`].
    pub role: Option<String>,
    /// Actor photo path — only populated for [`TagKind::Role`].
    pub thumb: Option<String>,
    /// Smart-filter URI Plex uses to construct "find more with this
    /// tag" queries — populated for Genre / Director / etc.
    pub filter: Option<String>,
}

// -----------------------------------------------------------------------------
// DTO.
// -----------------------------------------------------------------------------

/// Wire shape shared by every tag-family element.
#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct TagDto {
    #[serde(default)]
    pub(crate) id: Option<u64>,
    #[serde(default)]
    pub(crate) tag: Option<String>,
    #[serde(default)]
    pub(crate) role: Option<String>,
    #[serde(default)]
    pub(crate) thumb: Option<String>,
    #[serde(default)]
    pub(crate) filter: Option<String>,
}

impl TagDto {
    pub(crate) fn into_tag(self, kind: TagKind) -> Tag {
        Tag {
            kind,
            value: self.tag.unwrap_or_default(),
            id: self.id,
            role: self.role,
            thumb: self.thumb,
            filter: self.filter,
        }
    }
}

/// Bundle of every tag-family vector pulled off a `MetadataDto`.
/// Keeps [`collect`] argument-free at the call site while staying
/// crate-private.
#[derive(Debug, Default)]
pub(crate) struct TagFamilies {
    pub(crate) genres: Vec<TagDto>,
    pub(crate) directors: Vec<TagDto>,
    pub(crate) writers: Vec<TagDto>,
    pub(crate) countries: Vec<TagDto>,
    pub(crate) producers: Vec<TagDto>,
    pub(crate) roles: Vec<TagDto>,
    pub(crate) collections: Vec<TagDto>,
    pub(crate) labels: Vec<TagDto>,
    pub(crate) moods: Vec<TagDto>,
    pub(crate) styles: Vec<TagDto>,
}

/// Collect every tag family into a single `Vec<Tag>` for ergonomic
/// downstream filtering. Internal — called from the metadata `into_*`
/// conversion methods.
pub(crate) fn collect(families: TagFamilies) -> Vec<Tag> {
    let TagFamilies {
        genres,
        directors,
        writers,
        countries,
        producers,
        roles,
        collections,
        labels,
        moods,
        styles,
    } = families;
    let mut out = Vec::with_capacity(
        genres.len()
            + directors.len()
            + writers.len()
            + countries.len()
            + producers.len()
            + roles.len()
            + collections.len()
            + labels.len()
            + moods.len()
            + styles.len(),
    );
    let mut push = |dtos: Vec<TagDto>, kind: TagKind| {
        for d in dtos {
            out.push(d.into_tag(kind.clone()));
        }
    };
    push(genres, TagKind::Genre);
    push(directors, TagKind::Director);
    push(writers, TagKind::Writer);
    push(countries, TagKind::Country);
    push(producers, TagKind::Producer);
    push(roles, TagKind::Role);
    push(collections, TagKind::Collection);
    push(labels, TagKind::Label);
    push(moods, TagKind::Mood);
    push(styles, TagKind::Style);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_kind_as_wire_is_canonical() {
        assert_eq!(TagKind::Genre.as_wire(), "Genre");
        assert_eq!(TagKind::Role.as_wire(), "Role");
        assert_eq!(TagKind::Other("Awards".to_owned()).as_wire(), "Awards");
    }

    #[test]
    fn tag_dto_round_trips_full_role() {
        let json = serde_json::json!({
            "id": 42,
            "tag": "Amy Adams",
            "role": "Louise Banks",
            "thumb": "/library/people/42/thumb"
        });
        let dto: TagDto = serde_json::from_value(json).unwrap();
        let tag = dto.into_tag(TagKind::Role);
        assert_eq!(tag.kind, TagKind::Role);
        assert_eq!(tag.value, "Amy Adams");
        assert_eq!(tag.id, Some(42));
        assert_eq!(tag.role.as_deref(), Some("Louise Banks"));
        assert_eq!(tag.thumb.as_deref(), Some("/library/people/42/thumb"));
    }

    #[test]
    fn tag_dto_minimal_genre() {
        let json = serde_json::json!({"tag": "Sci-Fi"});
        let dto: TagDto = serde_json::from_value(json).unwrap();
        let tag = dto.into_tag(TagKind::Genre);
        assert_eq!(tag.value, "Sci-Fi");
        assert!(tag.id.is_none());
        assert!(tag.role.is_none());
        assert!(tag.thumb.is_none());
    }

    #[test]
    fn collect_preserves_per_family_order() {
        let f = TagFamilies {
            genres: vec![TagDto {
                tag: Some("Action".to_owned()),
                ..Default::default()
            }],
            directors: vec![TagDto {
                tag: Some("Villeneuve".to_owned()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let tags = collect(f);
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].kind, TagKind::Genre);
        assert_eq!(tags[0].value, "Action");
        assert_eq!(tags[1].kind, TagKind::Director);
        assert_eq!(tags[1].value, "Villeneuve");
    }
}
