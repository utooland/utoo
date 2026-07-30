use std::collections::HashSet;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, OnceLock};

use anyhow::{Context, Result};
use dashmap::DashMap;
use reqwest::Url;

use utoo_ruborist::builder::PeerDeps;
use utoo_ruborist::manifest::PackageJson;
use utoo_ruborist::registry::is_npm_registry;
use utoo_ruborist::spec::Catalogs;

use super::cli_enum::{ConfigScope, InstallScope, OmitType};
use super::config_file::{Config, ConfigValue};
use super::http::client_builder;
use super::json::load_package_json;
#[cfg(windows)]
use super::platform_const::drive_root;
use super::registry::{REGISTRY_NPMMIRROR, select_fastest_registry};

static REGISTRY: LazyLock<ConfigValue<String>> =
    LazyLock::new(|| ConfigValue::new("registry", REGISTRY_NPMMIRROR.to_string()));

static LEGACY_PEER_DEPS: LazyLock<ConfigValue<bool>> =
    LazyLock::new(|| ConfigValue::new("legacy-peer-deps", true));

static CACHE_DIR: LazyLock<ConfigValue<String>> = LazyLock::new(|| {
    ConfigValue::new(
        "cache-dir",
        default_cache_dir().to_string_lossy().to_string(),
    )
});

/// The built-in default cache location (`~/.cache/nm`), used when no
/// `--cache-dir`/`UTOO_CACHE_DIR`/config value is set.
fn default_cache_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".cache/nm")
}

/// Registry candidates from the config *profiles*, most specific first. These
/// are consulted only when neither `--registry` nor `UTOO_REGISTRY` is set;
/// both take precedence and short-circuit before any profile is read. `.npmrc`
/// slots in per level: a project `.npmrc` beats the user-level utoo config (a
/// project must be able to redirect any npm client, which is exactly what
/// registry bridges and corporate proxies rely on), but never the same-level
/// `.utoo.toml`, where utoo-native config stays authoritative.
#[derive(Default)]
struct ProfileRegistries {
    project_config: Option<String>,
    project_npmrc: Option<String>,
    global_config: Option<String>,
    user_npmrc: Option<String>,
}

fn pick_profile_registry(sources: ProfileRegistries) -> Option<(String, &'static str)> {
    [
        (sources.project_config, ".utoo.toml"),
        (sources.project_npmrc, "project .npmrc"),
        (sources.global_config, "global config"),
        (sources.user_npmrc, "user .npmrc"),
    ]
    .into_iter()
    .find_map(|(value, source)| value.filter(|v| !v.is_empty()).map(|v| (v, source)))
}

pub async fn init_registry(registry: Option<String>) -> Result<()> {
    // init_registry is the process config chokepoint (see main.rs):
    // `load_levels` seeds the merged-config cache that later ConfigValue reads
    // rely on (from the invocation cwd, before any workspace chdir), and
    // returns the per-level views used for registry resolution below. Fail loud
    // on a malformed/unreadable utoo config rather than silently resolving
    // against the wrong registry; the error carries the offending file + reason.
    let (global_config, local_config) = Config::load_levels()
        .await
        .context("failed to load utoo config")?;

    // An explicit `--registry` or `UTOO_REGISTRY` wins outright, so short-circuit
    // before touching any profile: only when neither is set do we read the
    // `.utoo.toml`/global `registry` keys and load `.npmrc` (see
    // `ProfileRegistries` for the inter-profile precedence).
    let (raw_registry, registry_source) = if let Some(reg) = registry.filter(|r| !r.is_empty()) {
        (reg, "CLI")
    } else if let Some(env) = std::env::var("UTOO_REGISTRY")
        .ok()
        .filter(|v| !v.is_empty())
    {
        (env, "UTOO_REGISTRY")
    } else {
        let npmrc = super::npmrc::load().await;
        let registry_of = |c: &Config| c.get("registry").ok().flatten();
        let sources = ProfileRegistries {
            project_config: local_config.as_ref().and_then(registry_of),
            project_npmrc: npmrc.project_registry(),
            global_config: registry_of(&global_config),
            user_npmrc: npmrc.user_registry(),
        };
        match pick_profile_registry(sources) {
            Some(picked) => picked,
            // Nothing configured anywhere, auto-select fastest registry
            None => (select_fastest_registry().await, "auto-select"),
        }
    };
    tracing::debug!("Using registry from {}: {}", registry_source, raw_registry);

    // Canonicalize at the single resolution chokepoint so every downstream
    // consumer sees a validated, slash-normalized URL. Explicit invalid values
    // fail fast instead of silently falling back to a public registry.
    let final_registry = normalize_registry(&raw_registry)
        .with_context(|| format!("invalid registry from {registry_source}"))?;

    REGISTRY.set(Some(final_registry));

    // Auto-detect semver support for the selected registry
    let registry_url = get_registry();
    let supports = detect_supports_semver(&registry_url, None).await;
    let _ = SUPPORTS_SEMVER.set(supports);

    Ok(())
}

