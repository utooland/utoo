use std::collections::BTreeMap;

use anyhow::{Context, Result};
use rustc_hash::FxHashMap;
use turbo_rcstr::RcStr;
use turbo_tasks::{FxIndexMap, ResolvedVc, Vc};
use turbo_tasks_fs::{FileSystem, FileSystemPath};
use turbopack_core::{
    reference_type::{CommonJsReferenceSubType, ReferenceType},
    resolve::{
        ExternalTraced, ExternalType, ResolveAliasMap, SubpathValue,
        node::node_cjs_resolve_options,
        options::{ConditionValue, ImportMap, ImportMapping, ResolvedMap},
        parse::Request,
        pattern::Pattern,
        resolve,
    },
    source::Source,
};
use turbopack_node::execution_context::ExecutionContext;

use crate::{config::Config, mode::Mode, util::convert_to_project_relative};

pub fn mdx_import_source_file() -> RcStr {
    unreachable!()
}

#[turbo_tasks::function]
pub async fn get_postcss_package_mapping() -> Result<Vc<ImportMapping>> {
    Ok(ImportMapping::Alternatives(vec![
        ImportMapping::PrimaryAlternative("postcss".into(), None).resolved_cell(),
    ])
    .cell())
}

/// Computes the  client fallback import map, which provides
/// polyfills to Node.js externals.
#[turbo_tasks::function]
pub async fn get_client_fallback_import_map() -> Result<Vc<ImportMap>> {
    let import_map = ImportMap::empty();

    // insert_package_alias(
    //     &mut import_map,
    //     "@utoo/turbopack-ecmascript-runtime/",
    //     turbopack_ecmascript_runtime::embed_fs()
    //         .root()
    //         .to_resolved()
    //         .await?,
    // );

    Ok(import_map.cell())
}

// Make sure to not add any external requests here.
/// Computes the client import map.
#[turbo_tasks::function]
pub async fn get_client_import_map(
    project_path: FileSystemPath,
    config: Vc<Config>,
    execution_context: Vc<ExecutionContext>,
) -> Result<Vc<ImportMap>> {
    let mut import_map = ImportMap::empty();

    insert_shared_aliases(
        &mut import_map,
        project_path.clone(),
        execution_context,
        config,
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
    _project_path: FileSystemPath,
    _execution_context: Vc<ExecutionContext>,
    _config: Vc<Config>,
) -> Result<()> {
    // FIXME: maybe we don't need this
    // let pack_package = get_pack_package(project_path.clone()).await?.clone_value();
    // import_map.insert_singleton_alias("@swc/helpers", pack_package.clone());
    // import_map.insert_singleton_alias("styled-jsx", pack_package.clone());
    // import_map.insert_singleton_alias("react", project_path.clone());
    // import_map.insert_singleton_alias("react-dom", project_path.clone());
    //
    insert_package_alias(
        import_map,
        "@vercel/turbopack-ecmascript-runtime/",
        turbopack_ecmascript_runtime::embed_fs()
            .root()
            .await?
            .clone_value(),
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
        import_map.insert_exact_alias(
            pattern,
            request_to_import_mapping(project_path.clone(), &request),
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
        import_map.insert_wildcard_alias(
            pattern,
            request_to_import_mapping(project_path.clone(), &request),
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

/// Inserts an alias to an import mapping into an import map.
#[allow(dead_code)]
fn insert_package_alias(import_map: &mut ImportMap, prefix: &str, package_root: FileSystemPath) {
    import_map.insert_wildcard_alias(
        prefix,
        ImportMapping::PrimaryAlternative("./*".into(), Some(package_root)).resolved_cell(),
    );
}

#[turbo_tasks::function]
pub async fn get_pack_package(context_directory: FileSystemPath) -> Result<Vc<FileSystemPath>> {
    let result = resolve(
        context_directory.clone(),
        ReferenceType::CommonJs(CommonJsReferenceSubType::Undefined),
        Request::parse(Pattern::Constant("@utoo/pack/package.json".into())),
        node_cjs_resolve_options(context_directory.root().await?.clone_value()),
    );
    let source = result
        .first_source()
        .await?
        .context("@utoo/pack package not found")?;
    Ok(source.ident().path().await?.parent().cell())
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
#[allow(dead_code)]
fn request_to_import_mapping(
    context_path: FileSystemPath,
    request: &str,
) -> ResolvedVc<ImportMapping> {
    ImportMapping::PrimaryAlternative(request.into(), Some(context_path)).resolved_cell()
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
