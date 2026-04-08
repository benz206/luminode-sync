/// The runtime daemon.
///
/// Concurrency model (chosen for robustness over elegance):
///
///   Thread 1 — tokio runtime
///     Task A: Spotify poller (async, every poll_interval_secs)
///       Writes to Arc<RwLock<SyncState>> and the track-change channel.
///
///   Thread 2 — render loop (dedicated OS thread, no async)
///     Runs at target FPS (default 60).
///     Reads SyncState (non-blocking).
///     Looks up the current beatmap (no I/O — loaded on track change).
///     Schedules effects and writes to LED output (blocking GPIO call).
///
/// Why a dedicated render thread instead of a tokio task?
///   • LED writes block for ~2ms (DMA transfer).  That would stall other
///     async tasks on a single-threaded runtime.
///   • We want deterministic 60fps timing, not async jitter.
///   • The render budget is 16.67ms; `std::thread::sleep` is accurate enough
///     on Linux without a real-time scheduler.
///
/// Communication:
///   • Arc<RwLock<SyncState>> — shared playback anchor.
///   • mpsc::channel<TrackEvent> — new track or "nothing playing" signal,
///     so the render thread can load/unload the correct beatmap.

use std::sync::{Arc, mpsc, RwLock};
use std::time::{Duration, Instant};
use std::path::PathBuf;
use anyhow::Result;
use tracing::{debug, info, warn};

use beatmap_core::{Beatmap, LibraryIndex, TrackQuery};
use light_engine::{EffectScheduler, LightPlan};
use pi_output::{LedOutput, Pixel};
use runtime_sync::{SpotifyClient, SyncState, SyncAnchor};
use runtime_sync::clock::UpdateResult;

use crate::config::Config;

/// Events sent from the Spotify poller to the render thread.
#[derive(Debug)]
pub enum TrackEvent {
    /// A new track started playing.
    TrackChanged { track_id: String },
    /// Nothing is playing (paused or player inactive).
    NothingPlaying,
}

pub async fn run(config: Config) -> Result<()> {
    // ── Load auth ─────────────────────────────────────────────────────────────
    let auth = runtime_sync::SpotifyAuth::load(&config.spotify.token_file)
        .map_err(|e| anyhow::anyhow!("cannot load token file: {e}\nRun: beatmap-cli auth --client-id <id>"))?;

    // ── Load library index ────────────────────────────────────────────────────
    let index_path = config.library.join("index.json");
    let library_index = Arc::new(RwLock::new(
        LibraryIndex::load(&index_path).unwrap_or_default()
    ));
    let library_root = config.library.clone();

    // ── Load light plan ───────────────────────────────────────────────────────
    let plan = LightPlan::load(&config.plan)
        .map_err(|e| anyhow::anyhow!("cannot load light plan: {e}"))?;
    let plan = Arc::new(plan);

    // ── Initialise shared sync state ─────────────────────────────────────────
    let sync_state = SyncState::new(SyncAnchor::new(String::new(), 0, false));
    let sync_state = Arc::new(sync_state);

    // ── Channel for track change events ──────────────────────────────────────
    let (track_tx, track_rx) = mpsc::channel::<TrackEvent>();

    // ── LED output ────────────────────────────────────────────────────────────
    let output = pi_output::create_output(config.leds.count)?;

    // ── Spawn the render thread ───────────────────────────────────────────────
    let sync_state_render = Arc::clone(&sync_state);
    let plan_render = Arc::clone(&plan);
    let library_index_render = Arc::clone(&library_index);
    let library_root_render = library_root.clone();
    let target_fps = if config.sync.fps == 0 { 60 } else { config.sync.fps };

    std::thread::spawn(move || {
        render_loop(
            sync_state_render,
            track_rx,
            plan_render,
            library_index_render,
            library_root_render,
            output,
            target_fps,
        );
    });

    // ── Spotify poller (async, on the tokio runtime) ──────────────────────────
    let poll_interval =
        Duration::from_secs_f32(config.sync.poll_interval_secs.max(1.0));

    let mut spotify = SpotifyClient::new(
        config.spotify.client_id.clone(),
        auth,
        &config.spotify.token_file,
        poll_interval,
    );

    info!("lightd started — polling Spotify every {:.1}s", poll_interval.as_secs_f32());

    loop {
        match spotify.current_track().await {
            Ok(Some(track)) => {
                let result = sync_state.update(&track.id, track.progress_ms, track.is_playing);

                match result {
                    UpdateResult::TrackChanged => {
                        info!("Track changed → {} — {}", track.artist, track.title);
                        track_tx.send(TrackEvent::TrackChanged { track_id: track.id }).ok();
                    }
                    UpdateResult::Snapped { delta_ms } => {
                        debug!("Snapped: {delta_ms}ms seek/jump");
                    }
                    UpdateResult::SmoothCorrection { delta_ms } => {
                        debug!("Smooth correction: {delta_ms}ms over 300ms");
                    }
                    UpdateResult::StateChanged => {
                        info!("Play state changed: playing={}", track.is_playing);
                    }
                    UpdateResult::OnTarget => {}
                }
            }
            Ok(None) => {
                debug!("Nothing playing");
                track_tx.send(TrackEvent::NothingPlaying).ok();
            }
            Err(e) => {
                warn!("Spotify poll error: {e:#}");
                // Continue — the render thread will keep using the last good estimate.
            }
        }

        tokio::time::sleep(poll_interval).await;
    }
}

