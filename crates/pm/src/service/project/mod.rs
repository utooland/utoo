//! Project discovery, manifest mutation, dependency resolution and lock persistence.
//! The caller supplies the directory; project operations never change cwd.
pub mod context;
pub mod discovery;
pub mod lock;

use crate::util::logger::{finish_progress_bar, start_progress_bar};
use anyhow::Result;
use context::Context;
use std::path::Path;
use std::time::Instant;
use utoo_ruborist::lock::PackageLock;
use utoo_ruborist::progress::EventReceiver;

pub async fn resolve_and_save_lock<R: EventReceiver>(
    cwd: &Path,
    receiver: R,
) -> Result<PackageLock> {
    let (root, package) = utoo_ruborist::service::read_root_manifest(cwd, Context::glob()).await?;
    let options = Context::deps_options(root.clone(), receiver).await;
    start_progress_bar();
    let started = Instant::now();
    let lock = utoo_ruborist::service::build_deps(options, package).await?;
    finish_progress_bar("package-lock.json resolved", Some(started.elapsed()));
    lock::save_package_lock(&root, &lock).await?;
    Ok(lock)
}
