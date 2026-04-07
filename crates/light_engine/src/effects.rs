/// Effect implementations.
///
/// An effect takes an EffectContext and writes into a pixel buffer.
/// Effects are composited (base layer + one-shot triggers layered on top).
///
/// Anatomy:
///   • Base effects  — continuous, section-driven.  Run every frame.
///   • Trigger effects — one-shot, triggered by beats/cues.  Decay over time.

use crate::color::{hsv_to_rgb, Rgb};
use crate::scheduler::EffectContext;
use crate::LED_COUNT;

/// A frame of LED colors.
pub type Frame = Vec<Rgb>;

// ─── Base effects ─────────────────────────────────────────────────────────────

/// Render a base effect into `frame`.
pub fn render_base(name: &str, ctx: &EffectContext, palette: &[Rgb], frame: &mut Frame) {
    match name {
        "slow_gradient"    => slow_gradient(ctx, palette, frame),
        "breathe"          => breathe(ctx, palette, frame),
        "slow_chase"       => chase(ctx, palette, frame, 0.3),
        "fast_chase"       => chase(ctx, palette, frame, 0.8),
        "rise"             => rise(ctx, palette, frame),
        "strobe_chase"     => strobe_chase(ctx, palette, frame),
        "fade_out"         => fade_out(ctx, palette, frame),
        _                  => slow_gradient(ctx, palette, frame),
    }
}

/// Scrolling color gradient.  The `speed` field from the plan modulates rate.
fn slow_gradient(ctx: &EffectContext, palette: &[Rgb], frame: &mut Frame) {
    let n = LED_COUNT as f32;
    let offset = ctx.time_secs * 0.1 * ctx.speed;
    let palette_len = palette.len().max(1) as f32;

    for (i, pixel) in frame.iter_mut().enumerate() {
        let t = ((i as f32 / n + offset).rem_euclid(1.0)) * palette_len;
        *pixel = sample_palette(palette, t).scale(ctx.intensity);
    }
}

/// Pulsing breathe: whole strip fades in and out on beat/bar.
fn breathe(ctx: &EffectContext, palette: &[Rgb], frame: &mut Frame) {
    // Breathe on the bar (one full in-out per 4 beats).
    let breathe_phase = ctx.bar_phase;
    // Smooth sine envelope: 0.2 to 1.0 brightness.
    let brightness = 0.2 + 0.8 * ((breathe_phase * std::f32::consts::TAU).sin() * 0.5 + 0.5);
    let color = sample_palette(palette, ctx.section_phase * palette.len() as f32)
        .scale(brightness * ctx.intensity);
    for pixel in frame.iter_mut() {
        *pixel = color;
    }
}

/// Moving color chase across the strip.  `speed_factor` controls velocity.
fn chase(ctx: &EffectContext, palette: &[Rgb], frame: &mut Frame, speed_factor: f32) {
    let n = LED_COUNT as f32;
    // Beat-synced offset: jump discretely on each beat to stay rhythmically locked.
    let offset = ctx.beat_index as f32 * (1.0 / palette.len() as f32) * speed_factor
        + ctx.beat_phase * speed_factor / palette.len() as f32;

    for (i, pixel) in frame.iter_mut().enumerate() {
        let t = (i as f32 / n + offset).rem_euclid(1.0) * palette.len() as f32;
        *pixel = sample_palette(palette, t).scale(ctx.intensity);
    }
}

/// Buildup effect: brightness increases linearly towards 1.0 as section progresses.
fn rise(ctx: &EffectContext, palette: &[Rgb], frame: &mut Frame) {
    let brightness = ctx.section_phase * ctx.intensity;
    // Also add a beat-pulse layer.
    let pulse = 1.0 - ctx.beat_phase * 0.3;
    let n = LED_COUNT as f32;
    let offset = ctx.time_secs * 0.2;
    for (i, pixel) in frame.iter_mut().enumerate() {
        let t = ((i as f32 / n + offset).rem_euclid(1.0)) * palette.len() as f32;
        *pixel = sample_palette(palette, t).scale(brightness * pulse);
    }
}

