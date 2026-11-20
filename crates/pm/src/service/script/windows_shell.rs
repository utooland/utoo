use std::{borrow::Cow, env, ffi::OsStr};
use tokio::sync::Mutex;

/// Package hooks share the host PATH, but a global bin link can change it
/// within this process. Keep only the last snapshot and coalesce concurrent probes.
pub(super) struct ShellPathCache(Mutex<Option<(String, String)>>);

impl ShellPathCache {
    pub(super) const fn new() -> Self {
        Self(Mutex::const_new(None))
    }

    pub(super) async fn resolve(&self, path: String) -> anyhow::Result<String> {
        let mut cache = self.0.lock().await;
        if let Some((host, resolved)) = cache.as_ref()
            && host == &path
        {
            return Ok(resolved.clone());
        }

        let (host, resolved) = tokio::task::spawn_blocking(move || {
            let resolved = discover_script_path(&path).into_owned();
            (path, resolved)
        })
        .await?;
        *cache = Some((host, resolved.clone()));
        Ok(resolved)
    }
}

/// Git for Windows adds its `cmd` directory to PATH by default, while its
/// POSIX shell lives in the sibling `bin`. Make it available to nested scripts
/// as well as the initial `sh -c`, without displacing any existing tools.
fn discover_script_path(path: &str) -> Cow<'_, str> {
    let dirs = || env::split_paths(path).filter(|dir| dir.is_absolute());
    if dirs().any(|dir| dir.join("sh.exe").is_file()) {
        return Cow::Borrowed(path);
    }

    for cmd in dirs() {
        if !cmd
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(OsStr::new("cmd")))
            || !cmd.join("git.exe").is_file()
        {
            continue;
        }
        let Some(root) = cmd.parent() else { continue };
        for bin in [root.join("bin"), root.join("usr").join("bin")] {
            if bin.join("sh.exe").is_file()
                && let Ok(joined) = env::join_paths(env::split_paths(path).chain([bin]))
            {
                return Cow::Owned(joined.to_string_lossy().into_owned());
            }
        }
    }
    Cow::Borrowed(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn exposes_the_shell_from_the_git_installation() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("Portable Git");
        let cmd = root.join("cmd");
        let bin = root.join("bin");
        let usr_bin = root.join("usr").join("bin");
        for dir in [&cmd, &bin, &usr_bin] {
            fs::create_dir_all(dir).unwrap();
        }
        let host = env::join_paths([&cmd]).unwrap();
        let host = host.to_str().unwrap();
        fs::write(usr_bin.join("sh.exe"), "").unwrap();
        assert_eq!(discover_script_path(host), host);
        fs::write(cmd.join("git.exe"), "").unwrap();
        assert_eq!(
            env::split_paths(&*discover_script_path(host)).collect::<Vec<_>>(),
            [cmd.as_path(), usr_bin.as_path()]
        );
        fs::write(bin.join("sh.exe"), "").unwrap();
        assert_eq!(
            env::split_paths(&*discover_script_path(host)).collect::<Vec<_>>(),
            [cmd.as_path(), bin.as_path()]
        );

        let custom_shell = temp.path().join("tools");
        fs::create_dir(&custom_shell).unwrap();
        fs::write(custom_shell.join("sh.exe"), "").unwrap();
        let host = env::join_paths([&cmd, &custom_shell]).unwrap();
        let host = host.to_str().unwrap();
        assert_eq!(discover_script_path(host), host);
    }

    #[tokio::test]
    async fn reuses_the_probe_until_the_host_path_changes() {
        let cache = ShellPathCache::new();
        let root = tempdir().unwrap();
        let cmd = root.path().join("cmd");
        let bin = root.path().join("bin");
        fs::create_dir(&cmd).unwrap();
        fs::create_dir(&bin).unwrap();
        fs::write(cmd.join("git.exe"), "").unwrap();
        fs::write(bin.join("sh.exe"), "").unwrap();
        let host = cmd.to_str().unwrap().to_owned();
        let expected = env::join_paths([&cmd, &bin]).unwrap();
        assert_eq!(
            cache.resolve(host.clone()).await.unwrap(),
            expected.to_str().unwrap()
        );
        fs::remove_file(bin.join("sh.exe")).unwrap();
        assert_eq!(
            cache.resolve(host).await.unwrap(),
            expected.to_str().unwrap()
        );
        let changed = env::join_paths([&cmd, root.path()])
            .unwrap()
            .into_string()
            .unwrap();
        assert_eq!(cache.resolve(changed.clone()).await.unwrap(), changed);
    }
}
