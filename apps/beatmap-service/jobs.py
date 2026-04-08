"""SQLite-backed job store."""
import os
import uuid
from enum import Enum
from pathlib import Path

import aiosqlite

DB_PATH = Path(os.getenv("DATA_DIR", "/data")) / "jobs.db"


class Status(str, Enum):
    PENDING = "pending"
    RUNNING = "running"
    DONE = "done"
    FAILED = "failed"


async def init_db() -> None:
    async with aiosqlite.connect(DB_PATH) as db:
        await db.execute("""
            CREATE TABLE IF NOT EXISTS jobs (
                job_id     TEXT PRIMARY KEY,
                batch_id   TEXT NOT NULL,
                spotify_id TEXT NOT NULL,
                title      TEXT,
                status     TEXT NOT NULL DEFAULT 'pending',
                error      TEXT,
                created_at REAL DEFAULT (unixepoch('subsec')),
                updated_at REAL DEFAULT (unixepoch('subsec'))
            )
        """)
        await db.execute("CREATE INDEX IF NOT EXISTS idx_batch   ON jobs(batch_id)")
        await db.execute("CREATE INDEX IF NOT EXISTS idx_spotify ON jobs(spotify_id)")
        await db.commit()


async def create_job(batch_id: str, spotify_id: str, title: str | None) -> str:
    job_id = str(uuid.uuid4())
    async with aiosqlite.connect(DB_PATH) as db:
        await db.execute(
            "INSERT INTO jobs (job_id, batch_id, spotify_id, title) VALUES (?, ?, ?, ?)",
            (job_id, batch_id, spotify_id, title),
        )
        await db.commit()
    return job_id


async def update_job(job_id: str, status: Status, error: str | None = None) -> None:
    async with aiosqlite.connect(DB_PATH) as db:
        await db.execute(
            "UPDATE jobs SET status=?, error=?, updated_at=unixepoch('subsec') WHERE job_id=?",
            (status.value, error, job_id),
        )
        await db.commit()


async def get_job(job_id: str) -> dict | None:
    async with aiosqlite.connect(DB_PATH) as db:
        db.row_factory = aiosqlite.Row
        async with db.execute("SELECT * FROM jobs WHERE job_id=?", (job_id,)) as cur:
            row = await cur.fetchone()
            return dict(row) if row else None


async def get_batch(batch_id: str) -> list[dict]:
    async with aiosqlite.connect(DB_PATH) as db:
        db.row_factory = aiosqlite.Row
        async with db.execute(
            "SELECT * FROM jobs WHERE batch_id=? ORDER BY created_at", (batch_id,)
        ) as cur:
            return [dict(r) for r in await cur.fetchall()]


async def get_pending_job_ids() -> list[str]:
    """Used on startup to re-queue any jobs that didn't finish."""
    async with aiosqlite.connect(DB_PATH) as db:
        async with db.execute(
            "SELECT job_id FROM jobs WHERE status IN ('pending','running') ORDER BY created_at"
        ) as cur:
            return [r[0] for r in await cur.fetchall()]


async def reset_running_to_pending() -> None:
    """Mark any 'running' jobs as pending so they get retried after a restart."""
    async with aiosqlite.connect(DB_PATH) as db:
        await db.execute(
            "UPDATE jobs SET status='pending' WHERE status='running'"
        )
        await db.commit()
