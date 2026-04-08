import { NextRequest, NextResponse } from "next/server";
import path from "path";
import fs from "fs";

const DJTEST_DIR = path.resolve(process.cwd(), "..", "djtest");

export async function GET(req: NextRequest) {
  const filename = req.nextUrl.searchParams.get("file");
  if (!filename) return NextResponse.json({ error: "missing file" }, { status: 400 });

  const full = path.resolve(DJTEST_DIR, filename);
  if (!full.startsWith(DJTEST_DIR)) {
    return NextResponse.json({ error: "invalid path" }, { status: 400 });
  }
  if (!fs.existsSync(full)) {
    return NextResponse.json({ error: "not found" }, { status: 404 });
  }

  const stat = fs.statSync(full);
  const rangeHeader = req.headers.get("range");
  const fileSize = stat.size;

  if (rangeHeader) {
    const [startStr, endStr] = rangeHeader.replace("bytes=", "").split("-");
    const start = parseInt(startStr, 10);
    const end = endStr ? parseInt(endStr, 10) : Math.min(start + 1024 * 1024 - 1, fileSize - 1);
    const chunkSize = end - start + 1;
    const stream = fs.createReadStream(full, { start, end });

    // Convert Node stream to Web ReadableStream
    const readable = new ReadableStream({
      start(controller) {
        stream.on("data", (chunk) => controller.enqueue(chunk));
        stream.on("end", () => controller.close());
        stream.on("error", (e) => controller.error(e));
      },
    });

    return new NextResponse(readable, {
      status: 206,
      headers: {
        "Content-Range": `bytes ${start}-${end}/${fileSize}`,
        "Accept-Ranges": "bytes",
        "Content-Length": String(chunkSize),
        "Content-Type": "audio/mpeg",
      },
    });
  }

  const stream = fs.createReadStream(full);
  const readable = new ReadableStream({
    start(controller) {
      stream.on("data", (chunk) => controller.enqueue(chunk));
      stream.on("end", () => controller.close());
      stream.on("error", (e) => controller.error(e));
    },
  });

  return new NextResponse(readable, {
    headers: {
      "Content-Type": "audio/mpeg",
      "Content-Length": String(fileSize),
      "Accept-Ranges": "bytes",
    },
  });
}
