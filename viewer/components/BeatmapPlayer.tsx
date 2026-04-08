"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import type { Beatmap, EffectContext, Rgb, TrackEntry } from "@/lib/types";
import {
  EffectScheduler,
  LED_COUNT,
  SECTION_COLORS,
  beatPositionsMs,
  buildAlbumPalette,
  hexToRgb,
  idleContext,
  resolveContext,
  rgbToCss,
} from "@/lib/engine";
import LedStrip from "./LedStrip";

interface Props {
  track: TrackEntry | null;
  onAccentChange?: (color: string | undefined) => void;
}

function formatMs(ms: number): string {
  const s = Math.floor(ms / 1000);
  const m = Math.floor(s / 60);
  return `${m}:${String(s % 60).padStart(2, "0")}`;
}

async function findAudio(titleKey: string): Promise<string | null> {
  const titlePart = titleKey.split("\0")[1] ?? "";
  const normalized = titlePart
    .split(" ")
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join(" ");
  for (const c of [`${normalized}.mp3`, `${titlePart}.mp3`]) {
    const res = await fetch(`/api/audio?file=${encodeURIComponent(c)}`, { method: "HEAD" }).catch(() => null);
    if (res?.ok) return c;
  }
  return null;
}

export default function BeatmapPlayer({ track, onAccentChange }: Props) {
  const [beatmap, setBeatmap] = useState<Beatmap | null>(null);
  const [loading, setLoading] = useState(false);
  const [artError, setArtError] = useState(false);
  const [pixels, setPixels] = useState<Rgb[]>(() =>
    new Array(LED_COUNT).fill({ r: 0, g: 0, b: 0 })
  );
  const [ctx, setCtx] = useState<EffectContext>(idleContext(0));
  const [positionMs, setPositionMs] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [audioFile, setAudioFile] = useState<string | null>(null);

  const schedulerRef = useRef(new EffectScheduler());
  const rafRef = useRef<number | null>(null);
  const playStartRef = useRef<number | null>(null);
  const playOffsetRef = useRef(0);
  const beatmapRef = useRef<Beatmap | null>(null);
  const beatPositionsRef = useRef<number[]>([]);
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const playingRef = useRef(false);
  const albumPaletteRef = useRef<Rgb[] | undefined>(undefined);

  // Load beatmap when track changes
  useEffect(() => {
    if (!track) {
      setBeatmap(null);
      beatmapRef.current = null;
      beatPositionsRef.current = [];
      albumPaletteRef.current = undefined;
      setPositionMs(0);
      setPlaying(false);
      playingRef.current = false;
      setAudioFile(null);
      schedulerRef.current.reset();
      onAccentChange?.(undefined);
      return;
    }

    setLoading(true);
    setPlaying(false);
    playingRef.current = false;
    setPositionMs(0);
    playOffsetRef.current = 0;
    albumPaletteRef.current = undefined;
    setArtError(false);
    schedulerRef.current.reset();

    fetch(`/api/beatmap?path=${encodeURIComponent(track.path)}`)
      .then((r) => r.json())
      .then(async (bm: Beatmap) => {
        setBeatmap(bm);
        beatmapRef.current = bm;
        beatPositionsRef.current = beatPositionsMs(bm.timing);

        if (bm.track.dominant_color) {
          albumPaletteRef.current = buildAlbumPalette(bm.track.dominant_color);
          const [r, g, b] = bm.track.dominant_color;
          onAccentChange?.(`rgb(${r},${g},${b})`);
        } else {
          albumPaletteRef.current = undefined;
          onAccentChange?.(undefined);
        }

        setLoading(false);
        const af = await findAudio(track.key);
        setAudioFile(af);
        if (audioRef.current && af) {
          audioRef.current.src = `/api/audio?file=${encodeURIComponent(af)}`;
          audioRef.current.load();
        }
      })
      .catch(() => setLoading(false));
  }, [track]);

  // Animation loop
  const animate = useCallback(() => {
    rafRef.current = requestAnimationFrame(animate);
    const sched = schedulerRef.current;
    const timeSecs = sched.timeSecs();
    const bm = beatmapRef.current;

    let pos = playOffsetRef.current;
    if (playingRef.current && playStartRef.current !== null) {
      pos = playOffsetRef.current + (performance.now() - playStartRef.current);
    }
    if (bm && pos > bm.track.duration_ms) {
      pos = bm.track.duration_ms;
      setPlaying(false);
      playingRef.current = false;
    }

    const effectCtx = bm ? resolveContext(pos, bm, timeSecs) : idleContext(timeSecs);
    sched.processBeatAndCues(effectCtx, bm, pos);
    const frame = sched.render(effectCtx, bm, albumPaletteRef.current);

    setPixels([...frame]);
    setCtx(effectCtx);
    setPositionMs(Math.round(pos));
  }, []);

  useEffect(() => {
    rafRef.current = requestAnimationFrame(animate);
    return () => { if (rafRef.current) cancelAnimationFrame(rafRef.current); };
  }, [animate]);

  const togglePlay = () => {
    if (!beatmap) return;
    const nowPlaying = !playingRef.current;
    playingRef.current = nowPlaying;
    if (nowPlaying) {
      playStartRef.current = performance.now();
      if (audioRef.current && audioFile) {
        audioRef.current.currentTime = playOffsetRef.current / 1000;
        audioRef.current.play().catch(() => {});
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
    const wasPlaying = playingRef.current;
    playOffsetRef.current = ms;
    playStartRef.current = wasPlaying ? performance.now() : null;
    schedulerRef.current.reset();
    if (audioRef.current && audioFile) {
      audioRef.current.currentTime = ms / 1000;
    }
  };

  const duration = beatmap?.track.duration_ms ?? 1;
  const progress = duration > 0 ? positionMs / duration : 0;

  // Accent: prefer album dominant colour, fall back to section colour
  const sectionAccent = SECTION_COLORS[ctx.section];
  const albumRgb = beatmap?.track.dominant_color;
  const accentCss = albumRgb ? `rgb(${albumRgb[0]},${albumRgb[1]},${albumRgb[2]})` : sectionAccent;
  const artUrl = track ? `/api/albumart?path=${encodeURIComponent(track.path)}` : null;

  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      <audio ref={audioRef} preload="none" />

      {/* ── LED strip ───────────────────────────────────────────────────────── */}
      <div className="px-6 pt-6 pb-5">
        <div
          className="rounded-xl overflow-hidden"
          style={{
            background: "#0d0d0d",
            boxShadow: `0 0 60px 0 ${accentCss}1a, 0 0 20px 0 ${accentCss}0d`,
            border: "1px solid rgba(255,255,255,0.05)",
            padding: "14px 14px",
          }}
        >
          <LedStrip pixels={pixels} />
        </div>
      </div>

      {/* ── Track info + controls ────────────────────────────────────────────── */}
      <div className="px-6 pb-5 flex flex-col gap-5 min-w-0">

        {/* Track identity row — album art + text + meta */}
        <div className="flex items-center gap-4 min-w-0">
          {/* Album art */}
          <div
            className="shrink-0 rounded-lg overflow-hidden"
            style={{
              width: 72, height: 72,
              background: "rgba(255,255,255,0.06)",
              boxShadow: artUrl && !artError ? `0 0 20px 4px ${accentCss}30` : "none",
            }}
          >
            {artUrl && !artError ? (
              <img
                key={track?.path}
                src={artUrl}
                alt="Album art"
                onError={() => setArtError(true)}
                className="w-full h-full object-cover"
              />
            ) : albumRgb ? (
              <div
                className="w-full h-full"
                style={{ background: `linear-gradient(135deg, ${accentCss}60, ${accentCss}20)` }}
              />
            ) : null}
          </div>

          {/* Track text */}
          <div className="min-w-0 flex-1">
            {loading && (
              <p className="text-sm" style={{ color: "rgba(255,255,255,0.3)" }}>Loading…</p>
            )}
            {!loading && beatmap && (
              <>
                <p className="text-base font-semibold leading-tight truncate text-white">
                  {beatmap.track.title}
                </p>
                <p className="text-sm mt-0.5 truncate" style={{ color: "rgba(255,255,255,0.45)" }}>
                  {beatmap.track.artist}
                </p>
                {beatmap.track.album && (
                  <p className="text-xs mt-0.5 truncate" style={{ color: "rgba(255,255,255,0.25)" }}>
                    {beatmap.track.album}
                  </p>
                )}
              </>
            )}
            {!loading && !beatmap && !track && (
              <p className="text-sm" style={{ color: "rgba(255,255,255,0.2)" }}>
                Select a track to begin
              </p>
            )}
          </div>

          {/* BPM + audio status */}
          {beatmap && (
            <div className="text-right shrink-0">
              <p className="text-xs tabular-nums font-mono" style={{ color: "rgba(255,255,255,0.4)" }}>
                {beatmap.track.detected_bpm.toFixed(1)}
                <span className="ml-1" style={{ color: "rgba(255,255,255,0.2)" }}>BPM</span>
              </p>
              {audioFile ? (
                <p className="text-[11px] mt-0.5" style={{ color: `${accentCss}bb` }}>♪ audio</p>
              ) : (
                <p className="text-[11px] mt-0.5" style={{ color: "rgba(255,255,255,0.15)" }}>no audio</p>
              )}
            </div>
          )}
        </div>

        {/* ── Scrubber ──────────────────────────────────────────────────────── */}
        <div className="flex items-center gap-3">
          <span className="text-[11px] tabular-nums w-9 text-right shrink-0"
                style={{ color: "rgba(255,255,255,0.35)" }}>
            {formatMs(positionMs)}
          </span>
          <div
            className="relative flex-1 cursor-pointer group"
            style={{ height: 3, borderRadius: 999, background: "rgba(255,255,255,0.08)" }}
            onClick={(e) => {
              if (!beatmap) return;
              const rect = e.currentTarget.getBoundingClientRect();
              seek(Math.round(((e.clientX - rect.left) / rect.width) * beatmap.track.duration_ms));
            }}
          >
            <div
              className="h-full rounded-full transition-all duration-75"
              style={{ width: `${progress * 100}%`, background: accentCss }}
            />
            <div
              className="absolute top-1/2 rounded-full opacity-0 group-hover:opacity-100 transition-opacity"
              style={{
                width: 10, height: 10,
                left: `${progress * 100}%`,
                transform: "translate(-50%, -50%)",
                background: accentCss,
                boxShadow: `0 0 6px 1px ${accentCss}80`,
              }}
            />
          </div>
          <span className="text-[11px] tabular-nums w-9 shrink-0"
                style={{ color: "rgba(255,255,255,0.35)" }}>
            {formatMs(duration)}
          </span>
        </div>

        {/* ── Playback controls ─────────────────────────────────────────────── */}
        <div className="flex items-center justify-center gap-5">
          <button
            onClick={() => seek(0)}
            disabled={!beatmap}
            className="transition-opacity disabled:opacity-20"
            style={{ color: "rgba(255,255,255,0.4)" }}
            title="Restart"
          >
            <svg className="w-5 h-5" fill="currentColor" viewBox="0 0 24 24">
              <path d="M6 6h2v12H6zm3.5 6 8.5 6V6z" />
            </svg>
          </button>

          <button
            onClick={togglePlay}
            disabled={!beatmap}
            className="flex items-center justify-center rounded-full transition-all disabled:opacity-25"
            style={{
              width: 46, height: 46,
              background: accentCss,
              boxShadow: beatmap ? `0 0 20px 4px ${accentCss}50` : "none",
            }}
          >
            {playing ? (
              <svg className="w-5 h-5 text-white" fill="currentColor" viewBox="0 0 24 24">
                <path d="M6 19h4V5H6v14zm8-14v14h4V5h-4z" />
              </svg>
            ) : (
              <svg className="w-5 h-5 text-white" fill="currentColor" viewBox="0 0 24 24" style={{ marginLeft: 2 }}>
                <path d="M8 5v14l11-7z" />
              </svg>
            )}
          </button>

          <div className="w-5" />
        </div>

        {/* ── Info grid ─────────────────────────────────────────────────────── */}
        {beatmap && (
          <div
            className="grid grid-cols-4 gap-3 pt-4"
            style={{ borderTop: "1px solid rgba(255,255,255,0.06)" }}
          >
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
                      style={{ color: "rgba(255,255,255,0.35)" }}>
                  {Math.round(ctx.energy * 100)}
                </span>
              </div>
            </InfoCell>
            <InfoCell label="Bar">
              <div className="flex items-center gap-1.5 mt-0.5">
                <div className="flex-1 rounded-full overflow-hidden"
                     style={{ height: 3, background: "rgba(255,255,255,0.08)" }}>
                  <div className="h-full rounded-full"
                       style={{ width: `${ctx.bar_phase * 100}%`, background: "rgba(255,255,255,0.25)" }} />
                </div>
              </div>
            </InfoCell>
          </div>
        )}

        {/* ── Section timeline ──────────────────────────────────────────────── */}
        {beatmap && beatmap.sections.length > 0 && (
          <SectionTimeline
            beatmap={beatmap}
            positionMs={positionMs}
            onSeek={seek}
            accentCss={accentCss}
          />
        )}
      </div>
    </div>
  );
}

function InfoCell({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-1.5">
      <span className="text-[9px] tracking-[0.16em] uppercase" style={{ color: "rgba(255,255,255,0.25)" }}>
        {label}
      </span>
      {children}
    </div>
  );
}

function SectionTimeline({
  beatmap,
  positionMs,
  onSeek,
  accentCss,
}: {
  beatmap: Beatmap;
  positionMs: number;
  onSeek: (ms: number) => void;
  accentCss: string;
}) {
  const beatPositions = beatPositionsMs(beatmap.timing);
  const duration = beatmap.track.duration_ms;

  return (
    <div>
      <p className="text-[9px] tracking-[0.16em] uppercase mb-2"
         style={{ color: "rgba(255,255,255,0.25)" }}>
        Sections
      </p>
      <div
        className="relative overflow-hidden cursor-pointer"
        style={{ height: 22, borderRadius: 4, background: "rgba(255,255,255,0.04)" }}
        onClick={(e) => {
          const rect = e.currentTarget.getBoundingClientRect();
          onSeek(Math.round(((e.clientX - rect.left) / rect.width) * duration));
        }}
      >
        {beatmap.sections.map((sec, idx) => {
          const startMs = beatPositions[sec.start_beat] ?? 0;
          const nextSec = beatmap.sections[idx + 1];
          const endMs = nextSec ? (beatPositions[nextSec.start_beat] ?? duration) : duration;
          const left = (startMs / duration) * 100;
          const width = ((endMs - startMs) / duration) * 100;
          const color = SECTION_COLORS[sec.kind];
          return (
            <div
              key={idx}
              className="absolute top-0 h-full flex items-center px-1.5 overflow-hidden"
              style={{
                left: `${left}%`, width: `${width}%`,
                background: `${color}28`,
                borderRight: "1px solid rgba(0,0,0,0.25)",
              }}
              title={sec.kind}
            >
              <span className="text-[9px] truncate capitalize" style={{ color: `${color}cc` }}>
                {sec.kind}
              </span>
            </div>
          );
        })}

        {/* Cue markers */}
        {beatmap.cues.map((cue, idx) => (
          <div
            key={idx}
            className="absolute top-0 h-full"
            style={{ left: `${(cue.position_ms / duration) * 100}%`, width: 1, background: "rgba(255,255,255,0.5)" }}
            title={typeof cue.kind === "string" ? cue.kind : "custom"}
          />
        ))}

        {/* Playhead */}
        <div
          className="absolute top-0 h-full"
          style={{
            left: `${(positionMs / duration) * 100}%`,
            width: 2,
            background: accentCss,
            boxShadow: `0 0 6px 1px ${accentCss}80`,
            borderRadius: 1,
          }}
        />
      </div>
    </div>
  );
}
