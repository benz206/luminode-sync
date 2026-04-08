"""Single sequential job worker — one download+generate at a time."""
import asyncio
import glob
import logging
import os
import tempfile
from pathlib import Path

from jobs import Status, get_job, update_job

log = logging.getLogger(__name__)

BEATMAP_CLI = Path(os.getenv("BEATMAP_CLI", "/app/beatmap-cli"))
BEATMAP_LIBRARY = Path(os.getenv("BEATMAP_LIBRARY", "/data/library"))

# One queue, one worker — guarantees sequential processing.
_queue: asyncio.Queue[str] = asyncio.Queue()


async def enqueue(job_id: str) -> None:
    await _queue.put(job_id)


async def queue_size() -> int:
    return _queue.qsize()


async def run_worker() -> None:
    """Run forever, consuming one job at a time."""
    log.info("Worker started")
    while True:
        job_id = await _queue.get()
        try:
            await _process(job_id)
        except Exception as exc:
            log.exception("Unexpected error processing job %s", job_id)
            await update_job(job_id, Status.FAILED, str(exc))
        finally:
            _queue.task_done()


async def _run(cmd: list[str], timeout: int) -> tuple[int, str, str]:
    proc = await asyncio.create_subprocess_exec(
        *cmd,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
    )
    try:
        stdout, stderr = await asyncio.wait_for(proc.communicate(), timeout=timeout)
    except asyncio.TimeoutError:
        proc.kill()
        await proc.communicate()
        raise RuntimeError(f"Command timed out after {timeout}s: {cmd[0]}")
    return proc.returncode, stdout.decode(), stderr.decode()


async def _process(job_id: str) -> None:
    job = await get_job(job_id)
    if not job:
        log.warning("Job %s not found in DB — skipping", job_id)
        return

    spotify_id = job["spotify_id"]
    title = job.get("title") or spotify_id
    log.info("Processing %s (%s)", title, job_id)

    await update_job(job_id, Status.RUNNING)

    with tempfile.TemporaryDirectory(prefix="beatmap-") as tmpdir:
        track_url = f"https://open.spotify.com/track/{spotify_id}"

        # ── Step 1: download audio ─────────────────────────────────────────────
        rc, out, err = await _run(
            ["spotdl", "download", track_url, "--output", tmpdir + "/", "--format", "mp3"],
            timeout=300,
        )
        if rc != 0:
            raise RuntimeError(f"spotdl exited {rc}: {err[:500]}")

        audio_files = (
            glob.glob(f"{tmpdir}/*.mp3")
            + glob.glob(f"{tmpdir}/*.opus")
            + glob.glob(f"{tmpdir}/*.flac")
        )
        if not audio_files:
            raise RuntimeError(
                f"spotdl ran successfully but no audio file found. stdout: {out[:300]}"
            )

        audio_file = audio_files[0]
        log.info("Downloaded: %s", Path(audio_file).name)

        # ── Step 2: generate beatmap ───────────────────────────────────────────
        BEATMAP_LIBRARY.mkdir(parents=True, exist_ok=True)
        rc, out, err = await _run(
            [
                str(BEATMAP_CLI),
                "generate",
                audio_file,
                "--spotify-id", spotify_id,
                "--library", str(BEATMAP_LIBRARY),
            ],
            timeout=120,
        )
        if rc != 0:
            raise RuntimeError(f"beatmap-cli exited {rc}: {err[:500]}")

        log.info("Beatmap generated for %s", title)

    # tmpdir (and audio file) cleaned up automatically on exit from `with` block
    await update_job(job_id, Status.DONE)
