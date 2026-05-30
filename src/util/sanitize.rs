//! Fixture sanitiser.
//!
//! Captured PMS / plex.tv responses contain credentials and personally
//! identifiable values. Before any captured body lands under
//! `tests/fixtures/`, it must pass through [`sanitize`], which applies
//! a fixed ordered list of regex substitutions producing a body with
//! every secret replaced by a deterministic placeholder.
//!
//! Order matters: rules that match wider patterns (e.g. URLs)
//! must run before rules that match narrower sub-strings, otherwise
//! the wider rule never sees a clean input.
//!
//! The sanitiser is **idempotent**: `sanitize(sanitize(x)) == sanitize(x)`.
//! This is enforced by a unit test so that callers can safely re-run
//! the sanitiser on an already-sanitised file (e.g. as a pre-commit
//! hook) without corrupting it.

use std::borrow::Cow;
use std::net::Ipv4Addr;
use std::sync::OnceLock;

use regex::{Captures, Regex};

// -----------------------------------------------------------------------------
// Rule table.
// -----------------------------------------------------------------------------

/// How a [`Rule`] computes its replacement string.
///
/// Most rules are constant substitutions with a `$1`-style backreference,
/// but a few (notably the IPv4 family) need to inspect the matched text
/// to decide what to emit — Rust's `regex` crate does not support
/// negative look-around, so callers that want to discriminate must do it
/// in code.
enum Replacement {
    /// `regex::replace_all`-compatible literal with `$N` backrefs.
    Static(&'static str),
    /// Function from the matched text (full match, group 0) to the
    /// replacement. Returning [`Cow::Borrowed`] is cheaper when the
    /// rule wants to leave the text untouched.
    Computed(fn(&str) -> Cow<'static, str>),
}

/// A single regex-driven substitution.
struct Rule {
    /// Static label, used in error messages and the idempotency test.
    name: &'static str,
    /// Source of the regex. Stored as a string for `OnceLock` init.
    pattern: &'static str,
    /// How to produce the replacement.
    replacement: Replacement,
}

/// All sanitisation rules, in apply-order.
///
/// Backreferences with `$N` substitute capture group `N` (per the
/// [`regex`] crate's docs). Where a rule emits its placeholder as a
/// fixed string, that string is also the post-sanitisation value the
/// rule would match on a second pass — guaranteeing idempotency.
const RULES: &[Rule] = &[
    // 1. X-Plex-Token query parameter (most common, runs first so the
    //    URL-level rules below don't accidentally rewrite the token).
    Rule {
        name: "x_plex_token_query",
        pattern: r"(?i)([?&]X-Plex-Token=)[A-Za-z0-9_\-]+",
        replacement: Replacement::Static("${1}REDACTED_TOKEN"),
    },
    // 2. X-Plex-Token HTTP header (within captured raw HTTP / JSON).
    Rule {
        name: "x_plex_token_header",
        pattern: r#"(?i)(X-Plex-Token[:=]\s*"?)[A-Za-z0-9_\-]+"#,
        replacement: Replacement::Static("${1}REDACTED_TOKEN"),
    },
    // 3. authenticationToken attribute / field.
    Rule {
        name: "authentication_token_attr",
        pattern: r#"(authenticationToken\s*=\s*")[^"]+"#,
        replacement: Replacement::Static("${1}REDACTED_TOKEN"),
    },
    Rule {
        name: "authentication_token_json",
        pattern: r#"("authToken"\s*:\s*")[^"]+"#,
        replacement: Replacement::Static("${1}REDACTED_TOKEN"),
    },
    // 4. machineIdentifier (40-char hex string).
    Rule {
        name: "machine_identifier_attr",
        pattern: r#"(machineIdentifier\s*=\s*")[A-Fa-f0-9]{32,64}"#,
        replacement: Replacement::Static("${1}MACHINE_ID"),
    },
    Rule {
        name: "machine_identifier_json",
        pattern: r#"("machineIdentifier"\s*:\s*")[A-Fa-f0-9]{32,64}"#,
        replacement: Replacement::Static("${1}MACHINE_ID"),
    },
    // 5. clientIdentifier — generic UUID slot or 40-char hex.
    Rule {
        name: "client_identifier_attr",
        pattern: r#"(clientIdentifier\s*=\s*")[A-Za-z0-9\-]+"#,
        replacement: Replacement::Static("${1}CLIENT_ID"),
    },
    // 6. Bare UUIDs anywhere else. Match the canonical 8-4-4-4-12 form.
    Rule {
        name: "uuid",
        pattern: r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b",
        replacement: Replacement::Static("00000000-0000-0000-0000-000000000000"),
    },
    // 7. Email addresses.
    Rule {
        name: "email",
        pattern: r"\b[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}\b",
        replacement: Replacement::Static("user@example.com"),
    },
    // 8. Avatar / thumb URLs hosted at plex.tv user content.
    Rule {
        name: "avatar_url",
        pattern: r#"https?://plex\.tv/users/[A-Za-z0-9]+/avatar(\?[^\s"<>]*)?"#,
        replacement: Replacement::Static("https://plex.tv/users/USER_ID/avatar"),
    },
    // 9. IPv4 addresses, dispatched by privacy class.
    //
    // The `regex` crate has no look-around so we cannot express
    // "not RFC1918 / not loopback" in the pattern itself; instead we
    // match every IPv4 candidate and classify in code. Already-known
    // placeholder values are passed through verbatim (preserves
    // idempotency).
    Rule {
        name: "ipv4",
        pattern: r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b",
        replacement: Replacement::Computed(rewrite_ipv4),
    },
    // 10. IPv6 (very loose match — any colon-separated hex run of len ≥4).
    Rule {
        name: "ipv6",
        pattern: r"\b(?:[A-Fa-f0-9]{1,4}:){2,7}[A-Fa-f0-9]{1,4}\b",
        replacement: Replacement::Computed(rewrite_ipv6),
    },
    // 11. Plex friendly name attribute (e.g. `friendlyName="Dwi's Server"`).
    Rule {
        name: "friendly_name",
        pattern: r#"(friendlyName\s*=\s*")[^"]+"#,
        replacement: Replacement::Static("${1}Test Server"),
    },
    // 12. plex.tv user IDs in URL paths (`/users/12345/`).
    Rule {
        name: "user_id_path",
        pattern: r"/users/\d+/",
        replacement: Replacement::Static("/users/USER_ID/"),
    },
    // 13. Sonos RINCON IDs.
    Rule {
        name: "sonos_rincon",
        pattern: r"RINCON_[A-F0-9]+",
        replacement: Replacement::Static("RINCON_DEVICE"),
    },
    // 14. Local filesystem paths under common user directories.
    //     Last so that earlier email / URL rules win on overlap.
    Rule {
        name: "user_home_path",
        pattern: r"/(?:Users|home)/[A-Za-z0-9._\-]+",
        replacement: Replacement::Static("/Users/test"),
    },
];

/// IPv4 placeholders we never rewrite — keeps the sanitiser idempotent
/// and lets fixtures use these values verbatim as documentation.
const IPV4_PLACEHOLDERS: &[&str] = &[
    "127.0.0.1",
    "0.0.0.0",
    "255.255.255.255",
    "10.0.0.1",
    "203.0.113.10",
];

const IPV6_PLACEHOLDER: &str = "2001:db8::1";

fn rewrite_ipv4(matched: &str) -> Cow<'static, str> {
    if IPV4_PLACEHOLDERS.contains(&matched) {
        return Cow::Owned(matched.to_owned());
    }
    let Ok(addr) = matched.parse::<Ipv4Addr>() else {
        // Failed parse — leave alone (the regex is permissive).
        return Cow::Owned(matched.to_owned());
    };
    if addr.is_loopback() {
        Cow::Borrowed("127.0.0.1")
    } else if addr.is_unspecified() {
        Cow::Borrowed("0.0.0.0")
    } else if addr.is_broadcast() {
        Cow::Borrowed("255.255.255.255")
    } else if addr.is_private() {
        Cow::Borrowed("10.0.0.1")
    } else {
        // Public / link-local / documentation / anything else.
        Cow::Borrowed("203.0.113.10")
    }
}

fn rewrite_ipv6(matched: &str) -> Cow<'static, str> {
    if matched == IPV6_PLACEHOLDER {
        Cow::Owned(matched.to_owned())
    } else {
        Cow::Borrowed(IPV6_PLACEHOLDER)
    }
}

