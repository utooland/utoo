use clap::Parser;
use std::process;
use utoo_cli::{
    cmd::install::{install, update_package},
    constants::{cmd::INSTALL_ABOUT, APP_VERSION},
    helper::workspace::update_cwd_to_root,
    util::{
        config::{set_legacy_peer_deps, set_registry},
        logger::{log_error, write_verbose_logs_to_file},
        save_type::{parse_save_type, PackageAction},
    },
};

#[derive(Parser)]
#[command(
    name = "utoo-install",
    version = APP_VERSION,
    about = INSTALL_ABOUT
)]
struct Cli {
    /// Package specification (e.g. "lodash@4.17.21")
    spec: Option<String>,

    /// Workspace to install in
    #[arg(short, long)]
    workspace: Option<String>,

    /// Skip running scripts during installation
    #[arg(long = "ignore-scripts")]
    ignore_scripts: bool,

    /// Show verbose output
    #[arg(short, long)]
    verbose: bool,

    /// Show version information
    #[arg(short, long)]
    version: bool,

    /// Set registry URL
    #[arg(long, global = true)]
    registry: Option<String>,

    /// Set legacy peer dependencies
    #[arg(long, global = true, action = clap::ArgAction::SetTrue)]
    legacy_peer_deps: Option<bool>,

    /// Save as dev dependency
    #[arg(long)]
    save_dev: bool,

    /// Save as peer dependency
    #[arg(long)]
    save_peer: bool,

    /// Save as optional dependency
    #[arg(long)]
    save_optional: bool,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let cli = Cli::parse();

    // Set global registry
    set_registry(cli.registry);

    // Set legacy peer deps when set --legacy
    if cli.legacy_peer_deps == Some(true) {
        set_legacy_peer_deps(cli.legacy_peer_deps);
    }

    if let Some(spec) = cli.spec {
        let save_type = parse_save_type(cli.save_dev, cli.save_peer, cli.save_optional);
        if let Err(e) = update_package(
            PackageAction::Add,
            &spec,
            cli.workspace.clone(),
            cli.ignore_scripts,
            save_type,
        )
        .await
        {
            log_error(&e.to_string());
            let _ = write_verbose_logs_to_file();
            process::exit(1);
        }
    } else {
        let cwd = std::env::current_dir()?;
        let root_path = update_cwd_to_root(&cwd).await?;
        if let Err(e) = install(cli.ignore_scripts, &root_path).await {
            log_error(&e.to_string());
            let _ = write_verbose_logs_to_file();
            process::exit(1);
        }
    }

    Ok(())
}
