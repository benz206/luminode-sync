/// ML beat tracking via madmom (Python subprocess).
///
/// Shells out to `scripts/beat_tracker.py` which uses madmom's DBN models.
/// Returns beat and downbeat timestamps in seconds.
///
/// If Python or madmom is unavailable this returns an error and the caller
/// should fall back to the DSP beat tracker in `analysis`.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize)]
pub struct MlBeats {
    /// Beat timestamps in seconds.
    pub beats: Vec<f64>,
    /// Downbeat (bar-start) timestamps in seconds.
    pub downbeats: Vec<f64>,
    /// Median BPM.
    pub bpm: f64,
}

/// Run the ML beat tracker on `audio_path`.
///
/// Requires Python 3 with madmom installed (`pip install madmom`).
/// The `scripts/beat_tracker.py` script is searched for by walking up from
/// the current working directory — always run from the project root.
pub fn track(audio_path: &Path) -> Result<MlBeats> {
    let script = find_script().context(
        "scripts/beat_tracker.py not found — run from the project root",
    )?;

    let out = std::process::Command::new("python3")
        .arg(&script)
        .arg(audio_path)
        .output()
        .context("failed to spawn python3 — is Python 3 installed?")?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("{}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.trim()).context("parsing beat tracker JSON output")
}

/// Walk up from cwd looking for `scripts/beat_tracker.py`.
fn find_script() -> Result<std::path::PathBuf> {
    let mut dir = std::env::current_dir()?;
    for _ in 0..6 {
        let candidate = dir.join("scripts").join("beat_tracker.py");
        if candidate.exists() {
            return Ok(candidate);
        }
        if !dir.pop() {
            break;
        }
    }
    anyhow::bail!("not found after searching up from {}", std::env::current_dir()?.display())
}
