use crate::cli::Cli;
use crate::util::config_file::{Config, ConfigResult};
use anyhow::anyhow;
use clap::CommandFactory;
use colored::*;
use std::cmp::max;
use std::process::Command;

/// One row of the `ut help` command catalog: (name, comma-joined aliases, about).
type CommandRow = (String, String, String);

/// Build the builtin-command catalog by introspecting the clap definition,
/// so `ut help` can never drift from `ut <cmd> --help`. Rows follow the
/// declaration order of [`crate::cli::Commands`]; the about column takes the
/// first line of each command's `about` (matching clap's own compact listing).
fn builtin_commands() -> Vec<CommandRow> {
    let mut rows: Vec<CommandRow> = Cli::command()
        .get_subcommands()
        // clap injects a `help` subcommand; it isn't part of our catalog.
        .filter(|sc| sc.get_name() != "help" && !sc.is_hide_set())
        .map(|sc| {
            // `get_all_aliases` (not `_visible_`): our commands declare aliases
            // via `alias`/`aliases`, which clap hides from its own per-command
            // help — but `ut help` has always listed them.
            let aliases = sc.get_all_aliases().collect::<Vec<_>>().join(", ");
            let about = sc
                .get_about()
                .map(|s| s.to_string())
                .unwrap_or_default()
                .lines()
                .next()
                .unwrap_or_default()
                .to_string();
            (sc.get_name().to_string(), aliases, about)
        })
        .collect();
    // The `*` catch-all is an external subcommand, not a real clap command.
    rows.push(("*".to_string(), String::new(), "→ ut run *".to_string()));
    rows
}

pub struct ConfigService {
    config: Config,
}

impl ConfigService {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub fn get_available_commands(&self) -> ConfigResult<Vec<(String, String)>> {
        Ok(self
            .config
            .list()?
            .filter(|(key, _)| key.ends_with(".cmd"))
            .filter_map(|(key, _)| {
                let command = key.strip_suffix(".cmd")?;
                let alias = self.config.get(key).ok()??;
                Some((command.to_string(), alias))
            })
            .collect::<Vec<_>>())
    }

    pub fn print_help(&self) -> ConfigResult<()> {
        let config_commands = self.get_available_commands()?;
        // Check if there are any config commands other than the default utoo command
        let is_empty = config_commands.iter().all(|(name, _)| name == "*");

        println!("{}", "🌖 /juːtuː/ Unified Toolchain".bold());
        println!();
        println!("{}", "Usage:".bold());
        println!("  ut <COMMAND>");
        println!();
        println!("{}", "Configuration:".bold());
        println!(
            "  Global config:   {}",
            self.config.get_global_config_path()?.display()
        );
        let local_path = self.config.get_local_config_path()?;
        if local_path.exists() {
            println!("  Local config:    {}", local_path.display());
        }
        let registry = self
            .config
            .get("registry")?
            .unwrap_or_else(|| "https://registry.npmmirror.com".to_string());
        println!("  Registry:        {registry}");
        println!();
        println!("{}", "Commands:".bold());

        let builtins = builtin_commands();

        // Find the longest command name and option name
        let option_names = [
            "-h, --help",
            "-v, --version",
            "--json",
            "--quiet",
            "--no-color",
        ];

        // Helper function to get max length from an iterator of strings
        let get_max_length = |items: &[&str]| items.iter().map(|s| s.len()).max().unwrap_or(0);

        let max_width = max(
            builtins
                .iter()
                .map(|(name, _, _)| name.len())
                .max()
                .unwrap_or(0),
            get_max_length(&option_names),
        );

        // Print all commands
        for (name, alias, description) in &builtins {
            let description = if alias.is_empty() {
                description.to_string()
            } else {
                format!("{} ({})", description, alias.yellow())
            };
            println!(
                "  {:<width$}    {}",
                name.cyan(),
                description,
                width = max_width
            );
        }

        for (name, alias) in &config_commands {
            println!(
                "  {:<width$}    {} {}",
                name.cyan(),
                "→".bold(),
                alias,
                width = max_width
            );
        }

        println!();
        println!("{}", "Options:".bold());
        println!(
            "  {:<width$}    Print help information",
            "-h, --help".yellow(),
            width = max_width
        );
        println!(
            "  {:<width$}    Print version information",
            "-v, --version".yellow(),
            width = max_width
        );
        println!(
            "  {:<width$}    Emit a machine-readable JSON result",
            "--json".yellow(),
            width = max_width
        );
        println!(
            "  {:<width$}    Suppress non-essential diagnostics",
            "--quiet".yellow(),
            width = max_width
        );
        println!(
            "  {:<width$}    Disable ANSI colors",
            "--no-color".yellow(),
            width = max_width
        );
        println!();
        println!(
            "For more command information, run {}",
            "ut i --help".dimmed()
        );
        println!(
            "Github: {}",
            "https://github.com/utooland/utoo".blue().underline()
        );
        if is_empty {
            println!();
            println!("{}", "Notice:".yellow().bold());
            println!(
                "  No additional commands configured yet. Here are some common configurations:"
            );
            println!();
            println!(
                "  {}  Set registry",
                "utoo config set registry https://registry.npmmirror.com --global".cyan()
            );
            println!(
                "  {}  Set install command",
                "utoo config set hi.cmd \"utoo x cowsay hi\"".cyan()
            );
        }

        println!();
        Ok(())
    }

    pub fn get_available_cmd(&self, command: &str) -> ConfigResult<Option<String>> {
        // Check if there's a custom command configured for this command name
        self.config.get(&format!("{command}.cmd"))
    }

    pub fn execute_command(&self, command: &str, args: &[String]) -> ConfigResult<()> {
        tracing::debug!("Executing command: {command} with args: {args:?}");

        let cmd_alias = self.config.get(&format!("{command}.cmd"))?;
        let cmd_string = cmd_alias.as_deref().unwrap_or(command);
        let mut cmd_parts: Vec<&str> = cmd_string.split_whitespace().collect();

        if cmd_parts.is_empty() {
            return Err(anyhow!(
                "Invalid command alias for '{command}': '{cmd_string}'"
            ));
        }

        let program = cmd_parts.remove(0);
        let mut cmd = Command::new(program);
        cmd.args(cmd_parts).args(args);

        let status = cmd.status()?;
        std::process::exit(status.code().unwrap_or(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_commands_introspects_clap() {
        let rows = builtin_commands();

        // The `*` catch-all is always appended last.
        assert_eq!(rows.last().map(|(n, ..)| n.as_str()), Some("*"));

        // clap's injected `help` subcommand must not leak into the catalog.
        assert!(rows.iter().all(|(name, ..)| name != "help"));

        // Aliases and about text come straight from the clap definition.
        let install = rows
            .iter()
            .find(|(name, ..)| name == "install")
            .expect("install command present");
        assert_eq!(install.1, "i, add");
        assert_eq!(install.2, "Install project dependencies");

        // `about` is collapsed to its first line so multi-line help (e.g.
        // completions' long_about) stays on one row in the compact listing.
        let completions = rows
            .iter()
            .find(|(name, ..)| name == "completions")
            .expect("completions command present");
        assert_eq!(completions.2, "Generate shell completion scripts");
    }
}
