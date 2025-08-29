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
