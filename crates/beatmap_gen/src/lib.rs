pub mod decode;
pub mod analysis;
pub mod sections;
pub mod library;
pub mod ml_beats;
pub mod color;

pub use decode::AudioBuffer;
pub use analysis::{AnalysisResult, BeatTracker};

use anyhow::Result;
use beatmap_core::{
    Beatmap, CueKind, EnergyEnvelope, TimingData, TrackMeta,
    BEATMAP_VERSION,
};
use sha2::{Digest, Sha256};
use std::path::Path;

/// Top-level entry point: decode an audio file and produce a Beatmap.
///
/// This is the CPU-heavy path.  Runs offline only — never on the Pi.
pub fn generate(audio_path: &Path, spotify_id: Option<String>, isrc: Option<String>) -> Result<Beatmap> {
    // ── 1. Hash the source file so the generator can skip unchanged files ──────
    let source_hash = hash_file(audio_path)?;

    // ── 2. Decode to a mono f32 PCM buffer ────────────────────────────────────
    let buf = decode::decode_audio(audio_path)?;
    let duration_ms = ((buf.samples.len() as f64 / buf.sample_rate as f64) * 1000.0) as u32;

    // ── 3. Beat tracking: try ML (madmom) first, fall back to DSP ────────────
    let analysis = match ml_beats::track(audio_path) {
        Ok(ml) => {
            tracing::debug!("ML beat tracker: {:.1} BPM, {} beats", ml.bpm, ml.beats.len());
            analysis::analyze_from_beats(&buf, &ml.beats, &ml.downbeats, ml.bpm)?
        }
        Err(e) => {
            tracing::warn!("ML beat tracker unavailable ({e:#}) — falling back to DSP");
            analysis::analyze(&buf)?
        }
    };

    // ── 4. Classify sections from energy + onset density ──────────────────────
    let section_list = sections::classify(&analysis, &buf);

    // ── 5. Extract cue markers from strong transients ─────────────────────────
    let cues = extract_cues(&analysis);

    // ── 6. Build energy envelope ───────────────────────────────────────────────
    let energy = build_energy_envelope(&analysis, 4);

    // ── 7. Extract metadata from the audio file ────────────────────────────────
    let (title, artist, album) = decode::read_tags(audio_path)
        .unwrap_or_else(|_| {
            let stem = audio_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown")
                .to_owned();
            (stem, "Unknown".into(), None)
        });

    // ── 7b. Extract dominant colour from embedded album art ───────────────────
    let dominant_color = decode::read_cover_art(audio_path)
        .ok()
        .and_then(|art_bytes| color::dominant_color(&art_bytes));

    if let Some(c) = dominant_color {
        tracing::debug!("album art colour: #{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
    } else {
        tracing::debug!("no album art found — dominant_color will be None");
    }

    // ── 8. Assemble the beatmap ────────────────────────────────────────────────
    let timing = TimingData::from_beat_positions(
        &analysis.beat_times_ms,
        &analysis.downbeat_indices,
        analysis.time_sig,
    );

    let track = TrackMeta {
        title,
        artist,
        album,
        duration_ms,
        spotify_id,
        isrc,
        source_hash,
        detected_bpm: analysis.bpm,
        dominant_color,
    };

    let bm = Beatmap {
        version: BEATMAP_VERSION,
        track,
        timing,
        sections: section_list,
        cues,
        energy,
        calibration_ms: 0,
    };

    bm.validate().map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(bm)
}

fn hash_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    let hash = Sha256::digest(&bytes);
    Ok(hex::encode(hash))
}

fn extract_cues(analysis: &AnalysisResult) -> Vec<beatmap_core::Cue> {
    // A cue is a beat where the onset strength exceeds a high threshold AND
    // is a local maximum in a ±4-beat window.  This catches drops and impacts.
    let threshold = analysis.onset_strength_mean + 2.5 * analysis.onset_strength_std;
    let mut cues = Vec::new();

    for (i, &beat_ms) in analysis.beat_times_ms.iter().enumerate() {
        let strength = analysis.onset_strength_at_beat(i);
        if strength < threshold {
            continue;
        }
        // Local maximum check in ±4-beat window.
        let lo = i.saturating_sub(4);
        let hi = (i + 4).min(analysis.beat_times_ms.len());
        let is_local_max = (lo..hi).all(|j| {
            j == i || analysis.onset_strength_at_beat(j) <= strength
        });
        if is_local_max {
            let kind = if strength > analysis.onset_strength_mean + 4.0 * analysis.onset_strength_std {
                CueKind::Drop
            } else {
                CueKind::Impact
            };
            cues.push(beatmap_core::Cue { position_ms: beat_ms, kind });
        }
    }
    cues
}

fn build_energy_envelope(analysis: &AnalysisResult, sample_every_n_beats: u8) -> EnergyEnvelope {
    let n = sample_every_n_beats as usize;
    let beat_count = analysis.beat_times_ms.len();
    let num_samples = (beat_count + n - 1) / n;

    let mut values = Vec::with_capacity(num_samples);
    for i in 0..num_samples {
        let beat_idx = i * n;
        let energy = analysis.rms_energy_at_beat(beat_idx);
        // Normalise against the track's 95th-percentile energy.
        let norm = (energy / analysis.rms_95th * 255.0).clamp(0.0, 255.0) as u8;
        values.push(norm);
    }

    EnergyEnvelope { sample_every_n_beats, values }
}
