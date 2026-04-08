"""beatmap-service — HTTP API for queued beatmap generation."""
import logging
import uuid
from contextlib import asynccontextmanager

from fastapi import FastAPI, HTTPException
from fastapi.responses import FileResponse
from pydantic import BaseModel

import jobs as store
import worker
from spotify import resolve_tracks

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
    yield


app = FastAPI(title="beatmap-service", lifespan=lifespan)


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


@app.get("/queue")
async def queue_status():
    return {"queued": await worker.queue_size()}


@app.get("/beatmap/{spotify_id}")
async def get_beatmap(spotify_id: str):
    """
    Looks up a beatmap by Spotify ID from the library index.
    Returns the raw .beatmap file for the Pi to consume.
    """
    import json

    library = worker.BEATMAP_LIBRARY
    index_path = library / "index.json"

    if not index_path.exists():
        raise HTTPException(status_code=404, detail="Library index not found")

    index = json.loads(index_path.read_text())
    # by_spotify_id maps spotify_id → relative path string e.g. "beatmaps/abc123.beatmap"
    relative = index.get("by_spotify_id", {}).get(spotify_id)
    if not relative:
        raise HTTPException(status_code=404, detail="Beatmap not found for that Spotify ID")

    beatmap_path = library / relative
    if not beatmap_path.exists():
        raise HTTPException(status_code=404, detail="Beatmap file missing from disk")

    return FileResponse(
        path=str(beatmap_path),
        media_type="application/octet-stream",
        filename=beatmap_path.name,
    )
