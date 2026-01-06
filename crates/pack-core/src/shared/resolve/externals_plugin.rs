use anyhow::Result;
use turbo_esregex::EsRegex;
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{ResolvedVc, Vc};
use turbo_tasks_fs::{
    self, FileSystemPath,
    glob::{Glob, GlobOptions},
};
use turbopack_core::{
    reference_type::ReferenceType,
    resolve::{
        ExternalTraced, ExternalType, ResolveResult, ResolveResultItem, ResolveResultOption,
        parse::Request,
        pattern::Pattern,
        plugin::{
            AfterResolvePlugin, AfterResolvePluginCondition, BeforeResolvePlugin,
            BeforeResolvePluginCondition,
        },
    },
};

use crate::config::{
    ExternalConfig, ExternalSubPathTarget, ExternalTargetConverter, ExternalsConfig,
};

/// Parse a string to a Swc Regex (cached as a turbo task)
#[turbo_tasks::function]
pub fn cached_parse_str_to_regex(regex_str: RcStr) -> Vc<EsRegex> {
    let s = regex_str.as_str();
    let regex = if s.starts_with('/') {
        if let Some(last_slash) = s.rfind('/') {
            let pattern = &s[1..last_slash];
            let flags = &s[last_slash + 1..];
            EsRegex::new(pattern, flags)
        } else {
            EsRegex::new(s, "")
        }
    } else {
        EsRegex::new(s, "")
    };

    match regex {
        Ok(r) => r.cell(),
        Err(_) => {
            // Fallback to a regex that matches nothing if parsing fails
            // or we could return a Result, but let's keep it simple for now
            // to avoid Vc<Result<...>> complexity
            EsRegex::new("a^", "").unwrap().cell()
        }
    }
}

/// Convert string based on target converter type
fn apply_target_converter(input: &str, converter: Option<&ExternalTargetConverter>) -> String {
    match converter {
        Some(ExternalTargetConverter::PascalCase) => to_pascal_case(input),
        Some(ExternalTargetConverter::CamelCase) => to_camel_case(input),
        Some(ExternalTargetConverter::KebabCase) => to_kebab_case(input),
        Some(ExternalTargetConverter::SnakeCase) => to_snake_case(input),
        None => input.to_string(),
    }
}

/// Convert string to PascalCase
fn to_pascal_case(input: &str) -> String {
    input
        .split(['-', '_', '/'])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    let mut result = String::with_capacity(word.len());
                    result.extend(first.to_uppercase());
                    result.push_str(&chars.as_str().to_lowercase());
                    result
                }
            }
        })
        .collect()
}

/// Convert string to camelCase
fn to_camel_case(input: &str) -> String {
    let mut words = input.split(['-', '_', '/']).filter(|word| !word.is_empty());

    let mut result = match words.next() {
        Some(first_word) => first_word.to_lowercase(),
        None => return String::new(),
    };

    for word in words {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            result.extend(first.to_uppercase());
            result.push_str(&chars.as_str().to_lowercase());
        }
    }
    result
}

/// Convert string to kebab-case
fn to_kebab_case(input: &str) -> String {
    input
        .split(['_', '/'])
        .filter(|word| !word.is_empty())
        .map(|word| word.to_lowercase())
        .collect::<Vec<_>>()
        .join("-")
}

/// Convert string to snake_case
fn to_snake_case(input: &str) -> String {
    input
        .split(['-', '/'])
        .filter(|word| !word.is_empty())
        .map(|word| word.to_lowercase())
        .collect::<Vec<_>>()
        .join("_")
}

