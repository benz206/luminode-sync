# Architecture

## System overview

```
Dev machine (any OS)
  beatmap-cli generate <file>
       │
       ▼
  beatmap_gen
  ├── symphonia  (audio decode: FLAC/MP3/OGG/WAV)
  ├── realfft    (STFT → spectral flux)
  ├── analysis   (onset → beat tracking via DP)
  └── sections   (energy-based section classifier)
       │
       ▼  writes to library/
  .beatmap files + index.json
       │
   rsync / git
       │
       ▼
Raspberry Pi
  lightd
  ├── Spotify poller (tokio, every 3s)
  │     └── updates Arc<RwLock<SyncAnchor>>
  └── Render thread (std::thread, 60fps)
        ├── reads SyncAnchor (try_read, non-blocking)
        ├── loads Beatmap (on track change)
        ├── EffectScheduler.resolve_context(position_ms)
        ├── fires beat/cue triggers
        ├── render() → Vec<Rgb>
        └── pi_output::write() → GPIO/DMA
```

## Beatmap format

Wire format: MessagePack (via rmp-serde).
Typical size: 3–6 KB per track.

Beat timestamps: delta-encoded u16 milliseconds.
- beat[0] = first_beat_ms (u32 anchor)
- beat[k] = first_beat_ms + sum(deltas[0..k])
- Downbeats: bitset (1 bit per beat, packed into Vec<u8>)

### Why not absolute timestamps?
4 bytes × 600 beats = 2.4 KB vs 2 bytes × 600 = 1.2 KB.
Delta encoding halves the beat data size.

### Why not varints?
At 120 BPM, delta = 500ms.  As LEB128 varint: 2 bytes (≥128).
Same size as fixed u16, not worth the decode complexity.

### Why not tempo-grid ticks?
Works only for constant-tempo music.  Fragile with rubato.
Delta ms handles any tempo curve without special-casing.

## Sync algorithm

```
SyncAnchor = {
  spotify_progress_ms: u32,  // what Spotify last reported
  local_time: Instant,       // monotonic clock when we got it
  is_playing: bool,
  smooth_correction: Option<...>
}

estimate_ms() = spotify_progress_ms + local_time.elapsed()
              + smooth_correction_offset(t)

On new Spotify poll:
  diff = new_spotify_ms - estimate_ms()
  |diff| < 40ms  → ignore (API jitter)
  |diff| < 600ms → smooth correction over 300ms (eased)
  |diff| ≥ 600ms → hard snap (seek or systematic error)
  track_id changed → hard reset
  play/pause changed → hard reset
```

## Light plan separation

```
Beatmap (musical facts)    +    LightPlan (declarative rules)
  beat times                     section_rule: intro → breathe
  section types                  beat_rule: downbeat → kick_pulse
  energy envelope                cue_rule: drop → white_flash
  cue markers                    palette definitions
         │                               │
         └──────── EffectScheduler ──────┘
                         │
                    Vec<Rgb> frame
                         │
                   pi_output::write()
```

You can restyle any song by editing only the .toml plan.
No beatmap regeneration needed.

## CPU budget on Raspberry Pi 4

| Task               | Cost          | Notes |
|--------------------|---------------|-------|
| Spotify poll       | ~50ms/3s avg  | Network + JSON parse |
| beat_at_position() | < 1µs         | Binary search on ~600 elements |
| render_base()      | ~50µs         | 259 pixels × lerp + scale |
| Trigger compositing| ~10µs each    | Usually 1–2 active |
| GPIO write         | ~1–2ms        | DMA transfer, blocks |
| Total per frame    | < 3ms of 16ms budget | |

The render thread comfortably fits in 16.67ms at 60fps.
No SIMD or unsafe needed.

## MVP build order

1. beatmap_core — compile and test types
2. beatmap-cli generate — get beatmaps generating
3. lightd fallback path — drive LEDs without Spotify
4. runtime_sync — add Spotify polling
5. light_engine section rules — section-aware effects
6. beat/cue triggers — rhythmic response
7. Tune calibration_ms per-track
