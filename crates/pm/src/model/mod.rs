pub mod package;
pub mod publish_payload;

use crate::util::bool_enum::bool_enum;

bool_enum! {
    /// Whether to perform the real operation or just simulate it.
    /// `false` → Live, `true` → DryRun (`--dry-run`).
    pub RunMode { Live, DryRun }
}