/// Handle external config and return the appropriate resolve result
fn handle_external_config(
    external_config: &ExternalConfig,
    module_name: &str,
) -> Vc<ResolveResultOption> {
    let (external_name, external_type) = match external_config {
        ExternalConfig::Basic(name) => {
            // resolve basic config like "foo" or "commonjs foo" or "esm foo" or "script https://..."
            let name_str = name.as_str();
            if name_str.starts_with("commonjs ") {
                let actual_name = name_str.strip_prefix("commonjs ").unwrap_or(name_str);
                (actual_name.into(), ExternalType::CommonJs)
            } else if name_str.starts_with("esm ") {
                let actual_name = name_str.strip_prefix("esm ").unwrap_or(name_str);
                (actual_name.into(), ExternalType::EcmaScriptModule)
            } else if name_str.starts_with("script ") {
                let script_content = name_str.strip_prefix("script ").unwrap_or(name_str);
                // For script type in basic config, check if script_content already contains '@' separator
                let external_name = if script_content.contains('@') {
                    // If already in root@script format, use it directly
                    script_content.to_string()
                } else {
                    // Otherwise, concatenate module_name and script URL with '@' separator
                    // Format: module_name@script_url
                    format!("{module_name}@{script_content}")
                };
                (external_name.into(), ExternalType::Script)
            } else {
                // Default to Global
                (name.clone(), ExternalType::Global)
            }
        }
        ExternalConfig::Advanced(advanced) => {
            // advanced config.
            let external_type = match &advanced.r#type {
                Some(crate::config::ExternalType::CommonJs) => ExternalType::CommonJs,
                Some(crate::config::ExternalType::ESM) => ExternalType::EcmaScriptModule,
                Some(crate::config::ExternalType::Script) => {
                    // For script type, concatenate root and script URL with '@' separator
                    // Format: root@script
                    let external_name = if let Some(script_url) = &advanced.script {
                        format!("{}@{}", advanced.root, script_url)
                    } else {
                        // If no script URL is provided, just use root
                        advanced.root.to_string()
                    };
                    return ResolveResultOption::some(*ResolveResult::primary(
                        ResolveResultItem::External {
                            name: external_name.into(),
                            ty: ExternalType::Script,
                            traced: ExternalTraced::Traced,
                            target: None,
                        },
                    ));
                }
                Some(crate::config::ExternalType::Global) => ExternalType::Global,
                None => ExternalType::Global,
            };
            (advanced.root.clone(), external_type)
        }
        ExternalConfig::Umd(umd_config) => {
            // Handle UMD config
            let external_name = format!(
                "root {} commonjs {}",
                umd_config.root.as_str(),
                umd_config.commonjs.as_str()
            );

            (external_name.into(), ExternalType::Umd)
        }
    };

    ResolveResultOption::some(*ResolveResult::primary(ResolveResultItem::External {
        name: external_name,
        ty: external_type,
        traced: ExternalTraced::Traced,
        target: None,
    }))
}

#[turbo_tasks::value]
pub struct ExternalsPlugin {
    project_path: FileSystemPath,
    root: FileSystemPath,
    externals_config: ResolvedVc<ExternalsConfig>,
}

#[turbo_tasks::value_impl]
impl ExternalsPlugin {
    #[turbo_tasks::function]
    pub fn new(
        project_path: FileSystemPath,
        root: FileSystemPath,
        externals_config: ResolvedVc<ExternalsConfig>,
    ) -> Vc<Self> {
        ExternalsPlugin {
            project_path,
            root,
            externals_config,
        }
        .cell()
    }
}

#[turbo_tasks::value_impl]
impl BeforeResolvePlugin for ExternalsPlugin {
    #[turbo_tasks::function]
    async fn before_resolve_condition(&self) -> Result<Vc<BeforeResolvePluginCondition>> {
        let externals_config = self.externals_config.await?;

        // Extract all possible module names from external keys
        // For "react" -> ["react"]
        // For "@ant/bigfish/antd" -> ["@ant/bigfish/antd", "@ant/bigfish"]
        let mut modules = Vec::new();
        for key in externals_config.keys() {
            modules.push(key.clone());
            // Extract scoped package name if key contains additional path
            if key.starts_with('@')
                && let (Some(pos), Some(first_pos)) = (key.rfind('/'), key.find('/'))
                && pos > first_pos
            {
                modules.push(key[..pos].into());
            }
        }

        Ok(BeforeResolvePluginCondition::from_modules(Vc::cell(
            modules,
        )))
    }

