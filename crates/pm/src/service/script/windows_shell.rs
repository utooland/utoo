use std::{borrow::Cow, env, ffi::OsStr};

/// Git for Windows adds its `cmd` directory to PATH by default, while its
/// POSIX shell lives in the sibling `bin`. Make it available to nested scripts
/// as well as the initial `sh -c`, without displacing any existing tools.
pub(super) fn script_path(path: &str) -> Cow<'_, str> {
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
        assert_eq!(script_path(host), host);
        fs::write(cmd.join("git.exe"), "").unwrap();
        assert_eq!(
            env::split_paths(&*script_path(host)).collect::<Vec<_>>(),
            [cmd.as_path(), usr_bin.as_path()]
        );
        fs::write(bin.join("sh.exe"), "").unwrap();
        assert_eq!(
            env::split_paths(&*script_path(host)).collect::<Vec<_>>(),
            [cmd.as_path(), bin.as_path()]
        );

        let custom_shell = temp.path().join("tools");
        fs::create_dir(&custom_shell).unwrap();
        fs::write(custom_shell.join("sh.exe"), "").unwrap();
        let host = env::join_paths([&cmd, &custom_shell]).unwrap();
        let host = host.to_str().unwrap();
        assert_eq!(script_path(host), host);
    }
}