// ─── Render loop (runs on its own OS thread) ──────────────────────────────────

fn render_loop(
    sync: Arc<SyncState>,
    track_rx: mpsc::Receiver<TrackEvent>,
    plan: Arc<LightPlan>,
    library_index: Arc<RwLock<LibraryIndex>>,
    library_root: PathBuf,
    mut output: Box<dyn LedOutput>,
    target_fps: u32,
) {
    let frame_budget = Duration::from_nanos(1_000_000_000 / target_fps as u64);
    let mut scheduler = EffectScheduler::new();
    let mut current_beatmap: Option<Beatmap> = None;
    let mut current_track_id = String::new();

    // Beat tracking state — we fire triggers exactly once per beat.
    let mut last_beat_index: Option<usize> = None;
    let mut last_cue_check_ms: u32 = 0;

    info!("Render loop started at {target_fps} fps");

    loop {
        let frame_start = Instant::now();

        // ── Process track change events (non-blocking) ─────────────────────
        while let Ok(event) = track_rx.try_recv() {
            match event {
                TrackEvent::TrackChanged { track_id } => {
                    if track_id != current_track_id {
                        current_track_id = track_id.clone();
                        current_beatmap = load_beatmap_for_track(
                            &track_id,
                            &library_index,
                            &library_root,
                        );
                        if let Some(ref bm) = current_beatmap {
                            info!(
                                "Loaded beatmap for {} — {} ({} beats)",
                                bm.track.artist,
                                bm.track.title,
                                bm.timing.beat_count()
                            );
                        } else {
                            info!("No beatmap for track {track_id} — using fallback");
                        }
                        last_beat_index = None;
                        last_cue_check_ms = 0;
                    }
                }
                TrackEvent::NothingPlaying => {
                    // Keep rendering the last track until it stops or a new one starts.
                }
            }
        }

        // ── Get current playback estimate ──────────────────────────────────
        let estimate = match sync.try_estimate() {
            Some(e) => e,
            None => {
                // Lock held briefly by the poller — skip this frame.
                std::thread::sleep(frame_budget);
                continue;
            }
        };

        let position_ms = estimate.position_ms;

        // ── Build effect context ───────────────────────────────────────────
        let ctx = if let Some(ref bm) = current_beatmap {
            scheduler.resolve_context(position_ms, bm, &plan)
        } else {
            light_engine::EffectContext::idle(frame_start.elapsed().as_secs_f32())
        };

        // ── Fire beat triggers ─────────────────────────────────────────────
        if let Some(beat_index) = last_beat_index {
            if ctx.beat_index > beat_index {
                // One or more beats passed since the last frame.
                scheduler.on_beat(ctx.beat_index % 4 == 0, &plan);
            }
        }
        last_beat_index = Some(ctx.beat_index);

        // ── Fire cue triggers ──────────────────────────────────────────────
        if let Some(ref bm) = current_beatmap {
            let lookahead = (frame_budget.as_millis() as u32 * 2).max(50);
            for cue in bm.active_cues(last_cue_check_ms, position_ms + lookahead - last_cue_check_ms) {
                let kind_str = cue_kind_str(&cue.kind);
                scheduler.on_cue(kind_str, &plan);
            }
            last_cue_check_ms = position_ms;
        }

        // ── Render ────────────────────────────────────────────────────────
        let pixels_rgb = scheduler.render(&ctx, current_beatmap.as_ref(), &plan);

        // Convert light_engine::Rgb → pi_output::Pixel.
        let pixels: Vec<Pixel> = pixels_rgb
            .iter()
            .map(|c| Pixel::new(c.r, c.g, c.b))
            .collect();

        if let Err(e) = output.write(&pixels) {
            warn!("LED write error: {e}");
        }

        // ── Frame rate cap ─────────────────────────────────────────────────
        let elapsed = frame_start.elapsed();
        if elapsed < frame_budget {
            std::thread::sleep(frame_budget - elapsed);
        }
    }
}

fn load_beatmap_for_track(
    spotify_id: &str,
    library_index: &Arc<RwLock<LibraryIndex>>,
    library_root: &PathBuf,
) -> Option<Beatmap> {
    let query = TrackQuery {
        spotify_id: Some(spotify_id.to_owned()),
        isrc: None,
        artist: String::new(),
        title: String::new(),
        duration_ms: 0,
    };

    let index = library_index.read().ok()?;
    let (relative_path, _confidence) = index.lookup(&query)?;
    let full_path = library_root.join(relative_path);

    match Beatmap::load(&full_path) {
        Ok(bm) => Some(bm),
        Err(e) => {
            warn!("Failed to load beatmap at {}: {e}", full_path.display());
            None
        }
    }
}

fn cue_kind_str(kind: &beatmap_core::CueKind) -> &'static str {
    match kind {
        beatmap_core::CueKind::Drop      => "drop",
        beatmap_core::CueKind::Build     => "build",
        beatmap_core::CueKind::Fill      => "fill",
        beatmap_core::CueKind::Impact    => "impact",
        beatmap_core::CueKind::Custom(_) => "custom",
        _                                => "unknown",
    }
}
