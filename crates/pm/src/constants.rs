pub const APP_NAME: &str = "utoo";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const APP_ABOUT: &str = "🌖 /juːtuː/ Unified Toolchain: Open & Optimized";

pub mod cmd {
    pub const CLEAN_NAME: &str = "clean";
    pub const CLEAN_ALIAS: &str = "c";
    pub const CLEAN_ABOUT: &str = "Clean package cache in global storage";

    pub const DEPS_NAME: &str = "deps";
    pub const DEPS_ALIAS: &str = "d";
    pub const DEPS_ABOUT: &str = "Generate package-lock.json only";

    pub const UPDATE_NAME: &str = "update";
    pub const UPDATE_ALIAS: &str = "u";
    pub const UPDATE_ABOUT: &str = "Remove node_modules and reinstall";

    pub const INSTALL_NAME: &str = "install";
    pub const INSTALL_ALIAS: &str = "i";
    pub const INSTALL_ABOUT: &str = "Install project dependencies";

    pub const REBUILD_NAME: &str = "rebuild";
    pub const REBUILD_ALIAS: &str = "rb";
    pub const REBUILD_ABOUT: &str = "Rebuild scripts hooks in all packages";

    pub const UNINSTALL_NAME: &str = "uninstall";
    pub const UNINSTALL_ALIAS: &str = "un";
    pub const UNINSTALL_ABOUT: &str = "Uninstall spec dependencies";

    pub const EXECUTE_NAME: &str = "execute";
    pub const EXECUTE_ALIAS: &str = "x";
    pub const EXECUTE_ABOUT: &str = "Run a command from a local or remote npm package";

    pub const VIEW_NAME: &str = "view";
    pub const VIEW_ALIAS: &str = "v";
    pub const VIEW_ALIAS_INFO: &str = "info";
    pub const VIEW_ALIAS_SHOW: &str = "show";
    pub const VIEW_ALIASES: &str = "v, info, show";
    pub const VIEW_ABOUT: &str = "View package information";

    pub const LIST_NAME: &str = "list";
    pub const LIST_ALIAS: &str = "ls";
    pub const LIST_ABOUT: &str = "List dependencies like npm list";

    pub const RUN_NAME: &str = "run";
    pub const RUN_ALIAS: &str = "r";
    pub const RUN_ABOUT: &str = "Run scripts defined in package.json";

    pub const CONFIG_NAME: &str = "config";
    pub const CONFIG_ALIAS: &str = "cfg";
    pub const CONFIG_ABOUT: &str = "Manage configuration";

    pub const LINK_NAME: &str = "link";
    pub const LINK_ALIAS: &str = "ln";
    pub const LINK_ABOUT: &str = "Link a package like npm link";

    pub const INIT_NAME: &str = "init";
    pub const INIT_ALIAS: &str = "create";
    pub const INIT_ABOUT: &str = "Create a package.json file";
}
