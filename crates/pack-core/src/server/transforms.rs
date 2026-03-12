use anyhow::Result;
use turbo_tasks::Vc;
use turbopack::module_options::ModuleRule;

use crate::{config::Config, shared::transforms::modularize_imports::get_modularize_imports_rule};

pub async fn get_server_transforms_rules(config: Vc<Config>) -> Result<Vec<ModuleRule>> {
    let mut rules = vec![];

    let optimization_config = config.optimization().await?;
    let modularize_imports_config = &optimization_config
        .modularize_imports
        .clone()
        .unwrap_or_default();

    if !modularize_imports_config.is_empty() {
        rules.push(get_modularize_imports_rule(modularize_imports_config));
    }

    Ok(rules)
}
