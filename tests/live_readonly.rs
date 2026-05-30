//! Live, **read-only** integration tests against a real Plex Media Server.
//!
//! These are the opt-in counterpart to the `wiremock`-backed suites: instead
//! of a mocked PMS they drive the crate end-to-end against an actual server on
//! your network (or a reverse-proxied/public PMS URL). They exercise only
//! non-mutating endpoints — identity, library browsing + advanced filters,
//! search, hierarchy traversal (show→season→episode, artist→album→track,
//! photo albums), metadata reload + URL builders, on-deck / unwatched /
//! collections, playlists, sessions, server settings, admin/monitoring
//! (activities, butler tasks, updater, statistics), a bounded slice of
//! history, and the read-only plex.tv (`MyPlex`) cloud surface
//! (resources, devices, friends, home users, watchlist, Discover search)
//! — so they can never alter or harm your library or account.
//!
//! # Running
//!
//! Gated behind the `live-tests` Cargo feature **and** two credentials, so a
//! normal `cargo test` never touches the network and CI never runs them
//! (see `CLAUDE.md` §9.4):
//!
//! ```text
//! # credentials live in a git-ignored .env (see .env.example):
//! #   PLEX_TEST_TOKEN=<your X-Plex-Token>
//! #   PLEX_TEST_BASEURL=https://your-server:32400
//!
//! cargo test --features live-tests --test live_readonly -- --nocapture
//! ```
//!
//! Credentials are read from the process environment first, falling back to a
//! `.env` file in the crate root. When either is absent each test prints a
//! skip notice and passes as a no-op — that is what keeps CI green without
//! ever contacting a server.

#![cfg(feature = "live-tests")]

use std::path::Path;

use futures_util::{StreamExt, TryStreamExt};
use plex_rs::{
    BandwidthOptions, ClientIdentifier, DiscoverOptions, Error, FilterBuilder, LibrarySection,
    MyPlexClient, Playable, PlexServer, PlexToken, Reload, SectionKind, TranscodeOptions,
};
use url::Url;

/// Resolve a configuration value: process environment first, then a
/// `KEY=VALUE` line in the crate-root `.env`. Returns `None` when unset or
/// empty so callers can cleanly skip.
fn live_var(key: &str) -> Option<String> {
    if let Ok(v) = std::env::var(key) {
        let v = v.trim().to_owned();
        if !v.is_empty() {
            return Some(v);
        }
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".env");
    let contents = std::fs::read_to_string(path).ok()?;
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == key {
                let v = v.trim().trim_matches('"').trim_matches('\'').trim();
                if !v.is_empty() {
                    return Some(v.to_owned());
                }
            }
        }
    }
    None
}

/// Connect to the live server, or return `None` (with a skip notice) when
/// credentials are not configured. A connection failure with credentials
/// *present* is a genuine test failure, so it panics rather than skips.
async fn connect_live() -> Option<PlexServer> {
    let (Some(base), Some(token)) = (live_var("PLEX_TEST_BASEURL"), live_var("PLEX_TEST_TOKEN"))
    else {
        eprintln!(
            "[live] PLEX_TEST_BASEURL / PLEX_TEST_TOKEN not set — skipping live read-only test"
        );
        return None;
    };

    let url = Url::parse(&base).expect("PLEX_TEST_BASEURL must be a valid absolute URL");
    let token = PlexToken::new(token).expect("PLEX_TEST_TOKEN must be non-empty");
    match PlexServer::connect(url, token).await {
        Ok(server) => Some(server),
        Err(e) => panic!("[live] failed to connect to {base}: {e}"),
    }
}

#[tokio::test]
async fn identity_and_ping() {
    let Some(server) = connect_live().await else {
        return;
    };

    let id = server.identity();
    assert!(
        !id.machine_identifier.as_str().is_empty(),
        "machineIdentifier should be populated"
    );
    assert!(!id.version.is_empty(), "server version should be populated");
    eprintln!(
        "[live] connected to {:?} — PMS {} (machine {})",
        id.friendly_name.as_deref().unwrap_or("<unnamed>"),
        id.version,
        id.machine_identifier.as_str(),
    );

    // /identity round-trips and parses into the envelope.
    server.ping().await.expect("ping /identity should succeed");
}

