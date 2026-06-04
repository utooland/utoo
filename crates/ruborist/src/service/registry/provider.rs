//! `UnifiedRegistry`'s [`ManifestProvider`] implementation.
//!
//! Executes a single manifest job (full / version / extract) against the
//! network and the persistent [`ManifestStore`](super::super::store::ManifestStore).
//! The demand BFS loop owns caching, waiters, and de-duplication; this module
//! is the stateless I/O + parse boundary it drives.
//!
//! Lives as a child module of `registry` so it can reach `UnifiedRegistry`'s
//! private fields without widening their visibility.

use std::sync::Arc;

use anyhow::anyhow;
use async_trait::async_trait;
use deno_semver::Version;

use super::super::cache::{Versions, VersionsInfo};
use super::super::manifest;
use super::super::manifest_provider::{
    ManifestFullData, ManifestJob, ManifestJobDone, ManifestProvider, ProviderFullManifestBytes,
};
use super::UnifiedRegistry;
use crate::model::manifest::{CoreVersionManifest, extract_core_version_off_runtime};
use crate::traits::registry::RegistryError;

impl UnifiedRegistry {
    /// Metadata format for a version-manifest fetch. Semver registries accept
    /// the abbreviated install-v1 form for range/tag queries; non-semver (npmjs)
    /// exact-version fetches need the complete document. Derived from the
    /// registry's own capability, so the demand loop never has to pass it.
    fn version_fetch_format(&self) -> manifest::MetadataFormat {
        if self.supports_semver {
            manifest::MetadataFormat::Abbreviated
        } else {
            manifest::MetadataFormat::Complete
        }
    }

    /// Persist a resolved version manifest through the host store.
    ///
    /// De-duplicated by `(name, version)`: sibling specs frequently resolve to
    /// the same version and version manifests are immutable, so the manifest is
    /// persisted only on its first sighting this run.
    fn store_version_manifest(&self, name: &str, manifest: Arc<CoreVersionManifest>) {
        if self
            .stored_version
            .insert((name.to_string(), manifest.version.clone()))
        {
            self.store
                .store_version_manifest(name, &manifest.version, Arc::clone(&manifest));
        }
    }

    /// Fetch the full (abbreviated) manifest bytes, using the store's cached
    /// ETag for a conditional GET. A 304 hands back the cached versions list.
    async fn fetch_full_manifest_job(
        &self,
        name: &str,
    ) -> Result<ProviderFullManifestBytes, RegistryError> {
        let store_versions = self.store.load_versions(name).await.map(Arc::new);
        let etag = store_versions.as_ref().and_then(|v| v.etag.clone());

        match manifest::fetch_full_manifest_bytes(manifest::FetchManifestOptions {
            registry_url: &self.registry_url,
            name,
            format: manifest::MetadataFormat::Abbreviated,
            etag: etag.as_deref(),
            auth_token: self.auth_token.as_deref(),
        })
        .await
        .map_err(RegistryError)?
        {
            manifest::FetchManifestBytesResult::Ok(bytes, etag) => {
                Ok(ProviderFullManifestBytes::Fresh { bytes, etag })
            }
            manifest::FetchManifestBytesResult::NotModified => {
                let versions = store_versions.ok_or_else(|| {
                    RegistryError(anyhow!(
                        "304 Not Modified without cached versions for {name}"
                    ))
                })?;
                Ok(ProviderFullManifestBytes::NotModified { versions })
            }
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl ManifestProvider for UnifiedRegistry {
    async fn execute_manifest_job(&self, job: ManifestJob) -> Result<ManifestJobDone, Self::Error> {
        match job {
            ManifestJob::Full { name, spec } => {
                let data = match self.fetch_full_manifest_job(&name).await? {
                    ProviderFullManifestBytes::Fresh { bytes, etag } => {
                        let (manifest, speculative) =
                            manifest::parse_full_manifest_with_core_off_runtime(bytes, spec)
                                .await?;
                        let manifest = Arc::new(manifest);
                        let speculative = speculative.map(|(spec, core)| {
                            let core = Arc::new(core);
                            self.store_version_manifest(&name, Arc::clone(&core));
                            (spec, core)
                        });
                        let versions = Arc::new(VersionsInfo {
                            versions: Versions {
                                version_list: manifest.versions.clone(),
                                dist_tags: manifest.dist_tags.clone(),
                            },
                            etag,
                            last_updated: super::current_timestamp_secs(),
                        });
                        self.store.store_versions(&name, versions);
                        ManifestFullData::Full {
                            manifest,
                            speculative,
                        }
                    }
                    ProviderFullManifestBytes::NotModified { versions } => {
                        ManifestFullData::Versions(versions)
                    }
                };

                Ok(ManifestJobDone::Full { name, data })
            }
            ManifestJob::Version {
                name,
                spec,
                fetch_spec,
            } => {
                if Version::parse_from_npm(&fetch_spec).is_ok()
                    && let Some(manifest) =
                        self.store.load_version_manifest(&name, &fetch_spec).await
                    // Guard against a stale/corrupt store entry filed under the
                    // wrong key: only trust the cached manifest when its own
                    // version matches the exact spec we asked for.
                    && manifest.version == fetch_spec
                {
                    let manifest = Arc::new(manifest);
                    return Ok(ManifestJobDone::Version {
                        name,
                        spec,
                        manifest,
                    });
                }

                let bytes =
                    manifest::fetch_version_manifest_vec(manifest::FetchVersionManifestOptions {
                        registry_url: &self.registry_url,
                        name: &name,
                        spec: &fetch_spec,
                        format: self.version_fetch_format(),
                        auth_token: self.auth_token.as_deref(),
                    })
                    .await
                    .map_err(RegistryError)?;
                let manifest = Arc::new(
                    manifest::parse_json_vec_off_runtime::<CoreVersionManifest>(bytes).await?,
                );
                self.store_version_manifest(&name, Arc::clone(&manifest));
                Ok(ManifestJobDone::Version {
                    name,
                    spec,
                    manifest,
                })
            }
            ManifestJob::ExtractVersion {
                name,
                spec,
                version,
                full,
            } => {
                let (resolved_version, manifest) =
                    extract_core_version_off_runtime(full, version).await;
                let manifest = manifest.ok_or_else(|| {
                    RegistryError(anyhow!(
                        "Version {} not found in manifest for {}",
                        resolved_version,
                        name
                    ))
                })?;
                self.store_version_manifest(&name, Arc::clone(&manifest));
                Ok(ManifestJobDone::Version {
                    name,
                    spec,
                    manifest,
                })
            }
        }
    }
}
