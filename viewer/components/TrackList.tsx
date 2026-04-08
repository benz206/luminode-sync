"use client";

import { useState } from "react";
import type { TrackEntry } from "@/lib/types";

interface Props {
  tracks: TrackEntry[];
  selected: string | null;
  onSelect: (track: TrackEntry) => void;
  accentColor?: string;
}

export default function TrackList({ tracks, selected, onSelect, accentColor }: Props) {
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
      {/* Header */}
      <div className="px-5 pt-5 pb-3 shrink-0">
        <p className="text-[10px] font-semibold tracking-[0.18em] uppercase"
           style={{ color: "rgba(255,255,255,0.3)" }}>
          Library
        </p>
        <p className="text-[11px] mt-0.5" style={{ color: "rgba(255,255,255,0.18)" }}>
          {tracks.length} tracks
        </p>
      </div>

      <div className="h-px mx-5 shrink-0" style={{ background: "rgba(255,255,255,0.06)" }} />

      <ul className="flex-1 overflow-y-auto py-2">
        {tracks.map((t) => (
          <TrackItem
            key={t.path}
            track={t}
            isSelected={t.path === selected}
            accentColor={accentColor}
            onSelect={onSelect}
          />
        ))}
      </ul>
    </aside>
  );
}

function TrackItem({
  track,
  isSelected,
  accentColor,
  onSelect,
}: {
  track: TrackEntry;
  isSelected: boolean;
  accentColor?: string;
  onSelect: (t: TrackEntry) => void;
}) {
  const [imgError, setImgError] = useState(false);
  const artUrl = `/api/albumart?path=${encodeURIComponent(track.path)}`;
  const dotColor = track.dominant_color
    ? `rgb(${track.dominant_color[0]},${track.dominant_color[1]},${track.dominant_color[2]})`
    : undefined;

  const borderColor = isSelected ? (accentColor ?? "rgba(255,255,255,0.4)") : "transparent";

  return (
    <li>
      <button
        onClick={() => onSelect(track)}
        className="w-full text-left flex items-center gap-3 px-3 py-2 transition-all duration-150"
        style={{
          background: isSelected
            ? accentColor ? `${accentColor}18` : "rgba(255,255,255,0.07)"
            : "transparent",
          borderLeft: `2px solid ${borderColor}`,
        }}
      >
        {/* Album art thumbnail / colour dot fallback */}
        <div
          className="shrink-0 rounded overflow-hidden flex items-center justify-center"
          style={{ width: 32, height: 32, background: "rgba(255,255,255,0.06)" }}
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
            <div
              className="w-3 h-3 rounded-full"
              style={{ background: dotColor, boxShadow: `0 0 6px 1px ${dotColor}80` }}
            />
          ) : (
            <div className="w-3 h-3 rounded-full" style={{ background: "rgba(255,255,255,0.12)" }} />
          )}
        </div>

        <span
          className="text-[13px] leading-snug truncate font-medium transition-colors"
          style={{ color: isSelected ? "#fff" : "rgba(255,255,255,0.5)" }}
        >
          {track.label}
        </span>
      </button>
    </li>
  );
}
