use anyhow::Result;
use turbo_tasks::Vc;
use turbopack::module_options::ModuleRule;
use turbopack_core::environment::RuntimeVersions;
use turbopack_ecmascript_plugins::transform::styled_jsx::StyledJsxTransformer;

use super::get_ecma_transform_rule;
use crate::{config::Config, shared::transforms::EcmascriptTransformStage};

/// Returns a transform rule for the styled jsx transform.
pub async fn get_styled_jsx_transform_rule(
    config: Vc<Config>,
    target_browsers: Vc<RuntimeVersions>,
) -> Result<Option<ModuleRule>> {
    let enable_mdx_rs = config.mdx_rs().await?.is_some();
    let versions = *target_browsers.await?;

    let transformer = StyledJsxTransformer::new(versions);
    Ok(Some(get_ecma_transform_rule(
        Box::new(transformer),
        enable_mdx_rs,
        EcmascriptTransformStage::Main,
    )))
}
