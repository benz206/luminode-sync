"use client";

import { useEffect, useState } from "react";
import type { TrackEntry } from "@/lib/types";
import TrackList from "@/components/TrackList";
import BeatmapPlayer from "@/components/BeatmapPlayer";

export default function Home() {
  const [tracks, setTracks] = useState<TrackEntry[]>([]);
  const [selected, setSelected] = useState<TrackEntry | null>(null);
  const [accentColor, setAccentColor] = useState<string | undefined>(undefined);

  useEffect(() => {
    fetch("/api/tracks")
      .then((r) => r.json())
      .then(setTracks)
      .catch(console.error);
  }, []);

  return (
    <div className="flex h-full overflow-hidden" style={{ background: "#080808" }}>
      <TrackList
        tracks={tracks}
        selected={selected?.path ?? null}
        onSelect={setSelected}
        accentColor={accentColor}
      />
      <main className="flex-1 flex flex-col overflow-hidden min-w-0">
        <BeatmapPlayer track={selected} onAccentChange={setAccentColor} />
      </main>
    </div>
  );
}
