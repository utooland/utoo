//! Package binary execution command.

/// Execute a binary from a package, equivalent to `utoo execute`.
pub async fn run(command: &str, args: Vec<String>) -> anyhow::Result<()> {
    crate::service::execute::execute_package(command, args).await
}
