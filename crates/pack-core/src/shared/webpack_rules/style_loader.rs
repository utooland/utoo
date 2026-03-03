use anyhow::Result;
use turbo_rcstr::RcStr;
use turbo_tasks::Vc;
use turbopack::module_options::LoaderRuleItem;

use crate::config::OptionalJsonValue;

/// Returns an empty list of loader rules since inline CSS is now handled
/// natively in Rust via [`InlineCssModuleType`](crate::shared::transforms::inline_css::InlineCssModuleType).
pub async fn get_style_loader_rules(
    _inline_css: Vc<OptionalJsonValue>,
) -> Result<Vec<(RcStr, LoaderRuleItem)>> {
    Ok(Vec::new())
}
