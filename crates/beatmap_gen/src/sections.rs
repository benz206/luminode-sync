/// Section classification from energy and onset density.
///
/// The classifier looks at the RMS energy and onset density (beats/sec) in
/// each 8-beat window and compares them to the track's statistics to assign
/// one of the well-known section types.
///
/// This is intentionally simple.  For better results (verse vs chorus
/// discrimination in pop music, etc.) you would need spectral features like
/// MFCC or chroma, which are beyond the scope of this offline tool.  The
/// output is a reasonable starting point that users can manually annotate.

use beatmap_core::{Section, SectionKind};
use crate::analysis::AnalysisResult;
use crate::AudioBuffer;

/// Analyse the track and return a list of sections in beat-index order.
pub fn classify(analysis: &AnalysisResult, _buf: &AudioBuffer) -> Vec<Section> {
    let n_beats = analysis.beat_times_ms.len();
    if n_beats < 4 {
        return vec![Section { start_beat: 0, kind: SectionKind::Unknown, energy: 128 }];
    }

    // Compute per-beat RMS energy normalised to [0, 1].
    let energies: Vec<f32> = (0..n_beats)
        .map(|i| analysis.rms_energy_at_beat(i) / analysis.rms_95th)
        .map(|e| e.clamp(0.0, 1.0))
        .collect();

    // Compute per-beat onset strength normalised to [0, 1].
    let max_onset = analysis
        .beat_onset_strengths
        .iter()
        .cloned()
        .fold(0.0f32, f32::max)
        .max(1e-6);
    let onsets: Vec<f32> = analysis
        .beat_onset_strengths
        .iter()
        .map(|&s| s / max_onset)
        .collect();

    // Segment into blocks of 8 beats and classify each block.
    let block_size = 8;
    let mut sections: Vec<Section> = Vec::new();
    let mut last_kind = SectionKind::Unknown;

    let mut i = 0;
    while i < n_beats {
        let end = (i + block_size).min(n_beats);
        let block_energy: f32 = energies[i..end].iter().sum::<f32>() / (end - i) as f32;
        let block_onset: f32 = onsets[i..end].iter().sum::<f32>() / (end - i) as f32;

        let position_frac = i as f32 / n_beats as f32;
        let kind = classify_block(block_energy, block_onset, position_frac, n_beats, i);

        // Only emit a new section when the kind changes.
        if kind != last_kind {
            let energy_u8 = (block_energy * 255.0) as u8;
            sections.push(Section {
                start_beat: i as u16,
                kind: kind.clone(),
                energy: energy_u8,
            });
            last_kind = kind;
        }

        i += block_size;
    }

    // Always start with beat 0.
    if sections.is_empty() || sections[0].start_beat != 0 {
        let first_energy = energies.first().copied().unwrap_or(0.5);
        sections.insert(0, Section {
            start_beat: 0,
            kind: SectionKind::Intro,
            energy: (first_energy * 255.0) as u8,
        });
    }

    sections
}

fn classify_block(
    energy: f32,
    onset: f32,
    position_frac: f32,
    _n_beats: usize,
    _beat_idx: usize,
) -> SectionKind {
    // Heuristic classification using energy + onset density + position.
    //
    // Thresholds are intentionally coarse.  The generator labels broad strokes;
    // users can refine with the beatmap-cli edit command (future feature).
    let tail_frac = 1.0 - position_frac;
    let is_near_start = position_frac < 0.15;
    let is_near_end = tail_frac < 0.15;

    if is_near_start && energy < 0.35 {
        return SectionKind::Intro;
    }
    if is_near_end && energy < 0.35 {
        return SectionKind::Outro;
    }

    match (energy, onset) {
        (e, o) if e > 0.75 && o > 0.65 => SectionKind::Drop,
        (e, o) if e > 0.55 && o > 0.75 => SectionKind::Chorus,
        (e, o) if e > 0.45 && o > 0.55 => SectionKind::Buildup,
        (e, _) if e < 0.30 => SectionKind::Breakdown,
        (e, _) if e < 0.45 && position_frac > 0.5 => SectionKind::Bridge,
        _ => SectionKind::Verse,
    }
}
