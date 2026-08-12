use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, bail};
use bytes::Bytes;
use deno_semver::Version;
use serde::{Deserialize, Serialize};

use super::lock::resolve_package_spec_details;
use crate::constants::APP_VERSION;
#[cfg(test)]
use crate::util::cache::get_cache_dir;
use crate::util::cache::get_self_pin_cache_dir;
use crate::util::downloader::download_bytes;
use crate::util::extractor::extract_and_write;
use crate::util::integrity::{compute_integrity, verify_integrity, verify_shasum};
use crate::util::process_lock::{lock_exclusive, sibling_lock_path};
use crate::util::user_config::{init_registry, set_cache_dir};

pub const HANDOFF_ENV: &str = "UTOO_SELF_PIN_VERSION";
const DISABLE_ENV: &str = "UTOO_SELF_PIN";
// Earlier releases do not understand HANDOFF_ENV and may auto-update the
// global installation while serving a pin. Limit pins to self-pin-aware builds.
const MINIMUM_PINNED_VERSION: &str = "1.1.8";
static SELF_PIN_ACTIVE: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectManifest {
    package_manager: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct ProjectPin {
    manifest_path: PathBuf,
    version: String,
}

#[derive(Debug, Deserialize)]
struct ReleaseManifest {
    name: String,
    version: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct CacheMetadata {
    executable_integrity: String,
}

#[derive(Debug, PartialEq, Eq)]
struct PlatformTarget {
    package_name: &'static str,
    executable: &'static str,
    cache_key: &'static str,
}

#[derive(Debug)]
enum CachedRelease {
    Missing,
    Valid(PathBuf),
    Invalid(anyhow::Error),
}

/// Switch dependency-changing project commands to the exact Utoo version in
/// the nearest ancestor `package.json` containing `packageManager: "utoo@…"`.
///
/// A successful handoff replaces the current process on Unix and exits with
/// the child status on Windows, so this only returns when no switch is needed
/// or provisioning fails.
pub async fn handoff_if_needed(
    cwd: &Path,
    args: &[String],
    registry: Option<String>,
    cache_dir: Option<String>,
) -> Result<()> {
    if self_pin_disabled() {
        return Ok(());
    }
    let Some(pin) = find_project_pin(cwd).await? else {
        return Ok(());
    };
    validate_exact_version(&pin)?;
    if pin.version == APP_VERSION || handoff_version().as_deref() == Some(pin.version.as_str()) {
        SELF_PIN_ACTIVE.store(true, Ordering::Relaxed);
        return Ok(());
    }

    // Resolve the configured cache before network initialization so a warm pin
    // remains fully offline. A pinned child owns normal startup (including
    // `--from` migration), and unpinned invocations keep the existing order.
    set_cache_dir(cache_dir).await;

    let target = platform_target(std::env::consts::OS, std::env::consts::ARCH)?;
    let cache_path = release_cache_path_for(&target, &pin.version)?;
    let lock_path = sibling_lock_path(&cache_path, ".self-pin.lock")?;
    let _lock = lock_exclusive(&lock_path).await?;
    let executable = match cached_release_at(&cache_path, &target, &pin.version).await? {
        CachedRelease::Valid(executable) => executable,
        CachedRelease::Missing => {
            init_registry(registry).await?;
            provision_release_at(&cache_path, &target, &pin.version).await?
        }
        CachedRelease::Invalid(validation_error) => {
            let recovery_context = format!(
                "Failed to recover invalid pinned release cache at {} after validation failed: {validation_error:#}",
                cache_path.display()
            );
            init_registry(registry)
                .await
                .context(recovery_context.clone())?;
            provision_release_at(&cache_path, &target, &pin.version)
                .await
                .context(recovery_context)?
        }
    };
    if !crate::util::invocation::quiet() {
        eprintln!(
            "utoo: using pinned utoo@{} from {}",
            pin.version,
            executable.display()
        );
    }
    handoff(&executable, args, &pin.version)
}

pub fn is_active() -> bool {
    SELF_PIN_ACTIVE.load(Ordering::Relaxed) || std::env::var_os(HANDOFF_ENV).is_some()
}

async fn find_project_pin(start: &Path) -> Result<Option<ProjectPin>> {
    let mut current = start.to_path_buf();
    loop {
        let manifest_path = current.join("package.json");
        match crate::fs::read_to_string(&manifest_path).await {
            Ok(content) => {
                let manifest: ProjectManifest = serde_json::from_str(&content)
                    .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;
                if let Some(package_manager) = manifest.package_manager {
                    return Ok(package_manager
                        .strip_prefix("utoo@")
                        .map(|version| ProjectPin {
                            manifest_path,
                            version: version.to_string(),
                        }));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("Failed to read {}", manifest_path.display()));
            }
        }
        let Some(parent) = current.parent() else {
            return Ok(None);
        };
        current = parent.to_path_buf();
    }
}

fn self_pin_disabled() -> bool {
    std::env::var(DISABLE_ENV).is_ok_and(|value| value == "0")
}

fn handoff_version() -> Option<String> {
    std::env::var(HANDOFF_ENV).ok()
}

fn validate_exact_version(pin: &ProjectPin) -> Result<()> {
    let version = Version::parse_from_npm(&pin.version)
        .ok()
        .filter(|version| version.to_string() == pin.version);
    let Some(version) = version else {
        bail!(
            "{}: packageManager must pin an exact utoo version (for example, utoo@1.1.8)",
            pin.manifest_path.display(),
        );
    };
    let minimum = Version::parse_from_npm(MINIMUM_PINNED_VERSION)
        .expect("minimum pinned version must be valid");
    if version < minimum {
        bail!(
            "{}: packageManager self-pinning requires utoo@{} or newer",
            pin.manifest_path.display(),
            MINIMUM_PINNED_VERSION,
        );
    }
    Ok(())
}

fn platform_target(os: &str, arch: &str) -> Result<PlatformTarget> {
    let target = match (os, arch) {
        ("macos", "x86_64") => PlatformTarget {
            package_name: "@utoo/utoo-darwin-x64",
            executable: "bin/utoo",
            cache_key: "darwin-x64",
        },
        ("macos", "aarch64") => PlatformTarget {
            package_name: "@utoo/utoo-darwin-arm64",
            executable: "bin/utoo",
            cache_key: "darwin-arm64",
        },
        ("linux", "x86_64") => PlatformTarget {
            package_name: "@utoo/utoo-linux-x64",
            executable: "bin/utoo",
            cache_key: "linux-x64",
        },
        ("linux", "aarch64") => PlatformTarget {
            package_name: "@utoo/utoo-linux-arm64",
            executable: "bin/utoo",
            cache_key: "linux-arm64",
        },
        // Windows 11 on ARM64 can run the published x64 binary under emulation.
        ("windows", "x86_64" | "aarch64") => PlatformTarget {
            package_name: "@utoo/utoo-win32-x64",
            executable: "bin/utoo.exe",
            cache_key: "win32-x64",
        },
        _ => bail!("Utoo self-pinning does not support {os}-{arch}"),
    };
    Ok(target)
}

fn release_cache_path_for(target: &PlatformTarget, version: &str) -> Result<PathBuf> {
    Ok(get_self_pin_cache_dir()?
        .join(target.cache_key)
        .join(version))
}

async fn cached_release_at(
    cache_path: &Path,
    target: &PlatformTarget,
    version: &str,
) -> Result<CachedRelease> {
    if crate::fs::try_exists(cache_path.join("_resolved")).await?
        && crate::fs::try_exists(cache_path.join("_utoo-self-pin.json")).await?
    {
        return Ok(
            match validate_cached_release(cache_path, target, version).await {
                Ok(executable) => CachedRelease::Valid(executable),
                Err(error) => CachedRelease::Invalid(error),
            },
        );
    }
    Ok(CachedRelease::Missing)
}

async fn provision_release_at(
    cache_path: &Path,
    target: &PlatformTarget,
    version: &str,
) -> Result<PathBuf> {
    if !crate::util::invocation::quiet() {
        eprintln!("utoo: provisioning pinned utoo@{version}...");
    }
    let spec = format!("{}@{version}", target.package_name);
    let resolved = resolve_package_spec_details(&spec)
        .await
        .with_context(|| format!("Failed to resolve pinned release {spec}"))?;
    if resolved.name != target.package_name || resolved.version != version {
        bail!(
            "Registry resolved {spec} as {}@{}",
            resolved.name,
            resolved.version
        );
    }

    let token = crate::service::auth::token_for_url(&resolved.tarball_url).await;
    let archive = download_bytes(&resolved.tarball_url, token.as_deref())
        .await
        .with_context(|| format!("Failed to download pinned release {spec}"))?;
    verify_release_archive(
        &archive,
        resolved.integrity.as_deref(),
        resolved.shasum.as_deref(),
    )
    .with_context(|| format!("Failed to verify pinned release {spec}"))?;

    // `_resolved` is the package-cache commit marker. A visible slot on this
    // path is invalid: archive download and registry checksum verification have
    // already succeeded, so it is now safe to repopulate the slot via an atomic
    // cache commit while holding the platform-specific self-pin lock.
    if crate::fs::try_exists(cache_path.join("_resolved")).await? {
        crate::fs::remove_dir_all(cache_path).await?;
    }
    extract_and_write(archive, cache_path)
        .await
        .with_context(|| format!("Failed to cache pinned release {spec}"))?;
    write_cache_metadata(cache_path, target, version).await?;
    validate_cached_release(cache_path, target, version).await
}

fn verify_release_archive(
    archive: &Bytes,
    integrity: Option<&str>,
    shasum: Option<&str>,
) -> Result<()> {
    if let Some(integrity) = integrity {
        return verify_integrity(archive, integrity);
    }
    if let Some(shasum) = shasum {
        return verify_shasum(archive, shasum);
    }
    bail!("Registry response has neither dist.integrity nor dist.shasum")
}

async fn validate_cached_release(
    cache_path: &Path,
    target: &PlatformTarget,
    version: &str,
) -> Result<PathBuf> {
    let package_root = cache_path.join("package");
    let manifest_path = package_root.join("package.json");
    let manifest: ReleaseManifest = serde_json::from_str(
        &crate::fs::read_to_string(&manifest_path)
            .await
            .with_context(|| format!("Pinned release is missing {}", manifest_path.display()))?,
    )
    .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;
    if manifest.name != target.package_name || manifest.version != version {
        bail!(
            "Pinned release cache contains {}@{}, expected {}@{}",
            manifest.name,
            manifest.version,
            target.package_name,
            version
        );
    }

    let executable = release_executable(cache_path, target).await?;
    let metadata_path = cache_path.join("_utoo-self-pin.json");
    let metadata: CacheMetadata = serde_json::from_str(
        &crate::fs::read_to_string(&metadata_path)
            .await
            .with_context(|| format!("Pinned release is missing {}", metadata_path.display()))?,
    )
    .with_context(|| format!("Failed to parse {}", metadata_path.display()))?;
    let executable_bytes = crate::fs::read(&executable).await?;
    verify_integrity(&executable_bytes, &metadata.executable_integrity)
        .with_context(|| format!("Pinned executable is corrupt: {}", executable.display()))?;
    validate_executable_version(&executable, version).await?;
    Ok(executable)
}

async fn write_cache_metadata(
    cache_path: &Path,
    target: &PlatformTarget,
    version: &str,
) -> Result<()> {
    let executable = release_executable(cache_path, target).await?;
    validate_executable_version(&executable, version).await?;
    let metadata = CacheMetadata {
        executable_integrity: compute_integrity(&crate::fs::read(&executable).await?),
    };
    let metadata_path = cache_path.join("_utoo-self-pin.json");
    let staging_path = cache_path.join(format!("._utoo-self-pin.{}.tmp", std::process::id()));
    crate::fs::write(&staging_path, serde_json::to_vec(&metadata)?).await?;
    crate::fs::rename(&staging_path, &metadata_path)
        .await
        .with_context(|| format!("Failed to finalize pinned release cache for utoo@{version}"))
}

async fn release_executable(cache_path: &Path, target: &PlatformTarget) -> Result<PathBuf> {
    let executable = cache_path.join("package").join(target.executable);
    if !crate::fs::try_exists(&executable).await? {
        bail!(
            "Pinned release is missing executable {}",
            executable.display()
        );
    }
    Ok(executable)
}

async fn validate_executable_version(executable: &Path, version: &str) -> Result<()> {
    let output = tokio::process::Command::new(executable)
        .arg("--version")
        .env(HANDOFF_ENV, version)
        .env_remove("UTOO_MANAGED_PACKAGE_ROOT")
        .output()
        .await
        .with_context(|| format!("Failed to inspect pinned Utoo at {}", executable.display()))?;
    if !output.status.success() {
        bail!(
            "Pinned Utoo at {} failed its version check with {}",
            executable.display(),
            output.status
        );
    }
    let actual = String::from_utf8(output.stdout).context("Pinned Utoo version is not UTF-8")?;
    if actual.trim() != version {
        bail!(
            "Pinned executable reports utoo@{}, expected utoo@{version}",
            actual.trim()
        );
    }
    Ok(())
}

#[cfg(unix)]
fn handoff(executable: &Path, args: &[String], version: &str) -> Result<()> {
    use std::os::unix::process::CommandExt;

    let error = Command::new(executable)
        .args(args)
        .env(HANDOFF_ENV, version)
        .env_remove("UTOO_MANAGED_PACKAGE_ROOT")
        .exec();
    Err(error).with_context(|| format!("Failed to start pinned Utoo at {}", executable.display()))
}

#[cfg(windows)]
fn handoff(executable: &Path, args: &[String], version: &str) -> Result<()> {
    let status = Command::new(executable)
        .args(args)
        .env(HANDOFF_ENV, version)
        .env_remove("UTOO_MANAGED_PACKAGE_ROOT")
        .status()
        .with_context(|| format!("Failed to start pinned Utoo at {}", executable.display()))?;
    std::process::exit(status.code().unwrap_or(1));
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[tokio::test]
    async fn finds_exact_utoo_pin_from_project_ancestor() {
        let temp = TempDir::new().unwrap();
        let nested = temp.path().join("packages/app");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(
            temp.path().join("package.json"),
            r#"{"private":true,"packageManager":"utoo@1.0.6"}"#,
        )
        .unwrap();

        assert_eq!(
            find_project_pin(&nested).await.unwrap(),
            Some(ProjectPin {
                manifest_path: temp.path().join("package.json"),
                version: "1.0.6".to_string(),
            }),
        );
    }

    #[tokio::test]
    async fn nearest_explicit_package_manager_stops_ancestor_lookup() {
        let temp = TempDir::new().unwrap();
        let nested = temp.path().join("packages/app");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(
            temp.path().join("package.json"),
            r#"{"packageManager":"utoo@1.1.7"}"#,
        )
        .unwrap();
        std::fs::write(
            nested.join("package.json"),
            r#"{"packageManager":"pnpm@10.0.0"}"#,
        )
        .unwrap();

        assert_eq!(find_project_pin(&nested).await.unwrap(), None);
    }

    #[test]
    fn accepts_only_exact_semver_pins() {
        let manifest_path = PathBuf::from("package.json");
        for version in ["1.1.8", "2.0.0-beta.1", "3.0.0+build.5"] {
            assert!(
                validate_exact_version(&ProjectPin {
                    manifest_path: manifest_path.clone(),
                    version: version.to_string(),
                })
                .is_ok(),
                "expected {version} to be accepted"
            );
        }

        for version in [
            "latest", "^1.1.8", "~1.1.8", "1.x", "1.1", "v1.1.8", "1.1.7",
        ] {
            assert!(
                validate_exact_version(&ProjectPin {
                    manifest_path: manifest_path.clone(),
                    version: version.to_string(),
                })
                .is_err(),
                "expected {version} to be rejected"
            );
        }
    }

    #[test]
    fn maps_release_packages_for_supported_targets() {
        assert_eq!(
            platform_target("macos", "aarch64").unwrap(),
            PlatformTarget {
                package_name: "@utoo/utoo-darwin-arm64",
                executable: "bin/utoo",
                cache_key: "darwin-arm64",
            }
        );
        assert_eq!(
            platform_target("windows", "aarch64").unwrap(),
            PlatformTarget {
                package_name: "@utoo/utoo-win32-x64",
                executable: "bin/utoo.exe",
                cache_key: "win32-x64",
            }
        );
        assert!(platform_target("freebsd", "x86_64").is_err());
    }

    #[test]
    fn isolates_self_pin_caches_by_platform_package() {
        let version = "1.1.8";
        let mac_arm64 = platform_target("macos", "aarch64").unwrap();
        let mac_x64 = platform_target("macos", "x86_64").unwrap();
        let windows_arm64 = platform_target("windows", "aarch64").unwrap();
        let windows_x64 = platform_target("windows", "x86_64").unwrap();

        assert_ne!(
            release_cache_path_for(&mac_arm64, version).unwrap(),
            release_cache_path_for(&mac_x64, version).unwrap(),
        );
        let mac_arm64_path = release_cache_path_for(&mac_arm64, version).unwrap();
        let mac_x64_path = release_cache_path_for(&mac_x64, version).unwrap();
        assert!(mac_arm64_path.ends_with("darwin-arm64/1.1.8"));
        assert_eq!(
            mac_arm64_path.parent().unwrap().parent().unwrap(),
            crate::util::cache::get_self_pin_cache_dir().unwrap(),
        );
        assert_ne!(
            crate::util::cache::get_self_pin_cache_dir().unwrap(),
            get_cache_dir(),
        );
        assert_ne!(
            crate::util::package_cache::registry_cache_path("_utoo-self-darwin-arm64", version,),
            mac_arm64_path,
        );
        assert_ne!(
            sibling_lock_path(&mac_arm64_path, ".self-pin.lock").unwrap(),
            sibling_lock_path(&mac_x64_path, ".self-pin.lock").unwrap(),
        );
        let legacy_path = get_cache_dir().join("self").join(version);
        assert_eq!(mac_arm64_path.file_name(), legacy_path.file_name());
        assert_ne!(mac_arm64_path, legacy_path);
        let cached_version = mac_arm64_path.file_name().unwrap().to_string_lossy();
        let cache_spec = format!("_utoo-self-{}@{cached_version}", mac_arm64.cache_key);
        assert_eq!(
            utoo_ruborist::util::parse_package_spec(&cache_spec),
            ("_utoo-self-darwin-arm64", "1.1.8"),
        );
        assert_eq!(
            release_cache_path_for(&windows_arm64, version).unwrap(),
            release_cache_path_for(&windows_x64, version).unwrap(),
        );
    }

    #[tokio::test]
    async fn invalid_self_pin_metadata_requires_reprovision_without_early_deletion() {
        let temp = TempDir::new().unwrap();
        let cache_path = temp.path().join("self-pin/darwin-arm64/1.1.8");
        let package_root = cache_path.join("package");
        let target = platform_target("macos", "aarch64").unwrap();
        std::fs::create_dir_all(package_root.join("bin")).unwrap();
        std::fs::write(cache_path.join("_resolved"), b"").unwrap();
        std::fs::write(
            package_root.join("package.json"),
            r#"{"name":"@utoo/utoo-darwin-arm64","version":"1.1.8"}"#,
        )
        .unwrap();
        std::fs::write(package_root.join("bin/utoo"), b"not executed").unwrap();
        std::fs::write(cache_path.join("_utoo-self-pin.json"), b"not json").unwrap();

        assert!(matches!(
            cached_release_at(&cache_path, &target, "1.1.8")
                .await
                .unwrap(),
            CachedRelease::Invalid(_)
        ));
        assert!(cache_path.exists());
    }

    #[tokio::test]
    async fn corrupt_self_pin_executable_requires_reprovision_without_execution() {
        let temp = TempDir::new().unwrap();
        let cache_path = temp.path().join("self-pin/darwin-arm64/1.1.8");
        let package_root = cache_path.join("package");
        let target = platform_target("macos", "aarch64").unwrap();
        std::fs::create_dir_all(package_root.join("bin")).unwrap();
        std::fs::write(cache_path.join("_resolved"), b"").unwrap();
        std::fs::write(
            package_root.join("package.json"),
            r#"{"name":"@utoo/utoo-darwin-arm64","version":"1.1.8"}"#,
        )
        .unwrap();
        std::fs::write(package_root.join("bin/utoo"), b"corrupt executable").unwrap();
        std::fs::write(
            cache_path.join("_utoo-self-pin.json"),
            serde_json::to_vec(&CacheMetadata {
                executable_integrity: compute_integrity(b"expected executable"),
            })
            .unwrap(),
        )
        .unwrap();

        assert!(matches!(
            cached_release_at(&cache_path, &target, "1.1.8")
                .await
                .unwrap(),
            CachedRelease::Invalid(_)
        ));
        assert!(cache_path.exists());
    }

    #[test]
    fn requires_registry_checksum_metadata() {
        let archive = Bytes::from_static(b"release archive");
        let integrity = crate::util::integrity::compute_integrity(&archive);
        let shasum = crate::util::integrity::compute_shasum(&archive);

        assert!(verify_release_archive(&archive, Some(&integrity), None).is_ok());
        assert!(verify_release_archive(&archive, None, Some(&shasum)).is_ok());
        assert!(verify_release_archive(&archive, None, None).is_err());
        assert!(verify_release_archive(&archive, Some("sha512-invalid"), None).is_err());
    }
}
