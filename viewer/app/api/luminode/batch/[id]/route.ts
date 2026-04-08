import { NextRequest, NextResponse } from "next/server";

const BASE = process.env.LUMINODE_URL;

export async function GET(_req: NextRequest, { params }: { params: Promise<{ id: string }> }) {
  if (!BASE) return NextResponse.json({ error: "LUMINODE_URL not configured" }, { status: 503 });
  const { id } = await params;
  const res = await fetch(`${BASE}/batch/${id}`, { cache: "no-store" });
  const data = await res.json();
  return NextResponse.json(data, { status: res.status });
}
