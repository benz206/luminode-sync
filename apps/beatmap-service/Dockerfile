# Build from the repo root:
#   docker build -f apps/beatmap-service/Dockerfile -t beatmap-service .

# ── Stage 1: compile beatmap-cli ──────────────────────────────────────────────
FROM rust:latest AS rust-builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY apps/ apps/

RUN cargo build --release -p beatmap-cli

# ── Stage 2: Python runtime ───────────────────────────────────────────────────
FROM python:3.12-slim

# ffmpeg is required by streamrip for audio conversion
# nodejs is required by yt-dlp for JavaScript extraction (YouTube bot check bypass)
RUN apt-get update \
    && apt-get install -y --no-install-recommends ffmpeg nodejs \
    && rm -rf /var/lib/apt/lists/*

# streamrip: downloads from Deezer via ISRC when DEEZER_ARL is set.
# yt-dlp: fallback downloader using a YouTube search when no ARL is available.
RUN pip install --no-cache-dir streamrip yt-dlp

COPY --from=rust-builder /workspace/target/release/beatmap-cli /app/beatmap-cli

WORKDIR /app
COPY apps/beatmap-service/requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

COPY apps/beatmap-service/ .

# /data is the persistent volume mount point (library + jobs.db)
VOLUME /data

ENV BEATMAP_CLI=/app/beatmap-cli
ENV BEATMAP_LIBRARY=/data/library
ENV DATA_DIR=/data

# DEEZER_ARL: Deezer user token (from browser cookies → deezer.com → arl cookie).
# Required for streamrip to authenticate with Deezer.
ENV DEEZER_ARL=""

EXPOSE 8000

# entrypoint.sh writes the streamrip config using DEEZER_ARL, then starts uvicorn.
ENTRYPOINT ["/app/entrypoint.sh"]
CMD ["uvicorn", "main:app", "--host", "0.0.0.0", "--port", "8000"]
