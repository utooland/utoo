use std::path::Path;

use anyhow::{Result, bail};
use dialoguer::Input;
use serde_json::json;

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

    let pkg = if yes {
        build_default_package(&cwd)
    } else {
        build_interactive_package(&cwd)?
    };

    let content = serde_json::to_string_pretty(&pkg)? + "\n";

    println!("About to write to {}:\n", package_json_path.display());
    println!("{content}");

    if !yes {
        let confirm: String = Input::new()
            .with_prompt("Is this OK?")
            .default("yes".to_string())
            .interact_text()?;
        if confirm.trim().to_lowercase() != "yes" && confirm.trim().to_lowercase() != "y" {
            println!("Aborted.");
            return Ok(());
        }
    }

    tokio::fs::write(&package_json_path, &content).await?;
    Ok(())
}

/// Normalize a git remote URL to npm-conventional format.
///
/// - `git@host:user/repo.git` → `git+ssh://git@host/user/repo.git`
/// - `https://host/user/repo.git` → `git+https://host/user/repo.git`
/// - `http://host/user/repo.git` → `git+http://host/user/repo.git`
/// - Already prefixed with `git+` or other formats → returned as-is
fn normalize_git_url(url: &str) -> String {
    if url.starts_with("git+") {
        return url.to_string();
    }

    // SSH shorthand: git@host:user/repo.git
    if let Some(rest) = url.strip_prefix("git@") {
        if let Some((host, path)) = rest.split_once(':') {
            return format!("git+ssh://git@{host}/{path}");
        }
    }

    // https:// or http://
    if url.starts_with("https://") {
        return format!("git+{url}");
    }
    if url.starts_with("http://") {
        return format!("git+{url}");
    }

    url.to_string()
}

fn build_default_package(cwd: &Path) -> serde_json::Value {
    let name = dir_name(cwd);
    let author = detect_git_author();
    let repository = detect_git_repository(cwd);

    let mut pkg = json!({
        "name": name,
        "version": "1.0.0",
        "description": "",
        "main": "index.js",
        "scripts": {
            "test": "echo \"Error: no test specified\" && exit 1"
        },
        "keywords": [],
        "author": author,
        "license": "ISC"
    });

    if !repository.is_empty() {
        pkg["repository"] = json!({
            "type": "git",
            "url": normalize_git_url(&repository)
        });
    }

    pkg
}

fn build_interactive_package(cwd: &Path) -> Result<serde_json::Value> {
    let default_name = dir_name(cwd);
    let default_author = detect_git_author();
    let default_repo = detect_git_repository(cwd);

    let name: String = Input::new()
        .with_prompt("package name")
        .default(default_name)
        .interact_text()?;

    let version: String = Input::new()
        .with_prompt("version")
        .default("1.0.0".to_string())
        .interact_text()?;

    let description: String = Input::new()
        .with_prompt("description")
        .default(String::new())
        .allow_empty(true)
        .interact_text()?;

    let entry_point: String = Input::new()
        .with_prompt("entry point")
        .default("index.js".to_string())
        .interact_text()?;

    let test_command: String = Input::new()
        .with_prompt("test command")
        .default(String::new())
        .allow_empty(true)
        .interact_text()?;

    let git_repository: String = Input::new()
        .with_prompt("git repository")
        .default(default_repo)
        .allow_empty(true)
        .interact_text()?;

    let keywords_input: String = Input::new()
        .with_prompt("keywords")
        .default(String::new())
        .allow_empty(true)
        .interact_text()?;

    let author: String = Input::new()
        .with_prompt("author")
        .default(default_author)
        .allow_empty(true)
        .interact_text()?;

    let license: String = Input::new()
        .with_prompt("license")
        .default("ISC".to_string())
        .interact_text()?;

    let test_script = if test_command.is_empty() {
        "echo \"Error: no test specified\" && exit 1".to_string()
    } else {
        test_command
    };

    let keywords: Vec<String> = if keywords_input.is_empty() {
        vec![]
    } else {
        keywords_input
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    };

    let mut pkg = json!({
        "name": name,
        "version": version,
        "description": description,
        "main": entry_point,
        "scripts": {
            "test": test_script
        },
        "keywords": keywords,
        "author": author,
        "license": license
    });

    if !git_repository.is_empty() {
        pkg["repository"] = json!({
            "type": "git",
            "url": normalize_git_url(&git_repository)
        });
    }

    Ok(pkg)
}

fn dir_name(cwd: &Path) -> String {
    cwd.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("package")
        .to_string()
}

fn detect_git_author() -> String {
    let name = std::process::Command::new("git")
        .args(["config", "user.name"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok().map(|s| s.trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_default();

    let email = std::process::Command::new("git")
        .args(["config", "user.email"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok().map(|s| s.trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_default();

    match (name.is_empty(), email.is_empty()) {
        (true, true) => String::new(),
        (false, true) => name,
        (true, false) => format!("<{email}>"),
        (false, false) => format!("{name} <{email}>"),
    }
}

fn detect_git_repository(cwd: &Path) -> String {
    std::process::Command::new("git")
        .args(["config", "--get", "remote.origin.url"])
        .current_dir(cwd)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout)
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            } else {
                None
            }
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dir_name() {
        let path = Path::new("/home/user/my-project");
        assert_eq!(dir_name(path), "my-project");
    }

    #[test]
    fn test_dir_name_root() {
        let path = Path::new("/");
        // Root path has no file_name, should fallback to "package"
        assert_eq!(dir_name(path), "package");
    }

    #[test]
    fn test_build_default_package_structure() {
        let cwd = Path::new("/tmp/test-pkg");
        let pkg = build_default_package(cwd);

        assert_eq!(pkg["name"], "test-pkg");
        assert_eq!(pkg["version"], "1.0.0");
        assert_eq!(pkg["main"], "index.js");
        assert_eq!(pkg["license"], "ISC");
        assert!(pkg["scripts"]["test"].is_string());
        assert!(pkg["keywords"].is_array());
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

        assert_eq!(pkg["name"], dir.path().file_name().unwrap().to_str().unwrap());
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

    #[test]
    fn test_normalize_git_url_ssh_shorthand() {
        assert_eq!(
            normalize_git_url("git@github.com:user/repo.git"),
            "git+ssh://git@github.com/user/repo.git"
        );
    }

    #[test]
    fn test_normalize_git_url_https() {
        assert_eq!(
            normalize_git_url("https://github.com/user/repo.git"),
            "git+https://github.com/user/repo.git"
        );
    }

    #[test]
    fn test_normalize_git_url_http() {
        assert_eq!(
            normalize_git_url("http://github.com/user/repo.git"),
            "git+http://github.com/user/repo.git"
        );
    }

    #[test]
    fn test_normalize_git_url_already_prefixed() {
        let url = "git+ssh://git@github.com/user/repo.git";
        assert_eq!(normalize_git_url(url), url);

        let url2 = "git+https://github.com/user/repo.git";
        assert_eq!(normalize_git_url(url2), url2);
    }

    #[test]
    fn test_normalize_git_url_other_format() {
        let url = "file:///path/to/repo";
        assert_eq!(normalize_git_url(url), url);
    }
}
