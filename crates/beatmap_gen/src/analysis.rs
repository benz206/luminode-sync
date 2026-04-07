/// Beat detection via spectral flux onset detection + autocorrelation beat tracking.
///
/// Algorithm:
///   1. STFT with hop=HOP_SIZE samples → magnitude spectrum per frame.
///   2. Spectral flux onset strength = sum of positive spectral differences.
///      (Half-wave rectified to ignore decreasing energy — standard practice.)
///   3. Autocorrelation of the onset strength function to find the tempo period.
///   4. Dynamic programming beat tracking: follow the dominant period while
///      staying close to onset peaks.
///   5. Downbeats: every `time_sig` beats (refined by onset strength peaks at
///      those positions).
///
/// This is the "librosa" approach ported to Rust.  It is not perfect but
/// produces musically sensible results for 4/4 electronic music (which is
/// the primary use case here).  For higher accuracy, swap in aubio via FFI.

use anyhow::Result;
use realfft::RealFftPlanner;
use std::f32::consts::PI;

use crate::AudioBuffer;

/// Hop size in samples.  At 44100 Hz, 512 samples ≈ 11.6 ms per frame.
/// This gives ~86 frames/sec onset resolution.
const HOP_SIZE: usize = 512;

/// FFT window size.  Larger → better frequency resolution.
const FFT_SIZE: usize = 2048;

/// Minimum BPM to consider.
const BPM_MIN: f32 = 60.0;
/// Maximum BPM to consider.
const BPM_MAX: f32 = 200.0;

pub struct AnalysisResult {
    /// Beat times in ms from track start (absolute).
    pub beat_times_ms: Vec<u32>,
    /// Which of the above are bar-start downbeats.
    pub downbeat_indices: Vec<usize>,
    /// Detected tempo in BPM.
    pub bpm: f32,
    /// Detected time signature numerator.
    pub time_sig: u8,
    /// Onset strength per beat (parallel to beat_times_ms).
    pub beat_onset_strengths: Vec<f32>,
    /// Mean of onset strength across all frames.
    pub onset_strength_mean: f32,
    /// Std dev of onset strength.
    pub onset_strength_std: f32,
    /// RMS energy per beat (parallel to beat_times_ms).
    pub beat_rms: Vec<f32>,
    /// 95th percentile RMS energy (for normalisation).
    pub rms_95th: f32,

    // Internal: keep onset frames for section classifier.
    pub(crate) onset_strength: Vec<f32>,
    pub(crate) rms_frames: Vec<f32>,
    pub(crate) sample_rate: u32,
}

impl AnalysisResult {
    pub fn onset_strength_at_beat(&self, beat_idx: usize) -> f32 {
        self.beat_onset_strengths.get(beat_idx).copied().unwrap_or(0.0)
    }

    pub fn rms_energy_at_beat(&self, beat_idx: usize) -> f32 {
        self.beat_rms.get(beat_idx).copied().unwrap_or(0.0)
    }

    /// Convert a sample index to milliseconds.
    pub fn samples_to_ms(&self, sample_idx: usize) -> u32 {
        (sample_idx as f64 / self.sample_rate as f64 * 1000.0) as u32
    }
}

pub struct BeatTracker;

pub fn analyze(buf: &AudioBuffer) -> Result<AnalysisResult> {
    let sr = buf.sample_rate as f32;
    let hop = HOP_SIZE;
    let n_fft = FFT_SIZE;

    // ── 1. Compute STFT magnitude frames ─────────────────────────────────────
    let frames = stft_magnitude(&buf.samples, n_fft, hop);
    let n_frames = frames.len();

    // ── 2. Spectral flux onset strength ──────────────────────────────────────
    let mut onset_strength = vec![0.0f32; n_frames];
    for i in 1..n_frames {
        let mut flux = 0.0f32;
        for k in 0..frames[i].len() {
            let diff = frames[i][k] - frames[i - 1][k];
            if diff > 0.0 {
                flux += diff;
            }
        }
        onset_strength[i] = flux;
    }

    // Apply super-Gaussian smoothing to reduce noise.
    let smoothed = smooth_onset(&onset_strength, 3);

    // ── 3. Onset strength statistics ─────────────────────────────────────────
    let mean = smoothed.iter().copied().sum::<f32>() / smoothed.len() as f32;
    let variance = smoothed.iter().map(|&x| (x - mean).powi(2)).sum::<f32>()
        / smoothed.len() as f32;
    let std = variance.sqrt();

    // ── 4. Autocorrelation to find the dominant tempo period ─────────────────
    let frames_per_sec = sr / hop as f32;
    let min_lag = (frames_per_sec * 60.0 / BPM_MAX) as usize;
    let max_lag = (frames_per_sec * 60.0 / BPM_MIN) as usize;

    let bpm_lag = autocorrelation_peak(&smoothed, min_lag, max_lag);
    let bpm = 60.0 / (bpm_lag as f32 / frames_per_sec);

    // ── 5. Dynamic programming beat tracking ─────────────────────────────────
    let beat_frames = dp_beat_track(&smoothed, bpm_lag);

    // ── 6. Convert frame indices to ms ────────────────────────────────────────
    let beat_times_ms: Vec<u32> = beat_frames
        .iter()
        .map(|&f| (f as f32 * hop as f32 / sr * 1000.0) as u32)
        .collect();

    // ── 7. Identify downbeats ─────────────────────────────────────────────────
    let time_sig = 4u8; // assume 4/4 for now; extend with meter detection later
    let downbeat_indices: Vec<usize> = (0..beat_times_ms.len())
        .step_by(time_sig as usize)
        .collect();

    // ── 8. Per-beat onset strength ────────────────────────────────────────────
    let beat_onset_strengths: Vec<f32> = beat_frames
        .iter()
        .map(|&f| smoothed.get(f).copied().unwrap_or(0.0))
        .collect();

    // ── 9. RMS energy per frame → per beat ───────────────────────────────────
    let rms_frames = compute_rms_frames(&buf.samples, hop);
    let beat_rms: Vec<f32> = beat_frames
        .iter()
        .map(|&f| rms_frames.get(f).copied().unwrap_or(0.0))
        .collect();

    // 95th percentile for normalisation.
    let mut sorted_rms = beat_rms.clone();
    sorted_rms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx_95 = (sorted_rms.len() as f32 * 0.95) as usize;
    let rms_95th = sorted_rms.get(idx_95).copied().unwrap_or(1.0).max(1e-6);

    Ok(AnalysisResult {
        beat_times_ms,
        downbeat_indices,
        bpm,
        time_sig,
        beat_onset_strengths,
        onset_strength_mean: mean,
        onset_strength_std: std,
        beat_rms,
        rms_95th,
        onset_strength: smoothed,
        rms_frames,
        sample_rate: buf.sample_rate,
    })
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

/// Compute STFT magnitude frames with a Hann window.
fn stft_magnitude(samples: &[f32], n_fft: usize, hop: usize) -> Vec<Vec<f32>> {
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(n_fft);

    let hann: Vec<f32> = (0..n_fft)
        .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f32 / (n_fft - 1) as f32).cos()))
        .collect();

    let mut frames = Vec::new();
    let mut pos = 0;
    let half_fft = n_fft / 2 + 1;

    while pos + n_fft <= samples.len() {
        let mut input: Vec<f32> = samples[pos..pos + n_fft]
            .iter()
            .zip(&hann)
            .map(|(&s, &w)| s * w)
            .collect();

        let mut output = fft.make_output_vec();
        fft.process(&mut input, &mut output).ok();

        let mags: Vec<f32> = output.iter().take(half_fft).map(|c| c.norm()).collect();
        frames.push(mags);
        pos += hop;
    }

    frames
}

