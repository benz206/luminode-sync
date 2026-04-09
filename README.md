# Luminode-Sync

Spotify-synced LED lighting system for Raspberry Pi. Drives 259 WS2812B LEDs in real time, locked to the beat of whatever is playing on Spotify.

## How it works

Two independent halves:

1. **`beatmap-cli`** (runs on dev machine) — decodes local audio files, extracts beat/section/cue data, writes compact `.beatmap` files to a library directory.
2. **`lightd`** (runs on Pi) — polls Spotify every 3 s, maintains a monotonic sync clock between polls, loads the matching beatmap, and renders light effects at 60 fps via GPIO/DMA.

The two halves share only `crates/beatmap_core` (the file format and library index). Nothing else couples them.

## Workspace layout

```
crates/beatmap_core   — shared types, serialization, library index
crates/beatmap_gen    — audio analysis (symphonia + realfft; runs offline only)
crates/runtime_sync   — Spotify API client + monotonic sync clock
crates/light_engine   — effect scheduler, lighting plan parser, beat/cue triggers
crates/pi_output      — LED output (ws2812 on ARM Linux, ASCII sim elsewhere)
apps/beatmap-cli      — offline CLI: generate / scan / inspect / validate / auth
apps/lightd           — runtime daemon
config/               — lightd.toml and plans/default.toml (deploy to Pi)
deploy/               — systemd service file + deploy.sh
docs/                 — architecture overview and sample beatmap
```

## Quickstart

### 1. Generate beatmaps (dev machine)

```bash
# Single file
cargo run -p beatmap-cli -- generate path/to/track.mp3

# Whole music library
cargo run -p beatmap-cli -- scan ~/Music

# Inspect a beatmap
cargo run -p beatmap-cli -- inspect beatmaps/beatmaps/<hash>.beatmap

# Authenticate with Spotify (needed for lightd)
cargo run -p beatmap-cli -- auth --client-id <your-spotify-client-id>
```

### 2. Deploy to the Pi

```bash
# Cross-compile (requires `cross` — cargo install cross)
cross build --release --target armv7-unknown-linux-gnueabihf

# Copy binaries, config, and service file to the Pi
./deploy/deploy.sh pi@raspberrypi.local
```

### 3. Start the daemon

```bash
sudo systemctl enable leds-sync.service
sudo systemctl start leds-sync.service
journalctl -u leds-sync.service -f
```

### 4. Run locally (ASCII LED simulator)

```bash
cargo run -p lightd -- --config config/lightd.toml
```

The simulator prints a compact bar chart to stderr — useful for testing effects without hardware.

## Configuration

Edit `config/lightd.toml` for daemon settings (Spotify credentials, library path, FPS).

Edit `config/plans/default.toml` to change how songs look — which effects play during each section, palettes used, how strongly beats pulse. **You never need to regenerate beatmaps to restyle a song.**

## Beatmap format

Wire format: **MessagePack** (`rmp-serde`). Typical size: 3–6 KB per track.

- Beat timestamps are delta-encoded `u16` milliseconds
- Downbeats are a packed bitset (1 bit per beat)
- Sections and cues are sparse arrays

See `docs/sample.beatmap.json` for an annotated example and `docs/architecture.md` for design rationale.

## Key constraints

- **Root required on Pi** — GPIO/DMA access for ws2812
- **`beatmap_gen` never runs on Pi** — audio analysis is offline-only (requires symphonia + realfft)
- **`LED_COUNT` (259)** is a compile-time constant in `crates/light_engine/src/lib.rs`; keep in sync with `leds.count` in the config when changing hardware
- Bump `BEATMAP_VERSION` in `beatmap_core/src/lib.rs` on any breaking format change
- **Beatmap lookup in `lightd`** only matches by Spotify track ID — ensure beatmaps are indexed with `--spotify-id` when generating, or the daemon falls back to the slow-gradient effect

## License

GNU General Public License v3.0