pub fn get_registry() -> String {
    REGISTRY.get_sync()
}

/// Parse, validate, and canonicalize a registry URL.
///
/// Registry values arrive unnormalized from the CLI, `UTOO_REGISTRY`, or the
/// config file — and a trailing slash is common (npm's own default is
/// `https://registry.npmjs.org/`). Downstream code joins package paths as
/// `{registry}/{pkg}`, so a trailing slash produces `//<pkg>`, which npm's
/// registry rejects with HTTP 406. Strip it here, at the single resolution
/// chokepoint, and reject non-`http(s)` URLs early with a clear message instead
/// of a late, opaque fetch failure.
///
/// Sub-paths are preserved, so a private registry served under a prefix
/// (Artifactory/Nexus/Verdaccio `…/api/npm/<repo>`) keeps resolving correctly.
fn normalize_registry(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    let url = Url::parse(trimmed)
        .map_err(|e| anyhow::anyhow!("invalid registry URL {trimmed:?}: {e}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        anyhow::bail!("registry must be an http(s) URL, got {trimmed:?}");
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}

/// Scheme + host(+port) origin of an http(s) URL — e.g. `https://registry.npmjs.org`
/// from `https://registry.npmjs.org/foo/-/foo-1.0.0.tgz`. `None` for non-http URLs.
fn url_origin(url: &str) -> Option<&str> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let host_end = rest.find('/').unwrap_or(rest.len());
    Some(&url[..url.len() - rest.len() + host_end])
}

/// Whether `resolved` is a tarball served by a trusted registry (the configured
/// registry, or a well-known public one), so it is safe to materialize into the
/// shared `<name>/<version>` global cache slot.
///
/// A registry download URL and a user-declared remote-tarball URL are
/// indistinguishable in an npm `package-lock.json` (both are `https://….tgz`,
/// the lockfile stores no source tag). We resolve that ambiguity by origin:
/// only a tarball whose host is a trusted registry may enter the shared cache;
/// any other https tarball is an untrusted remote tarball and is fetched +
/// extracted directly into `node_modules`, never the global store — which also
/// removes the cross-cache poisoning risk of an attacker URL self-declaring a
/// `name@version` that collides with a real registry package.
pub fn is_registry_tarball(resolved: &str) -> bool {
    let Some(origin) = url_origin(resolved) else {
        return false;
    };
    let configured = get_registry();
    let trusted = [
        url_origin(&configured),
        url_origin(super::registry::REGISTRY_NPMMIRROR),
        url_origin(super::registry::REGISTRY_NPMJS),
    ];
    trusted
        .into_iter()
        .flatten()
        .any(|t| origin.eq_ignore_ascii_case(t))
}

static CATALOGS: tokio::sync::OnceCell<Catalogs> = tokio::sync::OnceCell::const_new();

pub async fn get_catalogs() -> Catalogs {
    CATALOGS
        .get_or_init(|| async {
            Config::load(ConfigScope::Local)
                .await
                .map(|c| c.catalogs())
                .unwrap_or_default()
        })
        .await
        .clone()
}

pub fn set_legacy_peer_deps(value: Option<bool>) {
    LEGACY_PEER_DEPS.set(value);
}

pub async fn get_peer_deps() -> PeerDeps {
    match LEGACY_PEER_DEPS.get().await {
        true => PeerDeps::Skip,
        false => PeerDeps::Include,
    }
}

static OMIT: OnceLock<HashSet<OmitType>> = OnceLock::new();

pub fn set_omit(value: HashSet<OmitType>) {
    if !value.is_empty() {
        tracing::debug!("set omit: {:?}", value);
        let _ = OMIT.set(value);
    }
}

pub fn get_omit() -> HashSet<OmitType> {
    OMIT.get().cloned().unwrap_or_default()
}

static INSTALL_SCOPE: OnceLock<InstallScope> = OnceLock::new();

pub fn set_install_scope(scope: InstallScope) {
    let _ = INSTALL_SCOPE.set(scope);
}

pub fn get_install_scope() -> InstallScope {
    INSTALL_SCOPE.get().copied().unwrap_or_default()
}

// Manifest fetch concurrency limit, shared by resolver manifest fetches and
// tarball download/extract. Defaults to 64; overridable via the
// `--manifests-concurrency-limit` CLI flag / config.
const DEFAULT_MANIFESTS_CONCURRENCY_LIMIT: usize = 64;

static MANIFESTS_CONCURRENCY_LIMIT: LazyLock<ConfigValue<usize>> = LazyLock::new(|| {
    ConfigValue::new(
        "manifests-concurrency-limit",
        DEFAULT_MANIFESTS_CONCURRENCY_LIMIT,
    )
});

pub fn set_manifests_concurrency_limit(value: Option<usize>) {
    MANIFESTS_CONCURRENCY_LIMIT.set(value);
}

pub async fn get_manifests_concurrency_limit() -> usize {
    MANIFESTS_CONCURRENCY_LIMIT.get().await
}

pub fn get_manifests_concurrency_limit_sync() -> usize {
    MANIFESTS_CONCURRENCY_LIMIT.get_sync()
}

// Max lifecycle scripts run concurrently within one queue. `0` (the default)
// means "auto" — derived from the CPU count by `script_concurrency_limit`.
// Overridable via the `--script-concurrency-limit` CLI flag / config so a big
// monorepo can dial it up, or a constrained machine down.
static SCRIPT_CONCURRENCY_LIMIT: LazyLock<ConfigValue<usize>> =
    LazyLock::new(|| ConfigValue::new("script-concurrency-limit", 0));

pub fn set_script_concurrency_limit(value: Option<usize>) {
    SCRIPT_CONCURRENCY_LIMIT.set(value);
}

/// Configured script concurrency, or `0` when left on auto. Reads the merged
/// config (so a `.utoo.toml` value is honoured), hence async.
pub async fn get_script_concurrency_limit() -> usize {
    SCRIPT_CONCURRENCY_LIMIT.get().await
}

pub async fn set_cache_dir(cache_dir: Option<String>) {
    // Priority: CLI argument > UTOO_CACHE_DIR env > config > default.
    // `Some` means the user picked the cache dir explicitly; `None` means we
    // fall back to the built-in default. The distinction drives how a
    // cross-device cache is handled (see `resolve_cache_dir`).
    let explicit = if let Some(dir) = cache_dir {
        Some(dir)
    } else if let Ok(env_dir) = std::env::var("UTOO_CACHE_DIR")
        && !env_dir.is_empty()
    {
        Some(env_dir)
    } else {
        // Read from config file if no CLI arg or env var
        Config::load(ConfigScope::Local)
            .await
            .ok()
            .and_then(|config| config.get("cache-dir").ok().flatten())
    };

    let resolved = match std::env::current_dir() {
        Ok(project) => resolve_cache_dir(explicit, &project),
        Err(e) => {
            // Without the project dir we can't compare devices; skip relocation.
            // Log it so a silent cross-device copy-mode install is diagnosable.
            tracing::debug!("cannot resolve current dir for cache relocation: {e}");
            explicit
        }
    };
    CACHE_DIR.set(resolved);
}

/// Resolve the cache dir, accounting for the case where it lands on a different
/// filesystem/drive than the project (where `node_modules` is created).
/// Hardlink/clonefile cannot cross devices, so a cross-device cache forces the
/// cloner to fall back to copying every file.
///
/// This only covers the common "one cache, one project volume" case — it picks
/// a good cache location up front. It is *not* a substitute for the cloner's
/// per-package cross-device detection: in a multi-volume layout (e.g. monorepo
/// members on different mounts) some targets can still be cross-device with the
/// resolved cache, and the cloner must keep falling back to copy per package.
///
/// - default cache on a different device → relocate to a project-local cache so
///   linking keeps working (logged at INFO).
/// - explicitly configured cache on a different device → keep the user's choice
///   but WARN that installs will fall back to copying.
///
/// `explicit` is `Some` when the user set the cache dir, `None` for the default.
/// Returning `None` keeps the lazy built-in default.
fn resolve_cache_dir(explicit: Option<String>, project: &Path) -> Option<String> {
    // Borrow to derive the effective path; `explicit` is still returned by value below.
    let effective = match explicit.as_deref() {
        Some(dir) => PathBuf::from(dir),
        None => default_cache_dir(),
    };

    if !is_cross_device(&effective, project) {
        return explicit;
    }

    if explicit.is_some() {
        tracing::warn!(
            "configured cache-dir {} is on a different filesystem than the project at {}; \
             installs will fall back to copying files instead of hardlink/clone. Point \
             cache-dir/UTOO_CACHE_DIR at a path on the same filesystem to avoid the copy.",
            effective.display(),
            project.display(),
        );
        return explicit;
    }

    let relocated = relocated_cache_dir(project);
    tracing::info!(
        "default cache dir {} is on a different filesystem than the project at {}; \
         relocating cache to {} so packages can be hardlinked/cloned instead of copied. \
         Set cache-dir/UTOO_CACHE_DIR to override.",
        effective.display(),
        project.display(),
        relocated.display(),
    );
    Some(relocated.to_string_lossy().to_string())
}

/// Whether `cache` and `project` live on different filesystems, so links
/// between them are impossible. Defaults to `false` (assume same device) when
/// the device can't be determined, to avoid relocating on transient errors.
#[cfg(unix)]
fn is_cross_device(cache: &Path, project: &Path) -> bool {
    match (nearest_device(cache), nearest_device(project)) {
        (Some(a), Some(b)) => a != b,
        _ => false,
    }
}

/// `st_dev` of `path`, walking up to the nearest existing ancestor since the
/// cache dir itself may not exist yet.
#[cfg(unix)]
fn nearest_device(path: &Path) -> Option<u64> {
    let mut current = Some(path);
    while let Some(p) = current {
        if let Ok(meta) = std::fs::metadata(p) {
            return Some(meta.dev());
        }
        current = p.parent();
    }
    None
}

#[cfg(windows)]
fn is_cross_device(cache: &Path, project: &Path) -> bool {
    match (drive_root(cache), drive_root(project)) {
        (Some(a), Some(b)) => a != b,
        _ => false,
    }
}

/// A project-local cache on the same device/drive as `node_modules`. Lives under
/// `node_modules` (rather than the project root) so it is gitignored by
/// convention, doesn't pollute the working tree, and stays on the project's
/// filesystem. Same target on every platform — on Windows `node_modules` is on
/// the project's drive, so hardlinks still work.
fn relocated_cache_dir(project: &Path) -> PathBuf {
    project.join("node_modules/.cache/nm")
}

/// Resolve the global install/link prefix (npm's `prefix`).
///
/// Priority: `--prefix` CLI flag > `UTOO_PREFIX` env > persisted `prefix` config
/// key. Returns `None` when none is set, leaving `global_bin` to infer the
/// prefix from the running executable. Set persistently with
/// `ut config set prefix <path> --global`.
pub async fn resolve_global_prefix(cli_prefix: Option<&str>) -> Option<String> {
    if let Some(p) = cli_prefix
        && !p.is_empty()
    {
        return Some(p.to_string());
    }
    if let Ok(env_prefix) = std::env::var("UTOO_PREFIX")
        && !env_prefix.is_empty()
    {
        return Some(env_prefix);
    }
    Config::load(ConfigScope::Local)
        .await
        .ok()
        .and_then(|c| c.get("prefix").ok().flatten())
        .filter(|p| !p.is_empty())
}

pub fn get_cache_dir() -> PathBuf {
    // Cross-device relocation is resolved once in `set_cache_dir`.
    PathBuf::from(CACHE_DIR.get_sync())
}

// Package.json cache — keyed by directory path, covers root + workspace members.
// node_modules packages should use json::load_package_json directly.
static PACKAGE_JSON_CACHE: LazyLock<DashMap<PathBuf, PackageJson>> = LazyLock::new(DashMap::new);

/// Load and cache a package.json by directory path.
///
/// First call reads from disk and deserializes into `PackageJson`; subsequent
/// calls with the same path return the cached clone. Use for root project and
/// workspace member package.json files.
pub async fn get_or_load_package_json(path: &Path) -> Result<PackageJson> {
    if let Some(entry) = PACKAGE_JSON_CACHE.get(path) {
        return Ok(entry.value().clone());
    }
    let pkg: PackageJson = load_package_json(path).await?;
    PACKAGE_JSON_CACHE.insert(path.to_path_buf(), pkg.clone());
    Ok(pkg)
}

/// Write-through update: call after writing package.json to disk to keep the
/// cache consistent.
pub fn set_package_json(path: &Path, pkg: PackageJson) {
    PACKAGE_JSON_CACHE.insert(path.to_path_buf(), pkg);
}

// Semver support detection and caching
//
// Config key `supports-semver` is a TOML array of probe results. Each entry
// is the registry URL (positive → supports semver) or prefixed with "!"
// (negative → does NOT support):
//
//   [arrays]
//   supports-semver = [
//     "https://registry.npmmirror.com",
//     "!https://some-private.registry",
//   ]
//
// Multiple registries are cached simultaneously, so switching between them
// (e.g. `ut install --registry=xxx`) won't trigger a re-probe.
static SUPPORTS_SEMVER: OnceLock<bool> = OnceLock::new();

/// Probe a registry to detect whether it supports semver resolution.
///
/// 1. `is_npm_registry(url)` → return `false` immediately (known not to support)
/// 2. Search `arrays.supports-semver` for `"{url}"` (true) or `"!{url}"` (false)
/// 3. On cache miss, probe `GET $REGISTRY/utoo/%5E1` (5 s timeout)
/// 4. Append the result to the array for future runs
///
/// An optional `reqwest::Client` can be provided to reuse an existing client
/// (e.g. from the `ping` command). If `None`, a new client with 5s timeout is
/// created internally.
///
/// NOTE: Config file writes are not protected by file locking. Concurrent
/// `utoo` processes may race on the global config, potentially losing entries.
/// This is a known limitation — entries will be re-probed on the next run.
pub async fn detect_supports_semver(registry_url: &str, client: Option<&reqwest::Client>) -> bool {
    // Known: npm registry does not support semver queries
    if is_npm_registry(registry_url) {
        tracing::debug!("npm registry detected, skipping semver probe");
        return false;
    }

    // Check config cache (TOML array of "url" / "!url" entries)
    let mut config = Config::load(ConfigScope::Global).await.unwrap_or_default();
    let cached = config.get_array("supports-semver");

    if let Some(entries) = cached {
        for entry in entries {
            if entry == registry_url {
                tracing::debug!("Cache hit: supports-semver=true for {}", registry_url);
                return true;
            }
            if entry.strip_prefix('!') == Some(registry_url) {
                tracing::debug!("Cache hit: supports-semver=false for {}", registry_url);
                return false;
            }
        }
    }

    // Probe the registry
    let probe_url = format!("{}/utoo/%5E1", registry_url.trim_end_matches('/'));
    tracing::debug!("Probing semver support: {}", probe_url);

    let supports = async {
        let owned_client;
        let client = match client {
            Some(c) => c,
            None => {
                owned_client = client_builder()?
                    .timeout(std::time::Duration::from_secs(5))
                    .build()?;
                &owned_client
            }
        };
        let resp = client
            .get(&probe_url)
            .header("Accept", "application/vnd.npm.install-v1+json")
            .send()
            .await?;
        tracing::debug!("Semver probe response: {} for {}", resp.status(), probe_url);
        anyhow::Ok(resp.status().is_success())
    }
    .await
    .unwrap_or_else(|e| {
        tracing::debug!("Semver probe failed: {e:#}");
        false
    });

    // Update (or append) result in the cached array, deduplicating by registry URL
    let new_entry = if supports {
        registry_url.to_string()
    } else {
        format!("!{}", registry_url)
    };
    let mut entries = config
        .get_array("supports-semver")
        .map(|s| s.to_vec())
        .unwrap_or_default();
    // Remove any existing entry for this registry (both "url" and "!url" forms)
    entries.retain(|e| {
        let url = e.strip_prefix('!').unwrap_or(e);
        url != registry_url
    });
    entries.push(new_entry);
    if let Err(e) = config.set_array("supports-semver", entries, ConfigScope::Global) {
        tracing::warn!("Failed to save semver support to config: {}", e);
    }

    tracing::debug!("Detected supports-semver={} for {}", supports, registry_url);
    supports
}

pub fn get_supports_semver() -> Option<bool> {
    SUPPORTS_SEMVER.get().copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use tempfile::TempDir;

    #[test]
    fn url_origin_extracts_scheme_and_host() {
        assert_eq!(
            url_origin("https://registry.npmjs.org/abbrev/-/abbrev-2.0.0.tgz"),
            Some("https://registry.npmjs.org")
        );
        assert_eq!(
            url_origin("http://cdn.example.com:8080/x.tgz"),
            Some("http://cdn.example.com:8080")
        );
        assert_eq!(
            url_origin("https://registry.npmjs.org"),
            Some("https://registry.npmjs.org")
        );
        assert_eq!(url_origin("file:/abs/x.tgz"), None);
        assert_eq!(url_origin("git+https://github.com/u/r.git#abc"), None);
    }

    #[test]
    fn is_registry_tarball_trusts_known_registries_only() {
        // Well-known public registries → trusted (shared cache allowed).
        assert!(is_registry_tarball(
            "https://registry.npmjs.org/abbrev/-/abbrev-2.0.0.tgz"
        ));
        assert!(is_registry_tarball(
            "https://registry.npmmirror.com/abbrev/-/abbrev-2.0.0.tgz"
        ));
        // A remote tarball from an arbitrary host → NOT trusted (direct extract,
        // never the shared `<name>/<version>` slot).
        assert!(!is_registry_tarball(
            "https://cdn.example.com/foo-1.0.0.tgz"
        ));
        // A registry-host *lookalike* path on an untrusted host stays untrusted.
        assert!(!is_registry_tarball(
            "https://evil.example.com/abbrev/-/abbrev-2.0.0.tgz"
        ));
        // Non-http resolveds are not registry tarballs.
        assert!(!is_registry_tarball("file:../foo.tgz"));
    }

    fn src(value: &str) -> Option<String> {
        Some(value.to_string())
    }

    #[test]
    fn pick_profile_registry_precedence_order() {
        // `--registry` and `UTOO_REGISTRY` are handled ahead of these profiles
        // (they short-circuit in init_registry), so this covers the inter-profile
        // order only. Same-level utoo config beats the project .npmrc…
        let picked = pick_profile_registry(ProfileRegistries {
            project_config: src("https://project-toml.example.com"),
            project_npmrc: src("https://project-npmrc.example.com"),
            ..Default::default()
        });
        assert_eq!(
            picked,
            Some(("https://project-toml.example.com".to_string(), ".utoo.toml"))
        );

        // …but the project .npmrc beats the user-level global config: a
        // project must be able to redirect any npm client (registry bridges,
        // corporate proxies).
        let picked = pick_profile_registry(ProfileRegistries {
            project_npmrc: src("https://project-npmrc.example.com"),
            global_config: src("https://global-toml.example.com"),
            user_npmrc: src("https://user-npmrc.example.com"),
            ..Default::default()
        });
        assert_eq!(
            picked,
            Some((
                "https://project-npmrc.example.com".to_string(),
                "project .npmrc"
            ))
        );

        // User .npmrc is the last configured fallback before auto-select.
        let picked = pick_profile_registry(ProfileRegistries {
            user_npmrc: src("https://user-npmrc.example.com"),
            ..Default::default()
        });
        assert_eq!(
            picked,
            Some(("https://user-npmrc.example.com".to_string(), "user .npmrc"))
        );

        // Nothing configured → None (caller auto-selects).
        assert_eq!(pick_profile_registry(ProfileRegistries::default()), None);
    }

    #[test]
    fn pick_profile_registry_skips_empty_values() {
        // An empty config value is "unset", not a broken override, so a lower
        // profile with a real value wins over a higher one left blank.
        let picked = pick_profile_registry(ProfileRegistries {
            project_config: Some(String::new()),
            project_npmrc: src("https://project-npmrc.example.com"),
            ..Default::default()
        });
        assert_eq!(
            picked,
            Some((
                "https://project-npmrc.example.com".to_string(),
                "project .npmrc"
            ))
        );
    }

    #[test]
    fn normalize_registry_strips_trailing_slash() {
        // npm's own default form (trailing slash) → single-slash joins.
        assert_eq!(
            normalize_registry("https://registry.npmjs.org/").unwrap(),
            "https://registry.npmjs.org"
        );
        // Already canonical → unchanged.
        assert_eq!(
            normalize_registry("https://registry.npmmirror.com").unwrap(),
            "https://registry.npmmirror.com"
        );
        // Multiple trailing slashes and surrounding whitespace are normalized.
        assert_eq!(
            normalize_registry("  https://registry.npmjs.org///  ").unwrap(),
            "https://registry.npmjs.org"
        );
    }

    #[test]
    fn normalize_registry_preserves_subpath() {
        // Private registries served under a prefix keep their path.
        assert_eq!(
            normalize_registry("https://npm.corp/api/npm/npm-virtual/").unwrap(),
            "https://npm.corp/api/npm/npm-virtual"
        );
    }

    #[test]
    fn normalize_registry_rejects_invalid() {
        // Non-http(s) schemes and unparsable input are rejected, not silently
        // forwarded into a later HTTP fetch.
        assert!(normalize_registry("ftp://registry.example.com").is_err());
        assert!(normalize_registry("registry.npmjs.org").is_err());
        assert!(normalize_registry("not a url").is_err());
    }

    use super::super::config_file::ConfigValueParser;

    #[test]
    fn test_cache_dir_default() {
        let default_cache = dirs::home_dir()
            .unwrap()
            .join(".cache/nm")
            .to_string_lossy()
            .to_string();

        let cache_config = ConfigValue::new("cache-dir", default_cache.clone());
        let result = cache_config.get_sync();

        assert_eq!(result, default_cache);
    }

    #[test]
    fn test_config_value_parser_string() {
        let config = ConfigValue::new("test-key", "default-value".to_string());
        let parsed = config.parse_config_value("custom-value");
        assert_eq!(parsed, "custom-value");
    }

    #[test]
    fn test_config_value_parser_bool() {
        let config = ConfigValue::new("test-bool", false);
        assert!(config.parse_config_value("true"));
        assert!(config.parse_config_value("TRUE"));
        assert!(config.parse_config_value("True"));
        assert!(!config.parse_config_value("false"));
        assert!(!config.parse_config_value("anything"));
    }

    #[tokio::test]
    async fn test_resolve_global_prefix_cli_wins() {
        // A non-empty CLI prefix short-circuits before env/config are consulted.
        let resolved = resolve_global_prefix(Some("/opt/utoo")).await;
        assert_eq!(resolved.as_deref(), Some("/opt/utoo"));
    }

    #[tokio::test]
    async fn test_cache_dir_from_config_file() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let config_path = temp_dir.path().join("config.toml");

        let custom_path = "/tmp/config-cache-test";
        let config_content = format!(
            r#"
[values]
cache-dir = "{}"
"#,
            custom_path
        );
        std::fs::write(&config_path, config_content)?;

        let config = Config::load_from_path(&config_path).await?;
        let cache_dir = config.get("cache-dir")?.unwrap();

        assert_eq!(cache_dir, custom_path);
        Ok(())
    }

    // Two paths under the same temp dir share a device, so the resolver must
    // treat them as same-device regardless of platform.
    #[test]
    fn test_resolve_cache_dir_same_device_passthrough() -> Result<()> {
        let temp = TempDir::new()?;
        let project = temp.path().join("project");
        std::fs::create_dir_all(&project)?;
        let cache = temp.path().join("cache").to_string_lossy().to_string();

        // Explicit, same device → returned unchanged.
        assert_eq!(
            resolve_cache_dir(Some(cache.clone()), &project),
            Some(cache)
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn test_nearest_device_walks_to_existing_ancestor() -> Result<()> {
        let temp = TempDir::new()?;
        // A not-yet-created nested cache path resolves to its existing ancestor's device.
        let missing = temp.path().join("a/b/c/nm");
        assert_eq!(nearest_device(&missing), nearest_device(temp.path()));
        assert!(!is_cross_device(&missing, temp.path()));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn test_relocated_cache_dir_is_project_local() {
        let project = PathBuf::from("/work/repo");
        assert_eq!(
            relocated_cache_dir(&project),
            PathBuf::from("/work/repo/node_modules/.cache/nm")
        );
    }
}
