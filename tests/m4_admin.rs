//! M4.9 — server admin / monitoring endpoints integration tests.

use plex_rs::{BandwidthOptions, PlexServer, PlexToken};
use url::Url;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn root_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {"size":0,"machineIdentifier":"m","version":"v"}
    })
}

async fn connect(server: &MockServer) -> PlexServer {
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(root_body()))
        .mount(server)
        .await;
    PlexServer::connect(
        Url::parse(&server.uri()).unwrap(),
        PlexToken::new("token").unwrap(),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn activities_returns_running_jobs() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/activities"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "MediaContainer": {
                "Activity": [{
                    "uuid": "abc",
                    "type": "library.refresh.items",
                    "title": "Refreshing",
                    "subtitle": "Movies",
                    "progress": 42,
                    "cancellable": true
                }]
            }
        })))
        .expect(1)
        .mount(&server)
        .await;
    let plex = connect(&server).await;
    let acts = plex.activities().await.unwrap();
    assert_eq!(acts.len(), 1);
    assert_eq!(acts[0].uuid, "abc");
    assert_eq!(acts[0].progress, 42);
    assert!(acts[0].cancellable);
}

#[tokio::test]
async fn butler_tasks_returns_scheduled_jobs() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/butler"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ButlerTasks": {
                "ButlerTask": [{
                    "name": "BackupDatabase",
                    "title": "Backup",
                    "description": "Backs up the database",
                    "enabled": true,
                    "interval": 7,
                    "scheduleRandomized": false
                }]
            }
        })))
        .expect(1)
        .mount(&server)
        .await;
    let plex = connect(&server).await;
    let tasks = plex.butler_tasks().await.unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].name, "BackupDatabase");
    assert_eq!(tasks[0].interval_days, 7);
    assert!(tasks[0].enabled);
}

#[tokio::test]
async fn updater_status_reports_available_release() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/updater/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "MediaContainer": {
                "Release": [{
                    "key": "/updater/check/123",
                    "version": "1.41.0.0",
                    "added": "X",
                    "fixed": "Y",
                    "downloadURL": "https://plex.tv/123",
                    "state": "available"
                }]
            }
        })))
        .expect(1)
        .mount(&server)
        .await;
    let plex = connect(&server).await;
    let status = plex.updater_status().await.unwrap();
    assert_eq!(status.releases.len(), 1);
    assert_eq!(status.releases[0].version, "1.41.0.0");
}

#[tokio::test]
async fn bandwidth_stats_threads_filter_params() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/statistics/bandwidth"))
        .and(query_param("accountID", "1"))
        .and(query_param("timespan", "4"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "MediaContainer": {
                "StatisticsBandwidth": [{
                    "accountID": 1,
                    "deviceID": 100,
                    "at": 1_700_000_000,
                    "bytes": 1024,
                    "lan": false,
                    "timespan": 4
                }]
            }
        })))
        .expect(1)
        .mount(&server)
        .await;
    let plex = connect(&server).await;
    let opts = BandwidthOptions::default().with_account(1).with_timespan(4);
    let stats = plex.bandwidth_stats(&opts).await.unwrap();
    assert_eq!(stats[0].bytes, 1024);
    assert!(!stats[0].lan);
}

#[tokio::test]
async fn resource_stats_returns_cpu_and_memory() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/statistics/resources"))
        .and(query_param("timespan", "6"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "MediaContainer": {
                "StatisticsResources": [{
                    "at": 1_700_000_000,
                    "hostCpuUtilization": 12.5,
                    "hostMemoryUtilization": 60.0,
                    "processCpuUtilization": 3.2,
                    "processMemoryUtilization": 8.1,
                    "timespan": 6
                }]
            }
        })))
        .expect(1)
        .mount(&server)
        .await;
    let plex = connect(&server).await;
    let stats = plex.resource_stats().await.unwrap();
    assert_eq!(stats.len(), 1);
    assert!((stats[0].host_cpu_pct - 12.5).abs() < 0.01);
    assert_eq!(stats[0].timespan, 6);
}
