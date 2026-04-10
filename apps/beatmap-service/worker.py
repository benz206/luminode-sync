"""Single sequential job worker — one download+generate at a time."""
import asyncio
import glob
import json
import logging
import os
import shutil
import urllib.request
from pathlib import Path

import spotify as spotify_mod
from jobs import Status, get_job, update_job

log = logging.getLogger(__name__)

BEATMAP_CLI = Path(os.getenv("BEATMAP_CLI", "/app/beatmap-cli"))
BEATMAP_LIBRARY = Path(os.getenv("BEATMAP_LIBRARY", "/data/library"))

# One queue, one worker — guarantees sequential processing.
_queue: asyncio.Queue[str] = asyncio.Queue()

# Track the currently running job so it can be cancelled.
_current_job_id: str | None = None
_current_proc: asyncio.subprocess.Process | None = None

AUDIO_EXTENSIONS = ("flac", "mp3", "m4a", "ogg", "opus", "aac", "wav")

# Fixed work directory — matches the 'folder' set in the streamrip config by entrypoint.sh.
# Safe to reuse because the worker processes one job at a time.
STREAMRIP_WORK_DIR = Path("/tmp/streamrip-work")

# Use yt-dlp fallback when DEEZER_ARL is absent or streamrip fails.
DEEZER_ARL = os.getenv("DEEZER_ARL", "")


async def enqueue(job_id: str) -> None:
    await _queue.put(job_id)


async def queue_size() -> int:
    return _queue.qsize()


async def kill_running(job_id: str) -> bool:
    """Kill the subprocess if job_id matches the currently running job."""
    global _current_proc
    if _current_job_id == job_id and _current_proc is not None:
        try:
            _current_proc.kill()
        except ProcessLookupError:
            pass
        return True
    return False


async def run_worker() -> None:
    """Run forever, consuming one job at a time."""
    log.info("Worker started")
    while True:
        job_id = await _queue.get()
        try:
            # Skip jobs that were cancelled while queued.
            job = await get_job(job_id)
            if job and job["status"] == "cancelled":
                log.info("Skipping cancelled job %s", job_id)
                continue
            await _process(job_id)
        except Exception as exc:
            log.exception("Unexpected error processing job %s", job_id)
            await update_job(job_id, Status.FAILED, str(exc))
        finally:
            _queue.task_done()


async def _run(cmd: list[str], timeout: int) -> tuple[int, str, str]:
    global _current_proc
    proc = await asyncio.create_subprocess_exec(
        *cmd,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
    )
    _current_proc = proc
    try:
        stdout, stderr = await asyncio.wait_for(proc.communicate(), timeout=timeout)
    except asyncio.TimeoutError:
        proc.kill()
        await proc.communicate()
        raise RuntimeError(f"Command timed out after {timeout}s: {cmd[0]}")
    finally:
        _current_proc = None
    return proc.returncode, stdout.decode(), stderr.decode()


# ── ISRC / Deezer resolution ──────────────────────────────────────────────────

def _get_isrc_sync(spotify_id: str) -> str | None:
    """Use the existing spotipy client (client credentials) to fetch the ISRC."""
    try:
        track = spotify_mod._sp().track(spotify_id)
        return track.get("external_ids", {}).get("isrc")
    except Exception as exc:
        log.warning("Failed to fetch ISRC for %s: %s", spotify_id, exc)
        return None


def _deezer_id_by_isrc_sync(isrc: str) -> int | None:
    """Hit the public Deezer API to resolve an ISRC to a Deezer track ID."""
    try:
        url = f"https://api.deezer.com/track/isrc:{isrc}"
        with urllib.request.urlopen(url, timeout=10) as resp:
            data = json.loads(resp.read())
        if data.get("error") or not data.get("id"):
            return None
        return int(data["id"])
    except Exception as exc:
        log.warning("Deezer ISRC lookup failed for %s: %s", isrc, exc)
        return None


def _prepare_work_dir() -> Path:
    """
    Clean and recreate the fixed streamrip work directory before each job.
    Safe because the worker is strictly sequential — only one job runs at a time.
    The folder must match what entrypoint.sh wrote into the streamrip config.
    """
    if STREAMRIP_WORK_DIR.exists():
        shutil.rmtree(STREAMRIP_WORK_DIR)
    STREAMRIP_WORK_DIR.mkdir(parents=True)
    return STREAMRIP_WORK_DIR


def _find_audio(directory: Path) -> str | None:
    """Recursively find the first audio file under directory."""
    for ext in AUDIO_EXTENSIONS:
        matches = glob.glob(str(directory / "**" / f"*.{ext}"), recursive=True)
        matches += glob.glob(str(directory / f"*.{ext}"))
        if matches:
            return matches[0]
    return None


