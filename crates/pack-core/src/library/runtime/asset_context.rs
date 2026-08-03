use turbo_rcstr::rcstr;
use turbo_tasks::{Result, Vc};
use turbopack::{
    ModuleAssetContext,
    module_options::{EcmascriptOptionsContext, ModuleOptionsContext, TypescriptTransformOptions},
};
use turbopack_core::{
    compile_time_info::CompileTimeInfo, context::AssetContext, environment::Environment,
    ident::Layer,
};

/// Returns the runtime asset context to use to process runtime code assets.
#[turbo_tasks::function]
pub async fn get_runtime_asset_context(
    environment: Vc<Environment>,
) -> Result<Vc<Box<dyn AssetContext>>> {
    let module_options_context = ModuleOptionsContext {
        ecmascript: EcmascriptOptionsContext {
            enable_typescript_transform: Some(
                TypescriptTransformOptions::default().resolved_cell(),
            ),
            ignore_dynamic_requests: true,
            ..Default::default()
        },
        environment: Some(environment.to_resolved().await?),
        follow_reexports: true,
        module_fragments_enabled: false,
        ..Default::default()
    }
    .cell();
    let compile_time_info = CompileTimeInfo::builder(environment.to_resolved().await?)
        .cell()
        .await?;

    let asset_context: Vc<Box<dyn AssetContext>> = Vc::upcast(ModuleAssetContext::new(
        Default::default(),
        compile_time_info,
        module_options_context,
        Vc::default(),
        Layer::new(rcstr!("runtime")),
    ));

    Ok(asset_context)
}
