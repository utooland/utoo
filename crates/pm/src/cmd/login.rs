use anyhow::Result;
use colored::Colorize;

use crate::service::auth;
use crate::util::user_config::get_registry;

pub async fn login(registry_override: Option<String>) -> Result<()> {
    let registry = registry_override.unwrap_or_else(get_registry);

    println!("Login to {}", registry.cyan());

    let token = auth::web_login(&registry, |url| {
        println!("Login at: {}", url);
        let _ = open::that(url);
    })
    .await?;

    auth::save_token(token).await?;
    println!("{}", "Logged in successfully.".green());
    Ok(())
}
