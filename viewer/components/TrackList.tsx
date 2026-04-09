"use client";

import { useEffect, useRef, useState } from "react";
import type { TrackEntry } from "@/lib/types";
import BatchJobs from "@/components/BatchJobs";

export type Source = "local" | "remote";

interface Props {
  source: Source;
  onSourceChange: (s: Source) => void;
  tracks: TrackEntry[];
  queue: TrackEntry[];
  queueIndex: number;
  selected: string | null;
  accentColor?: string;
  onSelect: (track: TrackEntry) => void;
  onQueueJump: (idx: number) => void;
  onQueueRemove: (idx: number) => void;
  onPlayNext: (track: TrackEntry) => void;
  onAddToQueue: (track: TrackEntry) => void;
  onBatchComplete?: () => void;
}

type Tab = "library" | "queue" | "jobs";

interface CtxMenu {
  x: number;
  y: number;
  track: TrackEntry;
}

export default function TrackList({
  source,
  onSourceChange,
  tracks,
  queue,
  queueIndex,
  selected,
  accentColor,
  onSelect,
  onQueueJump,
  onQueueRemove,
  onPlayNext,
  onAddToQueue,
  onBatchComplete,
}: Props) {
  const [tab, setTab] = useState<Tab>("library");
  const [query, setQuery] = useState("");
  const [ctxMenu, setCtxMenu] = useState<CtxMenu | null>(null);

  const closeCtx = () => setCtxMenu(null);

  useEffect(() => {
    if (!ctxMenu) return;
    const handler = () => closeCtx();
    window.addEventListener("click", handler);
    window.addEventListener("contextmenu", handler);
    return () => {
      window.removeEventListener("click", handler);
      window.removeEventListener("contextmenu", handler);
    };
  }, [ctxMenu]);

  const filtered = query.trim()
    ? tracks.filter((t) => t.label.toLowerCase().includes(query.toLowerCase()))
    : tracks;

  // Only slice upcoming when something is actually playing; queueIndex=-1 means
  // nothing selected yet and we must not show all tracks as "Up next".
  const upcoming = queueIndex >= 0 ? queue.slice(queueIndex + 1) : [];

  const accent = accentColor ?? "rgba(255,255,255,0.4)";

  return (
    <aside
      className="w-64 shrink-0 flex flex-col overflow-hidden"
      style={{
        background: "rgba(255,255,255,0.025)",
        borderRight: "1px solid rgba(255,255,255,0.06)",
        ["--scrollbar-thumb" as string]: accentColor ? `${accentColor}40` : "rgba(255,255,255,0.12)",
        ["--scrollbar-thumb-hover" as string]: accentColor ? `${accentColor}70` : "rgba(255,255,255,0.28)",
      }}
    >
      {/* Source toggle */}
      <div className="px-3 pt-3 pb-2 shrink-0 flex gap-1">
        {(["local", "remote"] as Source[]).map((s) => (
          <button
            key={s}
            onClick={() => { onSourceChange(s); if (s === "remote") setTab("jobs"); }}
            className="flex-1 text-[10px] font-semibold tracking-wide uppercase py-1 rounded-md transition-colors"
            style={{
              color: source === s ? "#fff" : "rgba(255,255,255,0.25)",
              background: source === s ? `${accent}22` : "rgba(255,255,255,0.04)",
              border: `1px solid ${source === s ? `${accent}44` : "rgba(255,255,255,0.06)"}`,
            }}
          >
            {s === "local" ? "Local" : "Remote"}
          </button>
        ))}
      </div>

      {/* Tab bar */}
      <div className="px-3 pb-0 shrink-0 flex gap-1">
        {(source === "local" ? ["library", "queue"] : ["library", "queue", "jobs"]).map((t) => (
          <button
            key={t}
            onClick={() => setTab(t as Tab)}
            className="flex-1 text-[11px] font-semibold tracking-wide uppercase py-1.5 rounded-t-md transition-colors"
            style={{
              color: tab === t ? "#fff" : "rgba(255,255,255,0.3)",
              background: tab === t ? "rgba(255,255,255,0.07)" : "transparent",
              borderBottom: `2px solid ${tab === t ? accent : "transparent"}`,
            }}
          >
            {t === "library" ? "Library"
           : t === "queue"   ? `Queue${queue.length ? ` · ${queue.length}` : ""}`
           : "Jobs"}
          </button>
        ))}
      </div>

      <div className="h-px mx-3 shrink-0" style={{ background: "rgba(255,255,255,0.06)" }} />

      {/* ── Library tab ──────────────────────────────────────────────────────── */}
      {tab === "library" && (
        <>
          {/* Header / search */}
          <div className="px-3 pt-3 pb-2 shrink-0 flex flex-col gap-2">
            <p className="text-[10px] font-semibold tracking-[0.18em] uppercase"
               style={{ color: "rgba(255,255,255,0.3)" }}>
              {filtered.length}{query ? ` of ${tracks.length}` : ""} tracks
            </p>

            <div className="relative">
              <svg className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3 h-3 pointer-events-none"
                   style={{ color: "rgba(255,255,255,0.25)" }} viewBox="0 0 24 24" fill="currentColor">
                <path d="M15.5 14h-.79l-.28-.27A6.471 6.471 0 0 0 16 9.5 6.5 6.5 0 1 0 9.5 16c1.61 0 3.09-.59 4.23-1.57l.27.28v.79l5 4.99L20.49 19l-4.99-5zm-6 0C7.01 14 5 11.99 5 9.5S7.01 5 9.5 5 14 7.01 14 9.5 11.99 14 9.5 14z"/>
              </svg>
              <input
                type="text"
                placeholder="Search…"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                className="w-full text-[12px] pl-7 pr-7 py-1.5 rounded-md outline-none transition-colors"
                style={{
                  background: "rgba(255,255,255,0.06)",
                  border: "1px solid rgba(255,255,255,0.08)",
                  color: "rgba(255,255,255,0.8)",
                  caretColor: accentColor ?? "white",
                }}
              />
              {query && (
                <button
                  onClick={() => setQuery("")}
                  className="absolute right-2 top-1/2 -translate-y-1/2"
                  style={{ color: "rgba(255,255,255,0.3)" }}
                >
                  <svg className="w-3 h-3" viewBox="0 0 24 24" fill="currentColor">
                    <path d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z"/>
                  </svg>
                </button>
              )}
            </div>
          </div>

          <ul className="flex-1 overflow-y-auto py-1">
            {filtered.length === 0 && (
              <li className="px-4 py-3 text-[12px]" style={{ color: "rgba(255,255,255,0.2)" }}>
                No tracks found
              </li>
            )}
            {filtered.map((t) => (
              <TrackItem
                key={t.path}
                track={t}
                isSelected={t.path === selected}
                accentColor={accentColor}
                onSelect={onSelect}
                onContextMenu={(e, track) => {
                  e.preventDefault();
                  setCtxMenu({ x: e.clientX, y: e.clientY, track });
                }}
              />
            ))}
          </ul>
        </>
      )}

      {/* ── Queue tab ────────────────────────────────────────────────────────── */}
      {tab === "queue" && (
        <div className="flex-1 overflow-y-auto">
          {/* Now playing */}
          {queueIndex >= 0 && queue[queueIndex] && (
            <div className="px-3 pt-3 pb-1">
              <p className="text-[10px] font-semibold tracking-[0.18em] uppercase mb-1"
                 style={{ color: "rgba(255,255,255,0.3)" }}>Now playing</p>
              <QueueItem
                track={queue[queueIndex]}
                isPlaying
                accentColor={accentColor}
                onJump={() => onQueueJump(queueIndex)}
                onRemove={null}
              />
            </div>
          )}

          {/* Upcoming */}
          {upcoming.length > 0 && (
            <div className="px-3 pt-2 pb-1">
              <p className="text-[10px] font-semibold tracking-[0.18em] uppercase mb-1"
                 style={{ color: "rgba(255,255,255,0.3)" }}>
                Up next · {upcoming.length}
              </p>
              <ul>
                {upcoming.map((t, i) => {
                  const absIdx = queueIndex + 1 + i;
                  return (
                    <QueueItem
                      key={`${t.path}-${absIdx}`}
                      track={t}
                      isPlaying={false}
                      accentColor={accentColor}
                      onJump={() => onQueueJump(absIdx)}
                      onRemove={() => onQueueRemove(absIdx)}
                    />
                  );
                })}
              </ul>
            </div>
          )}

          {queueIndex < 0 && (
            <p className="px-4 py-6 text-[12px]" style={{ color: "rgba(255,255,255,0.2)" }}>
              Nothing playing — select a track to start
            </p>
          )}
          {queueIndex >= 0 && upcoming.length === 0 && (
            <p className="px-4 py-6 text-[12px]" style={{ color: "rgba(255,255,255,0.2)" }}>
              End of queue
            </p>
          )}
        </div>
      )}

      {/* ── Jobs tab ─────────────────────────────────────────────────────────── */}
      {tab === "jobs" && (
        <BatchJobs accentColor={accentColor} onBatchComplete={onBatchComplete} />
      )}

      {/* ── Context menu ─────────────────────────────────────────────────────── */}
      {ctxMenu && (
        <ContextMenu
          x={ctxMenu.x}
          y={ctxMenu.y}
          onPlayNow={() => { onSelect(ctxMenu.track); closeCtx(); }}
          onPlayNext={() => { onPlayNext(ctxMenu.track); closeCtx(); }}
          onAddToQueue={() => { onAddToQueue(ctxMenu.track); closeCtx(); }}
          onClose={closeCtx}
        />
      )}
    </aside>
  );
}

