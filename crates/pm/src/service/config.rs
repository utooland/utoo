use crate::constants::cmd;
use crate::util::config_file::{Config, ConfigResult};
use anyhow::anyhow;
use colored::*;
use std::cmp::max;
use std::process::Command;

/// The static command catalog rendered by `ut help`: (name, aliases, about).
const BUILTIN_COMMANDS: &[(&str, &str, &str)] = &[
    (cmd::INSTALL_NAME, cmd::INSTALL_ALIASES, cmd::INSTALL_ABOUT),
    (
        cmd::UNINSTALL_NAME,
        cmd::UNINSTALL_ALIAS,
        cmd::UNINSTALL_ABOUT,
    ),
    (cmd::REBUILD_NAME, cmd::REBUILD_ALIAS, cmd::REBUILD_ABOUT),
    (cmd::CLEAN_NAME, cmd::CLEAN_ALIAS, cmd::CLEAN_ABOUT),
    (cmd::DEPS_NAME, cmd::DEPS_ALIAS, cmd::DEPS_ABOUT),
    (cmd::UPDATE_NAME, cmd::UPDATE_ALIAS, cmd::UPDATE_ABOUT),
    (cmd::LIST_NAME, cmd::LIST_ALIAS, cmd::LIST_ABOUT),
    (cmd::EXECUTE_NAME, cmd::EXECUTE_ALIAS, cmd::EXECUTE_ABOUT),
    (cmd::VIEW_NAME, cmd::VIEW_ALIASES, cmd::VIEW_ABOUT),
    (cmd::LINK_NAME, cmd::LINK_ALIAS, cmd::LINK_ABOUT),
    (cmd::CONFIG_NAME, cmd::CONFIG_ALIAS, cmd::CONFIG_ABOUT),
    (cmd::RUN_NAME, cmd::RUN_ALIAS, cmd::RUN_ABOUT),
    (cmd::PACK_NAME, cmd::PACK_ALIAS, cmd::PACK_ABOUT),
    (cmd::PUBLISH_NAME, cmd::PUBLISH_ALIAS, cmd::PUBLISH_ABOUT),
    (cmd::PING_NAME, cmd::PING_ALIAS, cmd::PING_ABOUT),
    (cmd::LOGIN_NAME, cmd::LOGIN_ALIAS, cmd::LOGIN_ABOUT),
    (cmd::LOGOUT_NAME, cmd::LOGOUT_ALIAS, cmd::LOGOUT_ABOUT),
    (cmd::WHOAMI_NAME, cmd::WHOAMI_ALIAS, cmd::WHOAMI_ABOUT),
    (cmd::INIT_NAME, cmd::INIT_ALIAS, cmd::INIT_ABOUT),
    (
        cmd::COMPLETIONS_NAME,
        cmd::COMPLETIONS_ALIAS,
        "Generate shell completion scripts",
    ),
    ("*", "", "→ ut run *"),
];

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

        // Find the longest command name and option name
        let option_names = ["-h, --help", "-v, --version"];

        // Helper function to get max length from an iterator of strings
        let get_max_length = |items: &[&str]| items.iter().map(|s| s.len()).max().unwrap_or(0);

        let max_width = max(
            get_max_length(
                &BUILTIN_COMMANDS
                    .iter()
                    .map(|(name, _, _)| *name)
                    .collect::<Vec<_>>(),
            ),
            get_max_length(&option_names),
        );

        // Print all commands
        for (name, alias, description) in BUILTIN_COMMANDS {
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
