import { NextRequest, NextResponse } from "next/server";

const BASE = process.env.LUMINODE_URL;

export async function POST(req: NextRequest) {
  if (!BASE) return NextResponse.json({ error: "LUMINODE_URL not configured" }, { status: 503 });
  const body = await req.json();
  const res = await fetch(`${BASE}/submit`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const data = await res.json();
  return NextResponse.json(data, { status: res.status });
}
