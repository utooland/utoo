use std::mem::take;

use anyhow::{Result, bail};
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks_fs::FileSystemPath;
use turbopack_node::transforms::webpack::WebpackLoaderItem;

use turbo_tasks::{ResolvedVc, Vc};
use turbopack::module_options::LoaderRuleItem;

use crate::{
    config::OptionalJsonValue,
    import_map::{UTOO_STYLE_LOADER, get_utoopack_dependency_package},
};

pub async fn get_style_loader_rules(
    pack_path: FileSystemPath,
    inline_css: Vc<OptionalJsonValue>,
) -> Result<Vec<(RcStr, LoaderRuleItem)>> {
    let mut rules = Vec::new();

    let Some(inline_css_options) = &*inline_css.await? else {
        return Ok(rules);
    };
    let Some(inline_css_options) = inline_css_options.as_object().cloned() else {
        bail!("inline_css must be an object");
    };

    let style_loader = WebpackLoaderItem {
        loader: get_utoopack_dependency_package(pack_path.clone(), rcstr!(UTOO_STYLE_LOADER))
            .owned()
            .await?,
        options: take(
            serde_json::json!({
                "insert": inline_css_options.get("insert"),
                "injectType": inline_css_options.get("injectType"),
            })
            .as_object_mut()
            .unwrap(),
        ),
    };

    let loaders = ResolvedVc::cell(vec![style_loader]);

    for (pattern, rename) in [(rcstr!("*.css"), rcstr!("*.js"))] {
        rules.push((
            pattern,
            LoaderRuleItem {
                loaders,
                rename_as: Some(rename),
                condition: None,
            },
        ))
    }

    Ok(rules)
}
