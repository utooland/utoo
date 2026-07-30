use anyhow::{Context, Result};
use colored::Colorize;
use std::io::{self, Write};
use std::path::PathBuf;

use crate::model::RunMode;
use crate::model::cli_output::{PackFile, PackResult as CliPackResult};
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

    if !invocation::json() {
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
        return Ok(());
    }

    let mut files = result
        .files
        .iter()
        .map(|(path, size)| PackFile {
            path: path.replace('\\', "/"),
            size: *size,
        })
        .collect::<Vec<_>>();
    files.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    let tarball_path = tarball_path
        .map(|path| -> Result<String> {
            let path = if path.is_absolute() {
                path
            } else {
                std::env::current_dir()?.join(path)
            };
            Ok(path.to_string_lossy().into_owned())
        })
        .transpose()?;
    let output = CliPackResult {
        name: result.name.clone(),
        version: result.version.clone(),
        filename: result.tarball_filename(),
        files,
        unpacked_size: result.unpacked_size,
        packed_size: result.packed_size,
        integrity: result.integrity.clone(),
        dry_run: mode == RunMode::DryRun,
        tarball_path,
    };
    emit("pack", &output, || Ok(()))
}
