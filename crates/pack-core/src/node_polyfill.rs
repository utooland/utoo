use std::{collections::BTreeMap, sync::LazyLock};

use anyhow::Result;
use serde::Deserialize;
use turbo_rcstr::RcStr;
use turbo_tasks::Vc;
use turbo_tasks_fs::FileSystem;
use turbopack_core::resolve::options::{ImportMap, ImportMapping};

const NODE_POLYFILL_MANIFEST: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/js/src/node-polyfills/generated/manifest.json"
));

#[derive(Deserialize)]
struct NodePolyfillManifest {
    aliases: BTreeMap<String, String>,
}

static NODE_POLYFILL_ALIASES: LazyLock<Vec<(RcStr, RcStr)>> = LazyLock::new(|| {
    let manifest: NodePolyfillManifest = serde_json::from_str(NODE_POLYFILL_MANIFEST)
        .expect("generated node polyfill manifest should be valid");

    manifest
        .aliases
        .into_iter()
        .map(|(original, alias)| (original.into(), alias.into()))
        .collect()
});

#[turbo_tasks::function]
pub async fn get_node_polyfill_import_map() -> Result<Vc<ImportMap>> {
    let mut import_map = ImportMap::empty();

    for (original, alias) in NODE_POLYFILL_ALIASES.iter() {
        import_map.insert_exact_alias(
            original.clone(),
            ImportMapping::PrimaryAlternative(
                alias.clone(),
                Some(crate::embed_js::embed_fs().root().owned().await?),
            )
            .resolved_cell(),
        );
    }

    Ok(import_map.cell())
}

#[cfg(test)]
mod tests {
    use super::NODE_POLYFILL_ALIASES;

    const EMPTY_POLYFILL_PATH: &str = "@utoo/pack-runtime/node-polyfills/empty/empty.js";

    fn alias_target(module: &str) -> Option<&str> {
        NODE_POLYFILL_ALIASES
            .iter()
            .find(|(original, _)| original.as_str() == module)
            .map(|(_, alias)| alias.as_str())
    }

    #[test]
    fn loads_generated_node_stdlib_browser_aliases() {
        assert_eq!(NODE_POLYFILL_ALIASES.len(), 99);

        for (module, expected) in [
            (
                "buffer",
                "@utoo/pack-runtime/node-polyfills/generated/node_modules/buffer",
            ),
            (
                "node:buffer",
                "@utoo/pack-runtime/node-polyfills/generated/node_modules/buffer",
            ),
            (
                "timers/promises",
                "@utoo/pack-runtime/node-polyfills/generated/node_modules/isomorphic-timers-promises/cjs",
            ),
            (
                "setimmediate",
                "@utoo/pack-runtime/node-polyfills/generated/node_modules/setimmediate/setImmediate.js",
            ),
        ] {
            assert_eq!(alias_target(module), Some(expected), "{module}");
        }
    }

    #[test]
    fn includes_empty_polyfills_for_unsupported_node_builtins() {
        for module in ["child_process", "fs", "fs/promises", "module", "net", "tls"] {
            assert_eq!(alias_target(module), Some(EMPTY_POLYFILL_PATH), "{module}");
        }
    }

    #[test]
    fn includes_node_protocol_empty_polyfills() {
        for module in [
            "node:child_process",
            "node:fs",
            "node:fs/promises",
            "node:module",
            "node:net",
            "node:tls",
        ] {
            assert_eq!(alias_target(module), Some(EMPTY_POLYFILL_PATH), "{module}");
        }
    }
}
