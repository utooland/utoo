use std::path::Path;

pub fn to_relative_path(path: &Path, base: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_to_relative_path() {
        let base = PathBuf::from("/home/user/project");
        let absolute = PathBuf::from("/home/user/project/src/main.rs");
        let relative = to_relative_path(&absolute, &base);
        assert_eq!(relative, "src/main.rs");
    }

    #[test]
    fn test_to_relative_path_outside_base() {
        let base = PathBuf::from("/home/user/project");
        let absolute = PathBuf::from("/home/user/other/file.txt");
        let relative = to_relative_path(&absolute, &base);
        assert_eq!(relative, "/home/user/other/file.txt");
    }

    #[test]
    fn test_to_relative_path_same_path() {
        let base = PathBuf::from("/home/user/project");
        let absolute = PathBuf::from("/home/user/project");
        let relative = to_relative_path(&absolute, &base);
        assert_eq!(relative, "");
    }
}
