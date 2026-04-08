import { NextResponse } from "next/server";
import type { TrackEntry } from "@/lib/types";

const BASE = process.env.LUMINODE_URL;

export async function GET() {
  if (!BASE) return NextResponse.json({ error: "LUMINODE_URL not configured" }, { status: 503 });
  const res = await fetch(`${BASE}/beatmaps`, { cache: "no-store" });
  if (!res.ok) return NextResponse.json([], { status: res.status });

  const data = await res.json() as {
    total: number;
    beatmaps: { spotify_id: string; path: string; size_bytes: number | null }[];
  };

  const tracks: TrackEntry[] = data.beatmaps.map((b) => {
    // path is like "ab/Artist - Title.beatmap" — use filename without extension as label
    const filename = b.path.split("/").pop() ?? b.path;
    const label = filename.replace(/\.beatmap$/i, "");
    return { label, path: b.spotify_id, key: b.spotify_id };
  });

  tracks.sort((a, b) => a.label.localeCompare(b.label));
  return NextResponse.json(tracks);
}
