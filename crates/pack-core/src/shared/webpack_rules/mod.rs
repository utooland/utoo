use std::{collections::BTreeSet, str::FromStr};

use anyhow::Result;
use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};
use turbo_rcstr::rcstr;
use turbo_tasks::{OperationValue, ResolvedVc, Vc, trace::TraceRawVcs};
use turbo_tasks_fs::FileSystemPath;
use turbopack::module_options::{
    WebpackLoaderBuiltinConditionSet, WebpackLoaderBuiltinConditionSetMatch, WebpackLoadersOptions,
};
use turbopack_core::{
    file_source::FileSource,
    resolve::{ResolveResult, ResolveResultItem, options::ImportMapping},
};

use crate::{
    config::Config,
    embed_js::embed_file_path,
    shared::webpack_rules::{less::get_less_loader_rules, sass::get_sass_loader_rules},
};

pub(crate) mod less;
pub(crate) mod sass;

#[turbo_tasks::task_input]
#[derive(
    Copy,
    Clone,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    TraceRawVcs,
    OperationValue,
    Encode,
    Decode,
)]
#[serde(rename_all = "kebab-case")]
pub enum WebpackLoaderBuiltinCondition {
    /// Treated as always-present.
    Default,
    /// Client-side code.
    Browser,
    /// Code in `node_modules` that should typically not be modified by webpack loaders.
    Foreign,

    // These are provided by mode:
    Development,
    Production,

    // TODO: These are provided by Runtime:
    /// Server code on the Node.js runtime.
    Node,
    /// Server code on the edge runtime.
    EdgeLight,
}

impl WebpackLoaderBuiltinCondition {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Browser => "browser",
            Self::Foreign => "foreign",
            Self::Development => "development",
            Self::Production => "production",
            Self::Node => "node",
            Self::EdgeLight => "edge-light",
        }
    }
}

impl FromStr for WebpackLoaderBuiltinCondition {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "default" => Ok(Self::Default),
            "browser" => Ok(Self::Browser),
            "foreign" => Ok(Self::Foreign),
            "development" => Ok(Self::Development),
            "production" => Ok(Self::Production),
            "node" => Ok(Self::Node),
            "edge-light" => Ok(Self::EdgeLight),
            _ => Err(()),
        }
    }
}

impl PartialEq<WebpackLoaderBuiltinCondition> for &str {
    fn eq(&self, other: &WebpackLoaderBuiltinCondition) -> bool {
        *self == other.as_str()
    }
}

#[turbo_tasks::value]
struct UtooWebpackLoaderBuiltinConditionSet(BTreeSet<WebpackLoaderBuiltinCondition>);
#[turbo_tasks::value_impl]
impl UtooWebpackLoaderBuiltinConditionSet {
    #[turbo_tasks::function]
    fn new(
        conditions: BTreeSet<WebpackLoaderBuiltinCondition>,
    ) -> Vc<Box<dyn WebpackLoaderBuiltinConditionSet>> {
        Vc::upcast::<Box<dyn WebpackLoaderBuiltinConditionSet>>(
            UtooWebpackLoaderBuiltinConditionSet(conditions).cell(),
        )
    }
}

#[turbo_tasks::value_impl]
impl WebpackLoaderBuiltinConditionSet for UtooWebpackLoaderBuiltinConditionSet {
    fn match_condition(&self, condition: &str) -> WebpackLoaderBuiltinConditionSetMatch {
        match WebpackLoaderBuiltinCondition::from_str(condition) {
            Ok(cond) => {
                if self.0.contains(&cond) {
                    WebpackLoaderBuiltinConditionSetMatch::Matched
                } else {
                    WebpackLoaderBuiltinConditionSetMatch::Unmatched
                }
            }
            Err(_) => WebpackLoaderBuiltinConditionSetMatch::Invalid,
        }
    }
}

#[turbo_tasks::value(transparent)]
pub struct OptionWebpackLoadersOptions(Option<ResolvedVc<WebpackLoadersOptions>>);

#[turbo_tasks::function]
pub async fn webpack_loader_options(
    project_path: FileSystemPath,
    config: Vc<Config>,
    builtin_conditions: BTreeSet<WebpackLoaderBuiltinCondition>,
) -> Result<Vc<OptionWebpackLoadersOptions>> {
    let mut rules = config.webpack_rules(project_path.clone()).owned().await?;

    let (mut sass_rules, mut less_rules) = futures::try_join!(
        get_sass_loader_rules(config.sass_config()),
        get_less_loader_rules(config.less_config()),
    )?;

    rules.append(&mut sass_rules);
    rules.append(&mut less_rules);

    if rules.is_empty() {
        return Ok(Vc::cell(None));
    }

    Ok(Vc::cell(Some(
        WebpackLoadersOptions {
            rules: ResolvedVc::cell(rules),
            loader_runner_package: Some(loader_runner_package_mapping().to_resolved().await?),
            builtin_conditions: UtooWebpackLoaderBuiltinConditionSet::new(builtin_conditions)
                .to_resolved()
                .await?,
        }
        .resolved_cell(),
    )))
}

#[turbo_tasks::function]
#[allow(unused_variables)]
async fn loader_runner_package_mapping() -> Result<Vc<ImportMapping>> {
    Ok(ImportMapping::Direct(
        ResolveResult::primary(ResolveResultItem::Source(ResolvedVc::upcast(
            FileSource::new(
                embed_file_path(rcstr!("loader-runner/lib/LoaderRunner.js"))
                    .owned()
                    .await?,
            )
            .to_resolved()
            .await?,
        )))
        .resolved_cell(),
    )
    .cell())
}
