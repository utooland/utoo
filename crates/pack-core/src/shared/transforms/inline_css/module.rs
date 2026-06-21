use anyhow::Result;
use bincode::{Decode, Encode};
use turbo_rcstr::RcStr;
use turbo_tasks::{ResolvedVc, Vc, trace::TraceRawVcs};
use turbopack::{ModuleAssetContext, module_options::CustomModuleType};
use turbopack_core::{
    context::AssetContext, module::Module, reference_type::ReferenceType, source::Source,
};
use turbopack_ecmascript::EcmascriptInputTransforms;

use super::source_asset::InlineCssFileSource;

#[turbo_tasks::task_input]
#[derive(Eq, PartialEq, Clone, Copy, Debug, PartialOrd, Ord, Hash, TraceRawVcs, Encode, Decode)]
pub enum InjectType {
    Style,
    SingletonStyle,
    Link,
    LazyStyle,
    LazySingletonStyle,
}

impl InjectType {
    pub fn from_str(s: &str) -> Self {
        match s {
            "singletonStyleTag" => Self::SingletonStyle,
            "linkTag" => Self::Link,
            "lazyStyleTag" => Self::LazyStyle,
            "lazySingletonStyleTag" => Self::LazySingletonStyle,
            _ => Self::Style,
        }
    }
}

/// Custom module type that transforms CSS files into JavaScript modules
/// that inject styles into the DOM at runtime.
#[turbo_tasks::value]
pub struct InlineCssModuleType {
    pub insert: RcStr,
    pub inject_type: InjectType,
    pub minify: bool,
}

#[turbo_tasks::value_impl]
impl InlineCssModuleType {
    #[turbo_tasks::function]
    pub fn new(insert: RcStr, inject_type: InjectType, minify: bool) -> Vc<Self> {
        InlineCssModuleType {
            insert,
            inject_type,
            minify,
        }
        .cell()
    }

    #[turbo_tasks::function]
    pub(crate) fn create_module(
        source: ResolvedVc<Box<dyn Source>>,
        module_asset_context: ResolvedVc<ModuleAssetContext>,
        insert: RcStr,
        inject_type: InjectType,
        minify: bool,
    ) -> Vc<Box<dyn Module>> {
        let asset_context = ResolvedVc::upcast(module_asset_context);
        module_asset_context
            .process(
                Vc::upcast(
                    InlineCssFileSource {
                        css: source,
                        asset_context,
                        insert,
                        inject_type,
                        minify,
                    }
                    .cell(),
                ),
                ReferenceType::Undefined,
            )
            .module()
    }
}

#[turbo_tasks::value_impl]
impl CustomModuleType for InlineCssModuleType {
    #[turbo_tasks::function]
    fn create_module(
        &self,
        source: Vc<Box<dyn Source>>,
        module_asset_context: Vc<ModuleAssetContext>,
        _reference_type: ReferenceType,
    ) -> Vc<Box<dyn Module>> {
        InlineCssModuleType::create_module(
            source,
            module_asset_context,
            self.insert.clone(),
            self.inject_type,
            self.minify,
        )
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