async def _resolve_download_url(spotify_id: str) -> tuple[str, str | None]:
    """
    Returns (download_url, isrc).
    Prefers a Deezer URL (exact ISRC match) over the Spotify URL fallback.
    """
    # Run blocking I/O in a thread so we don't stall the event loop.
    isrc = await asyncio.to_thread(_get_isrc_sync, spotify_id)
    log.info("[%s] ISRC: %s", spotify_id, isrc or "(none)")

    if isrc:
        deezer_id = await asyncio.to_thread(_deezer_id_by_isrc_sync, isrc)
        if deezer_id:
            url = f"https://www.deezer.com/track/{deezer_id}"
            log.info("[%s] Resolved → Deezer ID %s", spotify_id, deezer_id)
            return url, isrc
        log.warning("[%s] ISRC %s not found on Deezer — falling back to Spotify URL", spotify_id, isrc)

    return f"https://open.spotify.com/track/{spotify_id}", isrc


# ── Job processor ─────────────────────────────────────────────────────────────

async def _process(job_id: str) -> None:
    global _current_job_id
    job = await get_job(job_id)
    if not job:
        log.warning("Job %s not found in DB — skipping", job_id)
        return

    spotify_id = job["spotify_id"]
    title = job.get("title") or spotify_id
    log.info("Processing %s (%s)", title, job_id)

    _current_job_id = job_id
    await update_job(job_id, Status.RUNNING)

    try:
        # ── Step 1: resolve download URL ───────────────────────────────────────
        log.info("[1/3] Resolving download URL for %s", spotify_id)
        download_url, isrc = await _resolve_download_url(spotify_id)
        log.info("[1/3] Download URL: %s", download_url)

        # ── Step 2: download audio ─────────────────────────────────────────────
        work_dir = _prepare_work_dir()
        audio_file: str | None = None

        # Try streamrip+Deezer first when an ARL is configured.
        if DEEZER_ARL:
            log.info("[2/3] Attempting streamrip download → %s", work_dir)
            rc, out, err = await _run(["rip", "url", download_url], timeout=300)

            refreshed = await get_job(job_id)
            if refreshed and refreshed["status"] == "cancelled":
                log.info("Job %s was cancelled during download", job_id)
                return

            if rc == 0:
                audio_file = _find_audio(work_dir)
                if audio_file:
                    log.info("[2/3] streamrip succeeded: %s", Path(audio_file).name)
                else:
                    log.warning("[2/3] streamrip exited 0 but no audio found — falling back to yt-dlp\nstdout: %s\nstderr: %s", out[:500], err[:500])
            else:
                log.warning("[2/3] streamrip exited %d — falling back to yt-dlp\nstdout: %s\nstderr: %s", rc, out[:500], err[:500])
        else:
            log.info("[2/3] No DEEZER_ARL set — using yt-dlp directly")

        # Fall back to yt-dlp with a YouTube search.
        if audio_file is None:
            search_title = job.get("title") or spotify_id
            log.info("[2/3] yt-dlp: searching YouTube for %r", search_title)
            ytdlp_cmd = [
                "yt-dlp",
                f"ytsearch1:{search_title}",
                "--no-playlist",
                "--js-runtimes", "node",
                "-x", "--audio-format", "mp3", "--audio-quality", "0",
                "-o", str(work_dir / "%(title)s.%(ext)s"),
            ]
            rc, out, err = await _run(ytdlp_cmd, timeout=300)

            refreshed = await get_job(job_id)
            if refreshed and refreshed["status"] == "cancelled":
                log.info("Job %s was cancelled during yt-dlp download", job_id)
                return

            if rc != 0:
                raise RuntimeError(f"yt-dlp exited {rc}:\nstdout: {out[:1500]}\nstderr: {err[:1500]}")

            audio_file = _find_audio(work_dir)
            if not audio_file:
                raise RuntimeError(f"yt-dlp ran successfully but no audio file found.\nstdout: {out[:500]}")
            log.info("[2/3] yt-dlp succeeded: %s", Path(audio_file).name)

        # ── Step 3: generate beatmap ───────────────────────────────────────────
        log.info("[3/3] Generating beatmap for %s", title)
        BEATMAP_LIBRARY.mkdir(parents=True, exist_ok=True)

        generate_cmd = [
            str(BEATMAP_CLI),
            "generate",
            audio_file,
            "--spotify-id", spotify_id,
            "--library", str(BEATMAP_LIBRARY),
        ]
        if isrc:
            generate_cmd += ["--isrc", isrc]

        rc, out, err = await _run(generate_cmd, timeout=120)
        if rc != 0:
            raise RuntimeError(f"beatmap-cli exited {rc}:\n{err[:2000]}")

        log.info("[3/3] Beatmap generated for %s", title)

        await update_job(job_id, Status.DONE)
    finally:
        _current_job_id = None
