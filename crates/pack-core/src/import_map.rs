use std::collections::BTreeMap;

use anyhow::Result;
use rustc_hash::FxHashMap;
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{FxIndexMap, ResolvedVc, Vc};
use turbo_tasks_fs::{FileSystem, FileSystemPath};
use turbopack_core::resolve::{
    ExternalTraced, ExternalType, ResolveAliasMap, SubpathValue,
    options::{ConditionValue, ImportMap, ImportMapping},
};
use turbopack_node::execution_context::ExecutionContext;

use crate::{config::Config, embed_js, util::convert_to_project_relative};

pub fn mdx_import_source_file() -> RcStr {
    unreachable!()
}

#[turbo_tasks::function]
#[allow(unused_variables)]
pub async fn get_postcss_package_mapping() -> Result<Vc<ImportMapping>> {
    Ok(ImportMapping::External(
        Some(rcstr!("postcss")),
        ExternalType::CommonJs,
        ExternalTraced::Untraced,
    )
    .cell())
}

// Make sure to not add any external requests here.
pub async fn insert_shared_aliases(
    import_map: &mut ImportMap,
    project_path: &FileSystemPath,
    _execution_context: Vc<ExecutionContext>,
    _config: Vc<Config>,
    pack_path: &FileSystemPath,
) -> Result<()> {
    import_map.insert_singleton_alias("@swc/helpers", pack_path.join("node_modules/@swc/helpers")?);
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
        rcstr!("@vercel/turbopack-node/"),
        turbopack_node::embed_js::embed_fs().root().owned().await?,
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
