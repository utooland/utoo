use crate::embed_js;
use anyhow::Result;
use turbo_rcstr::RcStr;
use turbo_tasks::Vc;
use turbopack_core::{code_builder::Code, context::AssetContext};
use turbopack_ecmascript::StaticEcmascriptCode;

#[turbo_tasks::function]
pub(crate) async fn embed_static_code(
    asset_context: Vc<Box<dyn AssetContext>>,
    path: RcStr,
    generate_source_map: bool,
) -> Result<Vc<Code>> {
    Ok(StaticEcmascriptCode::new(
        asset_context,
        embed_js::embed_file_path(path).owned().await?,
        generate_source_map,
    )
    .code())
}
