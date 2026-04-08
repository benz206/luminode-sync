# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with this repository.

## Project Overview

luminode-sync is a two-part Spotify-synced LED system for Raspberry Pi:

1. **Offline beatmap generator** (`beatmap-cli`) — runs on a dev machine.  Decodes local audio files, extracts beat/section structure, writes compact `.beatmap` files to a library directory.
2. **Runtime daemon** (`lightd`) — runs on the Pi.  Polls Spotify every 3 s, maintains a monotonic sync clock between polls, loads the matching beatmap, and drives the LED strip at 60 fps.

The two halves share only `crates/beatmap_core` (the file format). Nothing else couples them.

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
```

## Build & run

```bash
# Check entire workspace (runs on any host)
cargo check

# Build for Pi (requires `cross` — cargo install cross)
cross build --release --target armv7-unknown-linux-gnueabihf

# Deploy to Pi
./deploy/deploy.sh pi@raspberrypi.local

# Run beatmap-cli on dev machine
cargo run -p beatmap-cli -- generate <audio-file>
cargo run -p beatmap-cli -- scan ~/Music
cargo run -p beatmap-cli -- inspect path/to/track.beatmap
cargo run -p beatmap-cli -- auth --client-id <spotify-client-id>

# Run lightd locally (uses ASCII LED simulator)
cargo run -p lightd -- --config config/lightd.toml

# View Pi logs
journalctl -u leds-sync.service -f
```

There are no automated tests — beatmap validation is done by `beatmap-cli validate` and hardware correctness by running on the Pi.

## Beatmap format

Wire format: **MessagePack** (`rmp-serde`).  Typical size: 3–6 KB per track.

Beat timestamps are **delta-encoded u16 milliseconds**:
- `first_beat_ms: u32` — absolute position of beat 0
- `beat_deltas_ms: Vec<u16>` — ms from beat[i] to beat[i+1]
- `downbeat_bits: Vec<u8>` — bitset, 1 bit per beat

Do not change this encoding without bumping `BEATMAP_VERSION` in `beatmap_core/src/lib.rs`.

## Sync algorithm (clock.rs)

```
estimate_ms() = spotify_progress_ms + local_time.elapsed()
              + smooth_correction_eased(t)

On new Spotify poll:
  |diff| < 40 ms  → ignore (jitter)
  |diff| < 600 ms → smooth correction over 300 ms
  |diff| ≥ 600 ms → hard snap (seek / systematic error)
  track_id changed → hard reset
```

## Light plan

`config/plans/default.toml` is the **only file** to edit for changing visual behavior. It maps section types, beat classes, and cue markers to named effects and palettes. Beatmaps never need to be regenerated to change how a song looks.

## Daemon concurrency model

`lightd` uses two threads, not a single async runtime:

- **Tokio thread** — `SpotifyClient` polls every 3 s, writes to `Arc<RwLock<SyncState>>`, and sends `TrackEvent` on an `mpsc::channel`.
- **Render thread** — plain `std::thread`, wakes at 60 fps, reads `SyncState` non-blocking (`try_estimate()`), drains the channel, and calls the blocking GPIO write. No async.

The split exists because LED DMA writes block for ~2 ms — enough to stall a single-threaded async executor. Do not move rendering into a tokio task.

## Key constraints

- **Root required on Pi** — GPIO/DMA access for ws2812
- **beatmap_gen never runs on Pi** — audio analysis is offline-only
- `pi_output` auto-detects the platform: real ws2812 on ARM Linux, ASCII bar chart everywhere else
- Beatmap version byte must be incremented on any breaking format change
- Hot-path (render loop) must not allocate; avoid `.clone()` on `Vec` in `scheduler.rs`
- `LED_COUNT` (259) is a compile-time constant in `crates/light_engine/src/lib.rs`. The `leds.count` config field sets the output buffer size in `pi_output` but does **not** change `LED_COUNT` — keep them in sync manually when changing hardware
- `CueKind::Custom(_)` always maps to the string `"custom"` in `daemon.rs:cue_kind_str()`, not the inner label — custom cue rules in the plan must use `cue = "custom"`
- `lightd`'s `load_beatmap_for_track` (in `daemon.rs`) only queries the library index by Spotify track ID. The three-tier fallback (ISRC → title/duration) in `LibraryIndex::lookup` is available but not used at runtime. Beatmaps must be generated with `--spotify-id` for `lightd` to find them; otherwise the daemon falls back to the slow-gradient effect for every track.

## Adding a new effect

1. Add a render function in `crates/light_engine/src/effects.rs` following the `fn breathe(...)` pattern.
2. Add a match arm in `render_base()` in the same file.
3. Reference the new effect name in `config/plans/default.toml`.

## Adding a new trigger

1. Add a variant to `TriggerKind` in `effects.rs` and implement its `render()` arm.
2. Add a `"name" => Some(TriggerKind::...)` arm in `trigger_kind_from_name()`.
3. Reference the trigger name in a `[[cue_rule]]` or `[[beat_rule]]` in the plan.
