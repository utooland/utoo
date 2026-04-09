#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveType {
    Dev,
    Peer,
    Optional,
    Prod,
}

pub fn parse_save_type(save_dev: bool, save_peer: bool, save_optional: bool) -> SaveType {
    if save_dev {
        SaveType::Dev
    } else if save_peer {
        SaveType::Peer
    } else if save_optional {
        SaveType::Optional
    } else {
        SaveType::Prod
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageAction {
    Add,
    Remove,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, clap::ValueEnum)]
pub enum OmitType {
    Dev,
    Optional,
    Peer,
}

/// Whether to run lifecycle scripts during install/rebuild.
///
/// Replaces bare `ignore_scripts: bool` for readability.
/// See: <https://blakesmith.me/2019/05/07/rust-patterns-enums-instead-of-booleans.html>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptPolicy {
    Run,
    Ignore,
}

impl From<bool> for ScriptPolicy {
    fn from(ignore: bool) -> Self {
        if ignore { Self::Ignore } else { Self::Run }
    }
}