// ── TrackItem (library) ──────────────────────────────────────────────────────

function TrackItem({
  track,
  isSelected,
  accentColor,
  onSelect,
  onContextMenu,
}: {
  track: TrackEntry;
  isSelected: boolean;
  accentColor?: string;
  onSelect: (t: TrackEntry) => void;
  onContextMenu: (e: React.MouseEvent, t: TrackEntry) => void;
}) {
  const [imgError, setImgError] = useState(false);
  const artUrl = `/api/albumart?path=${encodeURIComponent(track.path)}`;
  const dotColor = track.dominant_color
    ? `rgb(${track.dominant_color[0]},${track.dominant_color[1]},${track.dominant_color[2]})`
    : undefined;

  return (
    <li>
      <button
        onClick={() => onSelect(track)}
        onContextMenu={(e) => onContextMenu(e, track)}
        className="w-full text-left flex items-center gap-2.5 px-3 py-1.5 transition-all duration-100"
        style={{
          background: isSelected
            ? accentColor ? `${accentColor}16` : "rgba(255,255,255,0.07)"
            : "transparent",
          borderLeft: `2px solid ${isSelected ? (accentColor ?? "rgba(255,255,255,0.4)") : "transparent"}`,
        }}
      >
        <div
          className="shrink-0 rounded overflow-hidden flex items-center justify-center"
          style={{ width: 30, height: 30, background: "rgba(255,255,255,0.06)" }}
        >
          {!imgError ? (
            <img
              src={artUrl}
              alt=""
              loading="lazy"
              onError={() => setImgError(true)}
              className="w-full h-full object-cover"
            />
          ) : dotColor ? (
            <div className="w-2.5 h-2.5 rounded-full"
                 style={{ background: dotColor, boxShadow: `0 0 5px 1px ${dotColor}80` }} />
          ) : (
            <div className="w-2.5 h-2.5 rounded-full" style={{ background: "rgba(255,255,255,0.1)" }} />
          )}
        </div>

        <span
          className="text-[12px] leading-snug truncate font-medium"
          style={{ color: isSelected ? "#fff" : "rgba(255,255,255,0.5)" }}
        >
          {track.label}
        </span>
      </button>
    </li>
  );
}

