//! Date / time conversion helpers between Plex wire formats and
//! [`chrono`].
//!
//! Plex serialises timestamps in two distinct shapes:
//!
//! 1. **Epoch seconds** as a stringified `i64` for `addedAt`,
//!    `updatedAt`, `lastViewedAt`, `originallyAvailableAt` (when the
//!    item is an episode airdate), etc.
//! 2. **ISO `YYYY-MM-DD`** for `originallyAvailableAt` on movies / TV.
//!
//! Some fields are *almost-always-present* but encoded as `"0"` or
//! `""` when unknown — `python-plexapi/plexapi/utils.py:toDatetime`
//! treats both as `None`. We mirror that behaviour: empty input and
//! the literal `"0"` both deserialise to [`Option::None`].

use chrono::{DateTime, NaiveDate, TimeZone, Utc};

use crate::Error;

/// Parse an epoch-seconds string emitted by Plex.
///
/// Returns [`None`] for empty input or the literal `"0"` — both
/// surface as "unknown" on the wire.
///
/// # Errors
/// Returns [`Error::Config`] only when the value is non-empty,
/// non-zero, and not a valid signed integer.
pub fn parse_epoch_secs(value: &str) -> Result<Option<DateTime<Utc>>, Error> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "0" {
        return Ok(None);
    }
    let secs = trimmed
        .parse::<i64>()
        .map_err(|e| Error::Config(format!("invalid epoch seconds {trimmed:?}: {e}")))?;
    Utc.timestamp_opt(secs, 0)
        .single()
        .map(Some)
        .ok_or_else(|| Error::Config(format!("epoch seconds {secs} out of range")))
}

/// Format a [`DateTime<Utc>`] as the integer-epoch-string Plex expects.
#[must_use]
pub fn format_epoch_secs(dt: DateTime<Utc>) -> String {
    dt.timestamp().to_string()
}

/// Parse a Plex ISO `YYYY-MM-DD` date.
///
/// Empty input becomes [`None`]; the value is otherwise required to be
/// a well-formed civil date.
///
/// # Errors
/// Returns [`Error::Config`] when the value is non-empty but cannot be
/// parsed as `YYYY-MM-DD`.
pub fn parse_iso_date(value: &str) -> Result<Option<NaiveDate>, Error> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    NaiveDate::parse_from_str(trimmed, "%Y-%m-%d")
        .map(Some)
        .map_err(|e| Error::Config(format!("invalid ISO date {trimmed:?}: {e}")))
}

/// `serde` adapter for an optional epoch-seconds field encoded as a
/// JSON string (e.g. `"addedAt": "1700000000"`).
///
/// Apply with `#[serde(with = "plex_rs::util::time::epoch_secs_str_opt")]`.
pub mod epoch_secs_str_opt {
    use super::{DateTime, Utc, parse_epoch_secs};
    use serde::{Deserialize, Deserializer, Serializer};

    /// Serialize `Some(dt)` as a quoted epoch-seconds string; `None`
    /// emits a JSON null.
    pub fn serialize<S>(value: &Option<DateTime<Utc>>, ser: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(dt) => ser.serialize_str(&dt.timestamp().to_string()),
            None => ser.serialize_none(),
        }
    }

    /// Deserialize from a stringified epoch-seconds value (the form
    /// Plex emits) or an explicit integer. Empty strings, `"0"`, and
    /// nulls all map to [`None`].
    pub fn deserialize<'de, D>(de: D) -> Result<Option<DateTime<Utc>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr<'a> {
            Str(&'a str),
            Owned(String),
            Int(i64),
            Null,
        }
        // `Option<Repr>` to also accept absent JSON null.
        match Option::<Repr<'_>>::deserialize(de)? {
            None | Some(Repr::Null | Repr::Int(0)) => Ok(None),
            Some(Repr::Str(s)) => parse_epoch_secs(s).map_err(serde::de::Error::custom),
            Some(Repr::Owned(s)) => parse_epoch_secs(&s).map_err(serde::de::Error::custom),
            Some(Repr::Int(n)) => {
                use chrono::TimeZone;
                Utc.timestamp_opt(n, 0)
                    .single()
                    .map(Some)
                    .ok_or_else(|| serde::de::Error::custom(format!("epoch {n} out of range")))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;

    #[test]
    fn parse_epoch_secs_handles_empty_and_zero() {
        assert!(parse_epoch_secs("").unwrap().is_none());
        assert!(parse_epoch_secs("0").unwrap().is_none());
        assert!(parse_epoch_secs("  ").unwrap().is_none());
    }

    #[test]
    fn parse_epoch_secs_valid_unix_value() {
        // 2023-11-14T22:13:20Z
        let dt = parse_epoch_secs("1700000000").unwrap().unwrap();
        assert_eq!(dt.timestamp(), 1_700_000_000);
        assert_eq!(dt.year(), 2023);
    }

    #[test]
    fn parse_epoch_secs_rejects_garbage() {
        let err = parse_epoch_secs("not-a-number").unwrap_err();
        assert!(matches!(err, Error::Config(_)));
    }

    #[test]
    fn format_epoch_secs_round_trips() {
        let dt = parse_epoch_secs("1700000000").unwrap().unwrap();
        assert_eq!(format_epoch_secs(dt), "1700000000");
    }

    #[test]
    fn parse_iso_date_ok() {
        let d = parse_iso_date("2024-03-15").unwrap().unwrap();
        assert_eq!((d.year(), d.month(), d.day()), (2024, 3, 15));
    }

    #[test]
    fn parse_iso_date_empty_is_none() {
        assert!(parse_iso_date("").unwrap().is_none());
        assert!(parse_iso_date("   ").unwrap().is_none());
    }

    #[test]
    fn parse_iso_date_rejects_malformed() {
        for bad in ["2024", "2024-13-01", "2024-02-30", "garbage", "2024/03/15"] {
            assert!(parse_iso_date(bad).is_err(), "expected error for {bad:?}");
        }
    }

    #[test]
    fn epoch_secs_str_opt_serde_string_form() {
        #[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq)]
        struct Wrap {
            #[serde(with = "epoch_secs_str_opt")]
            t: Option<DateTime<Utc>>,
        }
        // String form (what Plex emits).
        let s: Wrap = serde_json::from_str(r#"{"t":"1700000000"}"#).unwrap();
        assert!(s.t.is_some());
        // Round-trip.
        let back = serde_json::to_string(&s).unwrap();
        assert!(back.contains(r#""t":"1700000000""#));
    }

    #[test]
    fn epoch_secs_str_opt_serde_zero_and_empty_become_none() {
        #[derive(serde::Deserialize)]
        struct Wrap {
            #[serde(with = "epoch_secs_str_opt")]
            t: Option<DateTime<Utc>>,
        }
        let zero: Wrap = serde_json::from_str(r#"{"t":"0"}"#).unwrap();
        assert!(zero.t.is_none());
        let empty: Wrap = serde_json::from_str(r#"{"t":""}"#).unwrap();
        assert!(empty.t.is_none());
        let null: Wrap = serde_json::from_str(r#"{"t":null}"#).unwrap();
        assert!(null.t.is_none());
    }

    #[test]
    fn epoch_secs_str_opt_serde_integer_form_also_accepted() {
        // Some endpoints emit a JSON integer rather than a string.
        #[derive(serde::Deserialize)]
        struct Wrap {
            #[serde(with = "epoch_secs_str_opt")]
            t: Option<DateTime<Utc>>,
        }
        let s: Wrap = serde_json::from_str(r#"{"t":1700000000}"#).unwrap();
        assert!(s.t.is_some());
    }
}
