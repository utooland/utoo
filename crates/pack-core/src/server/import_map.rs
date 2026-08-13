use anyhow::Result;
use turbo_tasks::Vc;
use turbo_tasks_fs::FileSystemPath;
use turbopack_core::resolve::options::ImportMap;
use turbopack_node::execution_context::ExecutionContext;

use crate::{
    config::Config,
    import_map::{insert_alias_option, insert_server_reference_aliases, insert_shared_aliases},
};

/// Computes the client fallback import map, which provides
/// polyfills to Node.js externals.
#[turbo_tasks::function]
pub async fn get_server_fallback_import_map() -> Result<Vc<ImportMap>> {
    let import_map = ImportMap::empty();

    // TODO:
    Ok(import_map.cell())
}

// Make sure to not add any external requests here.
/// Computes the client import map.
#[turbo_tasks::function]
pub async fn get_server_import_map(
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
        [],
    )
    .await?;

    insert_alias_option(
        &mut import_map,
        &project_path,
        config.server_resolve_alias_options(),
        [],
    )
    .await?;

    // Auto-register server reference aliases from config
    insert_server_reference_aliases(&mut import_map, &project_path, config).await?;

    Ok(import_map.cell())
}
