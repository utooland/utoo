//! Registry connectivity command.

use anyhow::Result;
use colored::Colorize;

use crate::error::{CliError, ErrorKind};
use crate::model::cli_output::{ErrorDetails, PingResult};
use crate::util::http::client_builder;
use crate::util::invocation;
use crate::util::presenter::emit;
use crate::util::registry::ping_registry;
use crate::util::user_config::{detect_supports_semver, get_registry};

pub async fn run(registry: Option<&str>) -> Result<()> {
    let registry = registry.map(String::from).unwrap_or_else(get_registry);

    if !invocation::json() {
        println!("{} {}", "PING".green(), registry.cyan());
    }

    let client = client_builder()?
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let started = std::time::Instant::now();
    let result = ping_registry(&client, &registry).await;

    if result.success {
        let supports = detect_supports_semver(&registry, Some(&client)).await;
        let output = PingResult {
            registry: registry.clone(),
            latency_ms: result.latency_ms,
            supports_semver: supports,
        };
        emit("ping", &output, || {
            let semver_info = if supports {
                "supports-semver: yes".green()
            } else {
                "supports-semver: no".yellow()
            };
            println!(
                "{} {}ms ({})",
                "PONG".green(),
                result.latency_ms.to_string().cyan(),
                semver_info
            );
            Ok(())
        })
    } else {
        if !invocation::json() {
            anyhow::bail!(
                "{} registry did not respond ({}ms)",
                "FAIL".red(),
                result.latency_ms
            );
        }
        Err(CliError::new(
            ErrorKind::Transient,
            format!("registry did not respond ({}ms)", result.latency_ms),
        )
        .with_code("registry_unavailable")
        .with_details(ErrorDetails::Registry {
            registry,
            status: None,
            duration_ms: started.elapsed().as_millis() as u64,
        })
        .into())
    }
}
