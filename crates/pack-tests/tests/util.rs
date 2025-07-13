use std::path::PathBuf;

use dunce::canonicalize;
use once_cell::sync::Lazy;
use turbo_rcstr::RcStr;

pub static REPO_ROOT: Lazy<RcStr> = Lazy::new(|| {
    let package_root = PathBuf::from(env!("UTOO_WORKSPACE_DIR"));
    canonicalize(package_root).unwrap().to_str().unwrap().into()
});
