use anyhow::Result;
use serde::Serialize;

use crate::service::auth;
use crate::util::presenter::emit;
use crate::util::user_config::get_registry;

pub async fn whoami() -> Result<()> {
    let registry = get_registry();
    let token = auth::require_token(&registry).await?;

    let username = auth::whoami(&registry, &token).await?;
    let output = WhoamiOutput {
        username: &username,
        registry: &registry,
        authenticated: true,
    };
    emit("whoami", &output, || {
        println!("{username}");
        Ok(())
    })
}

#[derive(Serialize)]
struct WhoamiOutput<'a> {
    username: &'a str,
    registry: &'a str,
    authenticated: bool,
}
