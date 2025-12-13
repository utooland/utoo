use anyhow::Result;
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

use super::registry::RegistryService;
use crate::util::cache::get_cache_dir;
use crate::util::config::{
    get_legacy_peer_deps, get_preload_downloads_limit, get_preload_manifests_limit,
};
use crate::util::downloader::download;

/// Statistics for measuring request performance
#[derive(Debug, Default)]
struct RequestStats {
    durations: Vec<Duration>,
}

impl RequestStats {
    fn new() -> Self {
        Self {
            durations: Vec::new(),
        }
    }

    fn add(&mut self, duration: Duration) {
        self.durations.push(duration);
    }

    fn merge(&mut self, other: RequestStats) {
        self.durations.extend(other.durations);
    }

    fn get_percentile(&self, percentile: f64) -> Option<Duration> {
        if self.durations.is_empty() {
            return None;
        }
        let mut sorted = self.durations.clone();
        sorted.sort();
        let index = ((sorted.len() as f64 * percentile / 100.0).ceil() as usize).saturating_sub(1);
        sorted.get(index).copied()
    }

    fn max(&self) -> Option<Duration> {
        self.durations.iter().max().copied()
    }

    fn avg(&self) -> Option<Duration> {
        if self.durations.is_empty() {
            return None;
        }
        let sum: Duration = self.durations.iter().sum();
        Some(sum / self.durations.len() as u32)
    }
}

// Concurrency limits are now loaded from config via get_preload_manifests_limit()
// and get_preload_downloads_limit() - no more hardcoded constants!

/// Dependency item for preloading
/// Represents a package dependency that needs to be preloaded
#[derive(Debug, Clone)]
struct DepItem {
    name: String,
    spec: String,
}

/// Result of preloading a single package
/// This structure improves readability compared to returning an 8-element tuple
#[derive(Debug)]
struct PreloadResult {
    next_deps: Vec<DepItem>,
    manifest_success: bool,
    manifest_failed: bool,
    download_success: bool,
    download_failed: bool,
    download_skipped: bool,
    manifest_stats: RequestStats,
    download_stats: RequestStats,
}

/// Service for preloading package manifests to improve resolution speed
pub struct PreloadService;

