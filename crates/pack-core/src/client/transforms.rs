use anyhow::Result;
use turbo_tasks::Vc;
use turbopack::module_options::ModuleRule;

use crate::{
    config::Config,
    shared::transforms::{
        get_image_rule, get_wasm_rule, modularize_imports::get_modularize_imports_rule,
    },
};

pub async fn get_client_transforms_rules(config: Vc<Config>) -> Result<Vec<ModuleRule>> {
    let mut rules = vec![];

    let optimization_config = config.optimization().await?;
    let modularize_imports_config = &optimization_config
        .modularize_imports
        .clone()
        .unwrap_or_default();
    let wasm_as_asset = optimization_config.wasm_as_asset.unwrap_or(false);

    let image_config = config.image_config().await?;

    if !modularize_imports_config.is_empty() {
        rules.push(get_modularize_imports_rule(modularize_imports_config));
    }

    if let Some(image_config) = &*image_config {
        rules.push(get_image_rule(image_config.inline_limit.or(Some(10_000))).await?);
    }

    if wasm_as_asset {
        rules.push(get_wasm_rule().await?);
    }

    Ok(rules)
}
