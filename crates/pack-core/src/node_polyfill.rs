use std::sync::LazyLock;

use anyhow::Result;
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::Vc;
use turbo_tasks_fs::FileSystem;
use turbopack_core::resolve::options::{ImportMap, ImportMapping};

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

const EMPTY_POLYFILL_PATH: &str = "@utoo/pack-runtime/node-polyfills/empty/empty.js";

fn add_node_alias(aliases: &mut Vec<(RcStr, RcStr)>, original: &str, alias: &str) {
    aliases.push((RcStr::from(original), RcStr::from(alias)));
    aliases.push((RcStr::from(format!("node:{original}")), RcStr::from(alias)));
}

/// Node.js module to polyfill mapping
/// These polyfills are located in pack-core/js/src/node-polyfills/
static NODE_POLYFILL_ALIASES: LazyLock<Vec<(RcStr, RcStr)>> = LazyLock::new(|| {
    let mut aliases = vec![
        (
            rcstr!("assert"),
            rcstr!("@utoo/pack-runtime/node-polyfills/assert/assert.js"),
        ),
        (
            rcstr!("buffer"),
            rcstr!("@utoo/pack-runtime/node-polyfills/buffer/index.js"),
        ),
        (
            rcstr!("constants"),
            rcstr!("@utoo/pack-runtime/node-polyfills/constants-browserify/constants.json"),
        ),
        (
            rcstr!("crypto"),
            rcstr!("@utoo/pack-runtime/node-polyfills/crypto-browserify/index.js"),
        ),
        (
            rcstr!("domain"),
            rcstr!("@utoo/pack-runtime/node-polyfills/domain-browser/index.js"),
        ),
        (
            rcstr!("events"),
            rcstr!("@utoo/pack-runtime/node-polyfills/events/events.js"),
        ),
        (
            rcstr!("http"),
            rcstr!("@utoo/pack-runtime/node-polyfills/stream-http/index.js"),
        ),
        (
            rcstr!("https"),
            rcstr!("@utoo/pack-runtime/node-polyfills/https-browserify/index.js"),
        ),
        (
            rcstr!("os"),
            rcstr!("@utoo/pack-runtime/node-polyfills/os-browserify/browser.js"),
        ),
        (
            rcstr!("path"),
            rcstr!("@utoo/pack-runtime/node-polyfills/path-browserify/index.js"),
        ),
        (
            rcstr!("process"),
            rcstr!("@utoo/pack-runtime/node-polyfills/process/browser.js"),
        ),
        (
            rcstr!("punycode"),
            rcstr!("@utoo/pack-runtime/node-polyfills/punycode/punycode.js"),
        ),
        (
            rcstr!("querystring"),
            rcstr!("@utoo/pack-runtime/node-polyfills/querystring-es3/index.js"),
        ),
        (
            rcstr!("stream"),
            rcstr!("@utoo/pack-runtime/node-polyfills/stream-browserify/index.js"),
        ),
        (
            rcstr!("string_decoder"),
            rcstr!("@utoo/pack-runtime/node-polyfills/string_decoder/string_decoder.js"),
        ),
        (
            rcstr!("timers"),
            rcstr!("@utoo/pack-runtime/node-polyfills/timers-browserify/main.js"),
        ),
        (
            rcstr!("tty"),
            rcstr!("@utoo/pack-runtime/node-polyfills/tty-browserify/index.js"),
        ),
        (
            rcstr!("url"),
            rcstr!("@utoo/pack-runtime/node-polyfills/native-url/index.js"),
        ),
        (
            rcstr!("util"),
            rcstr!("@utoo/pack-runtime/node-polyfills/util/util.js"),
        ),
        (
            rcstr!("vm"),
            rcstr!("@utoo/pack-runtime/node-polyfills/vm-browserify/index.js"),
        ),
        (
            rcstr!("zlib"),
            rcstr!("@utoo/pack-runtime/node-polyfills/browserify-zlib/index.js"),
        ),
        (
            rcstr!("setimmediate"),
            rcstr!("@utoo/pack-runtime/node-polyfills/setimmediate/setImmediate.js"),
        ),
    ];

    let empty_modules = [
        "async_hooks",
        "child_process",
        "cluster",
        "dgram",
        "diagnostics_channel",
        "dns",
        "fs",
        "fs/promises",
        "http2",
        "inspector",
        "module",
        "net",
        "perf_hooks",
        "readline",
        "repl",
        "tls",
        "trace_events",
        "v8",
        "wasi",
        "worker_threads",
    ];

    for name in empty_modules {
        add_node_alias(&mut aliases, name, EMPTY_POLYFILL_PATH);
    }

    aliases
});

#[cfg(test)]
mod tests {
    use super::{EMPTY_POLYFILL_PATH, NODE_POLYFILL_ALIASES};

    fn alias_target(module: &str) -> Option<&str> {
        NODE_POLYFILL_ALIASES
            .iter()
            .find(|(original, _)| original.as_str() == module)
            .map(|(_, alias)| alias.as_str())
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
