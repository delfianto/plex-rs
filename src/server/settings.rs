//! Plex Media Server settings (`/:/prefs`).
//!
//! [`Settings::load`] fetches every server-level preference at once.
//! Each entry is exposed as a typed [`Setting`] carrying its id,
//! label, summary, group, current and default values, and (for
//! enum-typed settings) the list of valid values.
//!
//! Mutation paths:
//!
//! - [`Settings::set`] writes a single setting and re-loads the
//!   snapshot from the server (Plex returns the full collection on
//!   every PUT).
//! - [`Settings::set_many`] batches multiple updates into one PUT,
//!   matching python-plexapi's `Settings.save()` shape.
//!
//! ## Wire format
//!
//! - `GET /:/prefs` returns `<MediaContainer><Setting .../>…</MediaContainer>`.
//! - `PUT /:/prefs?<id1>=<v1>&<id2>=<v2>` writes settings; PMS
//!   accepts the values as URL-encoded strings regardless of the
//!   declared `type` and does the parsing server-side.

use std::collections::BTreeMap;

use serde::Deserialize;
use url::Url;

use crate::client::HttpClient;
use crate::error::{Error, Result};
use crate::server::PlexServer;

/// Endpoint path for the preferences endpoint.
const PREFS_PATH: &str = "/:/prefs";

// -----------------------------------------------------------------------------
// SettingKind & SettingValue.
// -----------------------------------------------------------------------------

/// Declared type of a [`Setting`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SettingKind {
    /// `type="text"`.
    Text,
    /// `type="int"`.
    Int,
    /// `type="double"`.
    Double,
    /// `type="bool"`.
    Bool,
    /// `type="enum"`. The list of valid values is in
    /// [`Setting::enum_values`].
    Enum,
    /// Any other wire value, preserved verbatim. Plex occasionally
    /// surfaces undocumented kinds; rather than reject them we
    /// passthrough for forward compatibility.
    Other(String),
}

impl SettingKind {
    fn from_wire(s: &str) -> Self {
        match s {
            "text" => Self::Text,
            "int" => Self::Int,
            "double" => Self::Double,
            "bool" => Self::Bool,
            "enum" => Self::Enum,
            other => Self::Other(other.to_owned()),
        }
    }
}

/// One setting value, typed by the declared [`SettingKind`].
///
/// Values arrive on the wire as strings; this enum captures the
/// declared type for callers that want to switch on it. Conversions
/// to / from primitive Rust types are inherent methods.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SettingValue {
    /// `text` / `enum` value, or any other unparsed string.
    Text(String),
    /// Parsed `int` value.
    Int(i64),
    /// Parsed `double` value.
    Double(f64),
    /// Parsed `bool` value (`true` / `"1"`).
    Bool(bool),
}

impl SettingValue {
    /// Best-effort string view of the value, in the spelling Plex
    /// expects on the write path. `true`/`false` → `"true"`/`"false"`;
    /// numerics → standard decimal.
    #[must_use]
    pub fn to_wire(&self) -> String {
        match self {
            Self::Text(s) => s.clone(),
            Self::Int(n) => n.to_string(),
            Self::Double(f) => f.to_string(),
            Self::Bool(b) => (if *b { "true" } else { "false" }).to_owned(),
        }
    }

    /// Borrow the string representation when this is a text value.
    /// Returns `None` for non-text variants.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Try to read this value as an integer.
    #[must_use]
    pub const fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(n) => Some(*n),
            _ => None,
        }
    }

    /// Try to read this value as a double.
    #[must_use]
    pub const fn as_double(&self) -> Option<f64> {
        match self {
            Self::Double(f) => Some(*f),
            _ => None,
        }
    }

    /// Try to read this value as a boolean.
    #[must_use]
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Parse a raw wire value according to the declared [`SettingKind`].
    fn parse(kind: &SettingKind, raw: &str) -> Self {
        match kind {
            SettingKind::Int => raw
                .parse::<i64>()
                .map_or_else(|_| Self::Text(raw.to_owned()), Self::Int),
            SettingKind::Double => raw
                .parse::<f64>()
                .map_or_else(|_| Self::Text(raw.to_owned()), Self::Double),
            SettingKind::Bool => Self::Bool(raw == "true" || raw == "1"),
            SettingKind::Text | SettingKind::Enum | SettingKind::Other(_) => {
                Self::Text(raw.to_owned())
            }
        }
    }
}

