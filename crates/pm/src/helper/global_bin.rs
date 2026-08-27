use crate::util::platform_const::GLOBAL_NODE_MODULES;
use anyhow::{Context, Result};
use std::env;
use std::path::{Path, PathBuf, absolute};

// Global install layout helpers.
//
// Two functions matter to callers: `get_global_bin_dir` (where executables are
// linked / found on PATH) and `get_global_package_dir` (where global packages
// live). Both accept an optional explicit `--prefix`; without one they infer the
// layout from the running executable.
//
// Two install layouts are supported when inferring:
//
// * **Flat / standalone** — the binary sits directly in some dir that is on
//   PATH (`/usr/local/bin/utoo`, a Homebrew Cellar, CI's `dist/utoo`, …). Global
//   executables go right next to it (its own dir) and packages one level up, so
//   they share that PATH entry. This does NOT assume the dir is named `bin`.
// * **npm-style global** — utoo itself was installed as a package. Native npm
//   releases run from a nested optional platform package, so their launcher
//   passes `UTOO_MANAGED_PACKAGE_ROOT`. We infer from that public package root;
//   older releases fall back to `current_exe()`. In either case we climb back
//   out past `lib/node_modules/<pkg>` (and any scope dir) to recover `<prefix>`.
//   Otherwise bins and globals would be written under utoo's own package dir.

const MANAGED_PACKAGE_ROOT_ENV: &str = "UTOO_MANAGED_PACKAGE_ROOT";

/// Global bin directory (where global executables are linked / found on PATH).
///
/// With an explicit prefix: `<prefix>/bin` on Unix, `<prefix>` on Windows (npm
/// places global shims directly in the prefix root on Windows). Without one, it
/// is inferred from the running executable (see module docs).
pub fn get_global_bin_dir(prefix: Option<&str>) -> Result<PathBuf> {
    match prefix {
        Some(prefix) => Ok(prefixed_bin_dir(&to_absolute(PathBuf::from(prefix)))),
        None => {
            let exe = executable_path_for_inference()?;
            Ok(bin_dir_from_exe(&exe))
        }
    }
}

/// Global package directory, e.g. `/usr/local/lib/node_modules`.
pub fn get_global_package_dir(prefix: Option<&str>) -> Result<PathBuf> {
    match prefix {
        Some(prefix) => Ok(to_absolute(PathBuf::from(prefix)).join(GLOBAL_NODE_MODULES)),
        None => {
            let exe = executable_path_for_inference()?;
            Ok(package_dir_from_exe(&exe))
        }
    }
}

/// Path used for npm-style prefix inference.
///
/// The npm launcher lives in the public `utoo` package while the native binary
/// lives in `utoo`'s optional platform dependency. Inferring from
/// `current_exe()` would therefore stop at the nested package's `node_modules`.
/// The launcher supplies its real package root so inference retains the same
/// behavior as releases where the native binary lived directly in `utoo/bin`.
fn executable_path_for_inference() -> Result<PathBuf> {
    if let Some(root) = env::var_os(MANAGED_PACKAGE_ROOT_ENV).filter(|root| !root.is_empty()) {
        return Ok(PathBuf::from(root).join("bin").join("utoo.js"));
    }
    env::current_exe().context("Failed to get current executable path")
}

/// `<prefix>/bin` on Unix, `<prefix>` on Windows — npm's global-shim location.
fn prefixed_bin_dir(prefix: &Path) -> PathBuf {
    if cfg!(windows) {
        prefix.to_path_buf()
    } else {
        prefix.join("bin")
    }
}

/// Make a prefix absolute (lexically, without touching the filesystem) so global
/// installs don't land in cwd-relative dirs when `prefix` came from a relative
/// `UTOO_PREFIX` / config value. An empty/unresolvable path is returned as-is.
fn to_absolute(prefix: PathBuf) -> PathBuf {
    if prefix.is_absolute() {
        prefix
    } else {
        absolute(&prefix).unwrap_or(prefix)
    }
}

/// `Some(prefix)` when `exe` lives in an npm-style global layout
/// (`<prefix>/lib/node_modules/<pkg>[/@scope]/bin/<exe>`); `None` for a flat
/// layout where the binary sits directly in an on-PATH dir.
fn npm_style_prefix(exe: &Path) -> Option<PathBuf> {
    let components: Vec<_> = exe.components().collect();
    let idx = components
        .iter()
        .rposition(|c| c.as_os_str() == "node_modules")?;

    // `GLOBAL_NODE_MODULES` is `lib/node_modules` on Unix and `node_modules` on
    // Windows, so on Unix we also drop the preceding `lib` component.
    let cut = if !cfg!(windows) && idx > 0 && components[idx - 1].as_os_str() == "lib" {
        idx - 1
    } else {
        idx
    };
    Some(components[..cut].iter().map(|c| c.as_os_str()).collect())
}

/// Bin dir inferred from the executable path. npm-style → `<prefix>/bin`
/// (`<prefix>` on Windows); flat → the binary's own dir (on PATH), whatever it
/// is named.
fn bin_dir_from_exe(exe: &Path) -> PathBuf {
    match npm_style_prefix(exe) {
        Some(prefix) => prefixed_bin_dir(&prefix),
        // Flat layout: the binary's own dir. `current_exe()` is always an
        // absolute path on real platforms, so `parent()` is `Some`;
        // `unwrap_or_default()` is only a non-panicking guard for the
        // impossible parentless case.
        None => exe.parent().map(Path::to_path_buf).unwrap_or_default(),
    }
}

