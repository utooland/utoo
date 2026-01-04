use std::path::{MAIN_SEPARATOR, Path};

use anyhow::{Context, Result};
use bincode::{Decode, Encode};
use dunce::{canonicalize, simplified};
use serde::Deserialize;
use turbo_rcstr::RcStr;
use turbo_tasks::{NonLocalValue, TaskInput, Vc, trace::TraceRawVcs};
use turbo_tasks_fs::FileSystem;
use turbopack::{condition::ContextCondition, module_options::RuleCondition};

use crate::config::Config;

#[derive(
    Default,
    PartialEq,
    Eq,
    Clone,
    Copy,
    Debug,
    TraceRawVcs,
    Deserialize,
    Hash,
    PartialOrd,
    Ord,
    TaskInput,
    NonLocalValue,
    Encode,
    Decode,
)]
#[serde(rename_all = "lowercase")]
pub enum Runtime {
    #[default]
    NodeJs,
    #[serde(alias = "experimental-edge")]
    Edge,
}

impl Runtime {
    pub fn conditions(&self) -> &'static [&'static str] {
        match self {
            Runtime::NodeJs => &["node"],
            Runtime::Edge => &["edge-light"],
        }
    }
}

#[turbo_tasks::function]
pub async fn get_transpiled_packages(config: Vc<Config>) -> Result<Vc<Vec<RcStr>>> {
    let transpile_packages: Vec<RcStr> = config
        .optimization()
        .await?
        .transpile_packages
        .clone()
        .unwrap_or_default();

    Ok(Vc::cell(transpile_packages))
}

pub async fn foreign_code_context_condition(config: Vc<Config>) -> Result<ContextCondition> {
    let transpiled_packages = get_transpiled_packages(config).await?;

    let result = ContextCondition::all(vec![
        ContextCondition::InDirectory("node_modules".to_string()),
        ContextCondition::not(ContextCondition::any(
            transpiled_packages
                .iter()
                .map(|package| ContextCondition::InDirectory(format!("node_modules/{package}")))
                .collect(),
        )),
    ]);
    Ok(result)
}

/// Determines if the module is an internal asset (i.e overlay, fallback) coming from the embedded
/// FS, don't apply user defined transforms.
//
// TODO: Turbopack specific embed fs paths should be handled by internals of Turbopack itself and
// user config should not try to leak this. However, currently we apply few transform options
// subject to Next.js's configuration even if it's embedded assets.
pub async fn internal_assets_conditions() -> Result<ContextCondition> {
    Ok(ContextCondition::any(vec![
        ContextCondition::InPath(
            turbopack_ecmascript_runtime::embed_fs()
                .root()
                .owned()
                .await?,
        ),
        ContextCondition::InPath(turbopack_node::embed_js::embed_fs().root().owned().await?),
    ]))
}

pub fn convert_to_project_relative(project_inside_path: &str, project_path: &str) -> Result<RcStr> {
    if project_inside_path.starts_with(MAIN_SEPARATOR) {
        pathdiff::diff_paths(
            simplified(Path::new(project_inside_path)),
            canonicalize(if project_path.starts_with(MAIN_SEPARATOR) {
                project_path.into()
            } else {
                let current_dir = std::env::current_dir().unwrap();
                let project_path = simplified(Path::new(project_path))
                    .to_string_lossy()
                    .to_string();
                if current_dir
                    .to_str()
                    .is_some_and(|c| c.rfind(&project_path).is_some_and(|index| index > 0))
                {
                    current_dir
                } else {
                    current_dir.join(project_path)
                }
            })
            .context(format!(
                r#"failed to canonicalize project path of "{project_path}"#
            ))?
            .to_string_lossy()
            .to_string(),
        )
        .map_or(
            Err(anyhow::Error::msg(
                r#"path: "{project_inside_path}" is out of project: "{project_path}"#,
            )),
            |p| {
                use std::path::Component;

                if p.as_os_str().is_empty() {
                    return Ok(".".into());
                }

                // Prepend "./" to relative paths that don't already have a relative prefix.
                let path_with_prefix = match p.components().next() {
                    Some(Component::Normal(_)) => format!("./{}", p.display()),
                    _ => p.to_string_lossy().to_string(),
                };

                Ok(path_with_prefix.into())
            },
        )
    } else {
        Ok(project_inside_path.into())
    }
}

// TODO: add a config to enable autoCssModules
pub fn module_styles_rule_condition() -> RuleCondition {
    RuleCondition::any(vec![
        RuleCondition::ResourcePathEndsWith(".module.css".into()),
        RuleCondition::ResourcePathEndsWith(".module.scss".into()),
        RuleCondition::ResourcePathEndsWith(".module.sass".into()),
        RuleCondition::ResourcePathEndsWith(".module.less".into()),
        RuleCondition::All(vec![
            RuleCondition::ResourceQueryContains("?modules".into()),
            RuleCondition::ResourcePathEndsWith(".less".into()),
        ]),
        RuleCondition::All(vec![
            RuleCondition::ResourceQueryContains("?modules".into()),
            RuleCondition::ResourcePathEndsWith(".css".into()),
        ]),
        RuleCondition::All(vec![
            RuleCondition::ResourceQueryContains("?modules".into()),
            RuleCondition::ResourcePathEndsWith(".sass".into()),
        ]),
        RuleCondition::All(vec![
            RuleCondition::ResourceQueryContains("?modules".into()),
            RuleCondition::ResourcePathEndsWith(".scss".into()),
        ]),
        RuleCondition::ContentTypeStartsWith("text/css+module".into()),
        RuleCondition::ContentTypeStartsWith("text/sass+module".into()),
        RuleCondition::ContentTypeStartsWith("text/scss+module".into()),
        RuleCondition::ContentTypeStartsWith("text/less+module".into()),
    ])
}
