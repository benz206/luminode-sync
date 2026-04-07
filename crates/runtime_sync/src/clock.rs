/// Local monotonic sync clock.
///
/// The core insight: Spotify polls are slow and jittery (~1–5 seconds apart,
/// with ±100 ms API latency).  We can't drive 60 fps LEDs directly from them.
///
/// Solution: each Spotify poll establishes a SyncAnchor — a pair of
/// (spotify_progress_ms, local_instant).  Between polls we extrapolate
/// playback position using the monotonic clock.
///
/// When a new poll arrives, we compare our extrapolated position to the
/// fresh Spotify position and decide whether to snap or smooth-correct.
///
/// Drift correction strategy:
///   • diff < IGNORE_MS:    do nothing (within normal jitter)
///   • IGNORE_MS ≤ diff < SNAP_MS: smooth correction over SMOOTH_WINDOW_MS
///   • diff ≥ SNAP_MS:      hard snap (seek, pause/resume, or track change)

use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Diffs below this are ignored (Spotify API jitter).
const IGNORE_MS: i64 = 40;
/// Diffs above this trigger a hard snap.
const SNAP_MS: i64 = 600;
/// Smooth corrections are applied over this window.
const SMOOTH_WINDOW_MS: u32 = 300;

/// A single sync anchor point established from a Spotify poll.
#[derive(Clone, Debug)]
pub struct SyncAnchor {
    /// Track identity.
    pub track_id: String,
    /// Spotify-reported playback position at the time of the poll.
    pub spotify_progress_ms: u32,
    /// Local monotonic clock at the moment we recorded spotify_progress_ms.
    /// Set to `Instant::now()` immediately after receiving the API response.
    pub local_time: Instant,
    /// Whether Spotify reported the track as playing.
    pub is_playing: bool,
    /// Smooth correction in progress: (delta_ms remaining, deadline).
    pub smooth_correction: Option<SmoothCorrection>,
}

#[derive(Clone, Debug)]
pub struct SmoothCorrection {
    /// Total correction to apply (positive = we are behind, negative = ahead).
    pub total_delta_ms: i64,
    /// Amount already applied.
    pub applied_ms: f64,
    /// When we started the correction.
    pub started_at: Instant,
    /// Total window over which to spread the correction.
    pub window_ms: u32,
}

impl SyncAnchor {
    /// Create a fresh anchor from a Spotify poll result.
    pub fn new(track_id: String, progress_ms: u32, is_playing: bool) -> Self {
        SyncAnchor {
            track_id,
            spotify_progress_ms: progress_ms,
            local_time: Instant::now(),
            is_playing,
            smooth_correction: None,
        }
    }

    /// Estimated playback position right now, in milliseconds.
    ///
    /// If paused, returns the frozen position.
    /// If playing, extrapolates from the anchor using the monotonic clock.
    pub fn estimate_ms(&self) -> u32 {
        if !self.is_playing {
            return self.spotify_progress_ms;
        }
        let elapsed_ms = self.local_time.elapsed().as_millis() as u32;
        let base = self.spotify_progress_ms + elapsed_ms;

        // Apply any in-progress smooth correction.
        if let Some(ref corr) = self.smooth_correction {
            let corr_elapsed = corr.started_at.elapsed().as_millis() as f64;
            let t = (corr_elapsed / corr.window_ms as f64).clamp(0.0, 1.0);
            // Ease-in/ease-out: smooth-step.
            let ease = t * t * (3.0 - 2.0 * t);
            let correction_applied = corr.total_delta_ms as f64 * ease;
            let corrected = base as i64 + correction_applied as i64;
            return corrected.max(0) as u32;
        }

        base
    }
}

/// Shared state read by the render loop and written by the Spotify poller.
///
/// The render loop calls `try_read()` — if the lock is momentarily held by
/// the poller, it continues with the previous estimate rather than blocking.
#[derive(Clone)]
pub struct SyncState(pub Arc<RwLock<SyncAnchor>>);

impl SyncState {
    pub fn new(initial: SyncAnchor) -> Self {
        SyncState(Arc::new(RwLock::new(initial)))
    }

    /// Non-blocking read.  Returns `None` if the write lock is held.
    pub fn try_estimate(&self) -> Option<PlaybackEstimate> {
        self.0.try_read().ok().map(|guard| PlaybackEstimate {
            track_id: guard.track_id.clone(),
            position_ms: guard.estimate_ms(),
            is_playing: guard.is_playing,
        })
    }

    /// Apply a new Spotify poll result.  Decides snap vs. smooth correction.
    pub fn update(&self, new_track_id: &str, new_progress_ms: u32, new_is_playing: bool) -> UpdateResult {
        let mut guard = self.0.write().unwrap();

        // ── Track change — always hard reset ─────────────────────────────────
        if new_track_id != guard.track_id {
            *guard = SyncAnchor::new(new_track_id.to_owned(), new_progress_ms, new_is_playing);
            return UpdateResult::TrackChanged;
        }

        // ── Play/pause state change — hard reset ─────────────────────────────
        if new_is_playing != guard.is_playing {
            *guard = SyncAnchor::new(new_track_id.to_owned(), new_progress_ms, new_is_playing);
            return UpdateResult::StateChanged;
        }

        // ── Paused: just update the position, no extrapolation needed ─────────
        if !new_is_playing {
            guard.spotify_progress_ms = new_progress_ms;
            return UpdateResult::OnTarget;
        }

        // ── Playing: compare our estimate to what Spotify says ────────────────
        let our_estimate = guard.estimate_ms() as i64;
        let delta = new_progress_ms as i64 - our_estimate;

        if delta.abs() < IGNORE_MS {
            // Close enough — update the anchor silently to avoid drift accumulation.
            guard.spotify_progress_ms = new_progress_ms;
            guard.local_time = Instant::now();
            guard.smooth_correction = None;
            UpdateResult::OnTarget
        } else if delta.abs() >= SNAP_MS {
            // Large discontinuity — seek or systematic error.
            *guard = SyncAnchor::new(new_track_id.to_owned(), new_progress_ms, new_is_playing);
            UpdateResult::Snapped { delta_ms: delta }
        } else {
            // Small drift — apply smooth correction.
            // Clear any previous correction (the new one supersedes it).
            guard.smooth_correction = Some(SmoothCorrection {
                total_delta_ms: delta,
                applied_ms: 0.0,
                started_at: Instant::now(),
                window_ms: SMOOTH_WINDOW_MS,
            });
            // Also update the anchor position so future estimates use the new base.
            guard.spotify_progress_ms = new_progress_ms;
            guard.local_time = Instant::now();
            UpdateResult::SmoothCorrection { delta_ms: delta }
        }
    }
}

/// What the render loop needs to know.
#[derive(Debug, Clone)]
pub struct PlaybackEstimate {
    pub track_id: String,
    pub position_ms: u32,
    pub is_playing: bool,
}

/// What happened after processing a Spotify poll.
#[derive(Debug)]
pub enum UpdateResult {
    OnTarget,
    Snapped { delta_ms: i64 },
    SmoothCorrection { delta_ms: i64 },
    TrackChanged,
    StateChanged,
}
