use anyhow::Result;
use colored::Colorize;

use crate::model::cli_output::LogoutResult;
use crate::service::auth;
use crate::util::presenter::emit;
use crate::util::user_config::get_registry;

pub async fn logout() -> Result<()> {
    let registry = get_registry();
    let token = auth::require_token(&registry).await?;

    let remote_revoked = auth::logout(&registry, &token).await?;

    emit(
        "logout",
        &LogoutResult {
            registry: registry.clone(),
            remote_revoked,
        },
        || {
            println!("Logged out from {}.", registry.green());
            Ok(())
        },
    )
}
