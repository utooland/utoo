use std::path::Path;

use anyhow::Result;

pub async fn init(yes: bool, cwd: Option<&Path>) -> Result<()> {
    crate::service::init::init(yes, cwd).await
}
