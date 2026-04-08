/// Light plan — the declarative configuration layer that maps musical
/// structure to lighting behavior.
///
/// The plan is separate from the beatmap. You can change how a song is lit
/// (colors, effects, intensity) by editing the plan without touching the beatmap.
///
/// The plan is a TOML file with three tables:
///   [palette.*]      — named color sets
///   [[section_rule]] — rules matched by SectionKind
///   [[beat_rule]]    — rules triggered on beat or downbeat
///   [[cue_rule]]     — rules triggered on cue markers
///   [fallback]       — effect when no beatmap is available

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::collections::HashMap;
use beatmap_core::SectionKind;
use crate::color::Rgb;

#[derive(Debug, Clone, Deserialize)]
pub struct LightPlan {
    #[serde(default)]
    pub palette: HashMap<String, Palette>,

    #[serde(default)]
    pub section_rule: Vec<SectionRule>,

    #[serde(default)]
    pub beat_rule: Vec<BeatRule>,

    #[serde(default)]
    pub cue_rule: Vec<CueRule>,

    #[serde(default)]
    pub fallback: FallbackConfig,
}

impl LightPlan {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let s = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("reading {}", path.as_ref().display()))?;
        toml::from_str(&s).context("parsing light plan TOML")
    }

    /// Find the section rule for a given SectionKind.
    /// Rules are matched in order; the first match wins.
    pub fn section_effect(&self, kind: &SectionKind) -> Option<&SectionRule> {
        self.section_rule
            .iter()
            .find(|r| r.section.as_deref() == Some(kind.as_str()) || r.section.is_none())
    }

    /// Find all beat rules matching `beat_class`.
    pub fn beat_effects<'a>(&'a self, beat_class: &'a str) -> impl Iterator<Item = &'a BeatRule> {
        self.beat_rule
            .iter()
            .filter(move |r| r.beat_class == beat_class)
    }

    /// Find cue rules matching `cue_type`.
    pub fn cue_effects<'a>(&'a self, cue_type: &'a str) -> impl Iterator<Item = &'a CueRule> {
        self.cue_rule
            .iter()
            .filter(move |r| r.cue == cue_type)
    }

    /// Resolve a palette name to a list of Rgb colors.
    pub fn resolve_palette(&self, name: &str) -> Vec<Rgb> {
        self.palette
            .get(name)
            .map(|p| p.colors())
            .unwrap_or_else(|| vec![Rgb::WHITE])
    }
}

/// A named palette.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Palette {
    /// List of #RRGGBB hex colors.
    pub colors: Vec<String>,
}

impl Palette {
    pub fn colors(&self) -> Vec<Rgb> {
        self.colors
            .iter()
            .filter_map(|s| Rgb::from_hex(s))
            .collect()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SectionRule {
    /// Which section this applies to (None = wildcard / default).
    pub section: Option<String>,
    /// Effect name — matched against EffectKind::name().
    pub effect: String,
    /// Palette name from the [palette] table.
    pub palette: Option<String>,
    /// Intensity multiplier [0, 1].
    #[serde(default = "one")]
    pub intensity: f32,
    /// Speed multiplier [0, 1].
    #[serde(default = "one")]
    pub speed: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BeatRule {
    /// "beat" or "downbeat"
    pub beat_class: String,
    /// One-shot trigger effect name.
    pub trigger: String,
    /// Trigger strength [0, 1].
    #[serde(default = "one")]
    pub strength: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CueRule {
    /// Cue type string: "drop", "impact", "fill", "build", or a custom label.
    pub cue: String,
    pub trigger: String,
    #[serde(default = "hundred")]
    pub duration_ms: u32,
    #[serde(default = "one_u32")]
    pub priority: u32,
    #[serde(default = "one")]
    pub strength: f32,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct FallbackConfig {
    #[serde(default = "default_fallback_effect")]
    pub effect: String,
    #[serde(default)]
    pub palette: Option<String>,
    #[serde(default = "half")]
    pub speed: f32,
}

fn one() -> f32 { 1.0 }
fn one_u32() -> u32 { 1 }
fn half() -> f32 { 0.5 }
fn hundred() -> u32 { 100 }
fn default_fallback_effect() -> String { "slow_gradient".to_owned() }
