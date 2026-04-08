"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import type { Beatmap, EffectContext, Rgb, TrackEntry } from "@/lib/types";
import {
  EffectScheduler,
  LED_COUNT,
  SECTION_COLORS,
  beatPositionsMs,
  buildAlbumPalette,
  idleContext,
  resolveContext,
} from "@/lib/engine";
import LedStrip from "./LedStrip";

interface Props {
  track: TrackEntry | null;
  hasPrev: boolean;
  hasNext: boolean;
  onPrev: () => void;
  onNext: () => void;
  onEnded: () => void;
  isShuffled: boolean;
  onShuffleToggle: () => void;
  volume: number;
  onVolumeChange: (v: number) => void;
  colorMode: "album" | "rainbow";
  onColorModeChange: (m: "album" | "rainbow") => void;
  beatMultiplier: 1 | 2 | 3;
  onBeatMultiplierChange: (m: 1 | 2 | 3) => void;
  syncOffset: number;
  onSyncOffsetChange: (ms: number) => void;
  onAccentChange?: (color: string | undefined) => void;
}

function fmt(ms: number): string {
  const s = Math.floor(ms / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

async function findAudio(titleKey: string): Promise<string | null> {
  const titlePart = titleKey.split("\0")[1] ?? "";
  const normalized = titlePart.split(" ").map((w) => w[0].toUpperCase() + w.slice(1)).join(" ");
  for (const c of [`${normalized}.mp3`, `${titlePart}.mp3`]) {
    const res = await fetch(`/api/audio?file=${encodeURIComponent(c)}`, { method: "HEAD" }).catch(() => null);
    if (res?.ok) return c;
  }
  return null;
}

export default function BeatmapPlayer({
  track, hasPrev, hasNext, onPrev, onNext, onEnded,
  isShuffled, onShuffleToggle,
  volume, onVolumeChange,
  colorMode, onColorModeChange,
  beatMultiplier, onBeatMultiplierChange,
  syncOffset, onSyncOffsetChange,
  onAccentChange,
}: Props) {
  const [beatmap, setBeatmap]       = useState<Beatmap | null>(null);
  const [loading, setLoading]       = useState(false);
  const [artError, setArtError]     = useState(false);
  const [pixels, setPixels]         = useState<Rgb[]>(() => new Array(LED_COUNT).fill({ r: 0, g: 0, b: 0 }));
  const [ctx, setCtx]               = useState<EffectContext>(idleContext(0));
  const [positionMs, setPositionMs] = useState(0);
  const [playing, setPlaying]       = useState(false);
  const [hasAudio, setHasAudio]     = useState(false);

  const schedulerRef    = useRef(new EffectScheduler());
  const rafRef          = useRef<number | null>(null);
  const playStartRef    = useRef<number | null>(null);
  const playOffsetRef   = useRef(0);
  const beatmapRef      = useRef<Beatmap | null>(null);
  const audioRef        = useRef<HTMLAudioElement | null>(null);
  const playingRef      = useRef(false);
  const albumPaletteRef = useRef<Rgb[] | undefined>(undefined);
  const onEndedRef      = useRef(onEnded);
  const audioFileRef    = useRef<string | null>(null);
  const endFiredRef     = useRef(false);
  // Keep hot values in refs so the rAF loop reads current values without re-creating
  const colorModeRef       = useRef(colorMode);
  const beatMultiplierRef  = useRef(beatMultiplier);
  const syncOffsetRef      = useRef(syncOffset);

  useEffect(() => { onEndedRef.current = onEnded; }, [onEnded]);
  useEffect(() => { colorModeRef.current = colorMode; },      [colorMode]);
  useEffect(() => { beatMultiplierRef.current = beatMultiplier; }, [beatMultiplier]);
  useEffect(() => { syncOffsetRef.current = syncOffset; },    [syncOffset]);
  useEffect(() => { if (audioRef.current) audioRef.current.volume = volume; }, [volume]);

  // ── Load beatmap + audio when track changes ─────────────────────────────────
  useEffect(() => {
    const resetState = () => {
      setBeatmap(null);
      beatmapRef.current = null;
      albumPaletteRef.current = undefined;
      setPositionMs(0);
      setPlaying(false);
      playingRef.current = false;
      audioFileRef.current = null;
      setHasAudio(false);
      setArtError(false);
      endFiredRef.current = false;
      playOffsetRef.current = 0;
      playStartRef.current = null;
      schedulerRef.current.reset();
      if (audioRef.current) { audioRef.current.pause(); audioRef.current.src = ""; }
    };

    if (!track) { resetState(); onAccentChange?.(undefined); return; }

    setLoading(true);
    resetState();

    fetch(`/api/beatmap?path=${encodeURIComponent(track.path)}`)
      .then((r) => r.json())
      .then(async (bm: Beatmap) => {
        setBeatmap(bm);
        beatmapRef.current = bm;

        if (bm.track.dominant_color) {
          albumPaletteRef.current = buildAlbumPalette(bm.track.dominant_color);
          const [r, g, b] = bm.track.dominant_color;
          onAccentChange?.(`rgb(${r},${g},${b})`);
        } else {
          onAccentChange?.(undefined);
        }

        setLoading(false);
        const af = await findAudio(track.key);
        audioFileRef.current = af;
        setHasAudio(!!af);
        if (af && audioRef.current) {
          audioRef.current.volume = volume;
          audioRef.current.src = `/api/audio?file=${encodeURIComponent(af)}`;
          audioRef.current.load();
        }
      })
      .catch(() => setLoading(false));
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [track]);

  // ── Audio events ────────────────────────────────────────────────────────────
  useEffect(() => {
    const audio = audioRef.current;
    if (!audio) return;
    const onEnded = () => {
      if (endFiredRef.current) return;
      endFiredRef.current = true;
      playingRef.current = false;
      setPlaying(false);
      onEndedRef.current();
    };
    const onError = () => setHasAudio(false);
    audio.addEventListener("ended", onEnded);
    audio.addEventListener("error", onError);
    return () => { audio.removeEventListener("ended", onEnded); audio.removeEventListener("error", onError); };
  }, []);

  // ── Animation loop ──────────────────────────────────────────────────────────
  // Stable: reads everything via refs, never re-created.
  const animate = useCallback(() => {
    rafRef.current = requestAnimationFrame(animate);
    const sched    = schedulerRef.current;
    const timeSecs = sched.timeSecs();
    const bm       = beatmapRef.current;
    const audio    = audioRef.current;
    const af       = audioFileRef.current;

    // ── Position: audio element is the source of truth when playing ──────────
    let pos: number;
    if (audio && af && !audio.paused && !audio.ended && audio.readyState >= 2) {
      pos = audio.currentTime * 1000;
      playOffsetRef.current = pos;
      playStartRef.current = performance.now();
    } else if (playingRef.current && playStartRef.current !== null) {
      pos = playOffsetRef.current + (performance.now() - playStartRef.current);
    } else {
      pos = playOffsetRef.current;
    }

    // End-of-track (no-audio / beatmap-only mode)
    if (bm && !af && playingRef.current && pos >= bm.track.duration_ms) {
      pos = bm.track.duration_ms;
      playingRef.current = false;
      playOffsetRef.current = bm.track.duration_ms;
      playStartRef.current = null;
      setPlaying(false);
      if (!endFiredRef.current) { endFiredRef.current = true; onEndedRef.current(); }
    }

    // syncOffset shifts which beatmap position we query; positive = flash later.
    const queryPos = pos + syncOffsetRef.current;
    const effectCtx = bm ? resolveContext(queryPos, bm, timeSecs) : idleContext(timeSecs);
    sched.processBeatAndCues(effectCtx, bm, queryPos);
    const frame = sched.render(effectCtx, bm, albumPaletteRef.current, colorModeRef.current, beatMultiplierRef.current);

    setPixels([...frame]);
    setCtx(effectCtx);
    setPositionMs(Math.round(pos));
  }, []); // ← intentionally empty: all state read through refs

  useEffect(() => {
    rafRef.current = requestAnimationFrame(animate);
    return () => { if (rafRef.current) cancelAnimationFrame(rafRef.current); };
  }, [animate]);

  // ── Playback controls ───────────────────────────────────────────────────────
  const togglePlay = () => {
    if (!beatmap) return;
    const nowPlaying = !playingRef.current;
    playingRef.current = nowPlaying;
    endFiredRef.current = false;

    if (nowPlaying) {
      playStartRef.current = performance.now();
      const audio = audioRef.current;
      const af = audioFileRef.current;
      if (audio && af) {
        if (audio.ended || audio.currentTime >= (audio.duration || Infinity) - 0.1) {
          audio.currentTime = 0;
          playOffsetRef.current = 0;
        } else {
          audio.currentTime = playOffsetRef.current / 1000;
        }
        audio.play().catch(() => {});
      }
    } else {
      if (playStartRef.current !== null) {
        playOffsetRef.current += performance.now() - playStartRef.current;
        playStartRef.current = null;
      }
      if (audioRef.current) audioRef.current.pause();
    }
    setPlaying(nowPlaying);
  };

  const seek = (ms: number) => {
    endFiredRef.current = false;
    playOffsetRef.current = ms;
    playStartRef.current = playingRef.current ? performance.now() : null;
    schedulerRef.current.reset();
    const audio = audioRef.current;
    if (audio && audioFileRef.current) {
      audio.currentTime = ms / 1000;
      if (playingRef.current) audio.play().catch(() => {});
    }
  };

  const handlePrev = () => (positionMs > 3000 ? seek(0) : onPrev());

  // ── Derived display values ──────────────────────────────────────────────────
  const duration   = beatmap?.track.duration_ms ?? 1;
  const progress   = Math.min(1, positionMs / duration);
  const albumRgb   = beatmap?.track.dominant_color;
  const sectionAcc = SECTION_COLORS[ctx.section];
  const accentCss  = colorMode === "rainbow"
    ? `hsl(${(Date.now() / 30) % 360},100%,60%)`
    : albumRgb ? `rgb(${albumRgb[0]},${albumRgb[1]},${albumRgb[2]})` : sectionAcc;
  const artUrl = track ? `/api/albumart?path=${encodeURIComponent(track.path)}` : null;

  return (
    <div className="flex-1 flex flex-col overflow-y-auto overflow-x-hidden">
      <audio ref={audioRef} preload="metadata" />

      {/* ── LED strip ─────────────────────────────────────────────────────── */}
      <div className="px-6 pt-6 pb-5 shrink-0">
        <div className="rounded-xl overflow-hidden" style={{
          background: "#0d0d0d",
          boxShadow: `0 0 60px 0 ${accentCss}16, 0 0 20px 0 ${accentCss}0a`,
          border: "1px solid rgba(255,255,255,0.05)",
          padding: "14px",
        }}>
          <LedStrip pixels={pixels} />
        </div>
      </div>

      <div className="px-6 pb-6 flex flex-col gap-5 shrink-0 min-w-0">

        {/* ── Track identity ─────────────────────────────────────────────── */}
        <div className="flex items-center gap-4 min-w-0">
          {/* Album art */}
          <div className="shrink-0 rounded-lg overflow-hidden" style={{
            width: 68, height: 68,
            background: "rgba(255,255,255,0.06)",
            boxShadow: artUrl && !artError ? `0 0 18px 3px ${accentCss}28` : "none",
          }}>
            {artUrl && !artError ? (
              <img key={track?.path} src={artUrl} alt="Album art"
                   onError={() => setArtError(true)} className="w-full h-full object-cover" />
            ) : albumRgb ? (
              <div className="w-full h-full"
                   style={{ background: `linear-gradient(135deg, ${accentCss}55, ${accentCss}18)` }} />
            ) : null}
          </div>

          {/* Text */}
          <div className="min-w-0 flex-1">
            {loading && <p className="text-sm" style={{ color: "rgba(255,255,255,0.3)" }}>Loading…</p>}
            {!loading && beatmap && (
              <>
                <p className="text-base font-semibold leading-tight truncate text-white">{beatmap.track.title}</p>
                <p className="text-sm mt-0.5 truncate" style={{ color: "rgba(255,255,255,0.45)" }}>{beatmap.track.artist}</p>
                {beatmap.track.album && (
                  <p className="text-xs mt-0.5 truncate" style={{ color: "rgba(255,255,255,0.22)" }}>{beatmap.track.album}</p>
                )}
              </>
            )}
            {!loading && !beatmap && !track && (
              <p className="text-sm" style={{ color: "rgba(255,255,255,0.2)" }}>Select a track to begin</p>
            )}
          </div>

          {/* Colour mode toggle */}
          <div className="shrink-0 flex flex-col items-end gap-1.5">
            <ColorModeToggle value={colorMode} onChange={onColorModeChange} accentCss={accentCss} />
            {beatmap && (
              <p className="text-[10px] tabular-nums font-mono" style={{ color: "rgba(255,255,255,0.3)" }}>
                {beatmap.track.detected_bpm.toFixed(1)} BPM
              </p>
            )}
          </div>
        </div>

        {/* ── Scrubber ──────────────────────────────────────────────────────── */}
        <div className="flex items-center gap-3">
          <span className="text-[11px] tabular-nums w-9 text-right shrink-0"
                style={{ color: "rgba(255,255,255,0.35)" }}>{fmt(positionMs)}</span>
          <div className="relative flex-1 group cursor-pointer"
               style={{ height: 4, borderRadius: 999, background: "rgba(255,255,255,0.08)" }}
               onClick={(e) => {
                 if (!beatmap) return;
                 const r = e.currentTarget.getBoundingClientRect();
                 seek(Math.round(((e.clientX - r.left) / r.width) * beatmap.track.duration_ms));
               }}>
            <div className="h-full rounded-full pointer-events-none"
                 style={{ width: `${progress * 100}%`, background: accentCss }} />
            <div className="absolute top-1/2 rounded-full opacity-0 group-hover:opacity-100 transition-opacity pointer-events-none"
                 style={{ width: 12, height: 12, left: `${progress * 100}%`,
                          transform: "translate(-50%,-50%)", background: accentCss,
                          boxShadow: `0 0 8px 2px ${accentCss}70` }} />
          </div>
          <span className="text-[11px] tabular-nums w-9 shrink-0"
                style={{ color: "rgba(255,255,255,0.35)" }}>{fmt(duration)}</span>
        </div>

        {/* ── Main controls row ──────────────────────────────────────────────── */}
        <div className="flex items-center justify-between gap-3">

          {/* Shuffle */}
          <button onClick={onShuffleToggle} title="Shuffle"
                  style={{ color: isShuffled ? accentCss : "rgba(255,255,255,0.3)" }}>
            <svg className="w-4 h-4" fill="currentColor" viewBox="0 0 24 24">
              <path d="M10.59 9.17 5.41 4 4 5.41l5.17 5.17 1.41-1.41zm4.76-.05 3.65 3.88-3.65 3.12V14h-.02c-.8-.01-1.96-.14-3.04-.65l-1.45 1.45c1.28.75 2.71 1.15 4.49 1.19V18l4-3.5L16 11v1.12h-.01l-.64-.68zM4 18.99l1.41 1.41 14-14L18 5 4 18.99z"/>
            </svg>
          </button>

          {/* Prev / Play / Next */}
          <div className="flex items-center gap-4">
            <button onClick={handlePrev} disabled={!beatmap && !hasPrev} title="Previous"
                    className="disabled:opacity-20" style={{ color: "rgba(255,255,255,0.55)" }}>
              <svg className="w-5 h-5" fill="currentColor" viewBox="0 0 24 24">
                <path d="M6 6h2v12H6zm3.5 6 8.5 6V6z"/>
              </svg>
            </button>

            <button onClick={togglePlay} disabled={!beatmap}
                    className="flex items-center justify-center rounded-full disabled:opacity-25 transition-all"
                    style={{ width: 48, height: 48, background: accentCss,
                             boxShadow: beatmap ? `0 0 22px 5px ${accentCss}40` : "none" }}>
              {playing
                ? <svg className="w-5 h-5 text-white" fill="currentColor" viewBox="0 0 24 24"><path d="M6 19h4V5H6v14zm8-14v14h4V5h-4z"/></svg>
                : <svg className="w-5 h-5 text-white" fill="currentColor" viewBox="0 0 24 24" style={{ marginLeft: 2 }}><path d="M8 5v14l11-7z"/></svg>
              }
            </button>

            <button onClick={onNext} disabled={!hasNext} title="Next"
                    className="disabled:opacity-20" style={{ color: "rgba(255,255,255,0.55)" }}>
              <svg className="w-5 h-5" fill="currentColor" viewBox="0 0 24 24">
                <path d="M6 18l8.5-6L6 6v12zm2-8.14L11.03 12 8 14.14V9.86zM16 6h2v12h-2z"/>
              </svg>
            </button>
          </div>

          {/* Volume */}
          <div className="flex items-center gap-2">
            <svg className="w-3.5 h-3.5 shrink-0" fill="currentColor" viewBox="0 0 24 24"
                 style={{ color: "rgba(255,255,255,0.3)" }}>
              <path d="M3 9v6h4l5 5V4L7 9zm13.5 3c0-1.77-1.02-3.29-2.5-4.03v8.05c1.48-.73 2.5-2.25 2.5-4.02z"/>
            </svg>
            <input type="range" min={0} max={1} step={0.02} value={volume}
                   onChange={(e) => onVolumeChange(parseFloat(e.target.value))}
                   className="volume-slider w-20"
                   style={{ "--accent": accentCss } as React.CSSProperties} />
          </div>
        </div>

        {/* ── Beat multiplier + sync offset row ─────────────────────────────── */}
        <div className="flex items-center justify-between gap-4 pt-1"
             style={{ borderTop: "1px solid rgba(255,255,255,0.05)" }}>

          {/* Beat multiplier */}
          <div className="flex items-center gap-2">
            <span className="text-[10px] tracking-widest uppercase"
                  style={{ color: "rgba(255,255,255,0.25)" }}>Flash</span>
            <div className="flex rounded-md overflow-hidden"
                 style={{ border: "1px solid rgba(255,255,255,0.1)" }}>
              {([1, 2, 3] as const).map((m) => (
                <button key={m} onClick={() => onBeatMultiplierChange(m)}
                        className="px-2.5 py-1 text-[11px] font-mono transition-colors"
                        style={{
                          background: beatMultiplier === m ? accentCss : "transparent",
                          color: beatMultiplier === m ? "#fff" : "rgba(255,255,255,0.35)",
                          borderRight: m < 3 ? "1px solid rgba(255,255,255,0.1)" : "none",
                        }}>
                  {m}×
                </button>
              ))}
            </div>
          </div>

          {/* Sync offset */}
          <div className="flex items-center gap-1.5">
            <span className="text-[10px] tracking-widest uppercase"
                  style={{ color: "rgba(255,255,255,0.25)" }}>Sync</span>
            <button onClick={() => onSyncOffsetChange(syncOffset - 25)}
                    className="w-6 h-6 flex items-center justify-center rounded text-xs transition-colors"
                    style={{ background: "rgba(255,255,255,0.06)", color: "rgba(255,255,255,0.5)" }}>−</button>
            <span className="text-[11px] tabular-nums font-mono w-14 text-center"
                  style={{ color: syncOffset !== 0 ? accentCss : "rgba(255,255,255,0.35)" }}>
              {syncOffset > 0 ? `+${syncOffset}` : syncOffset}ms
            </span>
            <button onClick={() => onSyncOffsetChange(syncOffset + 25)}
                    className="w-6 h-6 flex items-center justify-center rounded text-xs transition-colors"
                    style={{ background: "rgba(255,255,255,0.06)", color: "rgba(255,255,255,0.5)" }}>+</button>
            {syncOffset !== 0 && (
              <button onClick={() => onSyncOffsetChange(0)}
                      className="text-[10px] ml-0.5 transition-opacity"
                      style={{ color: "rgba(255,255,255,0.25)" }}>↺</button>
            )}
          </div>
        </div>

        {/* ── Info grid ─────────────────────────────────────────────────────── */}
        {beatmap && (
          <div className="grid grid-cols-4 gap-3 pt-4"
               style={{ borderTop: "1px solid rgba(255,255,255,0.06)" }}>
            <InfoCell label="Section">
              <span className="font-mono text-sm font-semibold capitalize" style={{ color: accentCss }}>
                {ctx.section}
              </span>
            </InfoCell>
            <InfoCell label="Beat">
              <span className="font-mono text-sm" style={{ color: "rgba(255,255,255,0.75)" }}>
                {ctx.beat_index + 1}
              </span>
            </InfoCell>
            <InfoCell label="Energy">
              <div className="flex items-center gap-1.5 mt-0.5">
                <div className="flex-1 rounded-full overflow-hidden"
                     style={{ height: 3, background: "rgba(255,255,255,0.08)" }}>
                  <div className="h-full rounded-full transition-all duration-100"
                       style={{ width: `${ctx.energy * 100}%`, background: accentCss }} />
                </div>
                <span className="text-[11px] tabular-nums shrink-0"
                      style={{ color: "rgba(255,255,255,0.35)" }}>{Math.round(ctx.energy * 100)}</span>
              </div>
            </InfoCell>
            <InfoCell label="Bar">
              <div className="flex items-center gap-1.5 mt-0.5">
                <div className="flex-1 rounded-full overflow-hidden"
                     style={{ height: 3, background: "rgba(255,255,255,0.08)" }}>
                  <div className="h-full rounded-full"
                       style={{ width: `${ctx.bar_phase * 100}%`, background: "rgba(255,255,255,0.22)" }} />
                </div>
              </div>
            </InfoCell>
          </div>
        )}

        {/* ── Section timeline ──────────────────────────────────────────────── */}
        {beatmap && beatmap.sections.length > 0 && (
          <SectionTimeline beatmap={beatmap} positionMs={positionMs} onSeek={seek} accentCss={accentCss} />
        )}
      </div>
    </div>
  );
}

// ─── Sub-components ───────────────────────────────────────────────────────────

function ColorModeToggle({ value, onChange, accentCss }: {
  value: "album" | "rainbow"; onChange: (m: "album" | "rainbow") => void; accentCss: string;
}) {
  return (
    <div className="flex items-center rounded-full p-0.5 gap-0.5"
         style={{ background: "rgba(255,255,255,0.07)", border: "1px solid rgba(255,255,255,0.08)" }}>
      {(["album", "rainbow"] as const).map((mode) => {
        const active = value === mode;
        return (
          <button key={mode} onClick={() => onChange(mode)} title={mode === "album" ? "Album colour" : "Rainbow RGB"}
                  className="flex items-center justify-center rounded-full transition-all"
                  style={{ width: 26, height: 26, background: active ? accentCss : "transparent",
                           boxShadow: active ? `0 0 8px 1px ${accentCss}60` : "none",
                           color: active ? "#fff" : "rgba(255,255,255,0.35)" }}>
            {mode === "album"
              ? <svg className="w-3.5 h-3.5" viewBox="0 0 24 24" fill="currentColor"><path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 14.5c-2.49 0-4.5-2.01-4.5-4.5S9.51 7.5 12 7.5s4.5 2.01 4.5 4.5-2.01 4.5-4.5 4.5zm0-5.5c-.55 0-1 .45-1 1s.45 1 1 1 1-.45 1-1-.45-1-1-1z"/></svg>
              : <svg className="w-3.5 h-3.5" viewBox="0 0 24 24" fill="currentColor"><path d="M12 4c-4.42 0-8 3.58-8 8h2c0-3.31 2.69-6 6-6s6 2.69 6 6h2c0-4.42-3.58-8-8-8zm-1 8h2V9h-2v3zm-3 0h2V9H8v3zm6 0h2V9h-2v3z"/></svg>
            }
          </button>
        );
      })}
    </div>
  );
}

function InfoCell({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-1.5">
      <span className="text-[9px] tracking-[0.16em] uppercase" style={{ color: "rgba(255,255,255,0.25)" }}>{label}</span>
      {children}
    </div>
  );
}

function SectionTimeline({ beatmap, positionMs, onSeek, accentCss }: {
  beatmap: Beatmap; positionMs: number; onSeek: (ms: number) => void; accentCss: string;
}) {
  const beatPositions = beatPositionsMs(beatmap.timing);
  const duration = beatmap.track.duration_ms;
  return (
    <div>
      <p className="text-[9px] tracking-[0.16em] uppercase mb-2" style={{ color: "rgba(255,255,255,0.25)" }}>Sections</p>
      <div className="relative overflow-hidden cursor-pointer"
           style={{ height: 22, borderRadius: 4, background: "rgba(255,255,255,0.04)" }}
           onClick={(e) => { const r = e.currentTarget.getBoundingClientRect(); onSeek(Math.round(((e.clientX - r.left) / r.width) * duration)); }}>
        {beatmap.sections.map((sec, idx) => {
          const startMs = beatPositions[sec.start_beat] ?? 0;
          const endMs = beatmap.sections[idx + 1] ? (beatPositions[beatmap.sections[idx + 1].start_beat] ?? duration) : duration;
          const color = SECTION_COLORS[sec.kind];
          return (
            <div key={idx} className="absolute top-0 h-full flex items-center px-1.5 overflow-hidden"
                 style={{ left: `${(startMs / duration) * 100}%`, width: `${((endMs - startMs) / duration) * 100}%`,
                          background: `${color}26`, borderRight: "1px solid rgba(0,0,0,0.2)" }} title={sec.kind}>
              <span className="text-[9px] truncate capitalize" style={{ color: `${color}bb` }}>{sec.kind}</span>
            </div>
          );
        })}
        {beatmap.cues.map((cue, idx) => (
          <div key={idx} className="absolute top-0 h-full"
               style={{ left: `${(cue.position_ms / duration) * 100}%`, width: 1, background: "rgba(255,255,255,0.45)" }} />
        ))}
        <div className="absolute top-0 h-full pointer-events-none"
             style={{ left: `${(positionMs / duration) * 100}%`, width: 2, background: accentCss,
                      boxShadow: `0 0 6px 1px ${accentCss}80`, borderRadius: 1 }} />
      </div>
    </div>
  );
}
