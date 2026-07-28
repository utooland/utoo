use anyhow::{Context, Result};
use colored::Colorize;
use serde::Serialize;
use std::io::{self, Write};
use std::path::PathBuf;

use crate::model::RunMode;
use crate::service::pm_pack as pack_service;
use crate::service::script::ScriptOutput;
use crate::util::format_print::print_pack_details;
use crate::util::invocation;
use crate::util::presenter::emit;

pub async fn pack(path: Option<String>, mode: RunMode) -> Result<()> {
    let package_root = if let Some(p) = path {
        PathBuf::from(p)
    } else {
        std::env::current_dir()?
    };

    let output = if invocation::json() {
        ScriptOutput::Machine
    } else {
        ScriptOutput::Verbose
    };
    let result = pack_service::pack(&package_root, output).await?;

    let tarball_path = match mode {
        RunMode::DryRun => None,
        RunMode::Live => {
            let tarball_path = package_root.join(result.tarball_filename());
            crate::fs::write(&tarball_path, &result.tarball_data)
                .await
                .with_context(|| {
                    format!("Failed to write tarball to {}", tarball_path.display())
                })?;
            Some(tarball_path)
        }
    };

    let output = PackOutput {
        name: &result.name,
        version: &result.version,
        files: result
            .files
            .iter()
            .map(|(path, size)| PackFile { path, size: *size })
            .collect(),
        unpacked_size: result.unpacked_size,
        packed_size: result.packed_size,
        integrity: &result.integrity,
        dry_run: mode == RunMode::DryRun,
        tarball: tarball_path.as_ref().map(|path| path.display().to_string()),
    };
    emit("pack", &output, || {
        let mut stdout = io::stdout().lock();
        print_pack_details(&mut stdout, &result, None)?;
        match &tarball_path {
            None => writeln!(
                stdout,
                "{}",
                "(dry run) Tarball not written to disk".yellow()
            )?,
            Some(path) => writeln!(stdout, "{} {}", "Tarball:".dimmed(), path.display())?,
        }
        Ok(())
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PackOutput<'a> {
    name: &'a str,
    version: &'a str,
    files: Vec<PackFile<'a>>,
    unpacked_size: u64,
    packed_size: u64,
    integrity: &'a str,
    dry_run: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tarball: Option<String>,
}

#[derive(Serialize)]
struct PackFile<'a> {
    path: &'a str,
    size: u64,
}