/// Enumerated-setting values. Plex emits these as a `|`-delimited
/// string; sometimes with `key:label` pairs, sometimes just keys.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum EnumValues {
    /// Flat list of valid values.
    List(Vec<String>),
    /// `key:label` pairs preserved in document order.
    Mapping(Vec<(String, String)>),
}

impl EnumValues {
    /// Parse Plex's pipe-separated enum string. Examples:
    /// - `"low|medium|high"` → [`Self::List`]
    /// - `"0:Off|1:On"` → [`Self::Mapping`]
    fn from_wire(raw: &str) -> Self {
        if raw.contains(':') {
            let mut pairs = Vec::new();
            for chunk in raw.split('|') {
                if let Some((k, v)) = chunk.split_once(':') {
                    pairs.push((k.to_owned(), v.to_owned()));
                } else {
                    // Mixed: this chunk has no colon. Treat key=label.
                    pairs.push((chunk.to_owned(), chunk.to_owned()));
                }
            }
            Self::Mapping(pairs)
        } else {
            Self::List(raw.split('|').map(str::to_owned).collect())
        }
    }
}

// -----------------------------------------------------------------------------
// Setting.
// -----------------------------------------------------------------------------

/// One PMS preference.
#[derive(Debug, Clone)]
#[non_exhaustive]
// Plex emits ~4 independent boolean flags per setting; collapsing
// them into a bitflag would be an abstraction not driven by the
// underlying schema.
#[allow(clippy::struct_excessive_bools)]
pub struct Setting {
    /// Stable id (e.g. `TranscoderQuality`).
    pub id: String,
    /// Short human-readable label.
    pub label: String,
    /// Long description.
    pub summary: String,
    /// Declared value type.
    pub kind: SettingKind,
    /// Factory-default value (typed per [`Self::kind`]).
    pub default: SettingValue,
    /// Current value.
    pub value: SettingValue,
    /// `true` when the setting is hidden in the standard UI.
    pub hidden: bool,
    /// `true` when the setting is only shown in "advanced" mode.
    pub advanced: bool,
    /// `true` when the value is a secret (e.g. credentials).
    pub secure: bool,
    /// Display-group name (`general`, `transcoder`, …).
    pub group: String,
    /// For enum-typed settings, the list of valid values. `None` for
    /// non-enum settings.
    pub enum_values: Option<EnumValues>,
}

// -----------------------------------------------------------------------------
// Settings — the collection.
// -----------------------------------------------------------------------------

/// All PMS settings.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Settings {
    /// Ordered map (sorted by id) of every setting.
    settings: BTreeMap<String, Setting>,
}

impl Settings {
    /// Fetch the full settings collection from the server.
    ///
    /// # Errors
    /// Any transport / parse [`Error`] variant.
    pub async fn load(server: &PlexServer) -> Result<Self> {
        let url = server.base_url().join(PREFS_PATH)?;
        Self::load_from(server.http(), url.as_str()).await
    }

    /// Crate-private load helper, used by [`Self::set`] and
    /// [`Self::set_many`] to reload after a write.
    async fn load_from(http: &HttpClient, url: &str) -> Result<Self> {
        let body = http.get_bytes(url).await?;
        let body_str = std::str::from_utf8(&body)
            .map_err(|e| Error::Config(format!("/:/prefs body not utf-8: {e}")))?;
        let env: PrefsEnvelope = serde_json::from_str(body_str)?;
        let mut settings = BTreeMap::new();
        for dto in env.container.setting {
            let s = dto.into_domain();
            settings.insert(s.id.clone(), s);
        }
        Ok(Self { settings })
    }

    /// All settings, sorted by id.
    #[must_use]
    pub fn all(&self) -> Vec<&Setting> {
        self.settings.values().collect()
    }

    /// Number of settings in the collection.
    #[must_use]
    pub fn len(&self) -> usize {
        self.settings.len()
    }

