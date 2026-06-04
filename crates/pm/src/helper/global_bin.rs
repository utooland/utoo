use crate::util::platform_const::GLOBAL_NODE_MODULES;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

// Global install layout helpers.
//
// Everything is derived from a single `prefix` (npm's global `prefix`): the
// directory whose `bin/` is on `PATH` and whose `<GLOBAL_NODE_MODULES>` holds
// globally-installed packages. Resolve it once via `get_global_prefix`, then
// the bin dir is `<prefix>/bin` and the package dir is
// `<prefix>/<GLOBAL_NODE_MODULES>`.

/// Resolve the global prefix.
///
/// With an explicit `--prefix` we trust it verbatim. Otherwise we infer it from
/// the running executable, supporting two install layouts:
///
/// * **Flat / standalone** — the binary lives directly in `<prefix>/bin`
///   (e.g. `/usr/local/bin/utoo`); the prefix is the parent of that `bin` dir.
/// * **npm-style global** — utoo itself was installed as a package, so the real
///   binary is at `<prefix>/lib/node_modules/utoo/bin/ut` and the on-`PATH`
///   `<prefix>/bin/ut` is just a symlink. On Linux `current_exe()` resolves the
///   symlink to that deep path, so we climb back out past `lib/node_modules/<pkg>`
///   (and any scope dir) to recover `<prefix>`. Otherwise bins and globals would
///   be written under utoo's own package dir instead of the real prefix.
pub fn get_global_prefix(prefix: Option<&str>) -> Result<PathBuf> {
    let prefix = match prefix {
        Some(prefix) => PathBuf::from(prefix),
        None => {
            let exe = std::env::current_exe().context("Failed to get current executable path")?;
            infer_prefix_from_exe(&exe)
        }
    };
    Ok(to_absolute(prefix))
}

/// Make a prefix absolute (lexically, without touching the filesystem) so global
/// installs don't land in cwd-relative dirs when `prefix` came from a relative
/// `UTOO_PREFIX` / config value. Inferred-from-exe prefixes are already absolute.
/// An empty/unresolvable path is returned unchanged.
fn to_absolute(prefix: PathBuf) -> PathBuf {
    if prefix.is_absolute() {
        prefix
    } else {
        std::path::absolute(&prefix).unwrap_or(prefix)
    }
}

/// Infer the prefix from a (symlink-resolved) executable path. See
/// [`get_global_prefix`] for the layouts handled.
fn infer_prefix_from_exe(exe: &Path) -> PathBuf {
    let components: Vec<_> = exe.components().collect();

    // npm-style: strip everything from the global `node_modules` segment down.
    // `GLOBAL_NODE_MODULES` is `lib/node_modules` on Unix and `node_modules` on
    // Windows, so on Unix we also drop the preceding `lib` component.
    if let Some(idx) = components
        .iter()
        .rposition(|c| c.as_os_str() == "node_modules")
    {
        let cut = if !cfg!(windows) && idx > 0 && components[idx - 1].as_os_str() == "lib" {
            idx - 1
        } else {
            idx
        };
        return components[..cut].iter().map(|c| c.as_os_str()).collect();
    }

    // Flat layout `<prefix>/bin/<exe>`: the prefix is the grandparent of the exe.
    exe.parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

/// Global bin directory: `<prefix>/bin` on Unix, `<prefix>` on Windows.
///
/// npm places global executable shims (`.cmd`/`.ps1`) directly in the prefix
/// root on Windows — that root is what ends up on `PATH` — whereas on Unix the
/// on-`PATH` dir is `<prefix>/bin`.
pub fn get_global_bin_dir(prefix: Option<&str>) -> Result<PathBuf> {
    let prefix = get_global_prefix(prefix)?;
    if cfg!(windows) {
        Ok(prefix)
    } else {
        Ok(prefix.join("bin"))
    }
}

/// Global package directory, e.g. `/usr/local/lib/node_modules`.
pub fn get_global_package_dir(prefix: Option<&str>) -> Result<PathBuf> {
    Ok(get_global_prefix(prefix)?.join(GLOBAL_NODE_MODULES))
}

#[cfg(test)]
mod tests {
    use super::*;

    // These use Unix-style absolute path strings (`/usr/local`), which are not
    // absolute on Windows; the Windows bin-dir/normalization behavior is covered
    // by dedicated `#[cfg(windows)]` tests below.
    #[cfg(not(windows))]
    #[test]
    fn test_get_global_bin_dir_with_prefix() {
        let prefix: Option<&str> = Some("/usr/local");
        let result = get_global_bin_dir(prefix).unwrap();
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
        let prefix: Option<&str> = Some("");
        let result = get_global_bin_dir(prefix).unwrap();
        assert_eq!(result, PathBuf::from("bin"));
    }

    #[cfg(windows)]
    #[test]
    fn test_get_global_bin_dir_windows_is_prefix_root() {
        // npm places global shims directly in the prefix root on Windows.
        let result = get_global_bin_dir(Some("C:\\npm")).unwrap();
        assert_eq!(result, PathBuf::from("C:\\npm"));
    }

    #[test]
    fn test_get_global_prefix_normalizes_relative_to_absolute() {
        // A relative prefix (e.g. from `UTOO_PREFIX=relative`) must be made
        // absolute so installs don't land in cwd-relative dirs.
        let result = get_global_prefix(Some("relative-prefix")).unwrap();
        assert!(
            result.is_absolute(),
            "relative prefix should be absolute: {result:?}"
        );
        assert!(result.ends_with("relative-prefix"));
    }

    #[test]
    fn test_get_global_package_dir_with_empty_prefix() {
        let prefix: Option<&str> = Some("");
        let result = get_global_package_dir(prefix).unwrap();
        assert_eq!(result, PathBuf::from(GLOBAL_NODE_MODULES));
    }

    #[test]
    fn test_infer_prefix_flat_install() {
        // `<prefix>/bin/utoo` -> prefix is the grandparent.
        let exe = PathBuf::from("/usr/local").join("bin").join("utoo");
        assert_eq!(infer_prefix_from_exe(&exe), PathBuf::from("/usr/local"));
    }

    #[cfg(not(windows))]
    #[test]
    fn test_infer_prefix_npm_style_unix() {
        // utoo installed as an npm global package: the real binary lives deep in
        // `lib/node_modules/utoo/bin`; the prefix must climb back out past it.
        let exe = PathBuf::from("/home/admin/.local").join("lib/node_modules/utoo/bin/ut");
        assert_eq!(
            infer_prefix_from_exe(&exe),
            PathBuf::from("/home/admin/.local")
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn test_infer_prefix_npm_style_scoped() {
        // Scoped package: the scope dir after `node_modules` is stripped too.
        let exe = PathBuf::from("/usr/local").join("lib/node_modules/@scope/utoo/bin/ut");
        assert_eq!(infer_prefix_from_exe(&exe), PathBuf::from("/usr/local"));
    }
}