// ── QueueItem ────────────────────────────────────────────────────────────────

function QueueItem({
  track,
  isPlaying,
  accentColor,
  onJump,
  onRemove,
}: {
  track: TrackEntry;
  isPlaying: boolean;
  accentColor?: string;
  onJump: () => void;
  onRemove: (() => void) | null;
}) {
  const [imgError, setImgError] = useState(false);
  const artUrl = `/api/albumart?path=${encodeURIComponent(track.path)}`;
  const dotColor = track.dominant_color
    ? `rgb(${track.dominant_color[0]},${track.dominant_color[1]},${track.dominant_color[2]})`
    : undefined;
  const accent = accentColor ?? "rgba(255,255,255,0.4)";

  return (
    <li className="flex items-center gap-2 py-1 group">
      <button
        onClick={onJump}
        className="flex-1 min-w-0 flex items-center gap-2 rounded-md px-1.5 py-1 transition-colors"
        style={{
          background: isPlaying ? `${accent}14` : "transparent",
        }}
      >
        <div
          className="shrink-0 rounded overflow-hidden flex items-center justify-center"
          style={{ width: 26, height: 26, background: "rgba(255,255,255,0.06)" }}
        >
          {!imgError ? (
            <img
              src={artUrl}
              alt=""
              loading="lazy"
              onError={() => setImgError(true)}
              className="w-full h-full object-cover"
            />
          ) : dotColor ? (
            <div className="w-2 h-2 rounded-full" style={{ background: dotColor }} />
          ) : (
            <div className="w-2 h-2 rounded-full" style={{ background: "rgba(255,255,255,0.1)" }} />
          )}
        </div>

        <span
          className="text-[11px] leading-snug truncate"
          style={{ color: isPlaying ? "#fff" : "rgba(255,255,255,0.55)" }}
        >
          {track.label}
        </span>

        {isPlaying && (
          <span className="shrink-0 text-[9px] font-semibold tracking-wide uppercase ml-auto"
                style={{ color: accent }}>
            ▶
          </span>
        )}
      </button>

      {onRemove && (
        <button
          onClick={onRemove}
          className="shrink-0 opacity-0 group-hover:opacity-100 transition-opacity rounded p-0.5"
          style={{ color: "rgba(255,255,255,0.3)" }}
          title="Remove"
        >
          <svg className="w-3 h-3" viewBox="0 0 24 24" fill="currentColor">
            <path d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z"/>
          </svg>
        </button>
      )}
    </li>
  );
}

