/// Library index — a simple flat file that maps Spotify IDs, ISRCs, and
/// artist/title/duration to beatmap filenames.
///
/// Stored as JSON so it can be hand-edited and diffed in git.
/// The runtime loads this once at startup and caches it in memory.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LibraryIndex {
    /// spotify_id → relative beatmap path
    pub by_spotify_id: HashMap<String, String>,

    /// isrc → relative beatmap path
    pub by_isrc: HashMap<String, String>,

    /// "artist\0title\0duration_bucket" → relative beatmap path
    /// duration_bucket = duration_ms / 5000 (5-second buckets tolerate remasters)
    pub by_title: HashMap<String, String>,
}

impl LibraryIndex {
    pub fn load(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let s = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&s).unwrap_or_default())
    }

    pub fn save(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let s = serde_json::to_string_pretty(self).unwrap();
        std::fs::write(path, s)
    }

    /// Register a beatmap in the index.
    pub fn insert(
        &mut self,
        relative_path: &str,
        spotify_id: Option<&str>,
        isrc: Option<&str>,
        artist: &str,
        title: &str,
        duration_ms: u32,
    ) {
        if let Some(id) = spotify_id {
            self.by_spotify_id.insert(id.to_owned(), relative_path.to_owned());
        }
        if let Some(id) = isrc {
            self.by_isrc.insert(id.to_owned(), relative_path.to_owned());
        }
        let key = title_key(artist, title, duration_ms);
        self.by_title.insert(key, relative_path.to_owned());
    }

    /// Look up a beatmap path for the given track metadata.
    /// Returns `(path, confidence)` where confidence describes how strong the match is.
    pub fn lookup(&self, query: &TrackQuery) -> Option<(PathBuf, MatchConfidence)> {
        // 1. Spotify ID — definitive
        if let Some(id) = &query.spotify_id {
            if let Some(p) = self.by_spotify_id.get(id) {
                return Some((PathBuf::from(p), MatchConfidence::SpotifyId));
            }
        }

        // 2. ISRC — stable across re-releases
        if let Some(isrc) = &query.isrc {
            if let Some(p) = self.by_isrc.get(isrc) {
                return Some((PathBuf::from(p), MatchConfidence::Isrc));
            }
        }

        // 3. Artist + title + duration bucket — fuzzy fallback
        let key = title_key(&query.artist, &query.title, query.duration_ms);
        if let Some(p) = self.by_title.get(&key) {
            return Some((PathBuf::from(p), MatchConfidence::TitleMatch));
        }

        None
    }
}

fn title_key(artist: &str, title: &str, duration_ms: u32) -> String {
    let bucket = duration_ms / 5000;
    format!(
        "{}\0{}\0{}",
        artist.to_lowercase().trim(),
        title.to_lowercase().trim(),
        bucket
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchConfidence {
    /// Matched by Spotify track ID — exact.
    SpotifyId,
    /// Matched by ISRC — exact, stable across re-releases.
    Isrc,
    /// Matched by artist/title/duration bucket — approximate.
    TitleMatch,
}

/// Query parameters for a library lookup.
#[derive(Debug, Clone)]
pub struct TrackQuery {
    pub spotify_id: Option<String>,
    pub isrc: Option<String>,
    pub artist: String,
    pub title: String,
    pub duration_ms: u32,
}
