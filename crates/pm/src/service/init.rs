use std::path::Path;

use anyhow::{Result, bail};
use dialoguer::Input;
use serde::Serialize;

use crate::error::CliError;
use crate::helper::git;
use crate::util::invocation;

/// Initialize a new package.json file in the given directory (or current directory).
///
/// If `yes` is true, skip interactive prompts and use defaults.
/// If `cwd` is `None`, uses `std::env::current_dir()`.
pub async fn init(yes: bool, cwd: Option<&Path>) -> Result<()> {
    let cwd = match cwd {
        Some(p) => p.to_path_buf(),
        None => std::env::current_dir()?,
    };
    let package_json_path = cwd.join("package.json");

    if package_json_path.exists() {
        bail!("package.json already exists in {}", cwd.display());
    }

    if !yes && !invocation::interactive() {
        return Err(CliError::usage(
            "refusing to prompt for package metadata in non-interactive mode",
        )
        .with_suggestion("re-run with `utoo init --yes`")
        .into());
    }

    let cwd_clone = cwd.clone();
    let pkg = if yes {
        tokio::task::spawn_blocking(move || build_default_package(&cwd_clone)).await?
    } else {
        tokio::task::spawn_blocking(move || build_interactive_package(&cwd_clone)).await??
    };

    let content = serde_json::to_string_pretty(&pkg)? + "\n";
    // pkg is a typed GeneratedPackage; field order matches the canonical
    // npm-init layout (name…license, repository last) via declaration order.

    println!("About to write to {}:\n", package_json_path.display());
    println!("{content}");

    if !yes {
        let confirmed = tokio::task::spawn_blocking(|| {
            let confirm: String = Input::new()
                .with_prompt("Is this OK?")
                .default("yes".to_string())
                .interact_text()?;
            Ok::<bool, anyhow::Error>(matches!(
                confirm.trim().to_lowercase().as_str(),
                "yes" | "y"
            ))
        })
        .await??;
        if !confirmed {
            println!("Aborted.");
            return Ok(());
        }
    }

    tokio::fs::write(&package_json_path, &content).await?;
    Ok(())
}

const DEFAULT_VERSION: &str = "1.0.0";
const DEFAULT_ENTRY: &str = "index.js";
const DEFAULT_LICENSE: &str = "ISC";
const DEFAULT_TEST: &str = "echo \"Error: no test specified\" && exit 1";

struct PackageFields {
    name: String,
    version: String,
    description: String,
    main: String,
    test_script: String,
    repository: String,
    keywords: Vec<String>,
    author: String,
    license: String,
}

impl PackageFields {
    fn defaults(cwd: &Path) -> Self {
        Self {
            name: git::dir_name(cwd),
            version: DEFAULT_VERSION.to_string(),
            description: String::new(),
            main: DEFAULT_ENTRY.to_string(),
            test_script: DEFAULT_TEST.to_string(),
            repository: git::detect_repository(cwd),
            keywords: vec![],
            author: git::detect_author(),
            license: DEFAULT_LICENSE.to_string(),
        }
    }

    fn into_package(self) -> GeneratedPackage {
        let repository = (!self.repository.is_empty()).then(|| GeneratedRepository {
            repo_type: "git",
            url: git::normalize_url(&self.repository),
        });
        GeneratedPackage {
            name: self.name,
            version: self.version,
            description: self.description,
            main: self.main,
            scripts: GeneratedScripts {
                test: self.test_script,
            },
            keywords: self.keywords,
            author: self.author,
            license: self.license,
            repository,
        }
    }
}

/// The generated package.json, serialized verbatim to disk. Field declaration
/// order is the on-disk key order, matching npm init's canonical layout.
#[derive(Serialize)]
struct GeneratedPackage {
    name: String,
    version: String,
    description: String,
    main: String,
    scripts: GeneratedScripts,
    keywords: Vec<String>,
    author: String,
    license: String,
    /// Omitted entirely when no git repository was detected/entered.
    #[serde(skip_serializing_if = "Option::is_none")]
    repository: Option<GeneratedRepository>,
}

#[derive(Serialize)]
struct GeneratedScripts {
    test: String,
}

#[derive(Serialize)]
struct GeneratedRepository {
    #[serde(rename = "type")]
    repo_type: &'static str,
    url: String,
}

fn build_default_package(cwd: &Path) -> GeneratedPackage {
    PackageFields::defaults(cwd).into_package()
}

fn build_interactive_package(cwd: &Path) -> Result<GeneratedPackage> {
    let defaults = PackageFields::defaults(cwd);

    let fields = PackageFields {
        name: prompt("package name", defaults.name, false)?,
        version: prompt("version", defaults.version, false)?,
        description: prompt("description", String::new(), true)?,
        main: prompt("entry point", defaults.main, false)?,
        test_script: {
            let cmd = prompt("test command", String::new(), true)?;
            if cmd.is_empty() {
                DEFAULT_TEST.to_string()
            } else {
                cmd
            }
        },
        repository: prompt("git repository", defaults.repository, true)?,
        keywords: {
            let input = prompt("keywords", String::new(), true)?;
            input
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        },
        author: prompt("author", defaults.author, true)?,
        license: prompt("license", defaults.license, false)?,
    };

    Ok(fields.into_package())
}

fn prompt(label: &str, default: String, allow_empty: bool) -> Result<String> {
    Ok(Input::new()
        .with_prompt(label)
        .default(default)
        .allow_empty(allow_empty)
        .interact_text()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_build_default_package_structure() {
        let cwd = Path::new("/tmp/test-pkg");
        let pkg = build_default_package(cwd);

        assert_eq!(pkg.name, "test-pkg");
        assert_eq!(pkg.version, "1.0.0");
        assert_eq!(pkg.main, "index.js");
        assert_eq!(pkg.license, "ISC");
        assert!(!pkg.scripts.test.is_empty());
    }

    #[tokio::test]
    async fn test_init_refuses_existing_package_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();

        let result = init(true, Some(dir.path())).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("package.json already exists")
        );
    }

    #[tokio::test]
    async fn test_init_yes_creates_valid_package_json() {
        let dir = tempfile::tempdir().unwrap();
        init(true, Some(dir.path())).await.unwrap();

        let content = std::fs::read_to_string(dir.path().join("package.json")).unwrap();
        let pkg: serde_json::Value = serde_json::from_str(&content).unwrap();

        assert_eq!(
            pkg["name"],
            dir.path().file_name().unwrap().to_str().unwrap()
        );
        assert_eq!(pkg["version"], "1.0.0");
        assert_eq!(pkg["description"], "");
        assert_eq!(pkg["main"], "index.js");
        assert_eq!(pkg["license"], "ISC");
        assert!(pkg["keywords"].is_array());

        // Verify field order matches npm convention
        let keys: Vec<&String> = pkg.as_object().unwrap().keys().collect();
        let name_idx = keys.iter().position(|k| *k == "name").unwrap();
        let version_idx = keys.iter().position(|k| *k == "version").unwrap();
        let description_idx = keys.iter().position(|k| *k == "description").unwrap();
        let main_idx = keys.iter().position(|k| *k == "main").unwrap();
        let license_idx = keys.iter().position(|k| *k == "license").unwrap();
        assert!(name_idx < version_idx);
        assert!(version_idx < description_idx);
        assert!(description_idx < main_idx);
        assert!(main_idx < license_idx);
    }
}
