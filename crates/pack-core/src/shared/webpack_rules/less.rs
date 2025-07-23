use std::mem::take;

use anyhow::{Result, bail};
use serde_json::Value as JsonValue;
use turbo_rcstr::rcstr;
use turbo_tasks::{ResolvedVc, Vc};
use turbo_tasks_fs::FileSystemPath;
use turbopack::module_options::{LoaderRuleItem, OptionWebpackRules, WebpackRules};
use turbopack_node::transforms::webpack::WebpackLoaderItem;

use crate::import_map::get_utoopack_dependency_package;

#[turbo_tasks::function]
pub async fn maybe_add_less_loader(
    project_path: FileSystemPath,
    less_options: Vc<JsonValue>,
    webpack_rules: Option<Vc<WebpackRules>>,
) -> Result<Vc<OptionWebpackRules>> {
    let less_options = less_options.await?;
    let Some(less_options) = less_options.as_object().cloned() else {
        bail!("less_options must be an object");
    };

    let mut rules = if let Some(webpack_rules) = webpack_rules {
        webpack_rules.owned().await?
    } else {
        Default::default()
    };
    for (pattern, rename) in [("*.module.less", ".module.css"), ("*.less", ".css")] {
        // additionalData is a loader option but Next.js has it under `lessOptions` in
        // `config.js`
        let empty_additional_data = serde_json::Value::String("".to_string());
        let additional_data = less_options.get("prependData").or(less_options
            .get("additionalData")
            .or(Some(&empty_additional_data)));
        let rule = rules.get_mut(pattern);
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

        if let Some(rule) = rule {
            // Without `as`, loader result would be JS code, so we don't want to apply
            // less-loader on that.
            let Some(rename_as) = rule.rename_as.as_ref() else {
                continue;
            };
            // Only when the result should run through the less pipeline, we apply
            // less-loader.

            if rename_as != "*" {
                continue;
            }
            let mut loaders = rule.loaders.owned().await?;
            loaders.push(less_loader);
            rule.loaders = ResolvedVc::cell(loaders);
        } else {
            rules.insert(
                pattern.into(),
                LoaderRuleItem {
                    loaders: ResolvedVc::cell(vec![less_loader]),
                    rename_as: Some(format!("*{rename}").into()),
                },
            );
        }
    }

    Ok(Vc::cell(Some(ResolvedVc::cell(rules))))
}
