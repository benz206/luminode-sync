"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import type { TrackEntry } from "@/lib/types";
import TrackList from "@/components/TrackList";
import BeatmapPlayer from "@/components/BeatmapPlayer";

function fisherYates<T>(arr: T[]): T[] {
  const a = [...arr];
  for (let i = a.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [a[i], a[j]] = [a[j], a[i]];
  }
  return a;
}

export default function Home() {
  const [tracks, setTracks]             = useState<TrackEntry[]>([]);
  const [queue, setQueue]               = useState<TrackEntry[]>([]);
  const [queueIndex, setQueueIndex]     = useState(-1);
  const [isShuffled, setIsShuffled]     = useState(false);
  const [volume, setVolume]             = useState(0.8);
  const [colorMode, setColorMode]       = useState<"album" | "rainbow">("album");
  const [beatMultiplier, setBeatMultiplier] = useState<1 | 2 | 3>(1);
  const [syncOffset, setSyncOffset]     = useState(0);       // ms; tweaks beat timing
  const [accentColor, setAccentColor]   = useState<string | undefined>(undefined);

  // Keep a stable ref of queue + queueIndex for callbacks that close over stale values
  const queueRef      = useRef(queue);
  const queueIndexRef = useRef(queueIndex);
  useEffect(() => { queueRef.current = queue; },      [queue]);
  useEffect(() => { queueIndexRef.current = queueIndex; }, [queueIndex]);

  useEffect(() => {
    fetch("/api/tracks")
      .then((r) => r.json())
      .then((t: TrackEntry[]) => { setTracks(t); setQueue(t); })
      .catch(console.error);
  }, []);

  const currentTrack = queueIndex >= 0 ? (queue[queueIndex] ?? null) : null;

  // ── Track selection ─────────────────────────────────────────────────────────
  const selectTrack = (track: TrackEntry) => {
    const idx = queue.findIndex((t) => t.path === track.path);
    if (idx >= 0) { setQueueIndex(idx); return; }
    // Not in current queue → restore original order and jump to it
    setQueue(tracks);
    setIsShuffled(false);
    const origIdx = tracks.findIndex((t) => t.path === track.path);
    setQueueIndex(origIdx >= 0 ? origIdx : 0);
  };

  // ── Queue mutations ─────────────────────────────────────────────────────────
  /** Insert immediately after the currently playing track. */
  const addToQueueNext = useCallback((track: TrackEntry) => {
    setQueue((q) => {
      const next = [...q];
      next.splice(queueIndexRef.current + 1, 0, track);
      return next;
    });
  }, []);

  /** Append to the end of the queue. */
  const addToQueueEnd = useCallback((track: TrackEntry) => {
    setQueue((q) => [...q, track]);
  }, []);

  /** Remove the track at `idx` from the queue (cannot remove the current track). */
  const removeFromQueue = useCallback((idx: number) => {
    const cur = queueIndexRef.current;
    if (idx === cur) return;
    setQueue((q) => {
      const next = [...q];
      next.splice(idx, 1);
      return next;
    });
    if (idx < cur) setQueueIndex((i) => i - 1);
  }, []);

  /** Jump directly to a queue position. */
  const jumpToQueue = useCallback((idx: number) => {
    setQueueIndex(idx);
  }, []);

  // ── Navigation ──────────────────────────────────────────────────────────────
  const goNext = useCallback(() => {
    setQueueIndex((i) => (i < queueRef.current.length - 1 ? i + 1 : i));
  }, []);

  const goPrev = useCallback(() => {
    setQueueIndex((i) => Math.max(0, i - 1));
  }, []);

  // ── Shuffle ─────────────────────────────────────────────────────────────────
  const toggleShuffle = () => {
    if (isShuffled) {
      const cur = currentTrack;
      const restored = [...tracks];
      const newIdx = cur ? restored.findIndex((t) => t.path === cur.path) : 0;
      setQueue(restored);
      setQueueIndex(Math.max(0, newIdx));
      setIsShuffled(false);
    } else {
      const cur = currentTrack;
      const rest = fisherYates(tracks.filter((t) => t.path !== cur?.path));
      setQueue(cur ? [cur, ...rest] : rest);
      setQueueIndex(0);
      setIsShuffled(true);
    }
  };

  return (
    <div className="flex h-full overflow-hidden" style={{ background: "#080808" }}>
      <TrackList
        tracks={tracks}
        queue={queue}
        queueIndex={queueIndex}
        selected={currentTrack?.path ?? null}
        accentColor={accentColor}
        onSelect={selectTrack}
        onQueueJump={jumpToQueue}
        onQueueRemove={removeFromQueue}
        onPlayNext={addToQueueNext}
        onAddToQueue={addToQueueEnd}
      />
      <main className="flex-1 flex flex-col overflow-hidden min-w-0">
        <BeatmapPlayer
          track={currentTrack}
          hasPrev={queueIndex > 0}
          hasNext={queueIndex < queue.length - 1}
          onPrev={goPrev}
          onNext={goNext}
          onEnded={goNext}
          isShuffled={isShuffled}
          onShuffleToggle={toggleShuffle}
          volume={volume}
          onVolumeChange={setVolume}
          colorMode={colorMode}
          onColorModeChange={setColorMode}
          beatMultiplier={beatMultiplier}
          onBeatMultiplierChange={setBeatMultiplier}
          syncOffset={syncOffset}
          onSyncOffsetChange={setSyncOffset}
          onAccentChange={setAccentColor}
        />
      </main>
    </div>
  );
}