#[tokio::test]
async fn sections_metadata() {
    let Some(server) = connect_live().await else {
        return;
    };

    let sections = server
        .library()
        .sections()
        .await
        .expect("listing library sections should succeed");
    assert!(
        !sections.is_empty(),
        "a real server should expose at least one library section"
    );

    for s in &sections {
        assert!(!s.title.is_empty(), "section title should be non-empty");
        assert!(!s.uuid.is_empty(), "section uuid should be non-empty");
        eprintln!("[live] section #{} {:?} ({:?})", s.id(), s.title, s.kind);
    }
}

#[tokio::test]
async fn section_items_listable() {
    let Some(server) = connect_live().await else {
        return;
    };

    let sections = server.library().sections().await.expect("list sections");
    for s in &sections {
        let count = match &s.kind {
            SectionKind::Movie => {
                let items = s.movies().await.expect("list movies");
                if let Some(first) = items.first() {
                    assert!(!first.title.is_empty(), "movie title should be non-empty");
                }
                items.len()
            }
            SectionKind::Show => {
                let items = s.shows().await.expect("list shows");
                if let Some(first) = items.first() {
                    assert!(!first.title.is_empty(), "show title should be non-empty");
                }
                items.len()
            }
            SectionKind::Music => {
                let items = s.artists().await.expect("list artists");
                if let Some(first) = items.first() {
                    assert!(!first.title.is_empty(), "artist title should be non-empty");
                }
                items.len()
            }
            SectionKind::Photo => {
                let items = s.photoalbums().await.expect("list photo albums");
                if let Some(first) = items.first() {
                    assert!(!first.title.is_empty(), "album title should be non-empty");
                }
                items.len()
            }
            SectionKind::Other(kind) => {
                eprintln!(
                    "[live] section {:?}: unhandled kind {kind:?} — listing skipped",
                    s.title
                );
                continue;
            }
            // `SectionKind` is `#[non_exhaustive]`; tolerate future variants.
            _ => continue,
        };
        eprintln!(
            "[live] section {:?} ({:?}): {count} item(s)",
            s.title, s.kind
        );
    }
}

#[tokio::test]
async fn recently_added_probe() {
    let Some(server) = connect_live().await else {
        return;
    };

    let sections = server.library().sections().await.expect("list sections");
    for s in &sections {
        let recent = s
            .recently_added()
            .await
            .expect("recentlyAdded should succeed");
        for item in &recent {
            assert!(!item.title().is_empty(), "item title should be non-empty");
            assert!(!item.key().is_empty(), "item key should be non-empty");
        }
        eprintln!(
            "[live] {:?}: {} recently-added item(s)",
            s.title,
            recent.len()
        );
    }
}

#[tokio::test]
async fn search_roundtrip() {
    let Some(server) = connect_live().await else {
        return;
    };

    let sections = server.library().sections().await.expect("list sections");

    // Find the first movie/show section that actually has an item, and grab
    // a title to search for — a substring round-trip we can assert on.
    let mut probe: Option<(LibrarySection, String)> = None;
    for s in sections {
        let title = match s.kind {
            SectionKind::Movie => s
                .movies()
                .await
                .expect("list movies")
                .into_iter()
                .next()
                .map(|m| m.title),
            SectionKind::Show => s
                .shows()
                .await
                .expect("list shows")
                .into_iter()
                .next()
                .map(|m| m.title),
            _ => None,
        };
        if let Some(title) = title {
            probe = Some((s, title));
            break;
        }
    }

    let Some((section, full_title)) = probe else {
        eprintln!("[live] no movie/show items available — skipping search round-trip");
        return;
    };

    // Plex title search is a case-insensitive substring match, so the first
    // word of a known title must come back containing that word.
    let term = full_title
        .split_whitespace()
        .next()
        .unwrap_or(full_title.as_str());
    let hits = section.search(term).await.expect("section title search");
    let needle = term.to_lowercase();
    assert!(
        hits.iter()
            .any(|item| item.title().to_lowercase().contains(&needle)),
        "searching {term:?} should return an item whose title contains it (got {} hit(s))",
        hits.len()
    );
    eprintln!(
        "[live] search {term:?} -> {} hit(s) (seed title {full_title:?})",
        hits.len()
    );
}

