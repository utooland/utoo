use anyhow::{Context, Result};
use colored::Colorize;
use std::io::{self, Write};
use std::path::PathBuf;

use crate::model::RunMode;
use crate::service::pm_pack as pack_service;
use crate::util::format_print::print_pack_details;

pub async fn pack(path: Option<String>, mode: RunMode) -> Result<()> {
    let package_root = if let Some(p) = path {
        PathBuf::from(p)
    } else {
        std::env::current_dir()?
    };

    let result = pack_service::pack(&package_root).await?;

    let mut stdout = io::stdout().lock();
    print_pack_details(&mut stdout, &result, None)?;

    match mode {
        RunMode::DryRun => {
            writeln!(
                stdout,
                "{}",
                "(dry run) Tarball not written to disk".yellow()
            )?;
        }
        RunMode::Live => {
            let tarball_path = package_root.join(result.tarball_filename());
            crate::fs::write(&tarball_path, &result.tarball_data)
                .await
                .with_context(|| {
                    format!("Failed to write tarball to {}", tarball_path.display())
                })?;
            writeln!(stdout, "{} {}", "Tarball:".dimmed(), tarball_path.display())?;
        }
    }

    Ok(())
}
