"""beatmap-service — HTTP API for queued beatmap generation."""
import logging
import uuid
from contextlib import asynccontextmanager

from fastapi import FastAPI, HTTPException, UploadFile
from fastapi.responses import FileResponse
from pydantic import BaseModel

import jobs as store
import worker
from jobs import cancel_job
from spotify import resolve_tracks, search_track

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(name)s %(message)s")
log = logging.getLogger(__name__)


@asynccontextmanager
async def lifespan(app: FastAPI):
    store.DB_PATH.parent.mkdir(parents=True, exist_ok=True)
    await store.init_db()

    # Re-queue any jobs that were in-flight when the service last stopped.
    await store.reset_running_to_pending()
    pending = await store.get_pending_job_ids()
    for job_id in pending:
        await worker.enqueue(job_id)
    if pending:
        log.info("Re-queued %d pending jobs from previous run", len(pending))

    import asyncio
    asyncio.create_task(worker.run_worker())
    asyncio.create_task(_seed_library())
    yield


app = FastAPI(title="beatmap-service", lifespan=lifespan)


# ── Library seeding ────────────────────────────────────────────────────────────

async def _seed_library() -> None:
    """
    On startup, submit any tracks from the local library index that are not yet
    indexed by Spotify ID.  Runs in the background so it never delays startup.
    """
    import asyncio
    import json

    await asyncio.sleep(5)  # let the worker settle first

    index_path = worker.BEATMAP_LIBRARY / "index.json"
    if not index_path.exists():
        log.info("Seed: no index.json yet, skipping")
        return

    index = json.loads(index_path.read_text())
    already_indexed: set[str] = set(index.get("by_spotify_id", {}).keys())

    titles = []
    for key in index.get("by_title", {}):
        parts = key.split("\x00")
        if len(parts) >= 2:
            titles.append(parts[1])

    if not titles:
        log.info("Seed: nothing in by_title, skipping")
        return

    log.info("Seed: checking %d tracks from by_title index", len(titles))

    submitted = 0
    for title in titles:
        # Resolve to a Spotify ID via search.
        spotify_id = await asyncio.to_thread(search_track, title)
        if not spotify_id:
            log.debug("Seed: no Spotify match for %r", title)
            await asyncio.sleep(0.3)
            continue

        if spotify_id in already_indexed:
            log.debug("Seed: %s already indexed, skipping", spotify_id)
            await asyncio.sleep(0.1)
            continue

        # Check whether a completed or pending job already exists for this ID.
        existing = await _find_job_by_spotify_id(spotify_id)
        if existing and existing["status"] in ("done", "pending", "running"):
            log.debug("Seed: job already exists for %s (%s)", spotify_id, existing["status"])
            await asyncio.sleep(0.1)
            continue

        # Queue the job.
        batch_id = str(uuid.uuid4())
        job_id = await store.create_job(batch_id, spotify_id, title)
        await worker.enqueue(job_id)
        log.info("Seed: queued %r → %s (job %s)", title, spotify_id, job_id)
        submitted += 1

        await asyncio.sleep(0.5)  # stay within Spotify rate limits

    log.info("Seed: done — submitted %d new jobs", submitted)


async def _find_job_by_spotify_id(spotify_id: str) -> dict | None:
    """Return the most recent job for a Spotify ID, or None."""
    import aiosqlite
    from jobs import DB_PATH
    async with aiosqlite.connect(DB_PATH) as db:
        db.row_factory = aiosqlite.Row
        async with db.execute(
            "SELECT * FROM jobs WHERE spotify_id=? ORDER BY created_at DESC LIMIT 1",
            (spotify_id,),
        ) as cur:
            row = await cur.fetchone()
            return dict(row) if row else None


# ── Request / response models ──────────────────────────────────────────────────

class SubmitRequest(BaseModel):
    url: str  # Spotify track, playlist, or album URL/URI


class JobInfo(BaseModel):
    job_id: str
    spotify_id: str
    title: str | None
    status: str
    error: str | None = None


class SubmitResponse(BaseModel):
    batch_id: str
    total: int
    jobs: list[JobInfo]


class BatchResponse(BaseModel):
    batch_id: str
    total: int
    pending: int
    running: int
    done: int
    failed: int
    jobs: list[JobInfo]


# ── Endpoints ──────────────────────────────────────────────────────────────────

@app.post("/submit", response_model=SubmitResponse)
async def submit(req: SubmitRequest):
    """
    Accept a Spotify track, playlist, or album URL/URI.
    Enqueues one job per track, processed sequentially.
    """
    try:
        tracks = resolve_tracks(req.url)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    except Exception as e:
        raise HTTPException(status_code=502, detail=f"Spotify lookup failed: {e}")

    if not tracks:
        raise HTTPException(status_code=404, detail="No tracks found at that URL")

    batch_id = str(uuid.uuid4())
    job_infos: list[JobInfo] = []

    for track in tracks:
        job_id = await store.create_job(batch_id, track["id"], track.get("title"))
        await worker.enqueue(job_id)
        job_infos.append(JobInfo(
            job_id=job_id,
            spotify_id=track["id"],
            title=track.get("title"),
            status="pending",
        ))

    log.info("Batch %s: queued %d tracks", batch_id, len(tracks))
    return SubmitResponse(batch_id=batch_id, total=len(tracks), jobs=job_infos)


