use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use bytes::Bytes;
use dialoguer::Confirm;
use serde::Serialize;
use utoo_ruborist::service::{MetadataFormat, fetch_full_manifest_fresh};
use utoo_ruborist::tar::is_safe_tar_entry_path;

use crate::cli::{SkillCommands, SkillTarget};
use crate::service::auth;
use crate::util::downloader::download_bytes;
use crate::util::integrity::compute_integrity;
use crate::util::user_config::get_registry;

const SKILL_PACKAGE: &str = "@utoo/skills";

struct AgentTarget {
    name: &'static str,
    skills_dir: &'static str,
}

const AGENTS: &[AgentTarget] = &[
    AgentTarget {
        name: "claude",
        skills_dir: ".claude/skills",
    },
    AgentTarget {
        name: "codex",
        skills_dir: ".codex/skills",
    },
    AgentTarget {
        name: "cursor",
        skills_dir: ".cursor/skills",
    },
];

pub async fn run(command: SkillCommands) -> Result<()> {
    match command {
        SkillCommands::Setup { target, yes } => setup(target, yes).await,
    }
}

async fn setup(target: SkillTarget, yes: bool) -> Result<()> {
    let home = dirs::home_dir().ok_or_else(|| {
        crate::error::CliError::new(crate::error::ErrorKind::Local, "Home directory not found")
    })?;
    let targets = resolve_targets(target);
    let destinations: Vec<_> = targets
        .iter()
        .map(|target| (target.name, home.join(target.skills_dir).join("utoo")))
        .collect();

    if !yes {
        if crate::util::invocation::json() || !crate::util::invocation::interactive() {
            return Err(crate::error::CliError::usage(
                "refusing to install the Agent Skill without confirmation",
            )
            .with_suggestion("re-run with `utoo skill setup --yes`")
            .into());
        }
        let names = targets
            .iter()
            .map(|target| target.name)
            .collect::<Vec<_>>()
            .join(", ");
        if !Confirm::new()
            .with_prompt(format!("Install the utoo Agent Skill to {names}?"))
            .default(false)
            .interact()
            .context("Failed to read confirmation")?
        {
            return Err(crate::error::CliError::new(
                crate::error::ErrorKind::Cancelled,
                "cancelled",
            )
            .into());
        }
    }

    let (version, files) = download_skill().await?;
    for (_, destination) in &destinations {
        write_skill(destination, &files).await?;
    }

    let output = SkillSetupOutput {
        package: SKILL_PACKAGE,
        version,
        installed: destinations
            .iter()
            .map(|(agent, path)| InstalledSkill {
                agent,
                path: path.display().to_string(),
            })
            .collect(),
    };
    crate::util::presenter::emit("skill setup", &output, || {
        println!(
            "Installed utoo Agent Skill to: {}",
            targets
                .iter()
                .map(|target| target.name)
                .collect::<Vec<_>>()
                .join(", ")
        );
        Ok(())
    })
}

fn resolve_targets(target: SkillTarget) -> Vec<&'static AgentTarget> {
    let name = match target {
        SkillTarget::All => return AGENTS.iter().collect(),
        SkillTarget::Claude => "claude",
        SkillTarget::Codex => "codex",
        SkillTarget::Cursor => "cursor",
    };
    AGENTS.iter().filter(|agent| agent.name == name).collect()
}

