/**
 * TypeScript port of crates/light_engine.
 * Implements beat timing, effect rendering, and trigger compositing.
 */

import type {
  Beatmap,
  BeatContext,
  Cue,
  CueKind,
  EffectContext,
  Rgb,
  SectionKind,
  TimingData,
} from "./types";

export const LED_COUNT = 259;

// ─── Color helpers ────────────────────────────────────────────────────────────

function lerpU8(a: number, b: number, t: number): number {
  return Math.round(a + (b - a) * Math.max(0, Math.min(1, t)));
}

export function rgbLerp(a: Rgb, b: Rgb, t: number): Rgb {
  const tc = Math.max(0, Math.min(1, t));
  return {
    r: lerpU8(a.r, b.r, tc),
    g: lerpU8(a.g, b.g, tc),
    b: lerpU8(a.b, b.b, tc),
  };
}

export function rgbScale(c: Rgb, factor: number): Rgb {
  const f = Math.max(0, Math.min(1, factor));
  return { r: Math.round(c.r * f), g: Math.round(c.g * f), b: Math.round(c.b * f) };
}

export function rgbAdd(a: Rgb, b: Rgb): Rgb {
  return {
    r: Math.min(255, a.r + b.r),
    g: Math.min(255, a.g + b.g),
    b: Math.min(255, a.b + b.b),
  };
}

export function rgbScale2(c: Rgb, factor: number): Rgb {
  const f = Math.max(0, factor); // allow > 1 for brightness boost
  return {
    r: Math.min(255, Math.round(c.r * f)),
    g: Math.min(255, Math.round(c.g * f)),
    b: Math.min(255, Math.round(c.b * f)),
  };
}

export function hexToRgb(hex: string): Rgb {
  const s = hex.replace("#", "");
  return {
    r: parseInt(s.slice(0, 2), 16),
    g: parseInt(s.slice(2, 4), 16),
    b: parseInt(s.slice(4, 6), 16),
  };
}

export function rgbToCss(c: Rgb): string {
  return `rgb(${c.r},${c.g},${c.b})`;
}

// ─── HSV → RGB ────────────────────────────────────────────────────────────────

export function hsvToRgb(h: number, s: number, v: number): Rgb {
  const c = v * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = v - c;
  let r = 0, g = 0, b = 0;
  if      (h < 60)  { r = c; g = x; b = 0; }
  else if (h < 120) { r = x; g = c; b = 0; }
  else if (h < 180) { r = 0; g = c; b = x; }
  else if (h < 240) { r = 0; g = x; b = c; }
  else if (h < 300) { r = x; g = 0; b = c; }
  else              { r = c; g = 0; b = x; }
  return {
    r: Math.round((r + m) * 255),
    g: Math.round((g + m) * 255),
    b: Math.round((b + m) * 255),
  };
}

/**
 * Build a 12-stop full-spectrum rainbow palette.
 * `offset` (0–1) slowly rotates the hue over time so the strip cycles.
 */
export function buildRainbowPalette(offset = 0): Rgb[] {
  const stops = 12;
  return Array.from({ length: stops }, (_, i) => {
    const h = (((i / stops) + offset) % 1) * 360;
    return hsvToRgb(h, 1, 1);
  });
}

// ─── Palettes (from config/plans/default.toml) ────────────────────────────────

const PALETTE_DEFS: Record<string, string[]> = {
  cool:     ["#0033ff", "#00ccff", "#6600ff"],
  warm:     ["#ff6600", "#ff3300", "#ffcc00"],
  hot:      ["#ffffff", "#ff0000", "#ff6600"],
  electric: ["#00ff99", "#0066ff", "#cc00ff"],
  amber:    ["#ff8800", "#ffcc00", "#ff4400"],
  ice:      ["#aaeeff", "#ffffff", "#0044ff"],
};

export const PALETTES: Record<string, Rgb[]> = Object.fromEntries(
  Object.entries(PALETTE_DEFS).map(([k, v]) => [k, v.map(hexToRgb)])
);

/**
 * Build a 3-stop palette from a single dominant colour extracted from album art.
 *   [0] full colour          – main LED hue
 *   [1] brighter/whiter      – highlight on beat peak
 *   [2] darker/deeper        – trough between beats
 *
 * This palette is used to override all section palettes when a dominant_color
 * is available in the beatmap, so the strip always matches the album artwork.
 * The same [r,g,b] triple is shipped in the beatmap for the ESP32 to use.
 */
