use clap::Parser;
use log::info;
mod auth;
mod sync;
mod config;

/// OneDrive Sync: Sync your OneDrive files in the background
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Run as a background daemon
    #[arg(long)]
    daemon: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let args = Args::parse();

    if args.daemon {
        info!("Running as daemon...");
        // TODO: Implement daemon logic
    } else {
        info!("Running in interactive mode...");
        // TODO: Implement interactive setup/authorization
    }

    Ok(())
} 