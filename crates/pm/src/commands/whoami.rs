//! Registry identity command.

use anyhow::Result;

use crate::error::{CliError, classify};
use crate::model::cli_output::{ErrorDetails, WhoamiResult};
use crate::service::auth;
use crate::util::invocation;
use crate::util::presenter::emit;
use crate::util::user_config::get_registry;

pub async fn run() -> Result<()> {
    let registry = get_registry();
    let token = auth::require_token(&registry).await?;

    let started = std::time::Instant::now();
    let username = match auth::whoami(&registry, &token).await {
        Ok(username) => username,
        Err(error) => {
            if !invocation::json() {
                return Err(error);
            }
            let status = error.chain().find_map(|source| {
                source
                    .downcast_ref::<reqwest::Error>()
                    .and_then(reqwest::Error::status)
                    .map(|status| status.as_u16())
            });
            return Err(CliError::new(classify(&error), format!("{error:#}"))
                .with_code("registry_request_failed")
                .with_details(ErrorDetails::Registry {
                    registry,
                    status,
                    duration_ms: started.elapsed().as_millis() as u64,
                })
                .into());
        }
    };
    let output = WhoamiResult {
        username: username.clone(),
        registry: registry.clone(),
    };
    emit("whoami", &output, || {
        println!("{username}");
        Ok(())
    })
}