export function buildAlbumPalette(color: [number, number, number]): Rgb[] {
  const base: Rgb = { r: color[0], g: color[1], b: color[2] };
  // Brighter stop: lerp toward white
  const bright = rgbLerp(base, { r: 255, g: 255, b: 255 }, 0.45);
  // Deeper stop: scale down and add a slight hue shift (channel rotate)
  const deep: Rgb = {
    r: Math.round(base.r * 0.5 + base.b * 0.15),
    g: Math.round(base.g * 0.5 + base.r * 0.15),
    b: Math.round(base.b * 0.5 + base.g * 0.15),
  };
  return [base, bright, deep];
}

export function samplePalette(palette: Rgb[], t: number): Rgb {
  if (palette.length === 0) return { r: 255, g: 255, b: 255 };
  const n = palette.length;
  const tw = ((t % n) + n) % n;
  const lo = Math.floor(tw) % n;
  const hi = (lo + 1) % n;
  return rgbLerp(palette[lo], palette[hi], tw - Math.floor(tw));
}

// ─── Section + beat rules ─────────────────────────────────────────────────────

interface SectionRule {
  effect: string;
  palette: string;
  intensity: number;
  speed: number;
}

const SECTION_RULES: Record<SectionKind, SectionRule> = {
  intro:     { effect: "breathe",       palette: "cool",     intensity: 0.35, speed: 0.4 },
  verse:     { effect: "slow_chase",    palette: "cool",     intensity: 0.55, speed: 0.5 },
  chorus:    { effect: "fast_chase",    palette: "warm",     intensity: 0.80, speed: 0.8 },
  buildup:   { effect: "rise",          palette: "amber",    intensity: 0.70, speed: 0.7 },
  drop:      { effect: "strobe_chase",  palette: "hot",      intensity: 1.00, speed: 1.0 },
  breakdown: { effect: "breathe",       palette: "ice",      intensity: 0.30, speed: 0.3 },
  bridge:    { effect: "slow_gradient", palette: "electric", intensity: 0.50, speed: 0.5 },
  outro:     { effect: "fade_out",      palette: "cool",     intensity: 0.40, speed: 0.3 },
  unknown:   { effect: "slow_gradient", palette: "cool",     intensity: 0.50, speed: 0.5 },
};

interface BeatRuleEntry {
  trigger: string;
  strength: number;
  duration_ms: number;
}

const BEAT_RULES: Record<string, BeatRuleEntry[]> = {
  downbeat: [{ trigger: "kick_pulse", strength: 0.45, duration_ms: 120 }],
  beat:     [{ trigger: "soft_pulse", strength: 0.12, duration_ms: 120 }],
};

const CUE_RULES: Record<string, Array<{ trigger: string; duration_ms: number; strength: number }>> = {
  drop:   [{ trigger: "white_flash",  duration_ms: 80,  strength: 1.0 }],
  impact: [{ trigger: "color_burst",  duration_ms: 60,  strength: 0.8 }],
  fill:   [{ trigger: "kick_pulse",   duration_ms: 120, strength: 0.6 }],
  build:  [{ trigger: "color_burst",  duration_ms: 200, strength: 0.7 }],
};

// ─── Beat timing ──────────────────────────────────────────────────────────────

function toNumberArray(v: number[] | Uint8Array | Uint16Array): number[] {
  if (Array.isArray(v)) return v;
  return Array.from(v);
}

export function beatPositionsMs(timing: TimingData): number[] {
  const deltas = toNumberArray(timing.beat_deltas_ms as number[]);
  const positions: number[] = [timing.first_beat_ms];
  let cur = timing.first_beat_ms;
  for (const d of deltas) {
    cur += d;
    positions.push(cur);
  }
  return positions;
}

export function isDownbeat(timing: TimingData, index: number): boolean {
  const bits = toNumberArray(timing.downbeat_bits as number[]);
  const byteIdx = Math.floor(index / 8);
  const bitIdx = index % 8;
  return byteIdx < bits.length ? ((bits[byteIdx] >> bitIdx) & 1) === 1 : false;
}

function barPhaseAt(timing: TimingData, positions: number[], beatIndex: number, beatPhase: number): number {
  const n = positions.length;
  let downbeatStart = beatIndex;
  while (downbeatStart > 0 && !isDownbeat(timing, downbeatStart)) downbeatStart--;

  let downbeatEnd = beatIndex + 1;
  while (downbeatEnd < n && !isDownbeat(timing, downbeatEnd)) downbeatEnd++;

  const beatsSince = (beatIndex - downbeatStart) + beatPhase;
  const barLength = downbeatEnd - downbeatStart;
  return barLength > 0 ? Math.max(0, Math.min(1, beatsSince / barLength)) : 0;
}

