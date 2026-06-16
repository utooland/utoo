use anyhow::Result;

use crate::service::clean_cache::{collect_cache_entries, delete_cache_entries};
use crate::util::format_print::confirm;

pub async fn clean(pattern: &str) -> Result<()> {
    let to_delete = collect_cache_entries(pattern).await?;

    if to_delete.is_empty() {
        tracing::debug!("No matching cache files found");
        return Ok(());
    }

    println!("\nThe following caches will be deleted:");
    for (pkg, version, _) in &to_delete {
        println!("- {pkg}@{version}");
    }

    println!();
    tracing::debug!(
        "Note: This will only delete caches from global storage and won't affect dependencies in the current project. If you need to reinstall project dependencies, please run 'utoo update'",
    );

    if confirm(&format!(
        "\nConfirm to delete these {} packages? [y/N] ",
        to_delete.len()
    ))? {
        delete_cache_entries(to_delete).await;
        tracing::debug!("Cleanup completed");
    }

    Ok(())
}