    /// `true` when no settings are loaded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.settings.is_empty()
    }

    /// Look up a setting by id (case-sensitive, like Plex).
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Setting> {
        self.settings.get(id)
    }

    /// All settings belonging to `group`, sorted by id.
    #[must_use]
    pub fn group(&self, group: &str) -> Vec<&Setting> {
        self.settings
            .values()
            .filter(|s| s.group == group)
            .collect()
    }

    /// Every group name that appears, sorted.
    #[must_use]
    pub fn group_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.settings.values().map(|s| s.group.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        names
    }

    /// Write a single setting and re-fetch the snapshot.
    ///
    /// # Errors
    /// - [`Error::NotFound`] if `id` doesn't match a known setting.
    /// - [`Error::Config`] if the value type doesn't match the
    ///   setting's declared kind.
    /// - Any transport / parse [`Error`] variant.
    pub async fn set(mut self, server: &PlexServer, id: &str, value: SettingValue) -> Result<Self> {
        self.validate(id, &value)?;
        let url = build_write_url(server.base_url(), &[(id, value)])?;
        server.http().put_no_body(url.as_str()).await?;
        let reloaded =
            Self::load_from(server.http(), server.base_url().join(PREFS_PATH)?.as_str()).await?;
        self.settings = reloaded.settings;
        Ok(self)
    }

    /// Write several settings in one PUT.
    ///
    /// # Errors
    /// - [`Error::NotFound`] if any `id` doesn't match.
    /// - [`Error::Config`] if any value type doesn't match.
    /// - [`Error::Config`] if `updates` is empty.
    /// - Any transport / parse [`Error`] variant.
    pub async fn set_many(
        mut self,
        server: &PlexServer,
        updates: Vec<(&str, SettingValue)>,
    ) -> Result<Self> {
        if updates.is_empty() {
            return Err(Error::Config(
                "set_many requires at least one update".to_owned(),
            ));
        }
        for (id, value) in &updates {
            self.validate(id, value)?;
        }
        let url = build_write_url(server.base_url(), &updates)?;
        server.http().put_no_body(url.as_str()).await?;
        let reloaded =
            Self::load_from(server.http(), server.base_url().join(PREFS_PATH)?.as_str()).await?;
        self.settings = reloaded.settings;
        Ok(self)
    }

    /// Validate `id` exists and `value`'s variant matches the
    /// declared kind. Returns the matching [`Setting`] on success.
    fn validate(&self, id: &str, value: &SettingValue) -> Result<&Setting> {
        let setting = self.settings.get(id).ok_or_else(|| Error::NotFound {
            resource: format!("/:/prefs/{id}"),
        })?;
        let ok = matches!(
            (&setting.kind, value),
            (
                SettingKind::Text | SettingKind::Enum | SettingKind::Other(_),
                SettingValue::Text(_),
            ) | (SettingKind::Int, SettingValue::Int(_))
                | (SettingKind::Double, SettingValue::Double(_))
                | (SettingKind::Bool, SettingValue::Bool(_))
        );
        if !ok {
            return Err(Error::Config(format!(
                "setting {id}: declared {:?} but supplied value is {:?}",
                setting.kind, value,
            )));
        }
        if let (Some(EnumValues::List(opts)), SettingValue::Text(v)) = (&setting.enum_values, value)
        {
            if !opts.iter().any(|o| o == v) {
                return Err(Error::Config(format!(
                    "setting {id}: {v:?} not in enum {opts:?}",
                )));
            }
        }
        if let (Some(EnumValues::Mapping(opts)), SettingValue::Text(v)) =
            (&setting.enum_values, value)
        {
            if !opts.iter().any(|(k, _)| k == v) {
                let keys: Vec<&String> = opts.iter().map(|(k, _)| k).collect();
                return Err(Error::Config(format!(
                    "setting {id}: {v:?} not in enum {keys:?}",
                )));
            }
        }
        Ok(setting)
    }
}

/// Build the PUT URL with all `id=value` pairs encoded.
fn build_write_url(base: &Url, updates: &[(&str, SettingValue)]) -> Result<Url> {
    let mut url = base.join(PREFS_PATH)?;
    {
        let mut qp = url.query_pairs_mut();
        for (id, value) in updates {
            qp.append_pair(id, &value.to_wire());
        }
    }
    Ok(url)
}

// -----------------------------------------------------------------------------
// PlexServer integration.
// -----------------------------------------------------------------------------

impl PlexServer {
    /// Fetch every server-level preference.
    ///
    /// # Errors
    /// Any transport / parse [`Error`] variant.
    pub async fn settings(&self) -> Result<Settings> {
        Settings::load(self).await
    }
}

