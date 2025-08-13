pub const APP_NAME: &str = "utoo";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const APP_ABOUT: &str = "🌖 /juːtuː/ Unified Toolchain: Open & Optimized";

pub mod cmd {
    pub const CLEAN_NAME: &str = "clean";
    pub const CLEAN_ABOUT: &str = "Clean package cache in global storage";
    pub const DEPS_NAME: &str = "deps";
    pub const DEPS_ABOUT: &str = "List and analyze project dependencies";
    pub const UPDATE_ABOUT: &str = "Remove node_modules and reinstall";
    pub const INSTALL_NAME: &str = "install";
    pub const INSTALL_ABOUT: &str = "Install project dependencies";
    pub const REBUILD_NAME: &str = "rebuild";
    pub const REBUILD_ABOUT: &str = "Rebuild native modules";
    pub const UNINSTALL_NAME: &str = "uninstall";
    pub const UNINSTALL_ABOUT: &str = "Uninstall spec dependencies";
    pub const EXECUTE_NAME: &str = "execute";
    pub const EXECUTE_ABOUT: &str = "Run a command from a local or remote npm package";
    pub const VIEW_NAME: &str = "view";
    pub const VIEW_ABOUT: &str = "View package information";
}