// ── ContextMenu ──────────────────────────────────────────────────────────────

function ContextMenu({
  x,
  y,
  onPlayNow,
  onPlayNext,
  onAddToQueue,
  onClose,
}: {
  x: number;
  y: number;
  onPlayNow: () => void;
  onPlayNext: () => void;
  onAddToQueue: () => void;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);

  // Clamp to viewport
  const [pos, setPos] = useState({ x, y });
  useEffect(() => {
    if (!ref.current) return;
    const { width, height } = ref.current.getBoundingClientRect();
    setPos({
      x: Math.min(x, window.innerWidth - width - 8),
      y: Math.min(y, window.innerHeight - height - 8),
    });
  }, [x, y]);

  const items: { label: string; onClick: () => void }[] = [
    { label: "Play now", onClick: onPlayNow },
    { label: "Play next", onClick: onPlayNext },
    { label: "Add to queue", onClick: onAddToQueue },
  ];

  return (
    <div
      ref={ref}
      className="fixed z-50 flex flex-col overflow-hidden"
      style={{
        left: pos.x,
        top: pos.y,
        background: "#1a1a1a",
        border: "1px solid rgba(255,255,255,0.1)",
        borderRadius: 8,
        boxShadow: "0 8px 32px rgba(0,0,0,0.6)",
        minWidth: 160,
      }}
      onClick={(e) => e.stopPropagation()}
      onContextMenu={(e) => { e.preventDefault(); e.stopPropagation(); onClose(); }}
    >
      {items.map(({ label, onClick }) => (
        <button
          key={label}
          onClick={onClick}
          className="text-left px-3 py-2 text-[12px] transition-colors"
          style={{ color: "rgba(255,255,255,0.8)" }}
          onMouseEnter={(e) => (e.currentTarget.style.background = "rgba(255,255,255,0.08)")}
          onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
        >
          {label}
        </button>
      ))}
    </div>
  );
}
