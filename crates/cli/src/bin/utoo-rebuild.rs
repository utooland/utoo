use clap::Parser;
use std::process;
use utoo_cli::{
    cmd::rebuild::rebuild,
    constants::{cmd::REBUILD_ABOUT, APP_VERSION},
    util::logger::{log_error, log_info, write_verbose_logs_to_file},
};

#[derive(Parser)]
#[command(
    name = "utoo-rebuild",
    version = APP_VERSION,
    about = REBUILD_ABOUT
)]
struct Cli {
    /// Show verbose output
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    log_info("Executing dependency hook scripts and creating node_modules/.bin links");

    let cwd = std::env::current_dir()?;
    if let Err(e) = rebuild(&cwd).await {
        log_error(&e.to_string());
        let _ = write_verbose_logs_to_file();
        process::exit(1);
    }

    log_info("💫 All dependencies rebuild completed");
    Ok(())
}