#[tokio::test]
async fn sessions_listing() {
    let Some(server) = connect_live().await else {
        return;
    };

    // Almost always empty unless something is playing right now; the point is
    // that the endpoint parses cleanly either way.
    let sessions = server
        .sessions()
        .await
        .expect("listing active sessions should succeed");
    for s in &sessions {
        assert!(!s.session_key.is_empty(), "session key should be non-empty");
        assert!(
            !s.item.title().is_empty(),
            "session item should have a title"
        );
    }
    eprintln!("[live] {} active session(s)", sessions.len());
}

#[tokio::test]
async fn playlists_listing() {
    let Some(server) = connect_live().await else {
        return;
    };

    let playlists = server
        .playlists()
        .await
        .expect("listing playlists should succeed");
    for p in &playlists {
        assert!(!p.title.is_empty(), "playlist title should be non-empty");
    }
    eprintln!("[live] {} playlist(s)", playlists.len());
}

#[tokio::test]
async fn history_recent_bounded() {
    let Some(server) = connect_live().await else {
        return;
    };

    // History can be enormous; take only the most recent few so the test
    // stays fast and read-only.
    let recent = server
        .history()
        .stream()
        .take(5)
        .try_collect::<Vec<_>>()
        .await
        .expect("collecting recent history should succeed");
    for entry in &recent {
        assert!(
            !entry.item.title().is_empty(),
            "history item should have a title"
        );
    }
    eprintln!("[live] collected {} recent history entr(ies)", recent.len());
}

// ============================================================================
// Server settings + admin / monitoring.
// ============================================================================

#[tokio::test]
async fn server_settings() {
    let Some(server) = connect_live().await else {
        return;
    };

    let settings = server
        .settings()
        .await
        .expect("GET /:/prefs should succeed");
    assert!(
        !settings.is_empty(),
        "a real server always exposes preferences"
    );
    for s in settings.all() {
        assert!(!s.id.is_empty(), "every setting carries a non-empty id");
    }
    let groups = settings.group_names();
    assert!(!groups.is_empty(), "settings should be grouped");

    // `get()` must round-trip an id that `all()` just yielded.
    let first_id = settings.all()[0].id.clone();
    assert!(
        settings.get(&first_id).is_some(),
        "get() should find an id returned by all()"
    );
    eprintln!(
        "[live] {} setting(s) across {} group(s): {:?}",
        settings.len(),
        groups.len(),
        groups
    );
}

#[tokio::test]
async fn server_activities() {
    let Some(server) = connect_live().await else {
        return;
    };

    // Usually empty unless a scan/refresh is in flight; the point is that
    // `/activities` parses cleanly either way.
    let activities = server.activities().await.expect("GET /activities");
    for a in &activities {
        assert!(a.progress <= 100, "progress is a 0..=100 estimate");
    }
    eprintln!("[live] {} running activit(ies)", activities.len());
}

#[tokio::test]
async fn server_butler_tasks() {
    let Some(server) = connect_live().await else {
        return;
    };

    let tasks = server.butler_tasks().await.expect("GET /butler");
    assert!(!tasks.is_empty(), "PMS always ships scheduled butler tasks");
    for t in &tasks {
        assert!(!t.name.is_empty(), "butler task name should be non-empty");
    }
    eprintln!(
        "[live] {} butler task(s); first: {:?}",
        tasks.len(),
        tasks[0].name
    );
}

