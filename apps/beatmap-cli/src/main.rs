/// beatmap-cli — offline beatmap generation and inspection tool.
///
/// Usage:
///   beatmap-cli generate <audio-file> [--spotify-id <id>] [--isrc <isrc>]
///   beatmap-cli scan <directory> [--library <path>]
///   beatmap-cli inspect <beatmap-file>
///   beatmap-cli validate <beatmap-file>
///   beatmap-cli auth [--port 8888]

use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

fn default_library() -> PathBuf {
    dirs_or_home().join(".local/share/luminode-sync")
}

fn dirs_or_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

#[derive(Parser)]
#[command(
    name = "beatmap-cli",
    about = "Offline beatmap generator for luminode-sync",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate a beatmap from a local audio file.
    Generate {
        audio_file: PathBuf,

        /// Associate this beatmap with a Spotify track ID (e.g. 3n3Ppam7vgaVa1iaRUIOKE).
        #[arg(long)]
        spotify_id: Option<String>,

        /// ISRC code (stable cross-release identifier).
        #[arg(long)]
        isrc: Option<String>,

        /// Library root directory (default: ~/.local/share/luminode-sync).
        #[arg(long, default_value_os_t = default_library())]
        library: PathBuf,

        /// Force regeneration even if an up-to-date beatmap already exists.
        #[arg(long, short = 'f')]
        force: bool,

        /// Write the beatmap as a JSON file alongside the binary for inspection.
        #[arg(long)]
        dump_json: bool,
    },

    /// Scan a directory of audio files and generate beatmaps for all of them.
    Scan {
        directory: PathBuf,

        #[arg(long, default_value_os_t = default_library())]
        library: PathBuf,

        /// Force regeneration of all files, even unchanged ones.
        #[arg(long, short = 'f')]
        force: bool,
    },

    /// Print a human-readable description of a beatmap file.
    Inspect {
        beatmap_file: PathBuf,

        /// Dump the full beatmap as JSON.
        #[arg(long)]
        json: bool,
    },

    /// Validate a beatmap file and report any structural errors.
    Validate {
        beatmap_file: PathBuf,
    },

    /// Attach album-art colour to existing beatmaps without re-running audio analysis.
    ///
    /// For each audio file found, the command:
    ///   1. Hashes the file to find its beatmap.
    ///   2. Extracts embedded cover art via ID3/FLAC tags.
    ///   3. Saves the art as <hash>.jpg alongside the beatmap.
    ///   4. Computes the dominant RGB colour and stores it in TrackMeta.
    ///   5. Re-writes the beatmap in-place (timing/sections unchanged).
    PatchArt {
        /// Audio file or directory to scan.
        audio: PathBuf,

        #[arg(long, default_value_os_t = default_library())]
        library: PathBuf,

        /// Re-patch even if dominant_color is already set.
        #[arg(long, short = 'f')]
        force: bool,
    },

    /// Run the one-time Spotify OAuth flow and save tokens to disk.
    Auth {
        /// Your Spotify app client ID (from developer.spotify.com).
        #[arg(long, env = "SPOTIFY_CLIENT_ID")]
        client_id: String,

        /// Local port for the OAuth callback server.
        #[arg(long, default_value = "8888")]
        port: u16,

        /// Where to save the token file.
        #[arg(long, default_value_os_t = default_library().join("spotify_token.json"))]
        token_file: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("beatmap_cli=info".parse().unwrap()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Generate { audio_file, spotify_id, isrc, library, force, dump_json } => {
            cmd_generate(&audio_file, spotify_id, isrc, &library, force, dump_json)?;
        }
        Command::Scan { directory, library, force } => {
            cmd_scan(&directory, &library, force)?;
        }
        Command::Inspect { beatmap_file, json } => {
            cmd_inspect(&beatmap_file, json)?;
        }
        Command::Validate { beatmap_file } => {
            cmd_validate(&beatmap_file)?;
        }
        Command::PatchArt { audio, library, force } => {
            cmd_patch_art(&audio, &library, force)?;
        }
        Command::Auth { client_id, port, token_file } => {
            cmd_auth(&client_id, port, &token_file).await?;
        }
    }

    Ok(())
}

// ─── Command implementations ──────────────────────────────────────────────────

