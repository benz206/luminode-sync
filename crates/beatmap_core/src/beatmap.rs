use serde::{Deserialize, Serialize};
use std::path::Path;
use crate::{BeatmapError, TimingData, BEATMAP_VERSION};

/// Top-level beatmap structure. Serialized as MessagePack on disk.
///
/// Size budget for a typical 4-min song at 120 BPM:
///   • beat_deltas_ms: ~480 beats × 2 bytes  ≈  960 B
///   • downbeat_bits:  ~480 / 8              ≈   60 B
///   • sections:       ~8 sections × 4 bytes ≈   32 B
///   • cues:           ~4 cues    × 8 bytes  ≈   32 B
///   • energy:         ~60 samples × 1 byte  ≈   60 B
///   • metadata + overhead                   ≈  200 B
///   Total raw: ~1.3 KB minimum (short/slow song, sparse data).
///   Real-world range: 2–9 KB depending on tempo, duration, and metadata length.
///   Typical: 3–6 KB.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Beatmap {
    /// Format version — always first so readers can reject incompatible files.
    pub version: u8,

    pub track: TrackMeta,
    pub timing: TimingData,

    /// Sparse list of sections in chronological order.
    pub sections: Vec<Section>,

    /// Explicit cue markers — drops, fills, impacts. Sparse.
    pub cues: Vec<Cue>,

    /// Low-resolution energy envelope. Sampled every N beats.
    pub energy: EnergyEnvelope,

    /// Trim added to all beat positions at render time (ms).
    /// Positive = shift events later (audio leads perceived beat).
    /// Tune this per-track if sync feels early or late.
    pub calibration_ms: i32,
}

impl Beatmap {
    /// Deserialize from MessagePack bytes.
    pub fn from_msgpack(bytes: &[u8]) -> Result<Self, BeatmapError> {
        let bm: Beatmap = rmp_serde::from_slice(bytes)?;
        if bm.version != BEATMAP_VERSION {
            return Err(BeatmapError::VersionMismatch {
                got: bm.version,
                expected: BEATMAP_VERSION,
            });
        }
        bm.validate()?;
        Ok(bm)
    }

    /// Serialize to MessagePack bytes.
    pub fn to_msgpack(&self) -> Result<Vec<u8>, BeatmapError> {
        Ok(rmp_serde::to_vec_named(self)?)
    }

    /// Load from a `.beatmap` file.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, BeatmapError> {
        let bytes = std::fs::read(path)?;
        Self::from_msgpack(&bytes)
    }

    /// Save to a `.beatmap` file.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), BeatmapError> {
        let bytes = self.to_msgpack()?;
        std::fs::write(path, bytes)?;
        Ok(())
    }

    /// Dump as pretty JSON for human inspection.
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Basic structural validation.
    pub fn validate(&self) -> Result<(), BeatmapError> {
        if self.timing.beat_count() == 0 {
            return Err(BeatmapError::Validation("no beats".into()));
        }
        if self.track.duration_ms == 0 {
            return Err(BeatmapError::Validation("duration_ms is 0".into()));
        }
        // Sections must be in beat-index order.
        for w in self.sections.windows(2) {
            if w[0].start_beat >= w[1].start_beat {
                return Err(BeatmapError::Validation(
                    "sections are not in ascending beat order".into(),
                ));
            }
        }
        // Cues must be in timestamp order.
        for w in self.cues.windows(2) {
            if w[0].position_ms >= w[1].position_ms {
                return Err(BeatmapError::Validation(
                    "cues are not in ascending timestamp order".into(),
                ));
            }
        }
        Ok(())
    }

    /// Return the section active at `position_ms`, or None if before the first section.
    pub fn section_at(&self, position_ms: u32) -> Option<&Section> {
        let beat_ctx = self.timing.beat_at_position(position_ms);
        // Walk backwards to find the last section that starts at or before the current beat.
        self.sections
            .iter()
            .rev()
            .find(|s| s.start_beat as usize <= beat_ctx.index)
    }

    /// Return cues whose window overlaps `position_ms`.
    /// `window_ms` is how far ahead to look (for pre-triggering effects).
    pub fn active_cues(&self, position_ms: u32, window_ms: u32) -> impl Iterator<Item = &Cue> {
        self.cues
            .iter()
            .filter(move |c| c.position_ms >= position_ms && c.position_ms < position_ms + window_ms)
    }

    /// Interpolated energy [0.0, 1.0] at `position_ms`.
    pub fn energy_at(&self, position_ms: u32) -> f32 {
        self.energy.sample_at(position_ms, &self.timing)
    }
}