impl PreloadService {
    /// Preload package manifests recursively with unordered concurrency
    ///
    /// # Arguments
    /// * `initial_deps` - Initial list of (name, spec) pairs to preload
    /// * `package_lock_only` - If true, only preload manifests; if false, also download tgz files
    ///
    /// # Returns
    /// * `Ok(())` on success
    pub async fn preload(
        initial_deps: Vec<(String, String)>,
        package_lock_only: bool,
    ) -> Result<()> {
        let start_time = std::time::Instant::now();

        // Load concurrency limits from config
        let manifests_limit = get_preload_manifests_limit().await;
        let downloads_limit = get_preload_downloads_limit().await;

        tracing::debug!(
            "Starting preload with {} initial dependencies, package_lock_only={}, manifests_limit={}, downloads_limit={}",
            initial_deps.len(),
            package_lock_only,
            manifests_limit,
            downloads_limit,
        );

        // Create semaphores for concurrency control
        let manifest_semaphore = Arc::new(Semaphore::new(manifests_limit));
        let download_semaphore = Arc::new(Semaphore::new(downloads_limit));

        // Track processed packages to avoid duplicates (use name@spec as key)
        let mut processed = HashSet::new();

        // Statistics
        let mut manifest_success_count = 0usize;
        let mut manifest_failed_count = 0usize;
        let mut download_success_count = 0usize;
        let mut download_failed_count = 0usize;
        let mut download_skipped_count = 0usize;
        let mut manifest_stats = RequestStats::new();
        let mut download_stats = RequestStats::new();

        // Convert initial deps to DepItems
        let mut queue: Vec<DepItem> = initial_deps
            .into_iter()
            .map(|(name, spec)| DepItem { name, spec })
            .collect();

        let legacy_peer_deps = get_legacy_peer_deps().await;

        while !queue.is_empty() {
            tracing::debug!("Processing batch of {} packages", queue.len());

            let mut tasks = FuturesUnordered::new();

            // Add tasks to FuturesUnordered
            for item in queue.drain(..) {
                let key = format!("{}@{}", item.name, item.spec);

                // Skip if already processed
                if processed.contains(&key) {
                    continue;
                }
                processed.insert(key.clone());

                let name = item.name.clone();
                let spec = item.spec.clone();
                let manifest_sem = Arc::clone(&manifest_semaphore);
                let download_sem = Arc::clone(&download_semaphore);

                let task = async move {
                    // Acquire manifest permit
                    let _manifest_permit = manifest_sem.acquire().await.unwrap();

                    tracing::debug!("Preloading manifest for {}", key);

                    let mut result = PreloadResult {
                        next_deps: Vec::new(),
                        manifest_success: false,
                        manifest_failed: false,
                        download_success: false,
                        download_failed: false,
                        download_skipped: false,
                        manifest_stats: RequestStats::new(),
                        download_stats: RequestStats::new(),
                    };

                    // Measure manifest fetch time
                    let manifest_start = std::time::Instant::now();
                    match RegistryService::resolve_package(&name, &spec).await {
                        Ok(resolved) => {
                            let manifest_duration = manifest_start.elapsed();
                            result.manifest_stats.add(manifest_duration);

                            tracing::debug!(
                                "Successfully preloaded manifest for {} => {} in {:.2}ms",
                                key,
                                resolved.version,
                                manifest_duration.as_secs_f64() * 1000.0
                            );
                            result.manifest_success = true;

                            // Download tgz if needed
                            if !package_lock_only {
                                // Acquire download permit
                                let _download_permit = download_sem.acquire().await.unwrap();

                                let download_start = std::time::Instant::now();
                                match Self::download_package(
                                    &name,
                                    &resolved.version,
                                    &resolved.manifest,
                                )
                                .await
                                {
                                    Ok(was_cached) => {
                                        let download_duration = download_start.elapsed();
                                        result.download_stats.add(download_duration);

                                        if was_cached {
                                            result.download_skipped = true;
                                        } else {
                                            result.download_success = true;
                                        }
                                    }
                                    Err(e) => {
                                        // Preload download failure is not critical, just warn
                                        tracing::warn!(
                                            "Failed to download package {} during preload: {}",
                                            key,
                                            e
                                        );
                                        result.download_failed = true;
                                    }
                                }
                            }

                            // Extract dependencies for next level
                            // Always pass false for include_dev - only root/workspace load devDeps
                            result.next_deps = Self::extract_dependencies(
                                &resolved.manifest,
                                false, // Don't load devDeps recursively
                                legacy_peer_deps,
                            );
                        }
                        Err(e) => {
                            // Preload is best-effort, failure should not block
                            // Main build_deps will handle retries
                            tracing::debug!("Failed to preload manifest for {}: {}", key, e);
                            result.manifest_failed = true;
                            // result.next_deps is already an empty Vec from initialization
                        }
                    }

                    result
                };

                tasks.push(task);
            }

            // Collect results from all tasks
            let mut next_queue = Vec::new();
            while let Some(result) = tasks.next().await {
                // Collect statistics
                if result.manifest_success {
                    manifest_success_count += 1;
                }
                if result.manifest_failed {
                    manifest_failed_count += 1;
                }
                if result.download_success {
                    download_success_count += 1;
                }
                if result.download_failed {
                    download_failed_count += 1;
                }
                if result.download_skipped {
                    download_skipped_count += 1;
                }

                // Merge timing statistics
                manifest_stats.merge(result.manifest_stats);
                download_stats.merge(result.download_stats);

                // Collect deps for next level
                next_queue.extend(result.next_deps);
            }

            // Move to next level
            queue = next_queue;
        }

        let duration = start_time.elapsed();

        tracing::warn!(
            "Preload phase completed: {} success, {} failed, took {:.2}s",
            manifest_success_count,
            manifest_failed_count,
            duration.as_secs_f64()
        );

        tracing::debug!(
            "Preload completed in {:.2}s: {} packages processed | Manifests: {} success, {} failed | Downloads: {} success, {} failed, {} cached",
            duration.as_secs_f64(),
            processed.len(),
            manifest_success_count,
            manifest_failed_count,
            download_success_count,
            download_failed_count,
            download_skipped_count
        );

        // Calculate and log success rates
        let manifest_total = manifest_success_count + manifest_failed_count;
        if manifest_total > 0 {
            let manifest_success_rate =
                (manifest_success_count as f64 / manifest_total as f64) * 100.0;
            tracing::debug!(
                "Manifest success rate: {:.1}% ({}/{})",
                manifest_success_rate,
                manifest_success_count,
                manifest_total
            );
        }

        // Log manifest timing statistics
        if let (Some(avg), Some(p50), Some(p90), Some(p99), Some(max)) = (
            manifest_stats.avg(),
            manifest_stats.get_percentile(50.0),
            manifest_stats.get_percentile(90.0),
            manifest_stats.get_percentile(99.0),
            manifest_stats.max(),
        ) {
            tracing::debug!(
                "Manifest fetch timing: avg={:.0}ms, p50={:.0}ms, p90={:.0}ms, p99={:.0}ms, max={:.0}ms",
                avg.as_secs_f64() * 1000.0,
                p50.as_secs_f64() * 1000.0,
                p90.as_secs_f64() * 1000.0,
                p99.as_secs_f64() * 1000.0,
                max.as_secs_f64() * 1000.0
            );
        }

        if !package_lock_only {
            let download_total =
                download_success_count + download_failed_count + download_skipped_count;
            if download_total > 0 {
                let download_success_rate = ((download_success_count + download_skipped_count)
                    as f64
                    / download_total as f64)
                    * 100.0;
                tracing::debug!(
                    "Download success rate: {:.1}% ({}/{})",
                    download_success_rate,
                    download_success_count + download_skipped_count,
                    download_total
                );
            }

            // Log download timing statistics
            if let (Some(avg), Some(p50), Some(p90), Some(max)) = (
                download_stats.avg(),
                download_stats.get_percentile(50.0),
                download_stats.get_percentile(90.0),
                download_stats.max(),
            ) {
                tracing::debug!(
                    "Download timing: avg={:.0}ms, p50={:.0}ms, p90={:.0}ms, max={:.0}ms",
                    avg.as_secs_f64() * 1000.0,
                    p50.as_secs_f64() * 1000.0,
                    p90.as_secs_f64() * 1000.0,
                    max.as_secs_f64() * 1000.0
                );
            }
        }

        Ok(())
    }

