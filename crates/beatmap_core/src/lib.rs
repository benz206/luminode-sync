// beatmap_core — shared beatmap format, types, and serialization.
//
// Design principles:
//   • Every type here is #[non_exhaustive] so callers must handle unknown variants.
//   • Timestamps are delta-encoded u16 ms; see TimingData for rationale.
//   • Wire format is MessagePack. JSON is only used for human inspection.
//   • Version byte is always the first field so the reader can reject incompatible files.

pub mod beatmap;
pub mod timing;
pub mod library;

pub use beatmap::*;
pub use timing::*;
pub use library::*;

use thiserror::Error;

/// Current on-disk format version. Bump when making breaking changes.
pub const BEATMAP_VERSION: u8 = 1;

#[derive(Debug, Error)]
pub enum BeatmapError {
    #[error("unsupported beatmap version {got} (expected {expected})")]
    VersionMismatch { got: u8, expected: u8 },

    #[error("serialization error: {0}")]
    Serialize(#[from] rmp_serde::encode::Error),

    #[error("deserialization error: {0}")]
    Deserialize(#[from] rmp_serde::decode::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("validation failed: {0}")]
    Validation(String),
}
