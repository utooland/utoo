use anyhow::Result;
use style_loader::maybe_add_style_loader;
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{ResolvedVc, Vc};
use turbo_tasks_fs::FileSystemPath;
use turbopack::module_options::WebpackLoadersOptions;
use turbopack_core::resolve::{ExternalTraced, ExternalType, options::ImportMapping};
use turbopack_core::resolve::{ResolveResult, ResolveResultItem};

use self::less::maybe_add_less_loader;
use self::sass::maybe_add_sass_loader;
use crate::config::Config;
use crate::import_map::get_utoopack_dependency_package;

pub(crate) mod less;
pub(crate) mod sass;
pub(crate) mod style_loader;

pub async fn webpack_loader_options(
    project_path: FileSystemPath,
    config: Vc<Config>,
    conditions_strs: Vec<RcStr>,
) -> Result<Option<ResolvedVc<WebpackLoadersOptions>>> {
    let rules = *config
        .webpack_rules(conditions_strs, project_path.clone())
        .await?;
    let rules =
        *maybe_add_style_loader(project_path.clone(), config.inline_css(), rules.map(|v| *v))
            .await?;
    let rules = *maybe_add_less_loader(
        project_path.clone(),
        config.less_config(),
        rules.map(|v| *v),
    )
    .await?;
    let rules = *maybe_add_sass_loader(
        project_path.clone(),
        config.sass_config(),
        rules.map(|v| *v),
    )
    .await?;

    let conditions = config.webpack_conditions().to_resolved().await?;

    Ok(if let Some(rules) = rules {
        Some(
            WebpackLoadersOptions {
                rules,
                conditions,
                #[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
                loader_runner_package: Some(
                    loader_runner_package_mapping(project_path)
                        .to_resolved()
                        .await?,
                ),
                #[cfg(all(target_family = "wasm", target_os = "unknown"))]
                loader_runner_package: None,
            }
            .resolved_cell(),
        )
    } else {
        None
    })
}

#[turbo_tasks::function]
async fn loader_runner_package_mapping(project_path: FileSystemPath) -> Result<Vc<ImportMapping>> {
    Ok(
        ImportMapping::Direct(ResolveResult::primary(ResolveResultItem::External {
            name: get_utoopack_dependency_package(project_path, rcstr!("loader-runner"))
                .owned()
                .await?,
            ty: ExternalType::CommonJs,
            traced: ExternalTraced::Untraced,
        }))
        .cell(),
    )
}
