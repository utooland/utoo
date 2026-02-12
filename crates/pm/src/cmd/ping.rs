use anyhow::Result;
use colored::Colorize;

use crate::util::config::{get_registry, get_supports_semver};

pub async fn ping() -> Result<()> {
    let registry = get_registry();

    println!("{} {}", "PING".green(), registry.cyan());

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let ping_url = format!("{}/-/ping", registry);
    let start = std::time::Instant::now();

    match client.get(&ping_url).send().await {
        Ok(resp) => {
            let latency = start.elapsed().as_millis();
            if resp.status().is_success() {
                let semver_info = match get_supports_semver() {
                    Some(true) => "supports-semver: yes".green(),
                    Some(false) => "supports-semver: no".yellow(),
                    None => "supports-semver: unknown".dimmed(),
                };
                println!(
                    "{} {}ms ({})",
                    "PONG".green(),
                    latency.to_string().cyan(),
                    semver_info
                );
            } else {
                println!(
                    "{} HTTP {} ({}ms)",
                    "FAIL".red(),
                    resp.status(),
                    latency
                );
            }
        }
        Err(e) => {
            let latency = start.elapsed().as_millis();
            println!("{} {} ({}ms)", "FAIL".red(), e, latency);
        }
    }

    Ok(())
}
