#!/usr/bin/env python3
"""
seed_beatmaps.py — Submit all locally-saved beatmap tracks to the remote beatmap service.

Reads beatmaps/index.json from luminode-sync, resolves each track title against
the Spotify search API to get a Spotify ID, then POSTs to the beatmap service so
it queues download + generation with proper spotify_id indexing.

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


# ── API helper ─────────────────────────────────────────────────────────────────

def submit_to_api(spotify_id: str) -> dict | None:
    """POST to /submit and return the response JSON, or None on error."""
    url = f"{LUMINODE_URL}/submit"
    try:
        resp = requests.post(
            url,
            json={"url": f"spotify:track:{spotify_id}"},
            timeout=20,
        )
        if resp.ok:
            return resp.json()
        print(f"  [API {resp.status_code}] {resp.text[:200]}")
        return None
    except requests.RequestException as e:
        print(f"  [API error] {e}")
        return None


# ── Main ───────────────────────────────────────────────────────────────────────

def parse_titles(index: dict) -> list[str]:
    """
    Extract search-friendly titles from index.json by_title keys.
    Key format: "artist\x00title\x00duration_bucket"
    The local beatmaps use artist="unknown" with title="artist - track name".
    """
    titles = []
    for key in index.get("by_title", {}):
        parts = key.split("\x00")
        if len(parts) >= 2:
            titles.append(parts[1])  # the title/query part
    return titles


def main() -> None:
    if not SPOTIFY_CLIENT_ID or not SPOTIFY_CLIENT_SECRET:
        sys.exit(
            "Set SPOTIFY_CLIENT_ID and SPOTIFY_CLIENT_SECRET environment variables.\n"
            "Create a Spotify app at https://developer.spotify.com/dashboard."
        )

    if not INDEX_PATH.exists():
        sys.exit(f"Index not found: {INDEX_PATH}")

    index = json.loads(INDEX_PATH.read_text())

    # Skip tracks that are already indexed by spotify_id on the server.
    already_indexed = set(index.get("by_spotify_id", {}).keys())

    titles = parse_titles(index)
    if not titles:
        print("No tracks found in index.json — nothing to seed.")
        return

    print(f"Found {len(titles)} tracks in local index.")
    if DRY_RUN:
        print("[DRY RUN] No API calls will be made.\n")

    print("Fetching Spotify client-credentials token ...")
    token = get_spotify_token()

    submitted = 0
    skipped = 0
    not_found = 0

    for title in titles:
        print(f"\nSearching: {title!r}")
        spotify_id = search_spotify(token, title)

        if not spotify_id:
            print("  not found on Spotify — skipping")
            not_found += 1
            time.sleep(0.3)
            continue

        print(f"  → {spotify_id}", end="")

        if spotify_id in already_indexed:
            print("  [already in index — skipping]")
            skipped += 1
            time.sleep(0.3)
            continue

        if DRY_RUN:
            print("  [dry-run — would submit]")
            submitted += 1
            time.sleep(0.3)
            continue

        result = submit_to_api(spotify_id)
        if result:
            job_id = result.get("jobs", [{}])[0].get("job_id", "?")
            print(f"  submitted (job: {job_id})")
            submitted += 1
        else:
            print("  submission failed")

        # Be polite to Spotify rate limits (max ~30 req/min for client creds).
        time.sleep(0.5)

    print(f"\nDone. submitted={submitted} skipped={skipped} not_found={not_found}")
    if not DRY_RUN and submitted > 0:
        print(f"\nMonitor progress at: {LUMINODE_URL}/queue")
        print(f"List beatmaps at:    {LUMINODE_URL}/beatmaps")


if __name__ == "__main__":
    main()
