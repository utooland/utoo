use std::mem::take;

use anyhow::{Result, bail};
use serde_json::Value as JsonValue;
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{ResolvedVc, Vc};
use turbopack::module_options::LoaderRuleItem;
use turbopack_node::transforms::webpack::WebpackLoaderItem;

pub async fn get_sass_loader_rules(
    sass_options: Vc<JsonValue>,
) -> Result<Vec<(RcStr, LoaderRuleItem)>> {
    let sass_options = sass_options.await?;
    let Some(mut sass_options) = sass_options.as_object().cloned() else {
        bail!("sass_options must be an object");
    };

    // TODO: Remove this once we upgrade to sass-loader 16
    let silence_deprecations = if let Some(v) = sass_options.get("silenceDeprecations") {
        v.clone()
    } else {
        serde_json::json!(["legacy-js-api"])
    };

    let empty_additional_data = serde_json::Value::String("".to_string());
    sass_options.insert("silenceDeprecations".into(), silence_deprecations);
    // additionalData is a loader option but utoopack has it under `sassOptions` in
    // `project.json`
    let additional_data = sass_options
        .get("prependData")
        .or(sass_options.get("additionalData"))
        .or(Some(&empty_additional_data));

    let sass_loader = WebpackLoaderItem {
        loader: rcstr!("sass-loader"),
        options: take(
            serde_json::json!({
                "implementation": sass_options.get("implementation"),
                "sourceMap": true,
                "sassOptions": sass_options,
                "additionalData": additional_data
            })
            .as_object_mut()
            .unwrap(),
        ),
    };
    let resolve_url_loader = WebpackLoaderItem {
        loader: rcstr!("resolve-url-loader"),
        options: take(
            serde_json::json!({
                //https://github.com/vercel/turbo/blob/d527eb54be384a4658243304cecd547d09c05c6b/crates/turbopack-node/src/transforms/webpack.rs#L191
                "sourceMap": true
            })
            .as_object_mut()
            .unwrap(),
        ),
    };

    let loaders = ResolvedVc::cell(vec![resolve_url_loader, sass_loader]);

    let mut rules = Vec::new();

    for (pattern, rename) in [
        (rcstr!("*.module.s[ac]ss"), rcstr!("*.module.css")),
        (rcstr!("*.s[ac]ss"), rcstr!("*.css")),
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
