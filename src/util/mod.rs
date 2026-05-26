//! Internal helpers and small data types reused across the crate.
//!
//! Modules in this folder must not depend on anything in the rest of
//! the crate beyond [`crate::error`]. They are the building blocks
//! every higher-level module composes.

pub mod ids;
pub mod sanitize;
pub mod search_type;
pub mod time;

pub use ids::{ClientIdentifier, MachineIdentifier, PlayQueueId, PlexToken, RatingKey};
pub use search_type::SearchType;