@app.get("/batch/{batch_id}", response_model=BatchResponse)
async def get_batch(batch_id: str):
    rows = await store.get_batch(batch_id)
    if not rows:
        raise HTTPException(status_code=404, detail="Batch not found")

    infos = [JobInfo(**{k: r[k] for k in ("job_id", "spotify_id", "title", "status", "error")}) for r in rows]
    counts = {s: sum(1 for r in rows if r["status"] == s) for s in ("pending", "running", "done", "failed")}
    return BatchResponse(batch_id=batch_id, total=len(rows), jobs=infos, **counts)


@app.get("/job/{job_id}", response_model=JobInfo)
async def get_job(job_id: str):
    row = await store.get_job(job_id)
    if not row:
        raise HTTPException(status_code=404, detail="Job not found")
    return JobInfo(**{k: row[k] for k in ("job_id", "spotify_id", "title", "status", "error")})


@app.post("/job/{job_id}/cancel")
async def cancel_job_endpoint(job_id: str):
    """
    Cancel a job. Pending jobs are marked cancelled immediately.
    Running jobs have their subprocess killed, then are marked cancelled.
    """
    row = await store.get_job(job_id)
    if not row:
        raise HTTPException(status_code=404, detail="Job not found")

    status = row["status"]
    if status in ("done", "failed", "cancelled"):
        raise HTTPException(status_code=409, detail=f"Job already in terminal state: {status}")

    if status == "running":
        killed = await worker.kill_running(job_id)
        if killed:
            await store.update_job(job_id, store.Status.CANCELLED, "Cancelled by user")
            return {"cancelled": True, "was_running": True}
        # Race: job finished between check and kill
        raise HTTPException(status_code=409, detail="Job finished before it could be cancelled")

    # Pending: mark cancelled in DB; worker will skip it when it dequeues it
    updated = await cancel_job(job_id)
    if not updated:
        raise HTTPException(status_code=409, detail="Job could not be cancelled (status changed)")
    return {"cancelled": True, "was_running": False}


@app.get("/queue")
async def queue_status():
    return {"queued": await worker.queue_size()}


def _read_library_index() -> dict:
    import json
    index_path = worker.BEATMAP_LIBRARY / "index.json"
    if not index_path.exists():
        return {}
    return json.loads(index_path.read_text())


@app.get("/beatmaps")
async def list_beatmaps():
    """List all beatmaps currently in the library, keyed by Spotify ID."""
    index = _read_library_index()
    by_id = index.get("by_spotify_id", {})
    return {
        "total": len(by_id),
        "beatmaps": [
            {"spotify_id": sid, "path": rel, "size_bytes": _file_size(rel)}
            for sid, rel in by_id.items()
        ],
    }


def _file_size(relative: str) -> int | None:
    path = worker.BEATMAP_LIBRARY / relative
    return path.stat().st_size if path.exists() else None


@app.post("/beatmap/{spotify_id}", response_model=SubmitResponse, status_code=202)
async def queue_beatmap(spotify_id: str):
    """Queue generation for a single track by Spotify ID."""
    try:
        tracks = resolve_tracks(f"spotify:track:{spotify_id}")
    except Exception as e:
        raise HTTPException(status_code=502, detail=f"Spotify lookup failed: {e}")

    batch_id = str(uuid.uuid4())
    track = tracks[0]
    job_id = await store.create_job(batch_id, track["id"], track.get("title"))
    await worker.enqueue(job_id)
    return SubmitResponse(
        batch_id=batch_id,
        total=1,
        jobs=[JobInfo(job_id=job_id, spotify_id=track["id"], title=track.get("title"), status="pending")],
    )


@app.get("/beatmap/{spotify_id}")
async def get_beatmap(spotify_id: str):
    """
    Looks up a beatmap by Spotify ID from the library index.
    Returns the raw .beatmap file for the Pi to consume.
    """
    index = _read_library_index()
    relative = index.get("by_spotify_id", {}).get(spotify_id)
    if not relative:
        raise HTTPException(status_code=404, detail="Beatmap not found for that Spotify ID")

    beatmap_path = worker.BEATMAP_LIBRARY / relative
    if not beatmap_path.exists():
        raise HTTPException(status_code=404, detail="Beatmap file missing from disk")

    return FileResponse(
        path=str(beatmap_path),
        media_type="application/octet-stream",
        filename=beatmap_path.name,
    )


@app.post("/beatmap/{spotify_id}/upload", status_code=201)
async def upload_beatmap(spotify_id: str, file: UploadFile):
    """
    Upload a pre-generated .beatmap file and index it by Spotify ID.
    Used by the seed script to populate the library from local beatmaps.
    """
    import json as _json
    import shutil

    beatmaps_dir = worker.BEATMAP_LIBRARY / "beatmaps"
    beatmaps_dir.mkdir(parents=True, exist_ok=True)

    dest = beatmaps_dir / file.filename
    with dest.open("wb") as f:
        shutil.copyfileobj(file.file, f)

    relative = f"beatmaps/{file.filename}"

    index_path = worker.BEATMAP_LIBRARY / "index.json"
    if index_path.exists():
        index = _json.loads(index_path.read_text())
    else:
        index = {"by_title": {}, "by_spotify_id": {}}

    index.setdefault("by_spotify_id", {})[spotify_id] = relative
    index_path.write_text(_json.dumps(index))

    log.info("Uploaded beatmap for %s → %s", spotify_id, relative)
    return {"spotify_id": spotify_id, "path": relative}