// ─── Track metadata ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackMeta {
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_ms: u32,

    /// Primary match key at runtime.
    pub spotify_id: Option<String>,

    /// ISRC is stable across re-releases and remasters — useful as a fallback key.
    pub isrc: Option<String>,

    /// SHA-256 of the source audio file. Used by the generator to skip unchanged files.
    pub source_hash: String,

    /// BPM as detected (informational only — timing data is authoritative).
    pub detected_bpm: f32,

    /// Dominant RGB colour extracted from the album art at generation time.
    /// Stored as [r, g, b] bytes.  None if no cover art was found.
    /// Intended for use by both the viewer and embedded firmware (ESP32).
    #[serde(default)]
    pub dominant_color: Option<[u8; 3]>,
}

// ─── Sections ────────────────────────────────────────────────────────────────

/// A structural section of the track.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Section {
    /// Beat index (into TimingData::beat_positions_ms) where this section starts.
    pub start_beat: u16,

    pub kind: SectionKind,

    /// Coarse energy 0–255. Used by the light engine to modulate intensity.
    pub energy: u8,
}

/// Well-known section types. Unknown is a catch-all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SectionKind {
    Intro,
    Verse,
    Chorus,
    Buildup,
    Drop,
    Breakdown,
    Bridge,
    Outro,
    Unknown,
}

impl SectionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SectionKind::Intro     => "intro",
            SectionKind::Verse     => "verse",
            SectionKind::Chorus    => "chorus",
            SectionKind::Buildup   => "buildup",
            SectionKind::Drop      => "drop",
            SectionKind::Breakdown => "breakdown",
            SectionKind::Bridge    => "bridge",
            SectionKind::Outro     => "outro",
            SectionKind::Unknown   => "unknown",
        }
    }
}

// ─── Cues ─────────────────────────────────────────────────────────────────────

/// An explicit one-shot event marker in the timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cue {
    /// Absolute position in ms from track start.
    pub position_ms: u32,
    pub kind: CueKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CueKind {
    Drop,           // Main drop — strobe/white-flash territory
    Build,          // Buildup apex
    Fill,           // Drum fill / transition
    Impact,         // Generic strong transient hit
    Custom(String), // User-defined label
}

// ─── Energy envelope ──────────────────────────────────────────────────────────

/// A sparse, linearly-interpolated energy curve.
///
/// Instead of storing one value per frame, we sample every N beats.
/// At 120 BPM, sampling every 4 beats = 1 sample per 2 seconds.
/// A 5-min song → 75 samples × 1 byte = 75 bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnergyEnvelope {
    /// One sample per this many beats.
    pub sample_every_n_beats: u8,
    /// Energy 0–255, one per sample.
    pub values: Vec<u8>,
}

impl EnergyEnvelope {
    /// Interpolated energy [0.0, 1.0] at `position_ms`.
    pub fn sample_at(&self, position_ms: u32, timing: &TimingData) -> f32 {
        if self.values.is_empty() {
            return 0.5;
        }
        let beat_ctx = timing.beat_at_position(position_ms);
        let n = self.sample_every_n_beats as f32;
        let sample_f = beat_ctx.index as f32 / n;
        let lo = sample_f.floor() as usize;
        let hi = lo + 1;
        let t = sample_f.fract();

        let lo_val = self.values.get(lo).copied().unwrap_or(128) as f32 / 255.0;
        let hi_val = self.values.get(hi).copied().unwrap_or(128) as f32 / 255.0;
        lo_val + (hi_val - lo_val) * t
    }
}
