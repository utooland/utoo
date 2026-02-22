use clap::{Parser, Subcommand};
use std::env;

/// Detect the binary name to determine the mode of operation.
fn get_program_name() -> String {
    env::args()
        .next()
        .and_then(|path| {
            path.rsplit('/')
                .next()
                .map(|s| s.to_lowercase().replace(".exe", ""))
        })
        .unwrap_or_else(|| "utoo-runtime".to_string())
}

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

        /// Load from an application snapshot instead of initializing from scratch
        #[arg(long)]
        snapshot: Option<String>,
    },

    /// Build an application-level snapshot (captures framework init)
    Snapshot {
        /// Entry script to snapshot (e.g. app.js)
        script: String,

        /// Output snapshot file
        #[arg(short, long, default_value = "snapshot.bin")]
        output: String,
    },
}

fn main() {
    let program_name = get_program_name();

    // If called as "node", parse arguments in node-compatible mode
    if program_name == "node" {
        let args: Vec<String> = env::args().collect();
        if args.len() < 2 {
            eprintln!("Usage: node [options] script.js [args]");
            std::process::exit(1);
        }

        // Node flags that consume the next argument as a value.
        // When we see one of these, skip both the flag and its value.
        const FLAGS_WITH_VALUE: &[&str] = &[
            "--require", "-r",
            "--loader",
            "--import",
            "--conditions", "-C",
            "--input-type",
            "--inspect-publish-uid",
            "--max-old-space-size",
            "--max-semi-space-size",
            "--max-http-header-size",
            "--heapsnapshot-signal",
            "--redirect-warnings",
            "--diagnostic-dir",
            "--report-directory",
            "--report-filename",
            "--title",
            "--tls-cipher-list",
            "--icu-data-dir",
            "--openssl-config",
            "--dns-result-order",
            "--use-largepages",
            "--secure-heap",
            "--secure-heap-min",
            "--eval", "-e",
            "--print", "-p",
        ];

        // Find the first non-option argument (the script)
        let mut script_idx: Option<usize> = None;
        let mut i = 1;
        while i < args.len() {
            let arg = &args[i];

            // --flag=value form: skip just this arg
            if arg.starts_with('-') && arg.contains('=') {
                i += 1;
                continue;
            }

            // Flag that consumes the next arg as its value
            if FLAGS_WITH_VALUE.iter().any(|f| *f == arg.as_str()) {
                i += 2; // skip flag + value
                continue;
            }

            // Other flags (--inspect, --harmony, etc.)
            if arg.starts_with('-') {
                i += 1;
                continue;
            }

            // First non-flag argument is the script
            script_idx = Some(i);
            break;
        }

        let script_idx = match script_idx {
            Some(idx) => idx,
            None => {
                eprintln!("node: No script specified");
                std::process::exit(1);
            }
        };
        let script = &args[script_idx];

        // Resolve extensionless script paths (Node compat: `node httpServer` → `httpServer.js`)
        let script = if std::path::Path::new(script).exists() {
            script.clone()
        } else {
            let mut resolved = None;
            for ext in &[".js", ".ts", ".mjs", ".cjs"] {
                let candidate = format!("{}{}", script, ext);
                if std::path::Path::new(&candidate).exists() {
                    resolved = Some(candidate);
                    break;
                }
            }
            // Also try index.js inside directory
            if resolved.is_none() && std::path::Path::new(script).is_dir() {
                let idx = format!("{}/index.js", script);
                if std::path::Path::new(&idx).exists() {
                    resolved = Some(idx);
                }
            }
            resolved.unwrap_or_else(|| script.clone())
        };

        // Remaining args after the script become process.argv[2+]
        let script_args: Vec<String> = args[script_idx + 1..].to_vec();

        // Build the snapshot flag if NODE_UTOO_SNAPSHOT env is set
        let snapshot: Option<String> = if let Some(snapshot_path) = env::var_os("UTOO_RUNTIME_SNAPSHOT") {
            Some(snapshot_path.to_string_lossy().into_owned())
        } else {
            None
        };

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");

        let result = rt.block_on(async {
            if let Some(snapshot_path) = snapshot {
                utoo_runtime::runtime::run_from_app_snapshot(&snapshot_path, &script).await
            } else {
                utoo_runtime::runtime::run_script_with_args(&script, &script_args).await
            }
        });

        if let Err(e) = result {
            eprintln!("error: {e:#}");
            std::process::exit(1);
        }
        return;
    }

    // Normal utoo-runtime mode
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
        Commands::Run { script, snapshot } => {
            if let Some(snapshot_path) = snapshot {
                utoo_runtime::runtime::run_from_app_snapshot(&snapshot_path, &script).await
            } else {
                utoo_runtime::runtime::run_script(&script).await
            }
        }
        Commands::Snapshot { script, output } => {
            utoo_runtime::runtime::build_app_snapshot(&script, &output).await
        }
    }
}