#[tokio::test]
async fn server_updater_status() {
    let Some(server) = connect_live().await else {
        return;
    };

    let status = server.updater_status().await.expect("GET /updater/status");
    for rel in &status.releases {
        assert!(
            !rel.version.is_empty(),
            "a pending release should carry a version"
        );
    }
    eprintln!("[live] {} pending release(s)", status.releases.len());
}

#[tokio::test]
async fn server_statistics() {
    let Some(server) = connect_live().await else {
        return;
    };

    // The dashboard statistics endpoints (`/statistics/bandwidth`,
    // `/statistics/resources`) are **Plex Pass features**: PMS only mounts
    // those routes for an account with an active subscription and otherwise
    // 404s them as unknown routes (verified — the 404 carries `X-Plex-Protocol`
    // and matches a bogus path, so it is PMS itself, not a proxy). The paths
    // the crate builds are correct and byte-identical to python-plexapi's, so
    // when there is no Plex Pass we skip rather than fail.
    // See <https://support.plex.tv/articles/200871837-status-and-dashboard/>.
    if !server.identity().my_plex_subscription {
        eprintln!("[live] no active Plex Pass — /statistics/* is subscription-gated, skipping");
        return;
    }

    // Hours granularity (`4`) keeps bandwidth bounded; it is a timespan the
    // bandwidth endpoint supports (unlike `6`=seconds, which only the
    // resources endpoint aggregates). Tolerate NotFound defensively.
    match server
        .bandwidth_stats(&BandwidthOptions::default().with_timespan(4))
        .await
    {
        Ok(bandwidth) => {
            for stat in &bandwidth {
                assert!(stat.timespan > 0, "a sample carries its timespan code");
            }
            eprintln!("[live] {} bandwidth sample(s)", bandwidth.len());
        }
        Err(Error::NotFound { .. }) => {
            eprintln!("[live] /statistics/bandwidth unavailable — skipping");
        }
        Err(e) => panic!("GET /statistics/bandwidth: {e}"),
    }

    match server.resource_stats().await {
        Ok(resources) => {
            for r in &resources {
                assert!(
                    r.host_cpu_pct.is_finite() && r.process_memory_pct.is_finite(),
                    "utilization samples should be finite numbers"
                );
            }
            eprintln!("[live] {} resource sample(s)", resources.len());
        }
        Err(Error::NotFound { .. }) => {
            eprintln!("[live] /statistics/resources unavailable — skipping");
        }
        Err(e) => panic!("GET /statistics/resources: {e}"),
    }
}

// ============================================================================
// Section browsing — on-deck, unwatched, collections, advanced filter.
// ============================================================================

#[tokio::test]
async fn section_on_deck_and_unwatched() {
    let Some(server) = connect_live().await else {
        return;
    };

    let sections = server.library().sections().await.expect("list sections");
    for s in &sections {
        let on_deck = s.on_deck().await.expect("onDeck should succeed");
        let unwatched = s.unwatched().await.expect("unwatched should succeed");
        for item in on_deck.iter().chain(unwatched.iter()) {
            assert!(!item.title().is_empty(), "item title should be non-empty");
        }
        eprintln!(
            "[live] {:?}: {} on-deck, {} unwatched",
            s.title,
            on_deck.len(),
            unwatched.len()
        );
    }
}

#[tokio::test]
async fn section_collections() {
    let Some(server) = connect_live().await else {
        return;
    };

    let sections = server.library().sections().await.expect("list sections");
    let mut probed = false;
    for s in &sections {
        let collections = s.collections().await.expect("collections should succeed");
        for c in &collections {
            assert!(!c.title.is_empty(), "collection title should be non-empty");
        }
        eprintln!("[live] {:?}: {} collection(s)", s.title, collections.len());

        // Traverse the first collection's children once, if any exist.
        if let Some(first) = collections.first() {
            let items = first.items().await.expect("collection items should list");
            for item in &items {
                assert!(!item.title().is_empty(), "collection item needs a title");
            }
            eprintln!(
                "[live]   collection {:?} -> {} item(s)",
                first.title,
                items.len()
            );
            probed = true;
        }
    }
    if !probed {
        eprintln!("[live] no collections on this server — items traversal skipped");
    }
}

