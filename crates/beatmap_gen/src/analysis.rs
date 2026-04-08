/// Beat detection via spectral flux onset detection + autocorrelation beat tracking.
///
/// Two entry points:
///   • `analyze(buf)`                    — full DSP pipeline (onset → tempo → beats)
///   • `analyze_from_beats(buf, beats)`  — spectral features only; beats supplied externally
///
/// The second path is used when the ML beat tracker (madmom) is available.
/// Both paths return the same `AnalysisResult` so the rest of the pipeline is unchanged.

use anyhow::Result;
use realfft::RealFftPlanner;
use std::f32::consts::PI;

use crate::AudioBuffer;

const HOP_SIZE: usize = 512;
const FFT_SIZE: usize = 2048;
const BPM_MIN: f32 = 60.0;
const BPM_MAX: f32 = 200.0;
const SMOOTH_SIGMA: f32 = 2.5;

pub struct AnalysisResult {
    pub beat_times_ms: Vec<u32>,
    pub downbeat_indices: Vec<usize>,
    pub bpm: f32,
    pub time_sig: u8,
    pub beat_onset_strengths: Vec<f32>,
    pub onset_strength_mean: f32,
    pub onset_strength_std: f32,
    pub beat_rms: Vec<f32>,
    pub rms_95th: f32,

    #[allow(dead_code)]
    pub(crate) onset_strength: Vec<f32>,
    #[allow(dead_code)]
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

    pub fn samples_to_ms(&self, sample_idx: usize) -> u32 {
        (sample_idx as f64 / self.sample_rate as f64 * 1000.0) as u32
    }
}

pub struct BeatTracker;

// ─── Spectral features (shared by both entry points) ─────────────────────────

struct SpectralFeatures {
    onset_strength: Vec<f32>,   // Gaussian-smoothed spectral flux
    rms_frames: Vec<f32>,
    mean: f32,
    std: f32,
}

fn compute_spectral(buf: &AudioBuffer) -> SpectralFeatures {
    let frames = stft_magnitude(&buf.samples, FFT_SIZE, HOP_SIZE);
    let n_frames = frames.len();

    let mut onset_raw = vec![0.0f32; n_frames];
    for i in 1..n_frames {
        let mut flux = 0.0f32;
        for k in 0..frames[i].len() {
            let diff = frames[i][k] - frames[i - 1][k];
            if diff > 0.0 {
                flux += diff;
            }
        }
        onset_raw[i] = flux;
    }

    let onset_strength = smooth_gaussian(&onset_raw, SMOOTH_SIGMA);
    let rms_frames = compute_rms_frames(&buf.samples, HOP_SIZE);

    let mean = onset_strength.iter().sum::<f32>() / onset_strength.len().max(1) as f32;
    let variance = onset_strength.iter().map(|&x| (x - mean).powi(2)).sum::<f32>()
        / onset_strength.len().max(1) as f32;
    let std = variance.sqrt();

    SpectralFeatures { onset_strength, rms_frames, mean, std }
}

fn build_result(
    beat_times_ms: Vec<u32>,
    downbeat_indices: Vec<usize>,
    bpm: f32,
    time_sig: u8,
    spectral: SpectralFeatures,
    buf: &AudioBuffer,
) -> AnalysisResult {
    let sr = buf.sample_rate as f32;

    let beat_onset_strengths: Vec<f32> = beat_times_ms
        .iter()
        .map(|&ms| {
            let frame = (ms as f32 / 1000.0 * sr / HOP_SIZE as f32) as usize;
            spectral.onset_strength.get(frame).copied().unwrap_or(0.0)
        })
        .collect();

    let beat_rms: Vec<f32> = beat_times_ms
        .iter()
        .map(|&ms| {
            let frame = (ms as f32 / 1000.0 * sr / HOP_SIZE as f32) as usize;
            spectral.rms_frames.get(frame).copied().unwrap_or(0.0)
        })
        .collect();

    let mut sorted_rms = beat_rms.clone();
    sorted_rms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx_95 = (sorted_rms.len() as f32 * 0.95) as usize;
    let rms_95th = sorted_rms.get(idx_95).copied().unwrap_or(1.0).max(1e-6);

    AnalysisResult {
        beat_times_ms,
        downbeat_indices,
        bpm,
        time_sig,
        beat_onset_strengths,
        onset_strength_mean: spectral.mean,
        onset_strength_std: spectral.std,
        beat_rms,
        rms_95th,
        onset_strength: spectral.onset_strength,
        rms_frames: spectral.rms_frames,
        sample_rate: buf.sample_rate,
    }
}

