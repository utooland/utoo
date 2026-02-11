use anyhow::Result;
use colored::Colorize;

use crate::util::config::get_registry;

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
                println!("{} {}ms", "PONG".green(), latency.to_string().cyan());
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
