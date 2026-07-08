use std::mem::take;

use anyhow::{Result, bail};
use serde_json::Value as JsonValue;
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{ResolvedVc, Vc};
use turbopack::module_options::LoaderRuleItem;
use turbopack_node::transforms::webpack::WebpackLoaderItem;

fn get_less_loader_name(less_options: &mut serde_json::Map<String, JsonValue>) -> Result<RcStr> {
    match less_options.remove("loader") {
        Some(JsonValue::String(loader)) if !loader.is_empty() => Ok(RcStr::from(loader)),
        Some(JsonValue::Null) | None => Ok(rcstr!("less-loader")),
        Some(_) => bail!("styles.less.loader must be a non-empty string"),
    }
}

pub async fn get_less_loader_rules(
    less_options: Vc<JsonValue>,
) -> Result<Vec<(RcStr, LoaderRuleItem)>> {
    let less_options = less_options.await?;
    let Some(mut less_options) = less_options.as_object().cloned() else {
        bail!("less_options must be an object");
    };
    let loader = get_less_loader_name(&mut less_options)?;

    // additionalData is a loader option but utoopack has it under `lessOptions` in
    // `project.json`
    let empty_additional_data = serde_json::Value::String("".to_string());
    let additional_data = less_options.get("prependData").or(less_options
        .get("additionalData")
        .or(Some(&empty_additional_data)));

    let less_loader = WebpackLoaderItem {
        loader,
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
                module_type: None,
            },
        ));
    }

    Ok(rules)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn defaults_to_less_loader_package_name() {
        let mut less_options = serde_json::Map::new();

        let loader = get_less_loader_name(&mut less_options).unwrap();

        assert_eq!(loader.to_string(), "less-loader");
    }

    #[test]
    fn treats_null_less_loader_as_default() {
        let mut less_options = json!({
            "loader": null
        })
        .as_object()
        .unwrap()
        .clone();

        let loader = get_less_loader_name(&mut less_options).unwrap();

        assert_eq!(loader.to_string(), "less-loader");
    }

    #[test]
    fn uses_configured_less_loader_path_and_removes_it_from_options() {
        let mut less_options = json!({
            "loader": "/repo/node_modules/less-loader/dist/cjs.js",
            "javascriptEnabled": true
        })
        .as_object()
        .unwrap()
        .clone();

        let loader = get_less_loader_name(&mut less_options).unwrap();

        assert_eq!(
            loader.to_string(),
            "/repo/node_modules/less-loader/dist/cjs.js"
        );
        assert!(!less_options.contains_key("loader"));
        assert_eq!(less_options.get("javascriptEnabled"), Some(&json!(true)));
    }

    #[test]
    fn rejects_invalid_less_loader_path() {
        let mut less_options = json!({
            "loader": false
        })
        .as_object()
        .unwrap()
        .clone();

        let err = get_less_loader_name(&mut less_options).unwrap_err();

        assert_eq!(
            err.to_string(),
            "styles.less.loader must be a non-empty string"
        );
    }
}
