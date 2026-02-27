use anyhow::Result;

use crate::service::auth;
use crate::util::user_config::get_registry;

pub async fn whoami() -> Result<()> {
    let registry = get_registry().await;
    let token = auth::require_token(&registry).await?;

    let username = auth::whoami(&registry, &token).await?;
    println!("{}", username);
    Ok(())
}