fn cmd_generate(
    audio_file: &Path,
    spotify_id: Option<String>,
    isrc: Option<String>,
    library_path: &Path,
    force: bool,
    dump_json: bool,
) -> Result<()> {
    use beatmap_gen::library::Library;

    let mut library = Library::open(library_path)
        .with_context(|| format!("opening library at {}", library_path.display()))?;

    // Compute source hash first so we can skip unchanged files.
    let source_hash = {
        use sha2_hash::hash_file;
        hash_file(audio_file)?
    };

    if !force && library.has(&source_hash) {
        println!(
            "✓  Skipping {} — beatmap already up to date (use --force to regenerate)",
            audio_file.display()
        );
        return Ok(());
    }

    print!("Analyzing {} ... ", audio_file.display());
    let bm = beatmap_gen::generate(audio_file, spotify_id, isrc)
        .with_context(|| format!("generating beatmap for {}", audio_file.display()))?;

    println!(
        "done  ({} beats, {:.1} BPM, {} sections)",
        bm.timing.beat_count(),
        bm.track.detected_bpm,
        bm.sections.len()
    );

    if dump_json {
        let json_path = audio_file.with_extension("beatmap.json");
        let json = bm.to_json_pretty()?;
        std::fs::write(&json_path, json)?;
        println!("  JSON → {}", json_path.display());
    }

    library.store(&bm)?;

    let bm_path = library.beatmap_path(&bm.track.source_hash);
    let size = std::fs::metadata(&bm_path)?.len();
    println!("  Saved → {} ({size} bytes)", bm_path.display());

    // Save album art alongside the beatmap if available.
    if bm.track.dominant_color.is_some() {
        if let Ok(art_bytes) = beatmap_gen::decode::read_cover_art(audio_file) {
            let art_path = bm_path.with_extension("jpg");
            let _ = std::fs::write(&art_path, art_bytes);
        }
    }

    Ok(())
}

fn cmd_scan(directory: &Path, library_path: &Path, force: bool) -> Result<()> {
    use beatmap_gen::library::Library;
    use std::io::Write;

    let mut library = Library::open(library_path)?;
    let files = Library::scan_audio_files(directory);
    let total = files.len();
    println!("Found {total} audio files in {}\n", directory.display());

    let mut ok = 0usize;
    let mut skipped = 0usize;
    let mut errors = 0usize;

    for (i, path) in files.iter().enumerate() {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let prefix = format!("[{}/{}]", i + 1, total);

        // Print the "analyzing…" line without a newline so we can overwrite it on success.
        print!("{prefix} Analyzing {name} …");
        std::io::stdout().flush().ok();

        match cmd_generate_inner(path, None, None, &mut library, force) {
            Ok(Some(bm_summary)) => {
                println!("\r{prefix} ✓  {name}  —  {bm_summary}");
                ok += 1;
            }
            Ok(None) => {
                println!("\r{prefix} –  {name}  (skipped, already up to date)");
                skipped += 1;
            }
            Err(e) => {
                println!("\r{prefix} ✗  {name}");
                eprintln!("        {e:#}");
                errors += 1;
            }
        }
    }

    println!(
        "\nScan complete: {ok} generated, {skipped} skipped, {errors} errors"
    );
    Ok(())
}

/// Returns `Some(summary_string)` if a beatmap was generated, `None` if skipped.
fn cmd_generate_inner(
    path: &Path,
    spotify_id: Option<String>,
    isrc: Option<String>,
    library: &mut beatmap_gen::library::Library,
    force: bool,
) -> Result<Option<String>> {
    use sha2_hash::hash_file;
    let source_hash = hash_file(path)?;
    if !force && library.has(&source_hash) {
        return Ok(None);
    }
    let bm = beatmap_gen::generate(path, spotify_id, isrc)?;
    let summary = format!(
        "{} beats, {:.1} BPM, {} sections",
        bm.timing.beat_count(),
        bm.track.detected_bpm,
        bm.sections.len(),
    );
    library.store(&bm)?;
    Ok(Some(summary))
}

fn cmd_inspect(path: &Path, json: bool) -> Result<()> {
    use beatmap_core::Beatmap;
    let bm = Beatmap::load(path)?;

    if json {
        println!("{}", bm.to_json_pretty()?);
        return Ok(());
    }

    println!("━━━ Beatmap: {} ━━━", path.display());
    println!("  Version      : {}", bm.version);
    println!("  Track        : {} — {}", bm.track.artist, bm.track.title);
    if let Some(ref album) = bm.track.album {
        println!("  Album        : {album}");
    }
    println!("  Duration     : {:.1} s", bm.track.duration_ms as f32 / 1000.0);
    println!("  Detected BPM : {:.1}", bm.track.detected_bpm);
    println!("  Beats        : {}", bm.timing.beat_count());
    println!("  Time sig     : {}/4", bm.timing.time_sig);
    println!("  Sections     : {}", bm.sections.len());
    for s in &bm.sections {
        let beat_ms = bm.timing.beat_positions_ms().get(s.start_beat as usize).copied().unwrap_or(0);
        println!(
            "    beat {:4}  ({:6.1}s)  {:?}  energy={}",
            s.start_beat,
            beat_ms as f32 / 1000.0,
            s.kind,
            s.energy,
        );
    }
    println!("  Cues         : {}", bm.cues.len());
    for c in &bm.cues {
        println!(
            "    {:6.1}s  {:?}",
            c.position_ms as f32 / 1000.0,
            c.kind
        );
    }
    if let Some(ref id) = bm.track.spotify_id {
        println!("  Spotify ID   : {id}");
    }
    println!("  Calibration  : {}ms", bm.calibration_ms);
    println!("  Source hash  : {}", &bm.track.source_hash[..16]);

    Ok(())
}

