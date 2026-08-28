//! Shell-completion generation.

use anyhow::Context;

/// Generate a UTF-8 shell-completion script.
pub fn generate(shell: clap_complete::Shell) -> anyhow::Result<String> {
    let mut output = Vec::new();
    let mut command = crate::command();
    clap_complete::generate(shell, &mut command, crate::constants::APP_NAME, &mut output);
    String::from_utf8(output).context("Generated completion script is not UTF-8")
}
