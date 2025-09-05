use std::mem::take;

use anyhow::{Result, bail};
use serde_json::Value as JsonValue;
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{ResolvedVc, Vc};
use turbo_tasks_fs::FileSystemPath;
use turbopack::module_options::LoaderRuleItem;
use turbopack_node::transforms::webpack::WebpackLoaderItem;

use crate::import_map::get_utoopack_dependency_package;

pub async fn get_less_loader_rules(
    project_path: FileSystemPath,
    less_options: Vc<JsonValue>,
) -> Result<Vec<(RcStr, LoaderRuleItem)>> {
    let less_options = less_options.await?;
    let Some(less_options) = less_options.as_object().cloned() else {
        bail!("less_options must be an object");
    };

    // additionalData is a loader option but utoopack has it under `lessOptions` in
    // `project.json`
    let empty_additional_data = serde_json::Value::String("".to_string());
    let additional_data = less_options.get("prependData").or(less_options
        .get("additionalData")
        .or(Some(&empty_additional_data)));

    let less_loader = WebpackLoaderItem {
        loader: get_utoopack_dependency_package(project_path.clone(), rcstr!("less-loader"))
            .owned()
            .await?,
        options: take(
            serde_json::json!({
                "implementation": less_options.get("implementation"),
                "sourceMap": true,
                "lessOptions": less_options,
                "additionalData": additional_data
            })
            .as_object_mut()
            .unwrap(),
        ),
    };

    let loaders = ResolvedVc::cell(vec![less_loader]);
    let mut rules = Vec::new();

    for (pattern, rename) in [
        (rcstr!("*.module.less"), rcstr!("*.module.css")),
        (rcstr!("*.less"), rcstr!("*.css")),
    ] {
        rules.push((
            pattern,
            LoaderRuleItem {
                loaders,
                rename_as: Some(rename),
                condition: None,
            },
        ));
    }

    Ok(rules)
}
