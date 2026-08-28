//! Registry login command.

use anyhow::Result;
use colored::Colorize;

use crate::service::auth;
use crate::util::user_config::get_registry;

pub async fn run() -> Result<()> {
    let registry = get_registry();

    println!("Login to {}", registry.cyan());

    let token = auth::web_login(&registry, |url| {
        println!("Login at: {}", url);
        if let Err(e) = open::that(url) {
            tracing::warn!("Failed to open browser: {e}");
        }
    })
    .await?;

    auth::save_token(&registry, token).await?;
    println!("{}", "Logged in successfully.".green());
    Ok(())
}
