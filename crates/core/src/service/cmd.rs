use crate::config::Config;
use anyhow::Result;
use colored::*;
use std::process::Command;

pub struct CommandService {
    config: Config,
}

impl CommandService {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub fn get_available_commands(&self) -> Result<Vec<(String, String)>> {
        let mut commands: Vec<(String, String)> = Vec::new();

        // Get all config keys
        for (key, _) in self.config.list()? {
            if key.ends_with(".cmd") {
                // Remove .cmd suffix to get command name
                if let Some(command) = key.strip_suffix(".cmd") {
                    // Get the command alias
                    if let Some(alias) = self.config.get(&format!("{command}.cmd"))? {
                        commands.push((command.to_string(), alias));
                    }
                }
            }
        }

        // Add utoo as default command if no commands are configured
        if commands.is_empty() {
            commands.push((
                "*".to_string(),
                "utoo <command> (default wildcard)".to_string(),
            ));
        }

        Ok(commands)
    }

    pub fn print_help(&self) -> Result<()> {
        let commands = self.get_available_commands()?;
        // Check if there are any commands other than the default utoo command
        let is_empty = commands.iter().all(|(name, _)| name == "*");

        println!("{}", "🌖 /juːtuː/ Unified Toolchain".bold());
        println!();
        println!("{}", "Usage:".bold());
        println!("  ut <COMMAND>");
        println!();
        println!("{}", "Configuration:".bold());
        println!(
            "  Global config: {}",
            self.config.get_global_config_path()?.display()
        );
        let local_path = self.config.get_local_config_path()?;
        if local_path.exists() {
            println!("  Local config:  {}", local_path.display());
        }
        let registry = self
            .config
            .get("registry")?
            .unwrap_or_else(|| "https://registry.npmmirror.com".to_string());
        println!("  Registry:      {registry}");
        println!();
        println!("{}", "Commands:".bold());

        // Find the longest command name
        let max_width = commands
            .iter()
            .map(|(name, _)| name.len())
            .max()
            .unwrap_or(0)
            .max("config".len());

        println!(
            "  {:<width$}    Manage configuration",
            "config".cyan(),
            width = max_width
        );
        for (name, alias) in commands {
            println!(
                "  {:<width$}    {} {}",
                name.cyan(),
                "→".bold(),
                alias,
                width = max_width
            );
        }

        if is_empty {
            println!();
            println!("{}", "Notice:".yellow().bold());
            println!("  No commands configured yet. Here are some common configurations:");
            println!();
            println!(
                "  {}  Set registry",
                "ut config set registry https://registry.npmmirror.com --global".cyan()
            );
            println!(
                "  {}  Set install command",
                "ut config set install.cmd \"utoo install\"".cyan()
            );
            println!(
                "  {}  Set wildcard command",
                "ut config set *.cmd utoo --global".cyan()
            );
        }

        println!();
        println!("{}", "Options:".bold());
        println!("  {}     Print help information", "-h, --help".yellow());
        println!("  {}  Print version information", "-V, --version".yellow());
        println!();
        println!(
            "\nFor more information, visit: {}",
            "https://github.com/utooland/utoo/tree/next"
                .blue()
                .underline()
        );

        Ok(())
    }

    pub fn execute(&self, args: &[String]) -> Result<()> {
        if args.is_empty() {
            // Default to utoo when no arguments are provided
            let mut command = Command::new("utoo");
            let status: std::process::ExitStatus = command.status()?;
            std::process::exit(status.code().unwrap_or(1));
        }

        let command_name = &args[0];

        // First try to find specific command
        let (aliased_command, is_wildcard) =
            if let Some(cmd) = self.config.get(&format!("{command_name}.cmd"))? {
                (cmd, false)
            } else if let Some(cmd) = self.config.get("*.cmd")? {
                (cmd.replace("*", command_name), true)
            } else {
                // Default to utoo if no wildcard command is configured
                (format!("utoo {command_name}"), true)
            };

        // Split the aliased command into parts
        let mut parts: Vec<&str> = aliased_command.split_whitespace().collect();
        if parts.is_empty() {
            return Ok(());
        }

        // Get the command and its arguments
        let cmd = parts.remove(0);
        let mut command = Command::new(cmd);

        if is_wildcard {
            // For wildcard commands, forward all original arguments
            command.args(args);
        } else {
            // For specific commands, add the original arguments except the command name
            command.args(&args[1..]);
            // Add any additional arguments from the alias
            command.args(parts);
        }

        // Execute the command
        let status: std::process::ExitStatus = command.status()?;
        std::process::exit(status.code().unwrap_or(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use tempfile::tempdir;

    fn setup_test_env() -> (Config, tempfile::TempDir) {
        let temp_dir = tempdir().unwrap();
        let config_dir = temp_dir.path().join(".utoo");
        fs::create_dir_all(&config_dir).unwrap();

        // Set up temporary home directory
        unsafe { env::set_var("HOME", temp_dir.path()) };

        let mut config = Config::load(false).unwrap();
        // Use "true" command which exists on Unix systems
        config.set("test.cmd", "true".to_string(), true).unwrap();
        config.set("*.cmd", "true".to_string(), true).unwrap();
        config
            .set("registry", "https://test.registry.com".to_string(), true)
            .unwrap();

        (config, temp_dir)
    }

    #[test]
    fn test_execute_specific_command() {
        let (config, _temp_dir) = setup_test_env();
        let service = CommandService::new(config);

        let args = vec!["test".to_string()];
        let result = service.execute(&args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_wildcard_command() {
        let (config, _temp_dir) = setup_test_env();
        let service = CommandService::new(config);

        let args = vec!["unknown".to_string()];
        let result = service.execute(&args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_empty_args() {
        let (config, _temp_dir) = setup_test_env();
        let service = CommandService::new(config);

        let args = vec![];
        let result = service.execute(&args);
        assert!(result.is_ok());
    }
}
