//! Cache cleanup command.

use anyhow::Result;

use crate::error::{CliError, ErrorKind};
use crate::model::cli_output::{
    CleanPartialResult, CleanResult, CleanSummary, ErrorDetails, PackageVersion, PartialResult,
};
use crate::service::clean_cache::{collect_cache_entries, delete_cache_entries};
use crate::util::cli_enum::ConfirmationPolicy;
use crate::util::format_print::confirm;
use crate::util::invocation;
use crate::util::presenter::emit;

pub async fn run(pattern: &str, confirmation: ConfirmationPolicy) -> Result<()> {
    let to_delete = collect_cache_entries(pattern).await?;

    if to_delete.is_empty() {
        tracing::debug!("No matching cache files found");
        return emit(
            "clean",
            &CleanResult {
                pattern: pattern.to_string(),
                deleted: Vec::new(),
                summary: CleanSummary {
                    matched: 0,
                    deleted: 0,
                },
            },
            || Ok(()),
        );
    }

    if !invocation::json() {
        println!("\nThe following caches will be deleted:");
        for entry in &to_delete {
            println!("- {}@{}", entry.name, entry.version);
        }

        println!();
    }
    tracing::debug!(
        "Note: This will only delete caches from global storage and won't affect dependencies in the current project. If you need to reinstall project dependencies, please run 'utoo update'",
    );

    let confirmed = match confirmation {
        ConfirmationPolicy::Prompt => confirm(&format!(
            "\nConfirm to delete these {} packages? [y/N] ",
            to_delete.len()
        ))?,
        ConfirmationPolicy::AssumeYes => true,
    };
    if confirmed {
        let matched = to_delete.len() as u64;
        let (deleted, failed) = delete_cache_entries(to_delete).await;
        tracing::debug!("Cleanup completed");
        if !invocation::json() {
            return Ok(());
        }
        let deleted = deleted
            .into_iter()
            .map(|entry| PackageVersion {
                name: entry.name,
                version: entry.version,
            })
            .collect::<Vec<_>>();
        if let Some((entry, error)) = failed.into_iter().next() {
            return Err(CliError::new(
                ErrorKind::Local,
                format!("failed to delete {}@{}: {error}", entry.name, entry.version),
            )
            .with_code("cache_delete_failed")
            .with_partial_result(PartialResult::Clean(CleanPartialResult {
                deleted: deleted.clone(),
            }))
            .with_details(ErrorDetails::Filesystem {
                path: entry.path.to_string_lossy().into_owned(),
            })
            .into());
        }
        let output = CleanResult {
            pattern: pattern.to_string(),
            summary: CleanSummary {
                matched,
                deleted: deleted.len() as u64,
            },
            deleted,
        };
        return emit("clean", &output, || Ok(()));
    }

    if !invocation::json() {
        return Ok(());
    }
    emit(
        "clean",
        &CleanResult {
            pattern: pattern.to_string(),
            deleted: Vec::new(),
            summary: CleanSummary {
                matched: to_delete.len() as u64,
                deleted: 0,
            },
        },
        || Ok(()),
    )
}
