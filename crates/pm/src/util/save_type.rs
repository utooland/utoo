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

use super::bool_enum::bool_enum;

bool_enum! {
    /// Whether to run lifecycle scripts during install/rebuild.
    /// `false` → Run, `true` → Ignore (`--ignore-scripts`).
    pub ScriptPolicy { Run, Ignore }
}