    #[turbo_tasks::function]
    async fn before_resolve(
        &self,
        _lookup_path: FileSystemPath,
        _reference_type: ReferenceType,
        request: Vc<Request>,
    ) -> Result<Vc<ResolveResultOption>> {
        let externals_config = self.externals_config.await?;
        let request_value = request.await?;

        let (module_name, subpath_str) = match &*request_value {
            Request::Module {
                module: Pattern::Constant(name),
                path: Pattern::Constant(path),
                ..
            } => (name.as_str(), path.as_str()),
            Request::Raw {
                path: Pattern::Constant(name),
                ..
            } => (name.as_str(), ""),
            _ => return Ok(ResolveResultOption::none()),
        };

        // Try full path first (e.g., "@ant/bigfish/antd" from module="@ant/bigfish" + path="/antd")
        if !subpath_str.is_empty() {
            let full_path = format!("{}{}", module_name, subpath_str);
            if let Some(external_config) = externals_config.get(full_path.as_str()) {
                return Ok(handle_external_config(external_config, &full_path));
            }
            // If full path not in externals, skip to let after_resolve or normal resolution handle it
            return Ok(ResolveResultOption::none());
        }

        // Try module name only (e.g., "react")
        if let Some(external_config) = externals_config.get(module_name) {
            return Ok(handle_external_config(external_config, module_name));
        }

        Ok(ResolveResultOption::none())
    }
}

#[turbo_tasks::value_impl]
impl AfterResolvePlugin for ExternalsPlugin {
    #[turbo_tasks::function]
    pub async fn after_resolve_condition(&self) -> Result<Vc<AfterResolvePluginCondition>> {
        let externals_config = self.externals_config.await?;

        // Optimization: Instead of matching all node_modules, only match packages listed in externals.
        // This significantly reduces the number of AfterResolve tasks (Tier 1: Task Explosion).
        let mut packages = Vec::new();
        for (key, config) in externals_config.iter() {
            // Only need after_resolve for Advanced config with sub_path rules
            if let ExternalConfig::Advanced(advanced) = config
                && advanced.sub_path.is_some()
            {
                // Extract the base package name (e.g., "antd/lib" -> "antd", "@scope/pkg/sub" -> "@scope/pkg")
                let pkg_name = if key.starts_with('@') {
                    let parts: Vec<&str> = key.splitn(3, '/').collect();
                    if parts.len() >= 2 {
                        format!("{}/{}", parts[0], parts[1])
                    } else {
                        key.to_string()
                    }
                } else {
                    key.split('/').next().unwrap_or(key).to_string()
                };
                packages.push(pkg_name);
            }
        }

        let glob_str = if packages.is_empty() {
            // If no sub_path externals, return a glob that matches nothing (or a very safe tiny set)
            // Using a non-existent path to prevent any after_resolve tasks
            rcstr!(".utoopack_no_externals")
        } else {
            // Sort and dedup for cache stability
            packages.sort();
            packages.dedup();

            if packages.len() == 1 {
                format!("**/node_modules/{}/**", packages[0]).into()
            } else {
                format!("**/node_modules/{{{}}}/**", packages.join(",")).into()
            }
        };

        Ok(AfterResolvePluginCondition::new(
            self.root.clone(),
            *Glob::new(glob_str, GlobOptions::default())
                .to_resolved()
                .await?,
        ))
    }

