use std::path::{MAIN_SEPARATOR, Path};

use anyhow::{Context, Result};
use dunce::{canonicalize, simplified};
use serde::{Deserialize, Serialize};
use turbo_rcstr::RcStr;
use turbo_tasks::{NonLocalValue, TaskInput, Vc, trace::TraceRawVcs};
use turbo_tasks_fs::{FileSystem, FileSystemPath};
use turbopack::condition::ContextCondition;

use crate::config::Config;

#[derive(
    Default,
    PartialEq,
    Eq,
    Clone,
    Copy,
    Debug,
    TraceRawVcs,
    Serialize,
    Deserialize,
    Hash,
    PartialOrd,
    Ord,
    TaskInput,
    NonLocalValue,
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
            |p| Ok(p.to_string_lossy().to_string().into()),
        )
    } else {
        Ok(project_inside_path.into())
    }
}

// issue: https://github.com/umijs/mako/issues/2081
// issue: https://github.com/vercel/next.js/issues/82106
pub fn resolve_loader_path(loader_name: &str, project_dir: &FileSystemPath) -> RcStr {
    if loader_name.starts_with("./") || loader_name.starts_with("../") {
        // This is a relative path with explicit prefix, convert to absolute path
        let cwd = std::env::current_dir().unwrap_or_default();
        let project_path = std::path::Path::new(project_dir.path.as_str());
        let loader_path = std::path::Path::new(loader_name);

        // Handle the case where project_path might contain parts that are already in cwd
        let resolved_project_path = if project_path.is_relative() {
            // If project_path is relative, check if it starts with a path that's already in cwd
            let cwd_str = cwd.to_string_lossy();
            let project_str = project_path.to_string_lossy();

            // Check if the last components of cwd match the beginning of project_path
            let cwd_components: Vec<&str> = cwd_str.split('/').filter(|s| !s.is_empty()).collect();
            let project_components: Vec<&str> =
                project_str.split('/').filter(|s| !s.is_empty()).collect();

            // Find the longest common suffix of cwd that matches the beginning of project_path
            let mut common_length = 0;
            for i in 1..=std::cmp::min(cwd_components.len(), project_components.len()) {
                let cwd_suffix = &cwd_components[cwd_components.len() - i..];
                let project_prefix = &project_components[..i];
                if cwd_suffix == project_prefix {
                    common_length = i;
                }
            }

            if common_length > 0 {
                // Remove the common part from project_path
                let remaining_components = &project_components[common_length..];
                if !remaining_components.is_empty() {
                    cwd.join(remaining_components.join("/"))
                } else {
                    cwd
                }
            } else {
                cwd.join(project_path)
            }
        } else {
            // If project_path is absolute, use it directly
            project_path.to_path_buf()
        };

        // Then join with the loader path
        let full_path = resolved_project_path.join(loader_path);

        // Use canonicalize to normalize the path and remove any duplicate components
        if let Ok(canonical_path) = canonicalize(&full_path) {
            canonical_path.to_string_lossy().into()
        } else {
            // If canonicalization fails, check if the original path exists
            if full_path.exists() {
                full_path.to_string_lossy().into()
            } else {
                // If path doesn't exist, return the original loader name
                loader_name.into()
            }
        }
    } else {
        // This is not a relative path (could be a package name or absolute path), keep as is
        loader_name.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_canonicalize_with_dot_paths() {
        // 测试 canonicalize 是否能正确处理包含 ./ 的路径
        let test_path = PathBuf::from(
            "/Users/zoomdong/mako/crates/pack-tests/crates/pack-tests/tests/snapshot/webpack-loaders/custom-loader/./test-file-loader.js",
        );

        // 如果路径存在，测试 canonicalize
        if test_path.exists() {
            let canonicalized = canonicalize(&test_path);
            assert!(
                canonicalized.is_ok(),
                "canonicalize should succeed for existing path"
            );

            let canonical_path = canonicalized.unwrap();
            let canonical_str = canonical_path.to_string_lossy();

            // 验证规范化后的路径不包含 ./ 组件
            assert!(
                !canonical_str.contains("/./"),
                "canonicalized path should not contain /./"
            );
            assert!(
                !canonical_str.contains("/../"),
                "canonicalized path should not contain /../"
            );

            println!("Original path: {}", test_path.display());
            println!("Canonicalized path: {}", canonical_path.display());
        } else {
            // 如果路径不存在，创建一个临时路径来测试
            let temp_dir = std::env::temp_dir();

            // 创建一个包含 ./ 的路径
            let path_with_dot = temp_dir
                .join("custom-loader")
                .join(".")
                .join("test-file-loader.js");

            let canonicalized = canonicalize(&path_with_dot);
            if canonicalized.is_ok() {
                let canonical_path = canonicalized.unwrap();
                let canonical_str = canonical_path.to_string_lossy();

                // 验证规范化后的路径不包含 ./ 组件
                assert!(
                    !canonical_str.contains("/./"),
                    "canonicalized path should not contain /./"
                );

                println!("Test path with ./: {}", path_with_dot.display());
                println!("Canonicalized path: {}", canonical_path.display());
            }
        }
    }

    #[test]
    fn test_path_duplicate_resolution() {
        // 测试路径重复问题的解决方案
        println!("=== Testing path duplicate resolution ===");

        let cwd = PathBuf::from("/Users/zoomdong/mako/crates/pack-tests");
        let project_path =
            PathBuf::from("crates/pack-tests/tests/snapshot/webpack-loaders/custom-loader");
        let loader_name = "./test-file-loader.js";

        println!("cwd: {}", cwd.display());
        println!("project_path: {}", project_path.display());
        println!("loader_name: {}", loader_name);

        // 模拟 resolve_loader_path 的逻辑
        let cwd_str = cwd.to_string_lossy();
        let project_str = project_path.to_string_lossy();

        println!("cwd_str: {}", cwd_str);
        println!("project_str: {}", project_str);

        // 测试新的路径处理逻辑
        let cwd_components: Vec<&str> = cwd_str.split('/').filter(|s| !s.is_empty()).collect();
        let project_components: Vec<&str> =
            project_str.split('/').filter(|s| !s.is_empty()).collect();

        println!("cwd_components: {:?}", cwd_components);
        println!("project_components: {:?}", project_components);

        // Find the longest common suffix of cwd that matches the beginning of project_path
        let mut common_length = 0;
        for i in 1..=std::cmp::min(cwd_components.len(), project_components.len()) {
            let cwd_suffix = &cwd_components[cwd_components.len() - i..];
            let project_prefix = &project_components[..i];
            println!(
                "checking: cwd_suffix={:?}, project_prefix={:?}",
                cwd_suffix, project_prefix
            );
            if cwd_suffix == project_prefix {
                common_length = i;
                println!("found common length: {}", common_length);
            }
        }

        if common_length > 0 {
            // Remove the common part from project_path
            let remaining_components = &project_components[common_length..];
            println!("remaining_components: {:?}", remaining_components);

            let resolved_project_path = if !remaining_components.is_empty() {
                cwd.join(remaining_components.join("/"))
            } else {
                cwd
            };

            let full_path = resolved_project_path.join(PathBuf::from(loader_name));
            println!("resolved_path: {}", full_path.display());

            // 验证路径不包含重复的 crates/pack-tests
            let full_path_str = full_path.to_string_lossy();
            assert!(
                !full_path_str.contains("crates/pack-tests/crates/pack-tests"),
                "path should not contain duplicate crates/pack-tests"
            );

            // 验证路径格式正确
            let expected_path = "/Users/zoomdong/mako/crates/pack-tests/tests/snapshot/webpack-loaders/custom-loader/./test-file-loader.js";
            assert_eq!(
                full_path_str, expected_path,
                "path should match expected format"
            );

            println!("✅ Path correctly resolved without duplicates!");
        } else {
            println!("No common components found");
        }
    }
}