/// Compiled regex cache, built once and reused.
fn compiled() -> &'static Vec<(&'static Rule, Regex)> {
    static CACHE: OnceLock<Vec<(&'static Rule, Regex)>> = OnceLock::new();
    CACHE.get_or_init(|| {
        RULES
            .iter()
            .map(|r| {
                let re = Regex::new(r.pattern).unwrap_or_else(|e| {
                    panic!("invalid sanitiser regex {name}: {e}", name = r.name)
                });
                (r, re)
            })
            .collect()
    })
}

/// Apply every sanitisation rule, in order.
///
/// Returns an owned [`String`] — the input is not modified in place.
/// The result is **idempotent** with respect to a second pass; this
/// is asserted by the `idempotent_under_repeated_application` unit
/// test.
#[must_use]
pub fn sanitize(input: &str) -> String {
    let mut out = input.to_owned();
    for (rule, re) in compiled() {
        out = match &rule.replacement {
            Replacement::Static(s) => re.replace_all(&out, *s).into_owned(),
            Replacement::Computed(f) => re
                .replace_all(&out, |caps: &Captures<'_>| {
                    f(caps.get(0).map_or("", |m| m.as_str())).into_owned()
                })
                .into_owned(),
        };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_x_plex_token_query_param() {
        let s = "GET /library?X-Plex-Token=ABCDEFGHIJK_xyz HTTP/1.1";
        let out = sanitize(s);
        assert!(!out.contains("ABCDEFGHIJK_xyz"));
        assert!(out.contains("X-Plex-Token=REDACTED_TOKEN"));
    }

    #[test]
    fn redacts_x_plex_token_header() {
        let s = "X-Plex-Token: AbCdEfGhIjKlMnOpQrSt";
        assert_eq!(sanitize(s), "X-Plex-Token: REDACTED_TOKEN");
    }

    #[test]
    fn redacts_authentication_token_attribute() {
        let s = r#"<user authenticationToken="secret-token-value-xyz">"#;
        let out = sanitize(s);
        assert!(!out.contains("secret-token-value-xyz"));
        assert!(out.contains("REDACTED_TOKEN"));
    }

    #[test]
    fn redacts_auth_token_json() {
        let s = r#"{"authToken":"my-real-token-9876","other":"keep"}"#;
        let out = sanitize(s);
        assert!(!out.contains("my-real-token-9876"));
        assert!(out.contains(r#""authToken":"REDACTED_TOKEN""#));
        assert!(out.contains(r#""other":"keep""#));
    }

    #[test]
    fn rewrites_machine_identifier_hex() {
        let mid = "a".repeat(40);
        let s = format!(r#"<MediaContainer machineIdentifier="{mid}">"#);
        let out = sanitize(&s);
        assert!(!out.contains(mid.as_str()));
        assert!(out.contains("MACHINE_ID"));
    }

    #[test]
    fn rewrites_uuid_to_zero_uuid() {
        let s = "id=550e8400-e29b-41d4-a716-446655440000";
        let out = sanitize(s);
        assert_eq!(out, "id=00000000-0000-0000-0000-000000000000");
    }

    #[test]
    fn rewrites_email_addresses() {
        let s = "owner=jane.doe+plex@example.org";
        let out = sanitize(s);
        assert!(out.contains("user@example.com"));
        assert!(!out.contains("jane.doe"));
    }

    #[test]
    fn rewrites_rfc1918_addresses() {
        for raw in ["192.168.1.42", "10.0.5.6", "172.16.0.1", "172.31.255.254"] {
            let out = sanitize(raw);
            assert_eq!(out, "10.0.0.1", "rfc1918 rewrite failed for {raw}");
        }
    }

    #[test]
    fn rewrites_public_ipv4_addresses() {
        let out = sanitize("public=8.8.4.4");
        assert!(out.contains("203.0.113.10"));
    }

    #[test]
    fn preserves_loopback_addresses() {
        let s = "url=http://127.0.0.1:32400/";
        let out = sanitize(s);
        // 127.0.0.1 is intentionally not rewritten (it's already a placeholder).
        assert!(out.contains("127.0.0.1"));
    }

    #[test]
    fn rewrites_ipv6_addresses() {
        let s = "addr=2607:f8b0:4007:818::200e";
        let out = sanitize(s);
        assert!(out.contains("2001:db8::1"));
    }

    #[test]
    fn rewrites_user_id_path_segment() {
        let out = sanitize("/users/123456/avatar");
        assert!(out.contains("/users/USER_ID/avatar"));
    }

    #[test]
    fn rewrites_sonos_rincon_id() {
        let out = sanitize("RINCON_B8E937E392F801400");
        assert_eq!(out, "RINCON_DEVICE");
    }

    #[test]
    fn rewrites_home_path() {
        let out = sanitize("/Users/alice/Movies/foo.mkv");
        assert!(out.starts_with("/Users/test/"));
    }

    #[test]
    fn idempotent_under_repeated_application() {
        let raw = concat!(
            "GET /library?X-Plex-Token=secret-token HTTP/1.1\n",
            r#"<user authenticationToken="abc" email="jane@example.org">"#,
            "machineIdentifier=\"",
            "0123456789abcdef0123456789abcdef01234567",
            "\" addr=192.168.1.5"
        );
        let once = sanitize(raw);
        let twice = sanitize(&once);
        assert_eq!(
            once, twice,
            "sanitiser must be idempotent: differed on second pass"
        );
    }

    #[test]
    fn sanitiser_does_not_touch_clean_input() {
        let clean = "this body has nothing secret";
        assert_eq!(sanitize(clean), clean);
    }
}