async fn download_skill() -> Result<(String, Vec<SkillFile>)> {
    let registry = get_registry();
    let token = auth::token_for_url(&registry).await;
    let (manifest, _) = fetch_full_manifest_fresh(
        &registry,
        SKILL_PACKAGE,
        MetadataFormat::Complete,
        token.as_deref(),
    )
    .await
    .context("Failed to fetch @utoo/skills metadata")?;
    let version = utoo_ruborist::registry::resolve_target_version((&manifest).into(), "latest")
        .context("Failed to resolve the latest @utoo/skills version")?;
    let package = manifest.get_full_version(&version).ok_or_else(|| {
        crate::error::CliError::not_found(format!("@utoo/skills@{version} not found"))
    })?;
    let tarball = package.core.dist.tarball.as_deref().ok_or_else(|| {
        crate::error::CliError::new(
            crate::error::ErrorKind::Local,
            "@utoo/skills metadata has no tarball",
        )
    })?;
    let token = auth::token_for_url(tarball).await;
    let bytes = download_bytes(tarball, token.as_deref()).await?;
    if let Some(expected) = package.core.dist.integrity.as_deref() {
        let actual = compute_integrity(&bytes);
        if actual != expected {
            return Err(crate::error::CliError::new(
                crate::error::ErrorKind::Local,
                "@utoo/skills tarball integrity check failed",
            )
            .into());
        }
    }
    let files = tokio::task::spawn_blocking(move || extract_skill_files(bytes)).await??;
    Ok((version, files))
}

fn extract_skill_files(bytes: Bytes) -> Result<Vec<SkillFile>> {
    let decoder = flate2::read::GzDecoder::new(bytes.as_ref());
    let mut archive = tar::Archive::new(decoder);
    let mut files = Vec::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let archive_path = entry.path()?.into_owned();
        if !is_safe_tar_entry_path(&archive_path) {
            continue;
        }
        let Some(path) = archive_path.strip_prefix("package").ok() else {
            continue;
        };
        if !is_skill_file(path) {
            continue;
        }
        let mut contents = Vec::new();
        entry.read_to_end(&mut contents)?;
        files.push(SkillFile {
            path: path.to_path_buf(),
            contents,
        });
    }
    if !files.iter().any(|file| file.path == Path::new("SKILL.md")) {
        return Err(crate::error::CliError::new(
            crate::error::ErrorKind::Local,
            "@utoo/skills tarball does not contain SKILL.md",
        )
        .into());
    }
    Ok(files)
}

fn is_skill_file(path: &Path) -> bool {
    matches!(
        path.to_str(),
        Some("SKILL.md" | "reference.md" | "examples.md")
    ) || path.starts_with("references")
        || path.starts_with("scripts")
        || path.starts_with("agents")
}

async fn write_skill(destination: &Path, files: &[SkillFile]) -> Result<()> {
    tokio::fs::create_dir_all(destination)
        .await
        .map_err(|error| {
            crate::error::CliError::new(
                crate::error::ErrorKind::Local,
                format!("Failed to create {}", destination.display()),
            )
            .with_source(error)
        })?;
    for file in files {
        let path = destination.join(&file.path);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&path, &file.contents)
            .await
            .with_context(|| format!("Failed to write {}", path.display()))?;
    }
    Ok(())
}

struct SkillFile {
    path: PathBuf,
    contents: Vec<u8>,
}

#[derive(Serialize)]
struct SkillSetupOutput {
    package: &'static str,
    version: String,
    installed: Vec<InstalledSkill>,
}

#[derive(Serialize)]
struct InstalledSkill {
    agent: &'static str,
    path: String,
}

#[cfg(test)]
mod tests {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::{Builder, Header};

    use super::*;

    fn skill_tarball() -> Bytes {
        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut archive = Builder::new(encoder);
        for (path, contents) in [
            ("package/SKILL.md", b"---\nname: utoo\n---\n".as_slice()),
            ("package/reference.md", b"reference".as_slice()),
            ("package/install-skill.js", b"ignored".as_slice()),
        ] {
            let mut header = Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            archive.append_data(&mut header, path, contents).unwrap();
        }
        let encoder = archive.into_inner().unwrap();
        Bytes::from(encoder.finish().unwrap())
    }

    #[test]
    fn extracts_only_agent_skill_files() {
        let files = extract_skill_files(skill_tarball()).unwrap();
        let paths: Vec<_> = files.iter().map(|file| file.path.as_path()).collect();
        assert_eq!(paths, [Path::new("SKILL.md"), Path::new("reference.md")]);
    }

    #[test]
    fn resolves_all_supported_agents() {
        let names: Vec<_> = resolve_targets(SkillTarget::All)
            .iter()
            .map(|target| target.name)
            .collect();
        assert_eq!(names, ["claude", "codex", "cursor"]);
    }
}