fn cmd_validate(path: &Path) -> Result<()> {
    use beatmap_core::Beatmap;
    match Beatmap::load(path) {
        Ok(_)  => println!("✓  {} is valid", path.display()),
        Err(e) => {
            eprintln!("✗  {} failed validation: {e}", path.display());
            std::process::exit(1);
        }
    }
    Ok(())
}

async fn cmd_auth(client_id: &str, port: u16, token_file: &Path) -> Result<()> {
    use runtime_sync::spotify::run_auth_flow;

    if let Some(parent) = token_file.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let auth = run_auth_flow(client_id, port).await?;
    auth.save(token_file)?;
    println!("✓  Token saved to {}", token_file.display());
    Ok(())
}

fn cmd_patch_art(audio: &Path, library_path: &Path, force: bool) -> Result<()> {
    use beatmap_gen::library::Library;
    use std::io::Write;

    let library = Library::open(library_path)?;

    // Collect files to process — either a single file or a directory scan.
    let files: Vec<PathBuf> = if audio.is_dir() {
        Library::scan_audio_files(audio)
    } else {
        vec![audio.to_owned()]
    };

    let total = files.len();
    let mut patched = 0usize;
    let mut skipped = 0usize;
    let mut no_beatmap = 0usize;
    let mut no_art = 0usize;
    let mut errors = 0usize;

    for (i, path) in files.iter().enumerate() {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        print!("[{}/{}] {name} … ", i + 1, total);
        std::io::stdout().flush().ok();

        let result = patch_art_for_file(&path, &library, force);
        match result {
            Ok(PatchResult::Patched { color }) => {
                println!("✓  #{:02x}{:02x}{:02x}", color[0], color[1], color[2]);
                patched += 1;
            }
            Ok(PatchResult::AlreadyPatched) => {
                println!("–  (already has colour, use --force to re-patch)");
                skipped += 1;
            }
            Ok(PatchResult::NoBeatmap) => {
                println!("–  no beatmap found (generate first)");
                no_beatmap += 1;
            }
            Ok(PatchResult::NoArt) => {
                println!("–  no embedded cover art");
                no_art += 1;
            }
            Err(e) => {
                println!("✗  {e:#}");
                errors += 1;
            }
        }
    }

    println!(
        "\nDone: {patched} patched, {skipped} skipped, \
         {no_beatmap} missing beatmap, {no_art} no art, {errors} errors"
    );
    Ok(())
}

enum PatchResult {
    Patched { color: [u8; 3] },
    AlreadyPatched,
    NoBeatmap,
    NoArt,
}

fn patch_art_for_file(
    audio_path: &Path,
    library: &beatmap_gen::library::Library,
    force: bool,
) -> Result<PatchResult> {
    use sha2_hash::hash_file;
    use beatmap_core::Beatmap;

    let hash = hash_file(audio_path)?;
    let bm_path = library.beatmap_path(&hash);
    if !bm_path.exists() {
        return Ok(PatchResult::NoBeatmap);
    }

    let mut bm = Beatmap::load(&bm_path)?;

    if !force && bm.track.dominant_color.is_some() {
        return Ok(PatchResult::AlreadyPatched);
    }

    // Extract cover art bytes from the audio file.
    let art_bytes = match beatmap_gen::decode::read_cover_art(audio_path) {
        Ok(b) => b,
        Err(_) => return Ok(PatchResult::NoArt),
    };

    // Compute dominant colour.
    let Some(color) = beatmap_gen::color::dominant_color(&art_bytes) else {
        return Ok(PatchResult::NoArt);
    };

    // Save the raw art as a JPEG alongside the beatmap.
    let art_path = bm_path.with_extension("jpg");
    std::fs::write(&art_path, &art_bytes)?;

    // Patch and re-save the beatmap (timing/sections untouched).
    bm.track.dominant_color = Some(color);
    bm.save(&bm_path)?;

    Ok(PatchResult::Patched { color })
}

// ─── Inline hash helper (avoids a circular dep) ──────────────────────────────

mod sha2_hash {
    use anyhow::Result;
    use std::path::Path;
    use sha2::{Digest, Sha256};
    use hex;

    pub fn hash_file(path: &Path) -> Result<String> {
        let bytes = std::fs::read(path)?;
        Ok(hex::encode(Sha256::digest(&bytes)))
    }
}
