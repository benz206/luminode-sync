# Build from the repo root:
#   docker build -f apps/beatmap-service/Dockerfile -t beatmap-service .

# ── Stage 1: compile beatmap-cli ──────────────────────────────────────────────
FROM rust:1.82-slim AS rust-builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY apps/ apps/

RUN cargo build --release -p beatmap-cli

# ── Stage 2: Python runtime ───────────────────────────────────────────────────
FROM python:3.12-slim

# ffmpeg is required by spotdl for audio conversion
RUN apt-get update \
    && apt-get install -y --no-install-recommends ffmpeg \
    && rm -rf /var/lib/apt/lists/*

# spotdl itself
RUN pip install --no-cache-dir spotdl

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

EXPOSE 8000
CMD ["uvicorn", "main:app", "--host", "0.0.0.0", "--port", "8000"]
