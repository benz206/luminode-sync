use serde::{Deserialize, Serialize};

/// Beat timing data — the heart of every beatmap.
///
/// Timestamp encoding rationale:
///
///   Option A — absolute u32 ms per beat:  4 bytes/beat, simple, wastes space.
///   Option B — delta u16 ms:              2 bytes/beat, handles any tempo,
///                                          max inter-beat gap = 65,535 ms (~18 BPM).
///   Option C — varint delta:              Marginally smaller for slow songs,
///                                          not worth the decode complexity.
///   Option D — tempo-grid ticks:          Very compact for constant-tempo music,
///                                          fragile with rubato/tempo drift, hard
///                                          to generate correctly.
///
///   CHOSEN: Option B.  At 120 BPM (500 ms/beat), a 5-min song uses
///   ~600 beats × 2 B = 1.2 KB.  Downbeat flags cost 75 B as a bitset.
///   Simple decode: one 16-bit add per beat.
///
/// Downbeat encoding:
///   Packed as a little-endian bitset in `downbeat_bits`.
///   Byte 0 contains flags for beats 0–7, byte 1 for beats 8–15, etc.
///   Bit i within a byte = (byte >> (i % 8)) & 1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimingData {
    /// Absolute position of beat[0] in ms from track start.
    pub first_beat_ms: u32,

    /// Delta in ms from beat[i] to beat[i+1].
    /// beat[0] is at first_beat_ms.
    /// beat[k] is at first_beat_ms + sum(beat_deltas_ms[0..k]).
    pub beat_deltas_ms: Vec<u16>,

    /// Downbeat flags: bit i = beat i is a bar start (downbeat).
    pub downbeat_bits: Vec<u8>,

    /// Detected time signature numerator (beats per bar, usually 4).
    pub time_sig: u8,
}

/// The beat state at a given playback position.
#[derive(Debug, Clone, Copy)]
pub struct BeatContext {
    /// Index of the beat that is currently "active" (most recently passed).
    pub index: usize,

    /// Phase within the current inter-beat interval.
    /// 0.0 = exactly on the beat, 1.0 = just before the next beat.
    pub phase: f32,

    /// True if this beat is a bar downbeat.
    pub is_downbeat: bool,

    /// Phase within the current bar (0.0 = downbeat, 1.0 = just before next downbeat).
    pub bar_phase: f32,
}

impl TimingData {
    /// Build from a flat list of absolute beat positions (used by the generator).
    ///
    /// `downbeat_indices` is the set of beat indices that are bar starts.
    pub fn from_beat_positions(
        positions_ms: &[u32],
        downbeat_indices: &[usize],
        time_sig: u8,
    ) -> Self {
        assert!(!positions_ms.is_empty(), "must have at least one beat");

        let first_beat_ms = positions_ms[0];

        let beat_deltas_ms: Vec<u16> = positions_ms
            .windows(2)
            .map(|w| {
                let delta = w[1].saturating_sub(w[0]);
                // Clamp to u16::MAX — practically impossible at real musical tempos.
                delta.min(u16::MAX as u32) as u16
            })
            .collect();

        // Pack downbeat indices into a bitset.
        let n_beats = positions_ms.len();
        let bitset_len = (n_beats + 7) / 8;
        let mut downbeat_bits = vec![0u8; bitset_len];
        for &i in downbeat_indices {
            if i < n_beats {
                downbeat_bits[i / 8] |= 1 << (i % 8);
            }
        }

        TimingData {
            first_beat_ms,
            beat_deltas_ms,
            downbeat_bits,
            time_sig,
        }
    }

    /// Total number of beats in this track.
    pub fn beat_count(&self) -> usize {
        self.beat_deltas_ms.len() + 1
    }

    /// Decode all beat positions as absolute ms timestamps.
    ///
    /// Allocates once per call — call sparingly in hot paths.
    /// At runtime, prefer `beat_at_position` which walks the delta array directly.
    pub fn beat_positions_ms(&self) -> Vec<u32> {
        let mut positions = Vec::with_capacity(self.beat_count());
        let mut current = self.first_beat_ms;
        positions.push(current);
        for &delta in &self.beat_deltas_ms {
            current += delta as u32;
            positions.push(current);
        }
        positions
    }

    /// Check if beat `index` is a downbeat.
    #[inline]
    pub fn is_downbeat(&self, index: usize) -> bool {
        let byte_idx = index / 8;
        let bit_idx = index % 8;
        self.downbeat_bits
            .get(byte_idx)
            .map(|&b| (b >> bit_idx) & 1 == 1)
            .unwrap_or(false)
    }

    /// Compute the beat context for a given playback position.
    ///
    /// This is the primary hot-path call from the render loop.
    /// It does a linear scan which is fine for sequential playback;
    /// if seeks are common, switch to binary search on a cached positions Vec.
    pub fn beat_at_position(&self, position_ms: u32) -> BeatContext {
        // Binary search is O(log n) and works correctly for seeks.
        let mut lo = 0usize;
        let mut hi = self.beat_count();
        let mut current_ms = self.first_beat_ms;

        // We walk forward to find the beat just before position_ms.
        // For a 600-beat song this is still fast; consider caching positions_ms
        // in the daemon if profiling shows this matters.
        let positions = self.beat_positions_ms();

        let index = match positions.binary_search(&position_ms) {
            Ok(i) => i,
            Err(0) => 0,
            Err(i) if i >= positions.len() => positions.len() - 1,
            Err(i) => i - 1,
        };
        let _ = (lo, hi, current_ms); // suppress unused warnings

        let beat_start = positions[index];
        let beat_end = positions.get(index + 1).copied().unwrap_or(beat_start + 500);
        let phase = if beat_end > beat_start {
            (position_ms.saturating_sub(beat_start)) as f32 / (beat_end - beat_start) as f32
        } else {
            0.0
        };

        let is_downbeat = self.is_downbeat(index);

        // Compute bar phase: find the most recent downbeat.
        let bar_phase = self.bar_phase_at(index, phase);

        BeatContext {
            index,
            phase: phase.clamp(0.0, 1.0),
            is_downbeat,
            bar_phase,
        }
    }

    /// Fractional position within the current bar.
    /// 0.0 = exactly on the downbeat, 1.0 = just before the next downbeat.
    fn bar_phase_at(&self, beat_index: usize, beat_phase: f32) -> f32 {
        // Walk backwards to find the most recent downbeat.
        let downbeat_start = (0..=beat_index)
            .rev()
            .find(|&i| self.is_downbeat(i))
            .unwrap_or(0);

        // Walk forward to find the next downbeat.
        let n = self.beat_count();
        let downbeat_end = (beat_index + 1..n)
            .find(|&i| self.is_downbeat(i))
            .unwrap_or(n);

        let beats_since_downbeat = (beat_index - downbeat_start) as f32 + beat_phase;
        let bar_length_beats = (downbeat_end - downbeat_start) as f32;

        if bar_length_beats > 0.0 {
            (beats_since_downbeat / bar_length_beats).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}
