//! Server-admin / monitoring endpoints — activities, butler tasks,
//! updater status, and statistics.
//!
//! These surfaces are most useful for monitoring agents and
//! dashboards that want a snapshot of "what's PMS doing right now"
//! or "what's the load look like". Mutation is intentionally out
//! of scope — running these endpoints is a read-mostly affair.
//!
//! ## Endpoints
//!
//! | Method | Path                          | Returned                  |
//! | ------ | ----------------------------- | ------------------------- |
//! | GET    | `/activities`                 | [`Vec<Activity>`]         |
//! | GET    | `/butler`                     | [`Vec<ButlerTask>`]       |
//! | GET    | `/updater/status`             | [`UpdaterStatus`]         |
//! | GET    | `/statistics/bandwidth`       | [`Vec<BandwidthStat>`]    |
//! | GET    | `/statistics/resources`       | [`Vec<ResourceStat>`]     |
//!
//! Each [`PlexServer`] gains a corresponding accessor method.

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::error::Result;
use crate::server::PlexServer;

// -----------------------------------------------------------------------------
// Activity.
// -----------------------------------------------------------------------------

/// A currently-running activity on PMS (scan, optimize, refresh, …).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Activity {
    /// Stable per-activity UUID.
    pub uuid: String,
    /// Activity type (e.g. `library.refresh.items`).
    pub kind: String,
    /// Title shown in the UI.
    pub title: String,
    /// Subtitle / detail line.
    pub subtitle: Option<String>,
    /// 0..=100 progress estimate. PMS sets this even before the
    /// activity has measurable progress, so trust it loosely.
    pub progress: u8,
    /// `true` when the activity can be cancelled via the API.
    pub cancellable: bool,
}

// -----------------------------------------------------------------------------
// ButlerTask.
// -----------------------------------------------------------------------------

/// One scheduled background task (the "butler" subsystem).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ButlerTask {
    /// Task identifier (e.g. `BackupDatabase`).
    pub name: String,
    /// Human-readable title.
    pub title: String,
    /// Long-form description of what the task does.
    pub description: String,
    /// `true` when the task is currently enabled on the schedule.
    pub enabled: bool,
    /// Interval in days between runs.
    pub interval_days: u32,
    /// `true` when PMS randomizes the start time within the
    /// scheduled window.
    pub schedule_randomized: bool,
}

// -----------------------------------------------------------------------------
// UpdaterStatus.
// -----------------------------------------------------------------------------

/// Snapshot of `/updater/status` — current PMS version and any
/// pending update.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct UpdaterStatus {
    /// Releases available for upgrade (typically 0 or 1).
    pub releases: Vec<UpdateRelease>,
}

/// A single available update release.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct UpdateRelease {
    /// PMS-internal key for triggering the download.
    pub download_key: Option<String>,
    /// Version string of the available release.
    pub version: String,
    /// "What's new" — feature additions.
    pub added: Option<String>,
    /// "What's fixed" — bug fixes.
    pub fixed: Option<String>,
    /// Direct download URL (when known).
    pub download_url: Option<String>,
    /// Update state — `downloaded`, `available`, etc.
    pub state: Option<String>,
}

// -----------------------------------------------------------------------------
// BandwidthStat / ResourceStat.
// -----------------------------------------------------------------------------

/// One bandwidth sample.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BandwidthStat {
    /// User account responsible for the traffic.
    pub account_id: u64,
    /// Device on which the traffic happened.
    pub device_id: u64,
    /// Time of the sample.
    pub at: Option<DateTime<Utc>>,
    /// Number of bytes during the sample window.
    pub bytes: u64,
    /// `true` when the traffic stayed on the LAN.
    pub lan: bool,
    /// Sample timespan — `1=months, 2=weeks, 3=days, 4=hours, 6=seconds`.
    pub timespan: u32,
}

/// One resource-utilization sample.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ResourceStat {
    /// Time of the sample.
    pub at: Option<DateTime<Utc>>,
    /// Host CPU % (whole machine).
    pub host_cpu_pct: f32,
    /// Host memory %.
    pub host_memory_pct: f32,
    /// PMS process CPU %.
    pub process_cpu_pct: f32,
    /// PMS process memory %.
    pub process_memory_pct: f32,
    /// Sample timespan code (typically 6 = seconds).
    pub timespan: u32,
}

// -----------------------------------------------------------------------------
// BandwidthOptions.
// -----------------------------------------------------------------------------

/// Filter options for [`PlexServer::bandwidth_stats`].
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct BandwidthOptions {
    /// Filter to one user account.
    pub account_id: Option<u64>,
    /// Lower bound on sample time (epoch seconds).
    pub min_at_epoch: Option<i64>,
    /// Upper bound on sample time (epoch seconds).
    pub max_at_epoch: Option<i64>,
    /// Sample timespan code (`1=months`, `2=weeks`, `3=days`,
    /// `4=hours`, `6=seconds`).
    pub timespan: Option<u32>,
}

