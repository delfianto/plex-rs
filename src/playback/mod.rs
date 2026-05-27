//! Playback surface — queues, remote control, transcoding.
//!
//! Currently houses [`play_queue::PlayQueue`], which models the
//! `/playQueues/*` REST API used by every Plex player to start and
//! manage playback. Remote control (`/player/*`) and transcode URL
//! construction land in later milestones.

pub mod play_queue;

pub use play_queue::{CreatePlayQueue, PlayQueue, PlayQueueItem};
