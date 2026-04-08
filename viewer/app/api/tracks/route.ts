import { NextResponse } from "next/server";
import path from "path";
import fs from "fs";
import { decode } from "@msgpack/msgpack";
import type { TrackEntry } from "@/lib/types";

const BEATMAPS_DIR = path.resolve(process.cwd(), "..", "beatmaps");

export async function GET() {
  const indexPath = path.join(BEATMAPS_DIR, "index.json");
  const raw = fs.readFileSync(indexPath, "utf-8");
  const index = JSON.parse(raw) as { by_title: Record<string, string> };

  const tracks: TrackEntry[] = Object.entries(index.by_title).map(([key, relPath]) => {
    const parts = key.split("\0");
    const titlePart = parts[1] ?? key;
    const label = titlePart
      .split(" ")
      .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
      .join(" ");

    // Read dominant_color from the beatmap without fully decoding — just peek at
    // the msgpack map to extract the track.dominant_color field efficiently.
    let dominant_color: [number, number, number] | undefined;
    try {
      const bmPath = path.resolve(BEATMAPS_DIR, relPath);
      const buf = fs.readFileSync(bmPath);
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const bm = decode(buf) as any;
      const dc = bm?.track?.dominant_color;
      if (Array.isArray(dc) && dc.length === 3) {
        dominant_color = [dc[0], dc[1], dc[2]];
      }
    } catch {
      // beatmap unreadable or missing field — leave dominant_color undefined
    }

    return { label, path: relPath, key, dominant_color };
  });

  tracks.sort((a, b) => a.label.localeCompare(b.label));
  return NextResponse.json(tracks);
}
