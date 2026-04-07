use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

fn default_library() -> PathBuf {
    home_dir().join(".local/share/luminode-sync")
}

fn default_plan() -> PathBuf {
    PathBuf::from("/etc/luminode-sync/plans/default.toml")
}

fn home_dir() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/tmp"))
}

fn default_led_count() -> usize { 259 }
fn default_poll_secs() -> f32 { 3.0 }

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_library")]
    pub library: PathBuf,

    #[serde(default = "default_plan")]
    pub plan: PathBuf,

    pub spotify: SpotifyConfig,

    #[serde(default)]
    pub leds: LedsConfig,

    #[serde(default)]
    pub sync: SyncConfig,
}

#[derive(Debug, Deserialize)]
pub struct SpotifyConfig {
    pub client_id: String,
    pub token_file: PathBuf,
}

#[derive(Debug, Deserialize)]
pub struct LedsConfig {
    #[serde(default = "default_led_count")]
    pub count: usize,
}

impl Default for LedsConfig {
    fn default() -> Self {
        LedsConfig { count: default_led_count() }
    }
}

#[derive(Debug, Deserialize)]
pub struct SyncConfig {
    /// How often to poll Spotify (seconds).
    #[serde(default = "default_poll_secs")]
    pub poll_interval_secs: f32,

    /// Target render frame rate.
    #[serde(default)]
    pub fps: u32,
}

impl Default for SyncConfig {
    fn default() -> Self {
        SyncConfig { poll_interval_secs: default_poll_secs(), fps: 60 }
    }
}

impl Config {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let s = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("reading config from {}", path.as_ref().display()))?;
        toml::from_str(&s).context("parsing config TOML")
    }
}