/// Package dir inferred from the executable path. npm-style → `<prefix>/<GLOBAL>`;
/// flat → `<parent-of-bin-dir>/<GLOBAL>` (one level up from the binary).
fn package_dir_from_exe(exe: &Path) -> PathBuf {
    let base = match npm_style_prefix(exe) {
        Some(prefix) => prefix,
        None => exe
            .parent()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_default(),
    };
    base.join(GLOBAL_NODE_MODULES)
}

#[cfg(test)]
mod tests {
    use super::*;

    // These use Unix-style absolute path strings (`/usr/local`), which are not
    // absolute on Windows; Windows behavior is covered by dedicated
    // `#[cfg(windows)]` tests.
    #[cfg(not(windows))]
    #[test]
    fn test_get_global_bin_dir_with_prefix() {
        let result = get_global_bin_dir(Some("/usr/local")).unwrap();
        assert_eq!(result, PathBuf::from("/usr/local/bin"));
    }

    #[cfg(not(windows))]
    #[test]
    fn test_get_global_package_dir_with_prefix() {
        let result = get_global_package_dir(Some("/usr/local")).unwrap();
        assert_eq!(
            result,
            PathBuf::from("/usr/local").join(GLOBAL_NODE_MODULES)
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn test_get_global_bin_dir_with_empty_prefix() {
        let result = get_global_bin_dir(Some("")).unwrap();
        assert_eq!(result, PathBuf::from("bin"));
    }

    #[test]
    fn test_get_global_package_dir_with_empty_prefix() {
        let result = get_global_package_dir(Some("")).unwrap();
        assert_eq!(result, PathBuf::from(GLOBAL_NODE_MODULES));
    }

    #[cfg(windows)]
    #[test]
    fn test_get_global_bin_dir_windows_is_prefix_root() {
        // npm places global shims directly in the prefix root on Windows.
        let result = get_global_bin_dir(Some("C:\\npm")).unwrap();
        assert_eq!(result, PathBuf::from("C:\\npm"));
    }

    #[test]
    fn test_relative_prefix_is_normalized_to_absolute() {
        // A relative prefix (e.g. from `UTOO_PREFIX=relative`) must be made
        // absolute so installs don't land in cwd-relative dirs.
        let result = get_global_package_dir(Some("relative-prefix")).unwrap();
        assert!(
            result.is_absolute(),
            "relative prefix should be absolute: {result:?}"
        );
        assert!(result.ends_with(PathBuf::from("relative-prefix").join(GLOBAL_NODE_MODULES)));
    }

    // Flat layout: the binary's own dir is the bin dir, regardless of its name —
    // this is the CI scenario (`<workspace>/dist/utoo`), and the regression that
    // a `<prefix>/bin` assumption broke.
    #[test]
    fn test_flat_install_bin_dir_is_exe_own_dir() {
        let exe = PathBuf::from("/work/dist/utoo");
        assert_eq!(bin_dir_from_exe(&exe), PathBuf::from("/work/dist"));
        assert_eq!(
            package_dir_from_exe(&exe),
            PathBuf::from("/work").join(GLOBAL_NODE_MODULES)
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn test_npm_style_unix() {
        // utoo installed as an npm global package: the real binary lives deep in
        // `lib/node_modules/utoo/bin`; bins/packages resolve to the real prefix.
        let exe = PathBuf::from("/home/admin/.local/lib/node_modules/utoo/bin/ut");
        assert_eq!(
            bin_dir_from_exe(&exe),
            PathBuf::from("/home/admin/.local/bin")
        );
        assert_eq!(
            package_dir_from_exe(&exe),
            PathBuf::from("/home/admin/.local/lib/node_modules")
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn test_npm_style_scoped() {
        // Scoped package: the scope dir after `node_modules` is stripped too.
        let exe = PathBuf::from("/usr/local/lib/node_modules/@scope/utoo/bin/ut");
        assert_eq!(bin_dir_from_exe(&exe), PathBuf::from("/usr/local/bin"));
        assert_eq!(
            package_dir_from_exe(&exe),
            PathBuf::from("/usr/local/lib/node_modules")
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn test_npm_launcher_package_root_unix() {
        let launcher = PathBuf::from("/home/admin/.local/lib/node_modules/utoo/bin/utoo.js");
        assert_eq!(
            bin_dir_from_exe(&launcher),
            PathBuf::from("/home/admin/.local/bin")
        );
        assert_eq!(
            package_dir_from_exe(&launcher),
            PathBuf::from("/home/admin/.local/lib/node_modules")
        );
    }

    #[cfg(windows)]
    #[test]
    fn test_npm_style_windows() {
        // npm-style global on Windows: GLOBAL_NODE_MODULES is `node_modules`
        // (no `lib`), and the global bin dir is the prefix root. Exercises the
        // Component -> PathBuf rebuild reproducing the `C:\` drive prefix.
        let exe = PathBuf::from("C:\\npm\\node_modules\\utoo\\bin\\ut.exe");
        assert_eq!(bin_dir_from_exe(&exe), PathBuf::from("C:\\npm"));
        assert_eq!(
            package_dir_from_exe(&exe),
            PathBuf::from("C:\\npm").join(GLOBAL_NODE_MODULES)
        );
    }

    #[cfg(windows)]
    #[test]
    fn test_npm_launcher_package_root_windows() {
        let launcher = PathBuf::from("C:\\npm\\node_modules\\utoo\\bin\\utoo.js");
        assert_eq!(bin_dir_from_exe(&launcher), PathBuf::from("C:\\npm"));
        assert_eq!(
            package_dir_from_exe(&launcher),
            PathBuf::from("C:\\npm").join(GLOBAL_NODE_MODULES)
        );
    }
}
