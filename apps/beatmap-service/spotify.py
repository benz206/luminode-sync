"""Spotify URL parsing and playlist/album resolution via spotipy (client credentials)."""
import os
import re

import spotipy
from spotipy.oauth2 import SpotifyClientCredentials

_client: spotipy.Spotify | None = None


def _sp() -> spotipy.Spotify:
    global _client
    if _client is None:
        _client = spotipy.Spotify(
            auth_manager=SpotifyClientCredentials(
                client_id=os.environ["SPOTIFY_CLIENT_ID"],
                client_secret=os.environ["SPOTIFY_CLIENT_SECRET"],
            )
        )
    return _client


def parse_url(url: str) -> tuple[str, str]:
    """Return (kind, id) where kind is 'track' | 'playlist' | 'album'."""
    m = re.match(r"spotify:(track|playlist|album):([A-Za-z0-9]+)", url)
    if m:
        return m.group(1), m.group(2)
    m = re.match(r"https://open\.spotify\.com/(track|playlist|album)/([A-Za-z0-9]+)", url)
    if m:
        return m.group(1), m.group(2)
    raise ValueError(f"Unrecognised Spotify URL/URI: {url!r}")


def _track_item(track: dict) -> dict:
    artists = ", ".join(a["name"] for a in track.get("artists", []))
    return {
        "id": track["id"],
        "title": f"{artists} - {track['name']}",
    }


def resolve_tracks(url: str) -> list[dict]:
    """
    Returns a list of {"id": spotify_id, "title": "Artist - Title"} dicts.
    Works for individual tracks, playlists, and albums.
    """
    kind, sid = parse_url(url)
    sp = _sp()

    if kind == "track":
        track = sp.track(sid)
        return [_track_item(track)]

    if kind == "playlist":
        tracks = []
        page = sp.playlist_tracks(
            sid, fields="items(track(id,name,artists)),next", limit=100
        )
        while page:
            for item in page["items"]:
                t = item.get("track")
                if t and t.get("id"):
                    tracks.append(_track_item(t))
            page = sp.next(page) if page.get("next") else None
        return tracks

    if kind == "album":
        tracks = []
        page = sp.album_tracks(sid, limit=50)
        while page:
            for t in page["items"]:
                if t.get("id"):
                    tracks.append(_track_item(t))
            page = sp.next(page) if page.get("next") else None
        return tracks

    raise ValueError(f"Unsupported kind: {kind}")
