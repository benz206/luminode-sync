import { NextRequest, NextResponse } from "next/server";
import path from "path";
import fs from "fs";
import { decode } from "@msgpack/msgpack";

const BEATMAPS_DIR = path.resolve(process.cwd(), "..", "beatmaps");

export async function GET(req: NextRequest) {
  const relPath = req.nextUrl.searchParams.get("path");
  if (!relPath) return NextResponse.json({ error: "missing path" }, { status: 400 });

  // Safety: only allow paths inside BEATMAPS_DIR
  const full = path.resolve(BEATMAPS_DIR, relPath);
  if (!full.startsWith(BEATMAPS_DIR)) {
    return NextResponse.json({ error: "invalid path" }, { status: 400 });
  }

  const buf = fs.readFileSync(full);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const beatmap = decode(buf) as any;

  // Normalize typed arrays to plain arrays for JSON serialisation
  function normalize(v: unknown): unknown {
    if (v instanceof Uint8Array || v instanceof Uint16Array || v instanceof Int32Array) {
      return Array.from(v);
    }
    if (Array.isArray(v)) return v.map(normalize);
    if (v !== null && typeof v === "object") {
      return Object.fromEntries(
        Object.entries(v as Record<string, unknown>).map(([k, val]) => [k, normalize(val)])
      );
    }
    return v;
  }

  return NextResponse.json(normalize(beatmap));
}