// ─── Entry point 1: full DSP pipeline ────────────────────────────────────────

pub fn analyze(buf: &AudioBuffer) -> Result<AnalysisResult> {
    let sr = buf.sample_rate as f32;
    let spectral = compute_spectral(buf);

    let frames_per_sec = sr / HOP_SIZE as f32;
    let min_lag = (frames_per_sec * 60.0 / BPM_MAX).round() as usize;
    let max_lag = (frames_per_sec * 60.0 / BPM_MIN).round() as usize;

    let (bpm_lag, bpm_refined) =
        select_best_tempo_lag(&spectral.onset_strength, min_lag, max_lag, frames_per_sec);

    let beat_frames = dp_beat_track(&spectral.onset_strength, bpm_lag);

    let beat_times_ms: Vec<u32> = beat_frames
        .iter()
        .map(|&f| (f as f32 * HOP_SIZE as f32 / sr * 1000.0) as u32)
        .collect();

    let time_sig = 4u8;
    let downbeat_indices: Vec<usize> = (0..beat_times_ms.len()).step_by(time_sig as usize).collect();

    Ok(build_result(beat_times_ms, downbeat_indices, bpm_refined, time_sig, spectral, buf))
}

// ─── Entry point 2: beats from ML, spectral features from DSP ────────────────

/// Build an `AnalysisResult` using externally supplied beat positions (e.g. from madmom).
/// Still runs the STFT to get onset strength and RMS for sections / energy / cues.
pub fn analyze_from_beats(
    buf: &AudioBuffer,
    beat_secs: &[f64],
    downbeat_secs: &[f64],
    bpm: f64,
) -> Result<AnalysisResult> {
    let spectral = compute_spectral(buf);

    let beat_times_ms: Vec<u32> = beat_secs.iter().map(|&s| (s * 1000.0) as u32).collect();

    // Map each downbeat timestamp to the index of the nearest beat.
    let downbeat_indices: Vec<usize> = downbeat_secs
        .iter()
        .filter_map(|&ds| {
            let ds_ms = (ds * 1000.0) as i64;
            beat_times_ms
                .iter()
                .enumerate()
                .min_by_key(|&(_, &bms)| (bms as i64 - ds_ms).abs())
                .map(|(i, _)| i)
        })
        .collect();

    // Infer time signature from median beats-per-bar.
    let time_sig = infer_time_sig(&downbeat_indices);

    Ok(build_result(beat_times_ms, downbeat_indices, bpm as f32, time_sig, spectral, buf))
}

fn infer_time_sig(downbeat_indices: &[usize]) -> u8 {
    if downbeat_indices.len() < 2 {
        return 4;
    }
    let mut intervals: Vec<usize> = downbeat_indices.windows(2).map(|w| w[1] - w[0]).collect();
    intervals.sort_unstable();
    let median = intervals[intervals.len() / 2];
    match median {
        3 => 3,
        _ => 4,
    }
}

// ─── Tempo selection (used by the DSP path only) ──────────────────────────────

fn select_best_tempo_lag(
    onset: &[f32],
    min_lag: usize,
    max_lag: usize,
    frames_per_sec: f32,
) -> (usize, f32) {
    let ac = autocorrelation_curve(onset, min_lag, max_lag);

    let mut candidates: Vec<usize> = Vec::new();
    for lag in (min_lag + 1)..max_lag.min(onset.len().saturating_sub(1)) {
        if ac[lag] > ac[lag - 1] && ac[lag] >= ac[lag + 1] {
            candidates.push(lag);
        }
    }
    if let Some(global) = (min_lag..=max_lag.min(onset.len() / 2))
        .max_by(|&a, &b| ac[a].partial_cmp(&ac[b]).unwrap_or(std::cmp::Ordering::Equal))
    {
        candidates.push(global);
    }

    let n_orig = candidates.len();
    for i in 0..n_orig {
        let lag = candidates[i];
        let half = lag / 2;
        let double = lag * 2;
        if half >= min_lag && half <= max_lag { candidates.push(half); }
        if double >= min_lag && double <= max_lag { candidates.push(double); }
    }
    candidates.sort_unstable();
    candidates.dedup();

    let global_mean = onset.iter().sum::<f32>() / onset.len().max(1) as f32;
    let global_mean = global_mean.max(1e-10);

    let mut best_lag = candidates.first().copied().unwrap_or(min_lag);
    let mut best_score = f32::NEG_INFINITY;
    for lag in candidates {
        let score = onset_grid_score(onset, lag, global_mean);
        if score > best_score {
            best_score = score;
            best_lag = lag;
        }
    }

    let refined_lag = parabolic_peak(&ac, best_lag);
    let refined_bpm = 60.0 * frames_per_sec / refined_lag;
    (best_lag, refined_bpm)
}

