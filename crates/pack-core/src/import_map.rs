use std::{collections::BTreeMap, sync::LazyLock};

use anyhow::Result;
use rustc_hash::FxHashMap;
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{FxIndexMap, ResolvedVc, Vc};
use turbo_tasks_fs::{FileSystem, FileSystemPath};
use turbopack_core::resolve::{
    ExternalTraced, ExternalType, ResolveAliasMap, ResolveResult, ResolveResultItem, SubpathValue,
    options::{ConditionValue, ImportMap, ImportMapping, ResolvedMap},
};
use turbopack_node::execution_context::ExecutionContext;

use crate::{config::Config, embed_js, mode::Mode, util::convert_to_project_relative};

pub const UTOO_STYLE_LOADER: &str = "@utoo/style-loader";

/// Node.js module to polyfill mapping
/// These polyfills are located in pack-core/js/src/node-polyfills/
static NODE_POLYFILL_ALIASES: LazyLock<[(RcStr, RcStr); 23]> = LazyLock::new(|| {
    [
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
        (
            rcstr!("fs"),
            rcstr!("@utoo/pack-runtime/node-polyfills/empty/empty.js"),
        ),
    ]
});

pub fn mdx_import_source_file() -> RcStr {
    unreachable!()
}

#[turbo_tasks::function]
#[allow(unused_variables)]
pub async fn get_postcss_package_mapping(
    project_path: FileSystemPath,
) -> Result<Vc<ImportMapping>> {
    #[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
    {
        Ok(
            ImportMapping::Direct(ResolveResult::primary(ResolveResultItem::External {
                name: rcstr!("postcss"),
                ty: ExternalType::CommonJs,
                traced: ExternalTraced::Untraced,
            }))
            .cell(),
        )
    }

    #[cfg(all(target_family = "wasm", target_os = "unknown"))]
    {
        Ok(ImportMapping::PrimaryAlternative(rcstr!("postcss"), Some(project_path)).cell())
    }
}

/// Computes the client fallback import map, which provides
/// polyfills to Node.js externals.
#[turbo_tasks::function]
pub async fn get_client_fallback_import_map(node_polyfill: bool) -> Result<Vc<ImportMap>> {
    let mut import_map = ImportMap::empty();

    if node_polyfill {
        import_map.extend_ref(&*get_node_polyfill_import_map().await?);
    }

    Ok(import_map.cell())
}

#[turbo_tasks::function]
async fn get_node_polyfill_import_map() -> Result<Vc<ImportMap>> {
    let mut import_map = ImportMap::empty();
    for (original, alias) in NODE_POLYFILL_ALIASES.iter() {
        import_map.insert_exact_alias(
            original.clone(),
            request_to_import_mapping(crate::embed_js::embed_fs().root().owned().await?, alias),
        );
    }
    Ok(import_map.cell())
}

// Make sure to not add any external requests here.
/// Computes the client import map.
#[turbo_tasks::function]
pub async fn get_client_import_map(
    project_path: FileSystemPath,
    config: Vc<Config>,
    execution_context: Vc<ExecutionContext>,
    pack_path: FileSystemPath,
) -> Result<Vc<ImportMap>> {
    let mut import_map = ImportMap::empty();

    insert_shared_aliases(
        &mut import_map,
        &project_path,
        execution_context,
        config,
        &pack_path,
    )
    .await?;

    insert_alias_option(
        &mut import_map,
        &project_path,
        config.resolve_alias_options(),
        ["browser"],
    )
    .await?;

    Ok(import_map.cell())
}

// Make sure to not add any external requests here.
async fn insert_shared_aliases(
    import_map: &mut ImportMap,
    project_path: &FileSystemPath,
    _execution_context: Vc<ExecutionContext>,
    _config: Vc<Config>,
    pack_path: &FileSystemPath,
) -> Result<()> {
    import_map.insert_singleton_alias("@swc/helpers", pack_path.join("node_modules/@swc/helpers")?);
    import_map.insert_singleton_alias(
        UTOO_STYLE_LOADER,
        pack_path.join(&format!("node_modules/{UTOO_STYLE_LOADER}"))?,
    );
    // import_map.insert_singleton_alias("styled-jsx", pack_package.clone());
    import_map.insert_singleton_alias("react", project_path.clone());
    import_map.insert_singleton_alias("react-dom", project_path.clone());

    insert_package_alias(
        import_map,
        rcstr!("@vercel/turbopack-ecmascript-runtime/"),
        turbopack_ecmascript_runtime::embed_fs()
            .root()
            .owned()
            .await?,
    );

    insert_package_alias(
        import_map,
        rcstr!("@utoo/pack-runtime/"),
        embed_js::embed_fs().root().owned().await?,
    );

    Ok(())
}