export function beatAtPosition(timing: TimingData, positionMs: number): BeatContext {
  const positions = beatPositionsMs(timing);
  const n = positions.length;

  // Binary search for the last beat at or before positionMs.
  let lo = 0, hi = n - 1, idx = 0;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (positions[mid] <= positionMs) { idx = mid; lo = mid + 1; }
    else hi = mid - 1;
  }

  const beatStart = positions[idx];
  const beatEnd = positions[idx + 1] ?? beatStart + 500;
  const phase = beatEnd > beatStart
    ? Math.max(0, Math.min(1, (positionMs - beatStart) / (beatEnd - beatStart)))
    : 0;

  return {
    index: idx,
    phase,
    is_downbeat: isDownbeat(timing, idx),
    bar_phase: barPhaseAt(timing, positions, idx, phase),
  };
}

// ─── Energy envelope ──────────────────────────────────────────────────────────

export function energyAt(beatmap: Beatmap, positionMs: number): number {
  const { energy, timing } = beatmap;
  const vals = toNumberArray(energy.values as number[]);
  if (vals.length === 0) return 0.5;

  const beat = beatAtPosition(timing, positionMs);
  const n = energy.sample_every_n_beats;
  const sampleF = beat.index / n;
  const lo = Math.floor(sampleF);
  const hi = lo + 1;
  const t = sampleF - lo;

  const loVal = (vals[lo] ?? 128) / 255;
  const hiVal = (vals[hi] ?? 128) / 255;
  return loVal + (hiVal - loVal) * t;
}

// ─── Effect context ───────────────────────────────────────────────────────────

export function resolveContext(positionMs: number, beatmap: Beatmap, timeSecs: number): EffectContext {
  const adjusted = Math.max(0, positionMs + beatmap.calibration_ms);
  const beat = beatAtPosition(beatmap.timing, adjusted);

  const section = beatmap.sections
    .slice()
    .reverse()
    .find((s) => s.start_beat <= beat.index);
  const sectionKind: SectionKind = section?.kind ?? "unknown";
  const sectionStartBeat = section?.start_beat ?? 0;

  const nextSection = beatmap.sections.find((s) => s.start_beat > beat.index);
  const sectionEndBeat = nextSection?.start_beat ?? beatmap.timing.beat_deltas_ms.length + 1;
  const sectionLength = Math.max(1, sectionEndBeat - sectionStartBeat);
  const sectionPhase = Math.max(0, Math.min(1,
    (beat.index - sectionStartBeat + beat.phase) / sectionLength
  ));

  const energy = energyAt(beatmap, adjusted);
  const rule = SECTION_RULES[sectionKind];
  // Map energy [0, 1] → [0.5, 1.0] so LEDs always show at least 50% brightness.
  const energyScaled = 0.5 + 0.5 * energy;
  const intensity = rule.intensity * energyScaled;
  const speed = rule.speed;

  return {
    beat_index: beat.index,
    beat_phase: beat.phase,
    bar_phase: beat.bar_phase,
    section_phase: sectionPhase,
    section: sectionKind,
    energy,
    time_secs: timeSecs,
    intensity,
    speed,
  };
}

export function idleContext(timeSecs: number): EffectContext {
  return {
    beat_index: 0,
    beat_phase: (timeSecs * 2) % 1,
    bar_phase: (timeSecs * 0.5) % 1,
    section_phase: 0,
    section: "unknown",
    energy: 0.5,
    time_secs: timeSecs,
    intensity: 0.5,
    speed: 0.3,
  };
}

// ─── Base effects ─────────────────────────────────────────────────────────────

function slowGradient(ctx: EffectContext, palette: Rgb[], frame: Rgb[]): void {
  const n = LED_COUNT;
  const offset = ctx.time_secs * 0.1 * ctx.speed;
  const pLen = palette.length;
  for (let i = 0; i < n; i++) {
    const t = (((i / n + offset) % 1) + 1) % 1;
    frame[i] = rgbScale(samplePalette(palette, t * pLen), ctx.intensity);
  }
}

function breathe(ctx: EffectContext, palette: Rgb[], frame: Rgb[]): void {
  const brightness = 0.2 + 0.8 * (Math.sin(ctx.bar_phase * Math.PI * 2) * 0.5 + 0.5);
  const color = rgbScale(
    samplePalette(palette, ctx.section_phase * palette.length),
    brightness * ctx.intensity
  );
  frame.fill(color);
}

