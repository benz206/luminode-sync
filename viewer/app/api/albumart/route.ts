import { NextRequest, NextResponse } from "next/server";
import path from "path";
import fs from "fs";

const BEATMAPS_DIR = path.resolve(process.cwd(), "..", "beatmaps");

export async function GET(req: NextRequest) {
  const relPath = req.nextUrl.searchParams.get("path");
  if (!relPath) return new NextResponse(null, { status: 400 });

  // Derive the .jpg path from the .beatmap path (same stem).
  const jpgRel = relPath.replace(/\.beatmap$/, ".jpg");
  const full = path.resolve(BEATMAPS_DIR, jpgRel);

  if (!full.startsWith(BEATMAPS_DIR)) {
    return new NextResponse(null, { status: 400 });
  }
  if (!fs.existsSync(full)) {
    return new NextResponse(null, { status: 404 });
  }

  const data = fs.readFileSync(full);
  return new NextResponse(data, {
    headers: {
      "Content-Type": "image/jpeg",
      "Cache-Control": "public, max-age=86400, immutable",
    },
  });
}