    #[turbo_tasks::function]
    async fn after_resolve(
        &self,
        _fs_path: FileSystemPath,
        _lookup_path: FileSystemPath,
        _reference_type: ReferenceType,
        request: Vc<Request>,
    ) -> Result<Vc<ResolveResultOption>> {
        tracing::debug!("execute externals plugins after resolve");
        let externals_config = self.externals_config.await?;
        let request_value = &*request.await?;

        let Request::Module {
            module: Pattern::Constant(package),
            path: package_sub_path,
            ..
        } = request_value
        else {
            return Ok(ResolveResultOption::none());
        };

        // Get the sub path as a string
        let sub_path_str = match package_sub_path {
            Pattern::Constant(path) => path.as_str(),
            _ => return Ok(ResolveResultOption::none()),
        };

        tracing::debug!(
            "after_resolve: checking package: {}, sub_path: {}",
            package,
            sub_path_str
        );

        // Check if the package exists in externals config and has subPath configuration
        if let Some(ExternalConfig::Advanced(advanced)) = externals_config.get(package) {
            tracing::debug!("found advanced config for package: {}", package);
            if let Some(sub_path_config) = &advanced.sub_path {
                // Tier 2 Optimization: Pre-fetch all regex tasks for exclude list in parallel
                // to avoid serial await in loops (Parallelization Slums).
                if let Some(exclude_list) = &sub_path_config.exclude {
                    let mut regex_tasks = Vec::new();
                    for exclude_pattern in exclude_list {
                        let pattern = exclude_pattern.as_str();
                        if pattern.starts_with('/') && pattern.ends_with('/') {
                            regex_tasks
                                .push(Some(cached_parse_str_to_regex(exclude_pattern.clone())));
                        } else {
                            regex_tasks.push(None);
                        }
                    }

                    for (i, exclude_pattern) in exclude_list.iter().enumerate() {
                        let is_excluded = if let Some(regex_vc) = regex_tasks[i] {
                            regex_vc.await?.is_match(sub_path_str)
                        } else {
                            sub_path_str.contains(exclude_pattern.as_str())
                        };

                        if is_excluded {
                            tracing::debug!(
                                "sub_path '{}' excluded by pattern '{}'",
                                sub_path_str,
                                exclude_pattern
                            );
                            return Ok(ResolveResultOption::none());
                        }
                    }
                }

                // Tier 2 Optimization: Pre-fetch all rule regexes in parallel.
                let rule_regex_tasks: Vec<_> = sub_path_config
                    .rules
                    .iter()
                    .map(|rule| cached_parse_str_to_regex(rule.regex.clone()))
                    .collect();

                // Process each rule in order (matching must be ordered, but compilation is parallel)
                for (i, rule) in sub_path_config.rules.iter().enumerate() {
                    let rule_regex = rule_regex_tasks[i].await?;
                    // Compile regex and match against sub path
                    if rule_regex.is_match(sub_path_str) {
                        // Apply the target transformation
                        let external_name = match &rule.target {
                            ExternalSubPathTarget::Empty => {
                                // Return empty external to skip this module
                                return Ok(ResolveResultOption::some(*ResolveResult::primary(
                                    // TODO: this is a plan b for empty target.
                                    // ResolveResultItem::External {
                                    //     name: "{}".into(),
                                    //     ty: ExternalType::Global,
                                    //     traced: ExternalTraced::Traced,
                                    // },
                                    // ignore will just skip this module and the code which import the module will be undefined.
                                    ResolveResultItem::Ignore,
                                )));
                            }
                            ExternalSubPathTarget::Tpl(template) => {
                                // Replace regex capture groups in template
                                let mut external_name = template.to_string();

                                // Replace $1, $2, etc. with capture groups
                                if let Some(captures) = rule_regex.captures(sub_path_str) {
                                    let mut result = template.to_string();

                                    // Iterate directly over captures without collecting into Vec
                                    for (i, capture) in captures.enumerate().skip(1) {
                                        if let Some(capture_value) = capture {
                                            // Apply target converter to the capture group
                                            let converted_value = apply_target_converter(
                                                capture_value,
                                                rule.target_converter.as_ref(),
                                            );

                                            // Replace placeholder with converted value
                                            let placeholder = format!("${i}");
                                            result = result
                                                .replace(&placeholder, &converted_value)
                                                .replace(".", " ");
                                        }
                                    }

                                    external_name = result;
                                }

                                // Build the final external name with package root and transformed sub path
                                if external_name.is_empty() {
                                    advanced.root.to_string()
                                } else {
                                    format!("{} {}", advanced.root, external_name)
                                }
                            }
                        };

                        tracing::debug!("final external name: {}", external_name);

                        // Determine external type
                        let external_type = match &advanced.r#type {
                            Some(crate::config::ExternalType::CommonJs) => ExternalType::CommonJs,
                            Some(crate::config::ExternalType::ESM) => {
                                ExternalType::EcmaScriptModule
                            }
                            Some(crate::config::ExternalType::Script) => ExternalType::Script,
                            Some(crate::config::ExternalType::Global) => ExternalType::Global,
                            None => ExternalType::Global,
                        };

                        return Ok(ResolveResultOption::some(*ResolveResult::primary(
                            ResolveResultItem::External {
                                name: external_name.into(),
                                ty: external_type,
                                traced: ExternalTraced::Traced,
                                target: None,
                            },
                        )));
                    }
                }
            }
        }

        Ok(ResolveResultOption::none())
    }
}
