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

    let modularize_imports_config = &config
        .optimization()
        .await?
        .modularize_imports
        .clone()
        .unwrap_or_default();
    let inline_wasm = config.optimization().await?.inline_wasm.unwrap_or(true);

    let enable_mdx_rs = config.mdx_rs().await?.is_some();
    let image_config = config.image_config().await?;

    if !modularize_imports_config.is_empty() {
        rules.push(get_modularize_imports_rule(
            modularize_imports_config,
            enable_mdx_rs,
        ));
    }

    if let Some(image_config) = &*image_config {
        rules.push(get_image_rule(image_config.inline_limit.or(Some(10_000))).await?);
    }

    if !inline_wasm {
        rules.push(get_wasm_rule().await?);
    }

    Ok(rules)
}
