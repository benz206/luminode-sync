export type SectionKind =
  | "intro"
  | "verse"
  | "chorus"
  | "buildup"
  | "drop"
  | "breakdown"
  | "bridge"
  | "outro"
  | "unknown";

export type CueKind =
  | "drop"
  | "build"
  | "fill"
  | "impact"
  | { custom: string };

export interface TrackMeta {
  title: string;
  artist: string;
  album?: string;
  duration_ms: number;
  spotify_id?: string;
  isrc?: string;
  source_hash: string;
  detected_bpm: number;
  /** Dominant RGB from album art, extracted at beatmap-gen time. [r, g, b] 0–255.
   *  Used by viewer and ESP32 firmware. */
  dominant_color?: [number, number, number];
}

export interface TimingData {
  first_beat_ms: number;
  beat_deltas_ms: number[];
  downbeat_bits: number[];
  time_sig: number;
}

export interface Section {
  start_beat: number;
  kind: SectionKind;
  energy: number;
}

export interface Cue {
  position_ms: number;
  kind: CueKind;
}

export interface EnergyEnvelope {
  sample_every_n_beats: number;
  values: number[];
}

export interface Beatmap {
  version: number;
  track: TrackMeta;
  timing: TimingData;
  sections: Section[];
  cues: Cue[];
  energy: EnergyEnvelope;
  calibration_ms: number;
}

export interface TrackEntry {
  label: string;
  path: string;
  key: string;
  /** Dominant colour from album art, or undefined if not yet patched. */
  dominant_color?: [number, number, number];
}

export interface Rgb {
  r: number;
  g: number;
  b: number;
}

export interface BeatContext {
  index: number;
  phase: number;
  is_downbeat: boolean;
  bar_phase: number;
}

export interface EffectContext {
  beat_index: number;
  beat_phase: number;
  bar_phase: number;
  section_phase: number;
  section: SectionKind;
  energy: number;
  time_secs: number;
  intensity: number;
  speed: number;
}