#[tokio::test]
async fn section_advanced_filter() {
    let Some(server) = connect_live().await else {
        return;
    };

    let sections = server.library().sections().await.expect("list sections");
    // Pick a movie or show section and run a sorted, limited filter listing.
    let Some((section, libtype)) = sections.iter().find_map(|s| match s.kind {
        SectionKind::Movie => Some((s, 1)),
        SectionKind::Show => Some((s, 2)),
        _ => None,
    }) else {
        eprintln!("[live] no movie/show section — skipping advanced filter");
        return;
    };

    let builder = FilterBuilder::new()
        .libtype(libtype)
        .sort_by("titleSort")
        .limit(3);
    let hits = section
        .filter(&builder)
        .await
        .expect("filter should succeed");
    assert!(hits.len() <= 3, "limit(3) should cap the listing");
    for item in &hits {
        assert!(!item.title().is_empty(), "filtered item needs a title");
    }
    eprintln!(
        "[live] advanced filter on {:?} (libtype {libtype}) -> {} item(s)",
        section.title,
        hits.len()
    );
}

// ============================================================================
// Hierarchy traversal — show→season→episode, artist→album→track, photos.
// ============================================================================

#[tokio::test]
async fn show_season_episode_traversal() {
    let Some(server) = connect_live().await else {
        return;
    };

    let sections = server.library().sections().await.expect("list sections");
    let Some(section) = sections
        .iter()
        .find(|s| matches!(s.kind, SectionKind::Show))
    else {
        eprintln!("[live] no show section — skipping show traversal");
        return;
    };

    let shows = section.shows().await.expect("list shows");
    // Find the first show that actually has seasons.
    for show in &shows {
        let seasons = show.seasons().await.expect("list seasons");
        let Some(season) = seasons.first() else {
            continue;
        };
        assert!(!season.title.is_empty(), "season title should be non-empty");

        let episodes = season.episodes().await.expect("list episodes");
        for ep in &episodes {
            assert!(!ep.title.is_empty(), "episode title should be non-empty");
        }
        eprintln!(
            "[live] show {:?}: {} season(s); season {:?}: {} episode(s)",
            show.title,
            seasons.len(),
            season.title,
            episodes.len()
        );
        return;
    }
    eprintln!("[live] no show with seasons found — traversal skipped");
}

#[tokio::test]
async fn artist_album_track_traversal() {
    let Some(server) = connect_live().await else {
        return;
    };

    let sections = server.library().sections().await.expect("list sections");
    let Some(section) = sections
        .iter()
        .find(|s| matches!(s.kind, SectionKind::Music))
    else {
        eprintln!("[live] no music section — skipping artist traversal");
        return;
    };

    let artists = section.artists().await.expect("list artists");
    for artist in &artists {
        let albums = artist.albums().await.expect("list albums");
        let Some(album) = albums.first() else {
            continue;
        };
        assert!(!album.title.is_empty(), "album title should be non-empty");

        let tracks = album.tracks().await.expect("list tracks");
        for t in &tracks {
            assert!(!t.title.is_empty(), "track title should be non-empty");
        }
        eprintln!(
            "[live] artist {:?}: {} album(s); album {:?}: {} track(s)",
            artist.title,
            albums.len(),
            album.title,
            tracks.len()
        );
        return;
    }
    eprintln!("[live] no artist with albums found — traversal skipped");
}