/// Drop effect: fast strobe + chase hybrid.
fn strobe_chase(ctx: &EffectContext, palette: &[Rgb], frame: &mut Frame) {
    let n = LED_COUNT as f32;
    // 8 Hz strobe tied to beat_phase.
    let strobe_phase = (ctx.beat_phase * 8.0).fract();
    let strobe = if strobe_phase < 0.5 { 1.0f32 } else { 0.4 };
    let offset = ctx.beat_index as f32 * 0.15;

    for (i, pixel) in frame.iter_mut().enumerate() {
        let t = ((i as f32 / n + offset).rem_euclid(1.0)) * palette.len() as f32;
        *pixel = sample_palette(palette, t).scale(strobe * ctx.intensity);
    }
}

/// Outro: fade the whole strip to black over the section.
fn fade_out(ctx: &EffectContext, palette: &[Rgb], frame: &mut Frame) {
    let brightness = (1.0 - ctx.section_phase) * ctx.intensity;
    let color = sample_palette(palette, 0.0).scale(brightness);
    for pixel in frame.iter_mut() {
        *pixel = color;
    }
}

// ─── Trigger effects (one-shot) ───────────────────────────────────────────────

/// Active one-shot trigger layered over the base.
pub struct Trigger {
    pub kind: TriggerKind,
    /// 0.0 = just fired, 1.0 = fully decayed.
    pub progress: f32,
    pub strength: f32,
}

pub enum TriggerKind {
    KickPulse,
    SoftPulse,
    WhiteFlash,
    ColorBurst,
}

impl Trigger {
    /// Composite this trigger over `frame` in place.
    pub fn render(&self, frame: &mut Frame) {
        match self.kind {
            TriggerKind::KickPulse => {
                // Brightness bump that decays exponentially.
                let boost = self.strength * (1.0 - self.progress).powi(2);
                for pixel in frame.iter_mut() {
                    *pixel = pixel.scale(1.0 + boost).add(Rgb::new(
                        (50.0 * boost) as u8,
                        (50.0 * boost) as u8,
                        (50.0 * boost) as u8,
                    ));
                }
            }
            TriggerKind::SoftPulse => {
                let boost = self.strength * (1.0 - self.progress);
                for pixel in frame.iter_mut() {
                    *pixel = pixel.add(Rgb::new(
                        (30.0 * boost) as u8,
                        (30.0 * boost) as u8,
                        (30.0 * boost) as u8,
                    ));
                }
            }
            TriggerKind::WhiteFlash => {
                // Hard flash that decays over `progress`.
                let alpha = (1.0 - self.progress).powi(2) * self.strength;
                let white = Rgb::WHITE.scale(alpha);
                for pixel in frame.iter_mut() {
                    *pixel = pixel.add(white);
                }
            }
            TriggerKind::ColorBurst => {
                let alpha = (1.0 - self.progress).powi(2) * self.strength;
                // Complementary hue of the current pixel.
                for pixel in frame.iter_mut() {
                    let burst = Rgb::new(
                        (pixel.b as f32 * alpha) as u8,
                        (pixel.r as f32 * alpha) as u8,
                        (pixel.g as f32 * alpha) as u8,
                    );
                    *pixel = pixel.add(burst);
                }
            }
        }
    }
}

/// Name → TriggerKind mapping (for plan deserialization).
pub fn trigger_kind_from_name(name: &str) -> Option<TriggerKind> {
    match name {
        "kick_pulse"   => Some(TriggerKind::KickPulse),
        "soft_pulse"   => Some(TriggerKind::SoftPulse),
        "white_flash"  => Some(TriggerKind::WhiteFlash),
        "color_burst"  => Some(TriggerKind::ColorBurst),
        _ => None,
    }
}

// ─── Palette sampling ─────────────────────────────────────────────────────────

/// Sample a color from a palette at position `t` ∈ [0, palette_len].
/// Linearly interpolates between adjacent colors; wraps at the ends.
pub fn sample_palette(palette: &[Rgb], t: f32) -> Rgb {
    if palette.is_empty() {
        return Rgb::WHITE;
    }
    let n = palette.len();
    let t = t.rem_euclid(n as f32);
    let lo = t.floor() as usize % n;
    let hi = (lo + 1) % n;
    palette[lo].lerp(palette[hi], t.fract())
}

/// A EffectKind name exposed for plan matching.
pub enum EffectKind {}

pub struct Effect;
