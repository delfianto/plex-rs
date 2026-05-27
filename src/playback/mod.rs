//! Playback surface — queues, remote control, transcoding.
//!
//! Currently houses [`play_queue::PlayQueue`], which models the
//! `/playQueues/*` REST API used by every Plex player to start and
//! manage playback. Remote control (`/player/*`) and transcode URL
//! construction land in later milestones.

pub mod client;
pub mod play_queue;
pub mod transcode;

pub use client::{MediaType, PlexClient, RepeatMode};
pub use play_queue::{CreatePlayQueue, PlayQueue, PlayQueueItem};
pub use transcode::{LocationHint, StreamKind, TranscodeOptions, TranscodeProtocol, transcode_url};