    /// Download package tarball to cache
    ///
    /// Returns `Ok(true)` if the package was already cached, `Ok(false)` if it was downloaded
    async fn download_package(
        name: &str,
        version: &str,
        manifest: &serde_json::Value,
    ) -> Result<bool> {
        // Get tarball URL from manifest
        let tarball_url = manifest["dist"]["tarball"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Failed to get tarball URL from manifest"))?;

        // Calculate cache path
        let cache_dir = get_cache_dir();
        let cache_path = cache_dir.join(format!("{name}/{version}"));
        let cache_flag_path = cache_dir.join(format!("{name}/{version}/_resolved"));

        // Download if not cached
        if !tokio::fs::try_exists(&cache_flag_path).await? {
            tracing::debug!("Downloading {} to {}", tarball_url, cache_path.display());
            download(tarball_url, &cache_path).await?;
            Ok(false) // Downloaded
        } else {
            tracing::debug!("Package already cached: {}@{}", name, version);
            Ok(true) // Cached
        }
    }

    /// Helper to extract dependencies from a specific field in package.json
    fn extract_deps_from_field(manifest: &serde_json::Value, field: &str) -> Vec<DepItem> {
        let mut deps = Vec::new();
        if let Some(dep_obj) = manifest.get(field).and_then(|v| v.as_object()) {
            for (name, version) in dep_obj {
                if let Some(version_str) = version.as_str() {
                    // Skip local dependencies (file:, link:, workspace:, portal:)
                    if version_str.starts_with("file:")
                        || version_str.starts_with("link:")
                        || version_str.starts_with("workspace:")
                        || version_str.starts_with("portal:")
                    {
                        tracing::debug!(
                            "Skipping local dependency in preload: {}@{}",
                            name,
                            version_str
                        );
                        continue;
                    }

                    deps.push(DepItem {
                        name: name.clone(),
                        spec: version_str.to_string(),
                    });
                }
            }
        }
        deps
    }

    /// Extract dependencies from a manifest
    ///
    /// # Arguments
    /// * `manifest` - Package manifest JSON
    /// * `include_dev` - Whether to include devDependencies (true for root/workspace)
    /// * `legacy_peer_deps` - Whether to skip peerDependencies
    fn extract_dependencies(
        manifest: &serde_json::Value,
        include_dev: bool,
        legacy_peer_deps: bool,
    ) -> Vec<DepItem> {
        let mut deps = Vec::new();

        // Always include prod dependencies
        deps.extend(Self::extract_deps_from_field(manifest, "dependencies"));

        // Include dev deps for root/workspace nodes
        if include_dev {
            deps.extend(Self::extract_deps_from_field(manifest, "devDependencies"));
        }

        // Include peer deps unless legacy mode
        if !legacy_peer_deps {
            deps.extend(Self::extract_deps_from_field(manifest, "peerDependencies"));
        }

        // Always include optional dependencies
        deps.extend(Self::extract_deps_from_field(
            manifest,
            "optionalDependencies",
        ));

        deps
    }

    /// Collect all dependencies from root package manifest (including devDependencies)
    pub fn collect_root_dependencies(
        manifest: &serde_json::Value,
        legacy_peer_deps: bool,
    ) -> Vec<(String, String)> {
        let items = Self::extract_dependencies(manifest, true, legacy_peer_deps);
        items
            .into_iter()
            .map(|item| (item.name, item.spec))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_dependencies_with_dev() {
        let manifest = json!({
            "name": "test-package",
            "version": "1.0.0",
            "dependencies": {
                "lodash": "^4.17.21",
                "react": "^18.0.0"
            },
            "devDependencies": {
                "jest": "^29.0.0"
            },
            "peerDependencies": {
                "typescript": "^5.0.0"
            },
            "optionalDependencies": {
                "fsevents": "^2.3.2"
            }
        });

        // Root level: include dev, include peer (not legacy)
        // Should include: lodash, react, jest, typescript, fsevents (5 total)
        let deps = PreloadService::extract_dependencies(&manifest, true, false);
        assert_eq!(deps.len(), 5);

        // Non-root level: exclude dev, include peer (not legacy)
        // Should include: lodash, react, typescript, fsevents (4 total, no jest)
        let deps = PreloadService::extract_dependencies(&manifest, false, false);
        assert_eq!(deps.len(), 4);

        // Legacy mode: include dev, exclude peer
        // Should include: lodash, react, jest, fsevents (4 total, no typescript)
        let deps_legacy = PreloadService::extract_dependencies(&manifest, true, true);
        assert_eq!(deps_legacy.len(), 4);
    }

    #[test]
    fn test_collect_root_dependencies() {
        let manifest = json!({
            "dependencies": {
                "lodash": "^4.17.21"
            },
            "devDependencies": {
                "jest": "^29.0.0"
            }
        });

        let deps = PreloadService::collect_root_dependencies(&manifest, false);
        assert_eq!(deps.len(), 2);
        assert!(deps.contains(&("lodash".to_string(), "^4.17.21".to_string())));
        assert!(deps.contains(&("jest".to_string(), "^29.0.0".to_string())));
    }
}
