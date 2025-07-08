use anyhow::Result;
use clap::Parser;
use std::process;
use utoo_cli::{
    cmd::deps::{build_deps, build_workspace},
    constants::{cmd::DEPS_ABOUT, APP_VERSION},
    helper::workspace::update_cwd_to_root,
    util::logger::{log_error, write_verbose_logs_to_file},
};

#[derive(Parser)]
#[command(
    name = "utoo-deps",
    version = APP_VERSION,
    about = DEPS_ABOUT
)]
struct Cli {
    /// Only build workspace dependencies
    #[arg(long = "workspace-only")]
    workspace_only: bool,

    /// Show verbose output
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let cwd = std::env::current_dir()?;
    let root_path = update_cwd_to_root(&cwd).await?;
    let result = if cli.workspace_only {
        build_workspace(&root_path).await
    } else {
        build_deps(&root_path).await
    };

    if let Err(e) = result {
        log_error(&e.to_string());
        let _ = write_verbose_logs_to_file();
        process::exit(1);
    }
    result?;
    Ok(())
}