function chase(ctx: EffectContext, palette: Rgb[], frame: Rgb[], speedFactor: number): void {
  const n = LED_COUNT;
  const pLen = palette.length;
  const offset =
    ctx.beat_index * (1 / pLen) * speedFactor +
    ctx.beat_phase * speedFactor / pLen;
  for (let i = 0; i < n; i++) {
    const t = (((i / n + offset) % 1) + 1) % 1;
    frame[i] = rgbScale(samplePalette(palette, t * pLen), ctx.intensity);
  }
}

function rise(ctx: EffectContext, palette: Rgb[], frame: Rgb[]): void {
  const n = LED_COUNT;
  const brightness = ctx.section_phase * ctx.intensity;
  const pulse = 1 - ctx.beat_phase * 0.3;
  const offset = ctx.time_secs * 0.2;
  for (let i = 0; i < n; i++) {
    const t = (((i / n + offset) % 1) + 1) % 1;
    frame[i] = rgbScale(samplePalette(palette, t * palette.length), brightness * pulse);
  }
}

function strobeChase(ctx: EffectContext, palette: Rgb[], frame: Rgb[]): void {
  const n = LED_COUNT;
  const strobePhase = (ctx.beat_phase * 8) % 1;
  const strobe = strobePhase < 0.5 ? 1.0 : 0.4;
  const offset = ctx.beat_index * 0.15;
  for (let i = 0; i < n; i++) {
    const t = (((i / n + offset) % 1) + 1) % 1;
    frame[i] = rgbScale(samplePalette(palette, t * palette.length), strobe * ctx.intensity);
  }
}

function fadeOut(ctx: EffectContext, palette: Rgb[], frame: Rgb[]): void {
  const brightness = (1 - ctx.section_phase) * ctx.intensity;
  const color = rgbScale(samplePalette(palette, 0), brightness);
  frame.fill(color);
}

export function renderBase(effectName: string, ctx: EffectContext, palette: Rgb[], frame: Rgb[]): void {
  switch (effectName) {
    case "breathe":       return breathe(ctx, palette, frame);
    case "slow_chase":    return chase(ctx, palette, frame, 0.3);
    case "fast_chase":    return chase(ctx, palette, frame, 0.8);
    case "rise":          return rise(ctx, palette, frame);
    case "strobe_chase":  return strobeChase(ctx, palette, frame);
    case "fade_out":      return fadeOut(ctx, palette, frame);
    default:              return slowGradient(ctx, palette, frame);
  }
}

// ─── Triggers ────────────────────────────────────────────────────────────────

type TriggerKind = "kick_pulse" | "soft_pulse" | "white_flash" | "color_burst";

export interface ActiveTrigger {
  kind: TriggerKind;
  progress: number; // 0 = just fired, 1 = expired
  strength: number;
  duration_ms: number;
  fired_at: number; // performance.now()
}

function applyTrigger(trigger: ActiveTrigger, frame: Rgb[]): void {
  const { kind, progress, strength } = trigger;
  switch (kind) {
    case "kick_pulse": {
      const boost = strength * Math.pow(1 - progress, 2);
      const add = Math.round(50 * boost);
      for (let i = 0; i < frame.length; i++) {
        frame[i] = rgbAdd(rgbScale2(frame[i], 1 + boost), { r: add, g: add, b: add });
      }
      break;
    }
    case "soft_pulse": {
      const boost = strength * (1 - progress);
      const add = Math.round(30 * boost);
      for (let i = 0; i < frame.length; i++) {
        frame[i] = rgbAdd(frame[i], { r: add, g: add, b: add });
      }
      break;
    }
    case "white_flash": {
      const alpha = Math.pow(1 - progress, 2) * strength;
      const w = Math.round(255 * alpha);
      for (let i = 0; i < frame.length; i++) {
        frame[i] = rgbAdd(frame[i], { r: w, g: w, b: w });
      }
      break;
    }
    case "color_burst": {
      const alpha = Math.pow(1 - progress, 2) * strength;
      for (let i = 0; i < frame.length; i++) {
        const p = frame[i];
        frame[i] = rgbAdd(frame[i], {
          r: Math.round(p.b * alpha),
          g: Math.round(p.r * alpha),
          b: Math.round(p.g * alpha),
        });
      }
      break;
    }
  }
}

// ─── Scheduler ────────────────────────────────────────────────────────────────

export class EffectScheduler {
  private triggers: ActiveTrigger[] = [];
  private startedAt = performance.now();
  private lastBeatIndex: number | null = null;
  private lastCueCheckMs = 0;

  reset(): void {
    this.triggers = [];
    this.lastBeatIndex = null;
    this.lastCueCheckMs = 0;
  }