impl BandwidthOptions {
    /// Set [`Self::account_id`] (builder style).
    #[must_use]
    pub const fn with_account(mut self, account_id: u64) -> Self {
        self.account_id = Some(account_id);
        self
    }

    /// Set [`Self::min_at_epoch`] (builder style).
    #[must_use]
    pub const fn with_min_at(mut self, epoch: i64) -> Self {
        self.min_at_epoch = Some(epoch);
        self
    }

    /// Set [`Self::max_at_epoch`] (builder style).
    #[must_use]
    pub const fn with_max_at(mut self, epoch: i64) -> Self {
        self.max_at_epoch = Some(epoch);
        self
    }

    /// Set [`Self::timespan`] (builder style). `1=months`,
    /// `2=weeks`, `3=days`, `4=hours`, `6=seconds`.
    #[must_use]
    pub const fn with_timespan(mut self, ts: u32) -> Self {
        self.timespan = Some(ts);
        self
    }
}

// -----------------------------------------------------------------------------
// PlexServer accessors.
// -----------------------------------------------------------------------------

impl PlexServer {
    /// Currently-running activities (library scans, optimizes, …).
    ///
    /// # Errors
    /// Any transport / parse [`crate::Error`] variant.
    pub async fn activities(&self) -> Result<Vec<Activity>> {
        let url = self.base_url().join("/activities")?;
        let env: ActivityEnvelope = self.http().get_json(url.as_str()).await?;
        Ok(env
            .container
            .activities
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Scheduled butler tasks.
    ///
    /// # Errors
    /// Any transport / parse [`crate::Error`] variant.
    pub async fn butler_tasks(&self) -> Result<Vec<ButlerTask>> {
        let url = self.base_url().join("/butler")?;
        let env: ButlerEnvelope = self.http().get_json(url.as_str()).await?;
        Ok(env
            .container
            .butler_tasks
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Current PMS version + any pending update.
    ///
    /// # Errors
    /// Any transport / parse [`crate::Error`] variant.
    pub async fn updater_status(&self) -> Result<UpdaterStatus> {
        let url = self.base_url().join("/updater/status")?;
        let env: UpdaterEnvelope = self.http().get_json(url.as_str()).await?;
        Ok(UpdaterStatus {
            releases: env.container.releases.into_iter().map(Into::into).collect(),
        })
    }

    /// Bandwidth statistics. Filterable by account, time range,
    /// and aggregation window via [`BandwidthOptions`].
    ///
    /// # Errors
    /// Any transport / parse [`crate::Error`] variant.
    pub async fn bandwidth_stats(&self, opts: &BandwidthOptions) -> Result<Vec<BandwidthStat>> {
        let mut url = self.base_url().join("/statistics/bandwidth")?;
        {
            let mut qp = url.query_pairs_mut();
            if let Some(id) = opts.account_id {
                qp.append_pair("accountID", &id.to_string());
            }
            if let Some(min) = opts.min_at_epoch {
                qp.append_pair("at>", &min.to_string());
            }
            if let Some(max) = opts.max_at_epoch {
                qp.append_pair("at<", &max.to_string());
            }
            if let Some(ts) = opts.timespan {
                qp.append_pair("timespan", &ts.to_string());
            }
        }
        let env: BandwidthEnvelope = self.http().get_json(url.as_str()).await?;
        Ok(env.container.stats.into_iter().map(Into::into).collect())
    }

    /// Resource-utilization statistics (CPU / RAM, host + process).
    ///
    /// # Errors
    /// Any transport / parse [`crate::Error`] variant.
    pub async fn resource_stats(&self) -> Result<Vec<ResourceStat>> {
        let url = self.base_url().join("/statistics/resources?timespan=6")?;
        let env: ResourceEnvelope = self.http().get_json(url.as_str()).await?;
        Ok(env.container.stats.into_iter().map(Into::into).collect())
    }
}

// -----------------------------------------------------------------------------
// DTOs and conversions.
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ActivityEnvelope {
    #[serde(rename = "MediaContainer")]
    container: ActivityContainer,
}

#[derive(Debug, Deserialize, Default)]
struct ActivityContainer {
    #[serde(rename = "Activity", default)]
    activities: Vec<ActivityDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActivityDto {
    #[serde(default)]
    uuid: String,
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    subtitle: Option<String>,
    #[serde(default)]
    progress: u8,
    #[serde(default)]
    cancellable: bool,
}

impl From<ActivityDto> for Activity {
    fn from(d: ActivityDto) -> Self {
        Self {
            uuid: d.uuid,
            kind: d.kind,
            title: d.title,
            subtitle: d.subtitle.filter(|s| !s.is_empty()),
            progress: d.progress,
            cancellable: d.cancellable,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ButlerEnvelope {
    #[serde(rename = "MediaContainer")]
    container: ButlerContainer,
}

#[derive(Debug, Deserialize, Default)]
struct ButlerContainer {
    #[serde(rename = "ButlerTask", default)]
    butler_tasks: Vec<ButlerTaskDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ButlerTaskDto {
    #[serde(default)]
    name: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    interval: u32,
    #[serde(default)]
    schedule_randomized: bool,
}

impl From<ButlerTaskDto> for ButlerTask {
    fn from(d: ButlerTaskDto) -> Self {
        Self {
            name: d.name,
            title: d.title,
            description: d.description,
            enabled: d.enabled,
            interval_days: d.interval,
            schedule_randomized: d.schedule_randomized,
        }
    }
}

#[derive(Debug, Deserialize)]
struct UpdaterEnvelope {
    #[serde(rename = "MediaContainer")]
    container: UpdaterContainer,
}

#[derive(Debug, Deserialize, Default)]
struct UpdaterContainer {
    #[serde(rename = "Release", default)]
    releases: Vec<ReleaseDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseDto {
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    version: String,
    #[serde(default)]
    added: Option<String>,
    #[serde(default)]
    fixed: Option<String>,
    #[serde(default, rename = "downloadURL")]
    download_url: Option<String>,
    #[serde(default)]
    state: Option<String>,
}

impl From<ReleaseDto> for UpdateRelease {
    fn from(d: ReleaseDto) -> Self {
        Self {
            download_key: d.key.filter(|s| !s.is_empty()),
            version: d.version,
            added: d.added.filter(|s| !s.is_empty()),
            fixed: d.fixed.filter(|s| !s.is_empty()),
            download_url: d.download_url.filter(|s| !s.is_empty()),
            state: d.state.filter(|s| !s.is_empty()),
        }
    }
}

#[derive(Debug, Deserialize)]
struct BandwidthEnvelope {
    #[serde(rename = "MediaContainer")]
    container: BandwidthContainer,
}

#[derive(Debug, Deserialize, Default)]
struct BandwidthContainer {
    #[serde(rename = "StatisticsBandwidth", default)]
    stats: Vec<BandwidthDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BandwidthDto {
    #[serde(default, rename = "accountID")]
    account_id: u64,
    #[serde(default, rename = "deviceID")]
    device_id: u64,
    #[serde(default)]
    at: i64,
    #[serde(default)]
    bytes: u64,
    #[serde(default)]
    lan: bool,
    #[serde(default)]
    timespan: u32,
}

impl From<BandwidthDto> for BandwidthStat {
    fn from(d: BandwidthDto) -> Self {
        Self {
            account_id: d.account_id,
            device_id: d.device_id,
            at: if d.at == 0 {
                None
            } else {
                DateTime::<Utc>::from_timestamp(d.at, 0)
            },
            bytes: d.bytes,
            lan: d.lan,
            timespan: d.timespan,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ResourceEnvelope {
    #[serde(rename = "MediaContainer")]
    container: ResourceContainer,
}

#[derive(Debug, Deserialize, Default)]
struct ResourceContainer {
    #[serde(rename = "StatisticsResources", default)]
    stats: Vec<ResourceDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResourceDto {
    #[serde(default)]
    at: i64,
    #[serde(default)]
    host_cpu_utilization: f32,
    #[serde(default)]
    host_memory_utilization: f32,
    #[serde(default)]
    process_cpu_utilization: f32,
    #[serde(default)]
    process_memory_utilization: f32,
    #[serde(default)]
    timespan: u32,
}

impl From<ResourceDto> for ResourceStat {
    fn from(d: ResourceDto) -> Self {
        Self {
            at: if d.at == 0 {
                None
            } else {
                DateTime::<Utc>::from_timestamp(d.at, 0)
            },
            host_cpu_pct: d.host_cpu_utilization,
            host_memory_pct: d.host_memory_utilization,
            process_cpu_pct: d.process_cpu_utilization,
            process_memory_pct: d.process_memory_utilization,
            timespan: d.timespan,
        }
    }
}

// -----------------------------------------------------------------------------
// Tests.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_dto_round_trip() {
        let env: ActivityEnvelope = serde_json::from_value(serde_json::json!({
            "MediaContainer": {
                "Activity": [{
                    "uuid": "u1",
                    "type": "library.refresh.items",
                    "title": "Refreshing",
                    "subtitle": "Movies",
                    "progress": 42,
                    "cancellable": true
                }]
            }
        }))
        .unwrap();
        let acts: Vec<Activity> = env
            .container
            .activities
            .into_iter()
            .map(Into::into)
            .collect();
        assert_eq!(acts.len(), 1);
        assert_eq!(acts[0].uuid, "u1");
        assert_eq!(acts[0].kind, "library.refresh.items");
        assert_eq!(acts[0].progress, 42);
        assert!(acts[0].cancellable);
        assert_eq!(acts[0].subtitle.as_deref(), Some("Movies"));
    }

    #[test]
    fn butler_task_dto_round_trip() {
        let env: ButlerEnvelope = serde_json::from_value(serde_json::json!({
            "MediaContainer": {
                "ButlerTask": [{
                    "name": "BackupDatabase",
                    "title": "Backup",
                    "description": "Backs up the PMS db",
                    "enabled": true,
                    "interval": 1,
                    "scheduleRandomized": false
                }]
            }
        }))
        .unwrap();
        let tasks: Vec<ButlerTask> = env
            .container
            .butler_tasks
            .into_iter()
            .map(Into::into)
            .collect();
        assert_eq!(tasks[0].name, "BackupDatabase");
        assert_eq!(tasks[0].interval_days, 1);
        assert!(tasks[0].enabled);
    }

    #[test]
    fn updater_status_dto_round_trip() {
        let env: UpdaterEnvelope = serde_json::from_value(serde_json::json!({
            "MediaContainer": {
                "Release": [{
                    "key": "/updater/check/123",
                    "version": "1.41.0.0",
                    "added": "New transcode quality presets",
                    "fixed": "Memory leak in DLNA",
                    "downloadURL": "https://plex.tv/downloads/1.41.0.0",
                    "state": "available"
                }]
            }
        }))
        .unwrap();
        let releases: Vec<UpdateRelease> =
            env.container.releases.into_iter().map(Into::into).collect();
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].version, "1.41.0.0");
        assert_eq!(releases[0].state.as_deref(), Some("available"));
    }

    #[test]
    fn bandwidth_dto_round_trip() {
        let env: BandwidthEnvelope = serde_json::from_value(serde_json::json!({
            "MediaContainer": {
                "StatisticsBandwidth": [{
                    "accountID": 1,
                    "deviceID": 100,
                    "at": 1_700_000_000,
                    "bytes": 1_073_741_824_i64,
                    "lan": true,
                    "timespan": 3
                }]
            }
        }))
        .unwrap();
        let stats: Vec<BandwidthStat> = env.container.stats.into_iter().map(Into::into).collect();
        assert_eq!(stats[0].account_id, 1);
        assert_eq!(stats[0].device_id, 100);
        assert_eq!(stats[0].bytes, 1_073_741_824);
        assert!(stats[0].lan);
        assert_eq!(stats[0].timespan, 3);
        assert!(stats[0].at.is_some());
    }

    #[test]
    fn resource_dto_round_trip() {
        let env: ResourceEnvelope = serde_json::from_value(serde_json::json!({
            "MediaContainer": {
                "StatisticsResources": [{
                    "at": 1_700_000_000,
                    "hostCpuUtilization": 23.5,
                    "hostMemoryUtilization": 42.0,
                    "processCpuUtilization": 5.2,
                    "processMemoryUtilization": 7.1,
                    "timespan": 6
                }]
            }
        }))
        .unwrap();
        let stats: Vec<ResourceStat> = env.container.stats.into_iter().map(Into::into).collect();
        assert!((stats[0].host_cpu_pct - 23.5).abs() < 0.01);
        assert!((stats[0].process_memory_pct - 7.1).abs() < 0.01);
        assert_eq!(stats[0].timespan, 6);
    }

    #[test]
    fn bandwidth_options_builder_chain() {
        let opts = BandwidthOptions::default()
            .with_account(42)
            .with_min_at(1000)
            .with_max_at(2000)
            .with_timespan(4);
        assert_eq!(opts.account_id, Some(42));
        assert_eq!(opts.min_at_epoch, Some(1000));
        assert_eq!(opts.max_at_epoch, Some(2000));
        assert_eq!(opts.timespan, Some(4));
    }

    #[test]
    fn empty_envelopes_yield_empty_vecs() {
        let act: ActivityEnvelope =
            serde_json::from_value(serde_json::json!({"MediaContainer": {}})).unwrap();
        assert!(act.container.activities.is_empty());
        let butler: ButlerEnvelope =
            serde_json::from_value(serde_json::json!({"MediaContainer": {}})).unwrap();
        assert!(butler.container.butler_tasks.is_empty());
        let upd: UpdaterEnvelope =
            serde_json::from_value(serde_json::json!({"MediaContainer": {}})).unwrap();
        assert!(upd.container.releases.is_empty());
    }
}
