use anyhow::Result;
use colored::Colorize;

use crate::util::http::client_builder;
use crate::util::registry::ping_registry;
use crate::util::user_config::{detect_supports_semver, get_registry};

pub async fn ping(registry: Option<&str>) -> Result<()> {
    let registry = registry.map(String::from).unwrap_or_else(get_registry);

    println!("{} {}", "PING".green(), registry.cyan());

    let client = client_builder()?
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let result = ping_registry(&client, &registry).await;

    if result.success {
        let supports = detect_supports_semver(&registry, Some(&client)).await;
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
    } else {
        anyhow::bail!(
            "{} registry did not respond ({}ms)",
            "FAIL".red(),
            result.latency_ms
        );
    }

    Ok(())
}
