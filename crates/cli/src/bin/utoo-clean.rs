use clap::Parser;
use std::process;
use utoo_cli::{
    cmd::clean::clean,
    constants::{cmd::CLEAN_ABOUT, APP_VERSION},
};

#[derive(Parser)]
#[command(
    name = "utoo-clean",
    version = APP_VERSION,
    about = CLEAN_ABOUT
)]
struct Cli {
    /// Package pattern to clean (e.g., "react*", "@types/*", "*")
    #[arg(default_value = "*")]
    pattern: String,

    /// Show verbose output
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    if let Err(err) = clean(&cli.pattern).await {
        eprintln!("Error: {err}");
        process::exit(1);
    }
}