  timeSecs(): number {
    return (performance.now() - this.startedAt) / 1000;
  }

  private fireTrigger(kind: TriggerKind, strength: number, duration_ms: number): void {
    this.triggers.push({ kind, progress: 0, strength, duration_ms, fired_at: performance.now() });
  }

  onBeat(isDownbeat: boolean): void {
    const rules = BEAT_RULES[isDownbeat ? "downbeat" : "beat"] ?? [];
    for (const r of rules) this.fireTrigger(r.trigger as TriggerKind, r.strength, r.duration_ms);
  }

  onCue(cueTypeStr: string): void {
    const rules = CUE_RULES[cueTypeStr] ?? [];
    for (const r of rules) this.fireTrigger(r.trigger as TriggerKind, r.strength, r.duration_ms);
  }

  processBeatAndCues(ctx: EffectContext, beatmap: Beatmap | null, positionMs: number): void {
    // Beat triggers
    if (this.lastBeatIndex !== null && ctx.beat_index > this.lastBeatIndex) {
      this.onBeat(ctx.beat_index % 4 === 0);
    }
    this.lastBeatIndex = ctx.beat_index;

    // Cue triggers
    if (beatmap) {
      const lookahead = 50;
      const from = this.lastCueCheckMs;
      const to = positionMs + lookahead;
      for (const cue of beatmap.cues) {
        if (cue.position_ms >= from && cue.position_ms < to) {
          this.onCue(cueKindStr(cue.kind));
        }
      }
      this.lastCueCheckMs = positionMs;
    }
  }

  /**
   * @param albumPalette  3-stop palette from buildAlbumPalette(), or undefined.
   * @param colorMode     "album" → use albumPalette/section palette;
   *                      "rainbow" → full HSV spectrum cycling with time.
   */
  render(
    ctx: EffectContext,
    beatmap: Beatmap | null,
    albumPalette?: Rgb[],
    colorMode: "album" | "rainbow" = "album",
    beatMultiplier: 1 | 2 | 3 = 1,
  ): Rgb[] {
    const frame: Rgb[] = new Array(LED_COUNT).fill({ r: 0, g: 0, b: 0 });

    const rule = SECTION_RULES[ctx.section];
    const effectName = beatmap ? rule.effect : "slow_gradient";

    let palette: Rgb[];
    if (colorMode === "rainbow") {
      // Rotate the hue at 1/20th of the section speed — gives a slow, dreamy cycle.
      const offset = (ctx.time_secs * 0.05 * ctx.speed) % 1;
      palette = buildRainbowPalette(offset);
    } else {
      palette = albumPalette
        ?? (beatmap ? PALETTES[rule.palette] : null)
        ?? PALETTES.cool;
    }
    renderBase(effectName, ctx, palette, frame);

    // Beat-flash envelope: bright at beat start → dims to 35% at beat end.
    // Uses a square-root ease for a quick initial punch then a gradual tail.
    // This drives the "flash on beat" behaviour requested by the user and is
    // stored as beat_phase in the beatmap so an ESP32 can reproduce it exactly.
    // Beat-flash envelope — multiplier subdivides each beat into N sub-flashes.
    // e.g. 2× flashes on every 8th note, 3× on every triplet.
    // sqrt ease: bright at the sub-beat instant, decays quickly toward 50%.
    const subPhase = (ctx.beat_phase * beatMultiplier) % 1;
    const beatFlash = 0.5 + 0.5 * Math.pow(1 - subPhase, 0.5);
    for (let i = 0; i < frame.length; i++) {
      frame[i] = rgbScale(frame[i], beatFlash);
    }

    // Update and apply one-shot triggers (kick pulses, white flash, etc.)
    const now = performance.now();
    this.triggers = this.triggers.filter((t) => {
      t.progress = Math.min(1, (now - t.fired_at) / t.duration_ms);
      return t.progress < 1;
    });
    for (const t of this.triggers) applyTrigger(t, frame);

    return frame;
  }
}

function cueKindStr(kind: CueKind): string {
  if (typeof kind === "string") return kind;
  return "custom";
}

// ─── Section accent color (for UI) ───────────────────────────────────────────

export const SECTION_COLORS: Record<SectionKind, string> = {
  intro:     "#0033ff",
  verse:     "#00ccff",
  chorus:    "#ff6600",
  buildup:   "#ff8800",
  drop:      "#ff0000",
  breakdown: "#aaeeff",
  bridge:    "#00ff99",
  outro:     "#6600ff",
  unknown:   "#555555",
};
