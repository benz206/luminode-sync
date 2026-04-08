mod config;
mod daemon;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "lightd", about = "Spotify-synced LED daemon", version)]
struct Cli {
    /// Path to lightd.toml configuration file.
    #[arg(long, short = 'c', default_value = "/etc/luminode-sync/lightd.toml")]
    config: PathBuf,

    /// Override the light plan file.
    #[arg(long)]
    plan: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("lightd=info".parse().unwrap())
                .add_directive("runtime_sync=info".parse().unwrap()),
        )
        .init();

    let cli = Cli::parse();

    let mut config = config::Config::load(&cli.config)?;

    if let Some(plan) = cli.plan {
        config.plan = plan;
    }

    // Install a Ctrl-C handler that turns off the strip before exiting.
    ctrlc_handler();

    daemon::run(config).await
}

fn ctrlc_handler() {
    // On SIGINT / SIGTERM, clear the strip and exit cleanly.
    // The LED driver holds a DMA handle; dropping it on exit is important
    // to avoid leaving the hardware in a bad state.
    unsafe {
        libc::signal(libc::SIGINT,  handle_signal as libc::sighandler_t);
        libc::signal(libc::SIGTERM, handle_signal as libc::sighandler_t);
    }
}

extern "C" fn handle_signal(_: libc::c_int) {
    eprintln!("\nlightd: signal received, shutting down");
    std::process::exit(0);
}