#[tokio::test]
async fn photoalbum_traversal() {
    let Some(server) = connect_live().await else {
        return;
    };

    let sections = server.library().sections().await.expect("list sections");
    let Some(section) = sections
        .iter()
        .find(|s| matches!(s.kind, SectionKind::Photo))
    else {
        eprintln!("[live] no photo section — skipping photo traversal");
        return;
    };

    let albums = section.photoalbums().await.expect("list photo albums");
    let Some(album) = albums.first() else {
        eprintln!("[live] photo section empty — skipping photo traversal");
        return;
    };

    // `children()` is the mixed sub-album/photo listing; `photos()` filters it.
    let children = album.children().await.expect("list album children");
    let photos = album.photos().await.expect("list photos");
    for p in &photos {
        assert!(!p.title.is_empty(), "photo title should be non-empty");
    }
    eprintln!(
        "[live] album {:?}: {} child entr(ies), {} photo(s)",
        album.title,
        children.len(),
        photos.len()
    );
}

// ============================================================================
// Container items — playlists.
// ============================================================================

#[tokio::test]
async fn playlist_items_traversal() {
    let Some(server) = connect_live().await else {
        return;
    };

    let playlists = server.playlists().await.expect("list playlists");
    let Some(playlist) = playlists.first() else {
        eprintln!("[live] no playlists on this server — items traversal skipped");
        return;
    };

    let items = playlist.items().await.expect("playlist items should list");
    for item in &items {
        assert!(!item.title().is_empty(), "playlist item needs a title");
    }
    eprintln!(
        "[live] playlist {:?} ({:?}) -> {} item(s)",
        playlist.title,
        playlist.kind,
        items.len()
    );
}

// ============================================================================
// Reload (partial → full media chain) + token-bearing URL builders.
// ============================================================================

#[tokio::test]
async fn reload_movie_full_media_chain() {
    let Some(server) = connect_live().await else {
        return;
    };

    let sections = server.library().sections().await.expect("list sections");
    let Some(section) = sections
        .iter()
        .find(|s| matches!(s.kind, SectionKind::Movie))
    else {
        eprintln!("[live] no movie section — skipping reload test");
        return;
    };
    let Some(partial) = section
        .movies()
        .await
        .expect("list movies")
        .into_iter()
        .next()
    else {
        eprintln!("[live] movie section empty — skipping reload test");
        return;
    };
    let title = partial.title.clone();

    // Listing endpoints emit partials (often empty `media[]`); reload upgrades
    // to the full record fetched from `/library/metadata/<rk>`.
    let full = partial
        .reload()
        .await
        .expect("reloading movie should succeed");
    assert_eq!(full.title, title, "reload should preserve the title");
    assert!(
        !full.media.is_empty(),
        "the full movie record should carry its media chain"
    );

    // The URL builders operate on the real first-part key. Verify they produce
    // well-formed, same-host URLs — but NEVER print them: they embed the token.
    if let Some(part_key) = full.first_part_key() {
        let direct = full
            .direct_play_url()
            .expect("direct-play url should build for a movie with media");
        assert_eq!(
            direct.host_str(),
            server.base_url().host_str(),
            "direct-play url should target the same host"
        );

        let transcoded = TranscodeOptions::new()
            .video_resolution("1280x720")
            .build_for(&server, part_key)
            .expect("transcode url should build");
        assert_eq!(
            transcoded.host_str(),
            server.base_url().host_str(),
            "transcode url should target the same host"
        );
        assert!(
            transcoded.path().contains("transcode"),
            "transcode url should hit a transcode path"
        );
    }
    eprintln!(
        "[live] reloaded movie {title:?}: {} media item(s)",
        full.media.len()
    );
}

// ============================================================================
// plex.tv (MyPlex) cloud — read-only surface.
//
// These hit plex.tv / discover.provider.plex.tv rather than the local PMS.
// They reuse PLEX_TEST_TOKEN; if that token is server-scoped and plex.tv
// rejects it, each test prints a skip notice rather than failing.
// ============================================================================