fn autocorrelation_curve(signal: &[f32], min_lag: usize, max_lag: usize) -> Vec<f32> {
    let n = signal.len();
    let mut ac = vec![0.0f32; max_lag + 1];
    for lag in min_lag..=max_lag.min(n / 2) {
        let mut score = 0.0f32;
        for i in 0..n - lag {
            score += signal[i] * signal[i + lag];
        }
        ac[lag] = score;
    }
    ac
}

fn onset_grid_score(onset: &[f32], lag: usize, global_mean: f32) -> f32 {
    if lag == 0 { return f32::NEG_INFINITY; }
    let n = onset.len();
    let mut best_mean = 0.0f32;
    for start in 0..lag.min(n) {
        let mut sum = 0.0f32;
        let mut count = 0usize;
        let mut pos = start;
        while pos < n {
            sum += onset[pos];
            count += 1;
            pos += lag;
        }
        if count > 0 {
            let m = sum / count as f32;
            if m > best_mean { best_mean = m; }
        }
    }
    best_mean / global_mean
}

fn parabolic_peak(ac: &[f32], peak: usize) -> f32 {
    if peak == 0 || peak + 1 >= ac.len() { return peak as f32; }
    let y0 = ac[peak - 1];
    let y1 = ac[peak];
    let y2 = ac[peak + 1];
    let denom = 2.0 * y1 - y0 - y2;
    if denom.abs() < 1e-10 { return peak as f32; }
    peak as f32 + 0.5 * (y0 - y2) / denom
}

// ─── DSP helpers ──────────────────────────────────────────────────────────────

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
            .iter().zip(&hann).map(|(&s, &w)| s * w).collect();
        let mut output = fft.make_output_vec();
        fft.process(&mut input, &mut output).ok();
        frames.push(output.iter().take(half_fft).map(|c| c.norm()).collect());
        pos += hop;
    }
    frames
}

fn smooth_gaussian(data: &[f32], sigma: f32) -> Vec<f32> {
    let radius = (3.0 * sigma).ceil() as usize;
    let kernel: Vec<f32> = (0..=2 * radius)
        .map(|i| {
            let x = i as f32 - radius as f32;
            (-x * x / (2.0 * sigma * sigma)).exp()
        })
        .collect();

    let n = data.len();
    let mut out = vec![0.0f32; n];
    for i in 0..n {
        let mut sum = 0.0f32;
        let mut weight = 0.0f32;
        for (k, &w) in kernel.iter().enumerate() {
            let j = i as isize + k as isize - radius as isize;
            if j >= 0 && (j as usize) < n {
                sum += data[j as usize] * w;
                weight += w;
            }
        }
        out[i] = sum / weight.max(1e-10);
    }
    out
}

fn dp_beat_track(onset: &[f32], beat_lag: usize) -> Vec<usize> {
    let n = onset.len();
    if n == 0 { return vec![]; }

    let mut score = vec![f32::NEG_INFINITY; n];
    let mut prev = vec![0usize; n];

    for i in 0..(beat_lag * 2).min(n) {
        score[i] = onset[i];
        prev[i] = i;
    }

    let lambda = 100.0f32;
    for i in beat_lag..n {
        let lo = i.saturating_sub(beat_lag * 2);
        let mut best_score = f32::NEG_INFINITY;
        let mut best_prev = lo;
        for j in lo..i {
            if score[j] == f32::NEG_INFINITY { continue; }
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

    let last = (0..n).max_by(|&a, &b| score[a].partial_cmp(&score[b]).unwrap()).unwrap_or(0);
    let mut beats = Vec::new();
    let mut cur = last;
    loop {
        beats.push(cur);
        let p = prev[cur];
        if p == cur { break; }
        cur = p;
    }
    beats.reverse();
    beats.dedup();
    beats
}

fn compute_rms_frames(samples: &[f32], hop: usize) -> Vec<f32> {
    samples.chunks(hop).map(|chunk| {
        let sum_sq: f32 = chunk.iter().map(|&s| s * s).sum();
        (sum_sq / chunk.len() as f32).sqrt()
    }).collect()
}
