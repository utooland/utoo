use anyhow::Result;
use turbo_rcstr::rcstr;
use turbo_tasks::{ResolvedVc, Vc, fxindexmap};
use turbopack::{ModuleAssetContext, module_options::CustomModuleType};
use turbopack_core::{
    context::AssetContext, module::Module, reference_type::ReferenceType, source::Source,
};
use turbopack_ecmascript::EcmascriptInputTransforms;
use turbopack_static::ecma::StaticUrlJsModule;

use super::source_asset::StaticWasmFileSource;

/// Module type that outputs WASM files as static assets and exports their URL.
#[turbo_tasks::value]
pub struct StaticWasmModuleType;

#[turbo_tasks::value_impl]
impl StaticWasmModuleType {
    #[turbo_tasks::function]
    pub(crate) async fn create_module(
        source: ResolvedVc<Box<dyn Source>>,
        module_asset_context: ResolvedVc<ModuleAssetContext>,
    ) -> Result<Vc<Box<dyn Module>>> {
        let static_asset = StaticUrlJsModule::new(*source, Some(rcstr!("wasm")))
            .to_resolved()
            .await?;
        Ok(module_asset_context
            .process(
                Vc::upcast(StaticWasmFileSource { wasm: source }.cell()),
                ReferenceType::Internal(ResolvedVc::cell(fxindexmap!(
                    rcstr!("WASM") => ResolvedVc::upcast(static_asset)
                ))),
            )
            .module())
    }

    #[turbo_tasks::function]
    pub fn new() -> Vc<Self> {
        StaticWasmModuleType::cell(StaticWasmModuleType)
    }
}

#[turbo_tasks::value_impl]
impl CustomModuleType for StaticWasmModuleType {
    #[turbo_tasks::function]
    fn create_module(
        self: Vc<Self>,
        source: Vc<Box<dyn Source>>,
        module_asset_context: Vc<ModuleAssetContext>,
        _reference_type: ReferenceType,
    ) -> Vc<Box<dyn Module>> {
        StaticWasmModuleType::create_module(source, module_asset_context)
    }

    #[turbo_tasks::function]
    fn extend_ecmascript_transforms(
        self: Vc<Self>,
        _preprocess: Vc<EcmascriptInputTransforms>,
        _main: Vc<EcmascriptInputTransforms>,
        _postprocess: Vc<EcmascriptInputTransforms>,
    ) -> Result<Vc<Box<dyn CustomModuleType>>> {
        Ok(Vc::upcast(self))
    }
}
