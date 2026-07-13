use anyhow::Result;
use turbo_tasks::{ResolvedVc, Vc};
use turbopack::module_options::ModuleRule;
use turbopack_core::source_transform::SourceTransform;

use crate::{
    config::Config,
    shared::transforms::{
        get_image_rule, get_inline_css_rule, get_wasm_rule, inline_css::module::InjectType,
        modularize_imports::get_modularize_imports_rule,
    },
};

pub async fn get_client_transforms_rules(
    config: Vc<Config>,
    foreign_code: bool,
    postcss_transform: Option<ResolvedVc<Box<dyn SourceTransform>>>,
) -> Result<Vec<ModuleRule>> {
    let mut rules = vec![];

    let optimization_config = config.optimization().await?;
    let modularize_imports_config = &optimization_config
        .modularize_imports
        .clone()
        .unwrap_or_default();
    let wasm_as_asset = optimization_config.wasm_as_asset.unwrap_or(false);

    let image_config = config.image_config().await?;

    if !foreign_code && !modularize_imports_config.is_empty() {
        rules.push(get_modularize_imports_rule(modularize_imports_config));
    }

    if let Some(image_config) = &*image_config {
        rules.push(get_image_rule(image_config.inline_limit.or(Some(10_000))).await?);
    }

    if wasm_as_asset {
        rules.push(get_wasm_rule().await?);
    }

    if let Some(inline_css_options) = &*config.inline_css().await?
        && let Some(obj) = inline_css_options.as_object()
    {
        let insert = obj.get("insert").and_then(|v| v.as_str()).unwrap_or("head");
        let inject_type = obj
            .get("injectType")
            .and_then(|v| v.as_str())
            .map(InjectType::from_str)
            .unwrap_or(InjectType::Style);
        let css_modules_pattern = config
            .styles()
            .await?
            .css_modules
            .as_ref()
            .and_then(|css_modules| css_modules.local_ident_pattern());
        rules.push(
            get_inline_css_rule(
                insert.into(),
                inject_type,
                css_modules_pattern,
                postcss_transform,
            )
            .await?,
        );
    }

    Ok(rules)
}