/// Simple moving-average smoothing.
fn smooth_onset(data: &[f32], radius: usize) -> Vec<f32> {
    let n = data.len();
    let mut out = vec![0.0f32; n];
    for i in 0..n {
        let lo = i.saturating_sub(radius);
        let hi = (i + radius + 1).min(n);
        let sum: f32 = data[lo..hi].iter().sum();
        out[i] = sum / (hi - lo) as f32;
    }
    out
}

/// Find the lag with the highest autocorrelation in [min_lag, max_lag].
fn autocorrelation_peak(signal: &[f32], min_lag: usize, max_lag: usize) -> usize {
    let n = signal.len();
    let mut best_lag = min_lag;
    let mut best_score = f32::NEG_INFINITY;

    for lag in min_lag..=max_lag.min(n / 2) {
        let mut score = 0.0f32;
        for i in 0..n - lag {
            score += signal[i] * signal[i + lag];
        }
        if score > best_score {
            best_score = score;
            best_lag = lag;
        }
    }

    best_lag
}

/// Dynamic programming beat tracker.
///
/// Follows the estimated period `beat_lag` while maximising onset strength.
/// The penalty for deviating from the expected period grows quadratically.
fn dp_beat_track(onset: &[f32], beat_lag: usize) -> Vec<usize> {
    let n = onset.len();
    if n == 0 {
        return vec![];
    }

    // score[i] = accumulated score of the best beat path ending at frame i.
    let mut score = vec![f32::NEG_INFINITY; n];
    let mut prev = vec![0usize; n];

    // Bootstrap: first beat can be anywhere in the first 2 periods.
    for i in 0..(beat_lag * 2).min(n) {
        score[i] = onset[i];
        prev[i] = i;
    }

    // Lambda controls the penalty for period deviation.
    // Higher = stricter tempo adherence.
    let lambda = 100.0f32;

    for i in beat_lag..n {
        let lo = i.saturating_sub(beat_lag * 2);
        let hi = i; // exclusive
        let mut best_score = f32::NEG_INFINITY;
        let mut best_prev = lo;

        for j in lo..hi {
            if score[j] == f32::NEG_INFINITY {
                continue;
            }
            let delta = i as f32 - j as f32;
            let penalty = lambda * ((delta / beat_lag as f32).ln()).powi(2);
            let candidate = score[j] - penalty;
            if candidate > best_score {
                best_score = candidate;
                best_prev = j;
            }
        }

        score[i] = best_score + onset[i];
        prev[i] = best_prev;
    }

    // Back-track from the best final frame.
    let last = (0..n)
        .max_by(|&a, &b| score[a].partial_cmp(&score[b]).unwrap())
        .unwrap_or(0);

    let mut beats = Vec::new();
    let mut cur = last;
    loop {
        beats.push(cur);
        let p = prev[cur];
        if p == cur {
            break;
        }
        cur = p;
    }
    beats.reverse();
    // Deduplicate (DP sometimes produces consecutive identical frames).
    beats.dedup();
    beats
}

/// RMS energy per frame.
fn compute_rms_frames(samples: &[f32], hop: usize) -> Vec<f32> {
    samples
        .chunks(hop)
        .map(|chunk| {
            let sum_sq: f32 = chunk.iter().map(|&s| s * s).sum();
            (sum_sq / chunk.len() as f32).sqrt()
        })
        .collect()
}