// -----------------------------------------------------------------------------
// DTOs.
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct PrefsEnvelope {
    #[serde(rename = "MediaContainer")]
    container: PrefsContainer,
}

#[derive(Debug, Deserialize)]
struct PrefsContainer {
    #[serde(rename = "Setting", default)]
    setting: Vec<SettingDto>,
}

#[derive(Debug, Deserialize)]
struct SettingDto {
    id: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    summary: String,
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    default: String,
    #[serde(default)]
    value: String,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    advanced: bool,
    #[serde(default)]
    secure: bool,
    #[serde(default)]
    group: String,
    #[serde(default, rename = "enumValues")]
    enum_values: Option<String>,
}

impl SettingDto {
    fn into_domain(self) -> Setting {
        let kind = SettingKind::from_wire(&self.kind);
        let default = SettingValue::parse(&kind, &self.default);
        let value = SettingValue::parse(&kind, &self.value);
        let enum_values = self.enum_values.map(|s| EnumValues::from_wire(&s));
        Setting {
            id: self.id,
            label: self.label,
            summary: self.summary,
            kind,
            default,
            value,
            hidden: self.hidden,
            advanced: self.advanced,
            secure: self.secure,
            group: self.group,
            enum_values,
        }
    }
}

// -----------------------------------------------------------------------------
// Tests.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_settings(json: serde_json::Value) -> Settings {
        let env: PrefsEnvelope = serde_json::from_value(json).unwrap();
        let mut settings = BTreeMap::new();
        for dto in env.container.setting {
            let s = dto.into_domain();
            settings.insert(s.id.clone(), s);
        }
        Settings { settings }
    }

    fn sample_settings_json() -> serde_json::Value {
        serde_json::json!({
            "MediaContainer": {
                "size": 4,
                "Setting": [
                    {
                        "id": "TranscoderQuality",
                        "label": "Internet streaming quality",
                        "summary": "Default quality for streaming",
                        "type": "int",
                        "default": "100",
                        "value": "60",
                        "group": "transcoder",
                    },
                    {
                        "id": "FriendlyName",
                        "label": "Friendly name",
                        "summary": "User-visible server name",
                        "type": "text",
                        "default": "",
                        "value": "Living Room",
                        "group": "general",
                    },
                    {
                        "id": "logDebug",
                        "label": "Verbose logs",
                        "summary": "Log everything",
                        "type": "bool",
                        "default": "false",
                        "value": "true",
                        "group": "general",
                        "advanced": true,
                    },
                    {
                        "id": "LanNetworksBandwidth",
                        "label": "LAN bandwidth",
                        "summary": "Local subnets",
                        "type": "enum",
                        "default": "low",
                        "value": "high",
                        "group": "transcoder",
                        "enumValues": "low|medium|high",
                    }
                ]
            }
        })
    }

    #[test]
    fn parses_settings_collection_with_all_kinds() {
        let s = parse_settings(sample_settings_json());
        assert_eq!(s.len(), 4);

        let quality = s.get("TranscoderQuality").unwrap();
        assert_eq!(quality.kind, SettingKind::Int);
        assert_eq!(quality.value.as_int(), Some(60));
        assert_eq!(quality.default.as_int(), Some(100));
        assert_eq!(quality.group, "transcoder");

        let name = s.get("FriendlyName").unwrap();
        assert_eq!(name.kind, SettingKind::Text);
        assert_eq!(name.value.as_text(), Some("Living Room"));

        let logging = s.get("logDebug").unwrap();
        assert_eq!(logging.kind, SettingKind::Bool);
        assert_eq!(logging.value.as_bool(), Some(true));
        assert!(logging.advanced);

        let bw = s.get("LanNetworksBandwidth").unwrap();
        assert_eq!(bw.kind, SettingKind::Enum);
        assert_eq!(bw.value.as_text(), Some("high"));
        match &bw.enum_values {
            Some(EnumValues::List(opts)) => {
                assert_eq!(
                    opts,
                    &vec!["low".to_owned(), "medium".to_owned(), "high".to_owned()]
                );
            }
            other => panic!("expected List, got {other:?}"),
        }
    }

    #[test]
    fn enum_values_with_colons_parsed_as_mapping() {
        let raw = "0:Off|1:Low|2:Medium|3:High";
        let parsed = EnumValues::from_wire(raw);
        match parsed {
            EnumValues::Mapping(pairs) => {
                assert_eq!(pairs.len(), 4);
                assert_eq!(pairs[0], ("0".to_owned(), "Off".to_owned()));
                assert_eq!(pairs[3], ("3".to_owned(), "High".to_owned()));
            }
            other => panic!("expected Mapping, got {other:?}"),
        }
    }

    #[test]
    fn enum_values_without_colons_parsed_as_list() {
        let raw = "low|medium|high";
        let parsed = EnumValues::from_wire(raw);
        match parsed {
            EnumValues::List(opts) => assert_eq!(opts.len(), 3),
            other => panic!("expected List, got {other:?}"),
        }
    }

    #[test]
    fn setting_value_wire_format_matches_python() {
        assert_eq!(SettingValue::Text("hi".to_owned()).to_wire(), "hi");
        assert_eq!(SettingValue::Int(42).to_wire(), "42");
        assert_eq!(SettingValue::Bool(true).to_wire(), "true");
        assert_eq!(SettingValue::Bool(false).to_wire(), "false");
    }

    #[test]
    fn setting_kind_from_wire_handles_known_and_unknown() {
        assert_eq!(SettingKind::from_wire("int"), SettingKind::Int);
        assert_eq!(SettingKind::from_wire("bool"), SettingKind::Bool);
        match SettingKind::from_wire("future") {
            SettingKind::Other(s) => assert_eq!(s, "future"),
            other => panic!("expected Other, got {other:?}"),
        }
    }

    #[test]
    fn settings_grouping_respects_group_field() {
        let s = parse_settings(sample_settings_json());
        let groups = s.group_names();
        assert!(groups.contains(&"general"));
        assert!(groups.contains(&"transcoder"));
        assert_eq!(s.group("transcoder").len(), 2);
        assert_eq!(s.group("general").len(), 2);
    }

    #[test]
    fn validate_accepts_matching_value_type() {
        let s = parse_settings(sample_settings_json());
        s.validate("TranscoderQuality", &SettingValue::Int(80))
            .unwrap();
        s.validate("FriendlyName", &SettingValue::Text("X".to_owned()))
            .unwrap();
        s.validate("logDebug", &SettingValue::Bool(false)).unwrap();
    }

    #[test]
    fn validate_rejects_wrong_kind() {
        let s = parse_settings(sample_settings_json());
        let err = s
            .validate("TranscoderQuality", &SettingValue::Text("x".to_owned()))
            .unwrap_err();
        assert!(matches!(err, Error::Config(ref msg) if msg.contains("Int")));
    }

    #[test]
    fn validate_rejects_unknown_id() {
        let s = parse_settings(sample_settings_json());
        let err = s
            .validate("NoSuchSetting", &SettingValue::Int(1))
            .unwrap_err();
        assert!(matches!(err, Error::NotFound { resource } if resource.contains("NoSuchSetting")));
    }

    #[test]
    fn validate_rejects_value_outside_enum_list() {
        let s = parse_settings(sample_settings_json());
        let err = s
            .validate(
                "LanNetworksBandwidth",
                &SettingValue::Text("ludicrous".to_owned()),
            )
            .unwrap_err();
        assert!(
            matches!(err, Error::Config(ref msg) if msg.contains("ludicrous")),
            "got {err:?}"
        );
    }

    #[test]
    fn validate_accepts_valid_enum_choice() {
        let s = parse_settings(sample_settings_json());
        s.validate(
            "LanNetworksBandwidth",
            &SettingValue::Text("medium".to_owned()),
        )
        .unwrap();
    }

    #[test]
    fn build_write_url_encodes_multiple_pairs() {
        let base = Url::parse("http://pms.local:32400").unwrap();
        let url = build_write_url(
            &base,
            &[
                ("TranscoderQuality", SettingValue::Int(60)),
                ("FriendlyName", SettingValue::Text("Living Room".to_owned())),
            ],
        )
        .unwrap();
        let q = url.query().unwrap();
        assert!(q.contains("TranscoderQuality=60"), "{q}");
        // Spaces percent-encoded as `+` by form_urlencoded.
        assert!(q.contains("FriendlyName=Living+Room"), "{q}");
    }
}