/// Build a `MyPlexClient` from `PLEX_TEST_TOKEN`, or `None` (skip) when the
/// token is absent. Uses an ephemeral client identifier — fine for read-only
/// probes.
fn myplex_client() -> Option<MyPlexClient> {
    let Some(token) = live_var("PLEX_TEST_TOKEN") else {
        eprintln!("[live] PLEX_TEST_TOKEN not set — skipping MyPlex cloud test");
        return None;
    };
    let token = PlexToken::new(token).expect("PLEX_TEST_TOKEN must be non-empty");
    Some(
        MyPlexClient::new(token, ClientIdentifier::generated(), None)
            .expect("constructing a MyPlexClient should succeed"),
    )
}

/// Treat a plex.tv auth rejection as a skip (the configured token may be a
/// server-scoped token plex.tv does not accept) while still surfacing
/// transport / parse errors as genuine failures.
fn cloud_or_skip<T>(what: &str, r: std::result::Result<T, Error>) -> Option<T> {
    match r {
        Ok(v) => Some(v),
        Err(Error::Unauthorized) => {
            eprintln!(
                "[live] plex.tv rejected the token for {what} (server-scoped token?) — skipping"
            );
            None
        }
        Err(e) => panic!("[live] MyPlex {what} failed: {e}"),
    }
}

#[tokio::test]
async fn myplex_resources() {
    let Some(client) = myplex_client() else {
        return;
    };
    let Some(resources) = cloud_or_skip("resources", client.resources().await) else {
        return;
    };
    for r in &resources {
        assert!(!r.name.is_empty(), "resource should have a name");
    }
    eprintln!("[live] {} plex.tv resource(s)", resources.len());
}

#[tokio::test]
async fn myplex_devices() {
    let Some(client) = myplex_client() else {
        return;
    };
    let Some(devices) = cloud_or_skip("devices", client.devices().await) else {
        return;
    };
    for d in &devices {
        assert!(!d.product.is_empty(), "device should report a product");
    }
    eprintln!("[live] {} registered device(s)", devices.len());
}

#[tokio::test]
async fn myplex_friends() {
    let Some(client) = myplex_client() else {
        return;
    };
    let Some(friends) = cloud_or_skip("friends", client.friends().await) else {
        return;
    };
    for f in &friends {
        assert!(
            !f.username.is_empty() || !f.title.is_empty(),
            "friend should have a username or title"
        );
    }
    eprintln!("[live] {} shared friend(s)", friends.len());
}

#[tokio::test]
async fn myplex_home_users() {
    let Some(client) = myplex_client() else {
        return;
    };
    let Some(users) = cloud_or_skip("home_users", client.home_users().await) else {
        return;
    };
    for u in &users {
        assert!(!u.title.is_empty(), "home user should have a title");
    }
    eprintln!("[live] {} Plex Home user(s)", users.len());
}

#[tokio::test]
async fn myplex_watchlist() {
    let Some(client) = myplex_client() else {
        return;
    };
    let Some(items) = cloud_or_skip("watchlist", client.watchlist().await) else {
        return;
    };
    for item in &items {
        assert!(!item.title.is_empty(), "watchlist item should have a title");
        assert!(
            !item.rating_key.is_empty(),
            "watchlist item should carry a rating key"
        );
    }
    eprintln!("[live] {} watchlist item(s)", items.len());
}

#[tokio::test]
async fn myplex_discover_search() {
    let Some(client) = myplex_client() else {
        return;
    };
    let opts = DiscoverOptions::default();
    let Some(hits) = cloud_or_skip(
        "discover_search",
        client.discover_search("matrix", &opts).await,
    ) else {
        return;
    };
    for item in &hits {
        assert!(!item.title.is_empty(), "discover hit should have a title");
        assert!(
            !item.rating_key.is_empty(),
            "discover hit should carry a rating key"
        );
    }
    eprintln!("[live] discover \"matrix\" -> {} hit(s)", hits.len());
}
