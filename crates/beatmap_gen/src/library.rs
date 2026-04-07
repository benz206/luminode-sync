/// Library management for the beatmap generator.
///
/// The library directory layout:
///
///   ~/.local/share/luminode-sync/
///   ├── index.json          ← LibraryIndex (Spotify ID / ISRC / title → path)
///   └── beatmaps/
///       ├── <source_hash>.beatmap
///       └── ...
///
/// Beatmaps are keyed by the SHA-256 hash of their source audio file,
/// so regeneration is idempotent: if the hash hasn't changed, we skip.

use anyhow::{Context, Result};
use beatmap_core::{Beatmap, LibraryIndex};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct Library {
    root: PathBuf,
    pub index: LibraryIndex,
}

impl Library {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_owned();
        std::fs::create_dir_all(root.join("beatmaps")).context("creating beatmaps dir")?;
        let index_path = root.join("index.json");
        let index = if index_path.exists() {
            LibraryIndex::load(&index_path).context("loading index.json")?
        } else {
            LibraryIndex::default()
        };
        Ok(Library { root, index })
    }

    pub fn save_index(&self) -> Result<()> {
        self.index
            .save(self.root.join("index.json"))
            .context("saving index.json")
    }

    /// Path to the beatmap file for a given source hash.
    pub fn beatmap_path(&self, source_hash: &str) -> PathBuf {
        self.root.join("beatmaps").join(format!("{source_hash}.beatmap"))
    }

    /// Returns true if we already have an up-to-date beatmap for `source_hash`.
    pub fn has(&self, source_hash: &str) -> bool {
        self.beatmap_path(source_hash).exists()
    }

    /// Store a beatmap and register it in the index.
    pub fn store(&mut self, bm: &Beatmap) -> Result<()> {
        let path = self.beatmap_path(&bm.track.source_hash);
        bm.save(&path).context("saving beatmap")?;

        let relative = path
            .strip_prefix(&self.root)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();

        self.index.insert(
            &relative,
            bm.track.spotify_id.as_deref(),
            bm.track.isrc.as_deref(),
            &bm.track.artist,
            &bm.track.title,
            bm.track.duration_ms,
        );

        self.save_index()
    }

    /// Enumerate all supported audio files under `dir`.
    pub fn scan_audio_files(dir: &Path) -> Vec<PathBuf> {
        const SUPPORTED: &[&str] = &["mp3", "flac", "ogg", "m4a", "aac", "wav", "aiff"];
        WalkDir::new(dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter(|e| {
                e.path()
                    .extension()
                    .and_then(|x| x.to_str())
                    .map(|ext| SUPPORTED.contains(&ext.to_lowercase().as_str()))
                    .unwrap_or(false)
            })
            .map(|e| e.into_path())
            .collect()
    }
}