pub async fn insert_alias_option<const N: usize>(
    import_map: &mut ImportMap,
    project_path: &FileSystemPath,
    alias_options: Vc<ResolveAliasMap>,
    conditions: [&'static str; N],
) -> Result<()> {
    let conditions = BTreeMap::from(conditions.map(|c| (c.into(), ConditionValue::Set)));
    for (alias, value) in &alias_options.await? {
        if let Some(mapping) = export_value_to_import_mapping(value, &conditions, project_path) {
            import_map.insert_alias(alias, mapping);
        }
    }
    Ok(())
}

fn export_value_to_import_mapping(
    value: &SubpathValue,
    conditions: &BTreeMap<RcStr, ConditionValue>,
    project_path: &FileSystemPath,
) -> Option<ResolvedVc<ImportMapping>> {
    let mut result = Vec::new();
    value.add_results(
        conditions,
        &ConditionValue::Unset,
        &mut FxHashMap::default(),
        &mut result,
    );
    if result.is_empty() {
        None
    } else {
        Some(if result.len() == 1 {
            let relative_import =
                convert_to_project_relative(result[0].0, &project_path.path).ok()?;
            ImportMapping::PrimaryAlternative(relative_import, Some(project_path.clone()))
                .resolved_cell()
        } else {
            ImportMapping::Alternatives(
                result
                    .iter()
                    .filter_map(|(m, _)| {
                        let relative_import =
                            convert_to_project_relative(m, &project_path.path).ok()?;
                        Some(
                            ImportMapping::PrimaryAlternative(
                                relative_import,
                                Some(project_path.clone()),
                            )
                            .resolved_cell(),
                        )
                    })
                    .collect(),
            )
            .resolved_cell()
        })
    }
}

#[allow(dead_code)]
fn insert_exact_alias_map(
    import_map: &mut ImportMap,
    project_path: FileSystemPath,
    map: FxIndexMap<&'static str, String>,
) {
    for (pattern, request) in map {
        let request_rcstr: RcStr = request.into();
        import_map.insert_exact_alias(
            pattern,
            request_to_import_mapping(project_path.clone(), &request_rcstr),
        );
    }
}

#[allow(dead_code)]
fn insert_wildcard_alias_map(
    import_map: &mut ImportMap,
    project_path: FileSystemPath,
    map: FxIndexMap<&'static str, String>,
) {
    for (pattern, request) in map {
        let request_rcstr: RcStr = request.into();
        import_map.insert_wildcard_alias(
            pattern,
            request_to_import_mapping(project_path.clone(), &request_rcstr),
        );
    }
}

/// Inserts an alias to an alternative of import mappings into an import map.
#[allow(dead_code)]
fn insert_alias_to_alternatives<'a>(
    import_map: &mut ImportMap,
    alias: impl Into<String> + 'a,
    alternatives: Vec<ResolvedVc<ImportMapping>>,
) {
    import_map.insert_exact_alias(
        alias.into(),
        ImportMapping::Alternatives(alternatives).resolved_cell(),
    );
}

#[allow(dead_code)]
/// Inserts an alias to an import mapping into an import map.
fn insert_package_alias(import_map: &mut ImportMap, prefix: RcStr, package_root: FileSystemPath) {
    import_map.insert_wildcard_alias(
        prefix,
        ImportMapping::PrimaryAlternative(rcstr!("./*"), Some(package_root)).resolved_cell(),
    );
}

pub fn get_client_resolved_map(
    _context: FileSystemPath,
    _root: FileSystemPath,
    _mode: Mode,
) -> Vc<ResolvedMap> {
    let glob_mappings = vec![];
    ResolvedMap {
        by_glob: glob_mappings,
    }
    .cell()
}

/// Creates a direct import mapping to the result of resolving a request
/// in a context.
fn request_to_import_mapping(
    context_path: FileSystemPath,
    request: &RcStr,
) -> ResolvedVc<ImportMapping> {
    ImportMapping::PrimaryAlternative(request.clone(), Some(context_path)).resolved_cell()
}

/// Creates a direct import mapping to the result of resolving an external
/// request.
#[allow(dead_code)]
fn external_request_to_cjs_import_mapping(
    context_dir: FileSystemPath,
    request: &str,
) -> ResolvedVc<ImportMapping> {
    ImportMapping::PrimaryAlternativeExternal {
        name: Some(request.into()),
        ty: ExternalType::CommonJs,
        traced: ExternalTraced::Traced,
        lookup_dir: context_dir,
    }
    .resolved_cell()
}

/// Creates a direct import mapping to the result of resolving an external
/// request.
#[allow(dead_code)]
fn external_request_to_esm_import_mapping(
    context_dir: FileSystemPath,
    request: &str,
) -> ResolvedVc<ImportMapping> {
    ImportMapping::PrimaryAlternativeExternal {
        name: Some(request.into()),
        ty: ExternalType::EcmaScriptModule,
        traced: ExternalTraced::Traced,
        lookup_dir: context_dir,
    }
    .resolved_cell()
}
