use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "utoo-runtime", about = "utoo JS/TS runtime")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Execute a JavaScript or TypeScript file
    Run {
        /// Path to the script to execute
        script: String,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime")
        .block_on(run(cli));

    if let Err(e) = result {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Commands::Run { script } => {
            utoo_runtime::runtime::run_script(&script).await
        }
    }
}
