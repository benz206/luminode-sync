import { NextRequest, NextResponse } from "next/server";
import path from "path";
import fs from "fs";
import os from "os";

const BASE = process.env.LUMINODE_URL;

const TOKEN_FILE =
  process.env.SPOTIFY_TOKEN_FILE ??
  path.join(os.homedir(), ".local/share/luminode-sync/spotify_token.json");

interface SpotifyAuth {
  access_token: string;
  refresh_token: string;
  expires_at_epoch_secs: number;
  client_id?: string;
}

// ── Spotify token management ──────────────────────────────────────────────────

async function getValidToken(): Promise<string | null> {
  let auth: SpotifyAuth;
  try {
    auth = JSON.parse(fs.readFileSync(TOKEN_FILE, "utf-8"));
  } catch {
    return null; // no token file — can't resolve
  }

  const nowSecs = Math.floor(Date.now() / 1000);

  // Still valid (keep 60 s buffer)
  if (nowSecs + 60 < auth.expires_at_epoch_secs) {
    return auth.access_token;
  }

  // Needs refresh
  const clientId = auth.client_id ?? process.env.SPOTIFY_CLIENT_ID;
  if (!clientId) return null;

  try {
    const resp = await fetch("https://accounts.spotify.com/api/token", {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      body: new URLSearchParams({
        grant_type: "refresh_token",
        refresh_token: auth.refresh_token,
        client_id: clientId,
      }),
    });
    if (!resp.ok) return null;

    const data = await resp.json() as {
      access_token: string;
      refresh_token?: string;
      expires_in?: number;
    };

    const refreshed: SpotifyAuth = {
      ...auth,
      access_token: data.access_token,
      expires_at_epoch_secs: nowSecs + (data.expires_in ?? 3600),
    };
    if (data.refresh_token) refreshed.refresh_token = data.refresh_token;

    fs.writeFileSync(TOKEN_FILE, JSON.stringify(refreshed, null, 2));
    return refreshed.access_token;
  } catch {
    return null;
  }
}

// ── Deezer ISRC resolution ────────────────────────────────────────────────────

/** Resolve a Spotify track URL to a Deezer URL via ISRC for exact matching.
 *  Returns the original URL on any failure so the backend can try its own resolution. */
async function resolveTrackToDeezer(spotifyUrl: string): Promise<{
  url: string;
  isrc?: string;
  deezer_id?: number;
}> {
  const match = spotifyUrl.match(/open\.spotify\.com\/track\/([A-Za-z0-9]+)/);
  if (!match) return { url: spotifyUrl };
  const trackId = match[1];

  // 1. Get ISRC from Spotify
  const token = await getValidToken();
  if (!token) return { url: spotifyUrl };

  let isrc: string | undefined;
  try {
    const resp = await fetch(`https://api.spotify.com/v1/tracks/${trackId}`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    if (!resp.ok) return { url: spotifyUrl };
    const data = await resp.json() as { external_ids?: { isrc?: string } };
    isrc = data.external_ids?.isrc;
  } catch {
    return { url: spotifyUrl };
  }

  if (!isrc) return { url: spotifyUrl };

  // 2. Resolve ISRC → Deezer track ID
  try {
    const resp = await fetch(`https://api.deezer.com/track/isrc:${isrc}`);
    if (!resp.ok) return { url: spotifyUrl, isrc };
    const data = await resp.json() as { id?: number; error?: unknown };
    if (data.error || !data.id) return { url: spotifyUrl, isrc };

    return {
      url: `https://www.deezer.com/track/${data.id}`,
      isrc,
      deezer_id: data.id,
    };
  } catch {
    return { url: spotifyUrl, isrc };
  }
}

// ── Route handler ─────────────────────────────────────────────────────────────

export async function POST(req: NextRequest) {
  if (!BASE) {
    return NextResponse.json({ error: "LUMINODE_URL not configured" }, { status: 503 });
  }

  const body = await req.json() as { url?: string; [key: string]: unknown };
  const inputUrl: string = body.url ?? "";

  // Resolve single track URLs to Deezer via ISRC; playlists/albums pass through.
  let resolvedUrl = inputUrl;
  let resolution: { isrc?: string; deezer_id?: number } = {};

  if (inputUrl.includes("/track/")) {
    const result = await resolveTrackToDeezer(inputUrl);
    resolvedUrl = result.url;
    resolution = { isrc: result.isrc, deezer_id: result.deezer_id };
  }

  const res = await fetch(`${BASE}/submit`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ ...body, url: resolvedUrl }),
  });

  const data = await res.json();

  return NextResponse.json(
    {
      ...data,
      // Surface resolution info to the UI for transparency
      _resolved: resolvedUrl !== inputUrl ? {
        original_url: inputUrl,
        deezer_url: resolvedUrl,
        isrc: resolution.isrc,
        deezer_id: resolution.deezer_id,
      } : undefined,
    },
    { status: res.status },
  );
}
