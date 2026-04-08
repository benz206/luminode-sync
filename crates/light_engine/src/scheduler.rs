/// Effect scheduler — resolves the current playback position and beatmap into
/// an EffectContext, then renders a frame.
///
/// Data flow:
///   PlaybackEstimate + Beatmap + LightPlan → EffectContext → Frame → pi_output

use std::time::{Duration, Instant};
use beatmap_core::{Beatmap, SectionKind};
use crate::color::Rgb;
use crate::effects::{
    self, Frame, Trigger, trigger_kind_from_name,
};
use crate::plan::LightPlan;
use crate::LED_COUNT;

/// Everything a render effect needs to know about the current musical moment.
#[derive(Debug, Clone)]
pub struct EffectContext {
    /// Beat index (how many beats since the song started).
    pub beat_index: usize,
    /// Phase within the current beat [0, 1).
    pub beat_phase: f32,
    /// Phase within the current bar [0, 1).
    pub bar_phase: f32,
    /// Phase within the current section [0, 1).
    pub section_phase: f32,
    /// Section type.
    pub section: SectionKind,
    /// Normalised energy [0, 1] from the energy envelope.
    pub energy: f32,
    /// Elapsed time since playback started (for time-based animations).
    pub time_secs: f32,
    /// From the active section rule.
    pub intensity: f32,
    pub speed: f32,
}

impl EffectContext {
    /// Dummy context for the fallback (no-beatmap) path.
    pub fn idle(time_secs: f32) -> Self {
        EffectContext {
            beat_index: 0,
            beat_phase: (time_secs * 2.0).fract(),
            bar_phase: (time_secs * 0.5).fract(),
            section_phase: 0.0,
            section: SectionKind::Unknown,
            energy: 0.5,
            time_secs,
            intensity: 0.5,
            speed: 0.3,
        }
    }
}

pub struct RenderFrame {
    pub pixels: Frame,
}

/// The main scheduler — keeps state between frames.
pub struct EffectScheduler {
    /// Active one-shot triggers.
    triggers: Vec<ActiveTrigger>,
    /// Monotonic start time (used for time_secs derivation).
    started: Instant,
}

struct ActiveTrigger {
    trigger: Trigger,
    duration: Duration,
    fired_at: Instant,
}

impl EffectScheduler {
    pub fn new() -> Self {
        EffectScheduler {
            triggers: Vec::new(),
            started: Instant::now(),
        }
    }

    /// Build an EffectContext from the current playback position and beatmap.
    pub fn resolve_context(
        &self,
        position_ms: u32,
        beatmap: &Beatmap,
        plan: &LightPlan,
    ) -> EffectContext {
        // Apply calibration offset.
        let adjusted_ms = (position_ms as i64 + beatmap.calibration_ms as i64).max(0) as u32;

        let beat_ctx = beatmap.timing.beat_at_position(adjusted_ms);
        let section = beatmap.section_at(adjusted_ms);

        let (section_kind, section_start_beat) = section
            .map(|s| (s.kind.clone(), s.start_beat as usize))
            .unwrap_or((SectionKind::Unknown, 0));

        // Section phase: how far through the current section are we?
        // Find the next section to know when this one ends.
        let section_end_beat = beatmap
            .sections
            .iter()
            .find(|s| s.start_beat as usize > beat_ctx.index)
            .map(|s| s.start_beat as usize)
            .unwrap_or(beatmap.timing.beat_count());
        let section_length = (section_end_beat - section_start_beat).max(1) as f32;
        let section_phase =
            ((beat_ctx.index - section_start_beat) as f32 + beat_ctx.phase) / section_length;

        let energy = beatmap.energy_at(adjusted_ms);

        // Look up intensity/speed from the plan.
        let (intensity, speed) = plan
            .section_effect(&section_kind)
            .map(|r| (r.intensity * energy.max(0.3), r.speed))
            .unwrap_or((energy.max(0.3), 1.0));

        EffectContext {
            beat_index: beat_ctx.index,
            beat_phase: beat_ctx.phase,
            bar_phase: beat_ctx.bar_phase,
            section_phase: section_phase.clamp(0.0, 1.0),
            section: section_kind,
            energy,
            time_secs: self.started.elapsed().as_secs_f32(),
            intensity,
            speed,
        }
    }

    /// Fire beat-class triggers.
    pub fn on_beat(&mut self, is_downbeat: bool, plan: &LightPlan) {
        let class = if is_downbeat { "downbeat" } else { "beat" };
        for rule in plan.beat_effects(class) {
            if let Some(kind) = trigger_kind_from_name(&rule.trigger) {
                self.triggers.push(ActiveTrigger {
                    trigger: Trigger {
                        kind,
                        progress: 0.0,
                        strength: rule.strength,
                    },
                    duration: Duration::from_millis(120),
                    fired_at: Instant::now(),
                });
            }
        }
    }

    /// Fire cue triggers.
    pub fn on_cue(&mut self, cue_type: &str, plan: &LightPlan) {
        for rule in plan.cue_effects(cue_type) {
            if let Some(kind) = trigger_kind_from_name(&rule.trigger) {
                self.triggers.push(ActiveTrigger {
                    trigger: Trigger {
                        kind,
                        progress: 0.0,
                        strength: rule.strength,
                    },
                    duration: Duration::from_millis(rule.duration_ms as u64),
                    fired_at: Instant::now(),
                });
            }
        }
    }

    /// Render one frame.  Returns a Vec<Rgb> of length LED_COUNT.
    pub fn render(
        &mut self,
        ctx: &EffectContext,
        beatmap_opt: Option<&Beatmap>,
        plan: &LightPlan,
    ) -> Vec<Rgb> {
        let mut frame = vec![Rgb::BLACK; LED_COUNT];

        // ── Base effect ──────────────────────────────────────────────────────
        let (effect_name, palette_name) = if let Some(_bm) = beatmap_opt {
            let rule = plan.section_effect(&ctx.section);
            let effect = rule.map(|r| r.effect.as_str()).unwrap_or("slow_gradient");
            let palette = rule
                .and_then(|r| r.palette.as_deref())
                .unwrap_or("cool");
            (effect.to_owned(), palette.to_owned())
        } else {
            (plan.fallback.effect.clone(),
             plan.fallback.palette.clone().unwrap_or_else(|| "cool".to_owned()))
        };

        let palette = plan.resolve_palette(&palette_name);
        effects::render_base(&effect_name, ctx, &palette, &mut frame);

        // ── Trigger layer ─────────────────────────────────────────────────────
        // Update progress and remove expired triggers.
        self.triggers.retain_mut(|at| {
            let elapsed = at.fired_at.elapsed();
            at.trigger.progress = (elapsed.as_secs_f32() / at.duration.as_secs_f32()).min(1.0);
            at.trigger.progress < 1.0
        });

        for at in &self.triggers {
            at.trigger.render(&mut frame);
        }

        frame
    }
}

