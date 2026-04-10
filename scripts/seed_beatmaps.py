#!/usr/bin/env python3
"""
seed_beatmaps.py — Upload locally-generated beatmaps to the remote beatmap service.

Reads beatmaps/index.json from luminode-sync, resolves each track title against
the Spotify search API to get a Spotify ID, then uploads the local .beatmap file
directly to the service's /beatmap/{spotify_id}/upload endpoint.

No audio is downloaded or re-generated — the existing local files are used as-is.

Usage:
    cd luminode-sync
    SPOTIFY_CLIENT_ID=xxx SPOTIFY_CLIENT_SECRET=yyy \
    LUMINODE_URL=https://luminode.bzhou.ca \
    python scripts/seed_beatmaps.py

    # Dry-run (show what would be submitted without calling the API):
    DRY_RUN=1 python scripts/seed_beatmaps.py
"""

import json
import os
import sys
import time
from pathlib import Path

try:
    import requests
except ImportError:
    sys.exit("requests not installed — run: pip install requests")

# ── Config ─────────────────────────────────────────────────────────────────────

SCRIPT_DIR = Path(__file__).parent
REPO_ROOT = SCRIPT_DIR.parent
INDEX_PATH = REPO_ROOT / "beatmaps" / "index.json"

LUMINODE_URL = os.getenv("LUMINODE_URL", "https://luminode.bzhou.ca").rstrip("/")
SPOTIFY_CLIENT_ID = os.getenv("SPOTIFY_CLIENT_ID", "")
SPOTIFY_CLIENT_SECRET = os.getenv("SPOTIFY_CLIENT_SECRET", "")
DRY_RUN = os.getenv("DRY_RUN", "0").lower() in ("1", "true", "yes")


# ── Spotify helpers ────────────────────────────────────────────────────────────

def get_spotify_token() -> str:
    resp = requests.post(
        "https://accounts.spotify.com/api/token",
        data={"grant_type": "client_credentials"},
        auth=(SPOTIFY_CLIENT_ID, SPOTIFY_CLIENT_SECRET),
        timeout=15,
    )
    resp.raise_for_status()
    return resp.json()["access_token"]


def search_spotify(token: str, query: str) -> str | None:
    """Return the best-match Spotify track ID for a search query, or None."""
    resp = requests.get(
        "https://api.spotify.com/v1/search",
        headers={"Authorization": f"Bearer {token}"},
        params={"q": query, "type": "track", "limit": 1},
        timeout=15,
    )
    if resp.status_code == 429:
        retry_after = int(resp.headers.get("Retry-After", "5"))
        print(f"  [rate-limited] sleeping {retry_after}s ...")
        time.sleep(retry_after)
        return search_spotify(token, query)  # one retry
    resp.raise_for_status()
    items = resp.json().get("tracks", {}).get("items", [])
    return items[0]["id"] if items else None


# ── Upload helper ──────────────────────────────────────────────────────────────

def upload_beatmap(spotify_id: str, beatmap_path: Path) -> bool:
    """POST the .beatmap file to /beatmap/{spotify_id}/upload. Returns True on success."""
    url = f"{LUMINODE_URL}/beatmap/{spotify_id}/upload"
    try:
        with open(beatmap_path, "rb") as f:
            resp = requests.post(
                url,
                files={"file": (beatmap_path.name, f, "application/octet-stream")},
                timeout=30,
            )
        if resp.ok:
            return True
        print(f"  [API {resp.status_code}] {resp.text[:200]}")
        return False
    except requests.RequestException as e:
        print(f"  [API error] {e}")
        return False


# ── Main ───────────────────────────────────────────────────────────────────────

def main() -> None:
    if not SPOTIFY_CLIENT_ID or not SPOTIFY_CLIENT_SECRET:
        sys.exit(
            "Set SPOTIFY_CLIENT_ID and SPOTIFY_CLIENT_SECRET environment variables.\n"
            "Create a Spotify app at https://developer.spotify.com/dashboard."
        )

    if not INDEX_PATH.exists():
        sys.exit(f"Index not found: {INDEX_PATH}")

    index = json.loads(INDEX_PATH.read_text())
    already_indexed = set(index.get("by_spotify_id", {}).keys())

    # Build list of (search_title, local_beatmap_path) from by_title entries.
    tracks: list[tuple[str, Path]] = []
    for key, rel_path in index.get("by_title", {}).items():
        parts = key.split("\x00")
        if len(parts) >= 2:
            title = parts[1]
            beatmap_path = REPO_ROOT / "beatmaps" / rel_path
            if beatmap_path.exists():
                tracks.append((title, beatmap_path))
            else:
                print(f"[warn] beatmap file missing for {title!r}: {beatmap_path}")

    if not tracks:
        print("No tracks found in index.json — nothing to seed.")
        return

    print(f"Found {len(tracks)} local beatmaps to seed.")
    if DRY_RUN:
        print("[DRY RUN] No API calls will be made.\n")

    print("Fetching Spotify client-credentials token ...")
    token = get_spotify_token()

    uploaded = 0
    skipped = 0
    not_found = 0

    for title, beatmap_path in tracks:
        print(f"\nSearching: {title!r}")
        spotify_id = search_spotify(token, title)

        if not spotify_id:
            print("  not found on Spotify — skipping")
            not_found += 1
            time.sleep(0.3)
            continue

        print(f"  → {spotify_id}", end="")

        if spotify_id in already_indexed:
            print("  [already indexed — skipping]")
            skipped += 1
            time.sleep(0.3)
            continue

        if DRY_RUN:
            print(f"  [dry-run — would upload {beatmap_path.name}]")
            uploaded += 1
            time.sleep(0.3)
            continue

        if upload_beatmap(spotify_id, beatmap_path):
            print(f"  uploaded ({beatmap_path.name})")
            already_indexed.add(spotify_id)
            uploaded += 1
        else:
            print("  upload failed")

        time.sleep(0.5)

    print(f"\nDone. uploaded={uploaded} skipped={skipped} not_found={not_found}")
    if not DRY_RUN and uploaded > 0:
        print(f"\nList beatmaps at: {LUMINODE_URL}/beatmaps")


if __name__ == "__main__":
    main()
