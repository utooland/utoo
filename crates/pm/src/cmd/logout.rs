use anyhow::Result;
use colored::Colorize;

use crate::service::auth;
use crate::util::user_config::get_registry;

pub async fn logout() -> Result<()> {
    let registry = get_registry();
    let token = auth::require_token().await?;

    auth::logout(&registry, &token).await?;

    println!("Logged out from {}.", registry.green());
    Ok(())
}
