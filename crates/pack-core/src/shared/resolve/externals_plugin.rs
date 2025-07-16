use anyhow::Result;
use turbo_esregex::EsRegex;
use turbo_tasks::{ResolvedVc, Vc};
use turbo_tasks_fs::{self, FileSystemPath, glob::Glob};
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

/// Parse a string to a Swc Regex
fn parse_str_to_regex(regex_str: &str) -> Result<EsRegex> {
    if let Some(captures) = regex_str.strip_prefix('/')
        && let Some(last_slash) = captures.rfind('/')
    {
        let pattern = &captures[..last_slash];
        let flags = &captures[last_slash + 1..];

        return EsRegex::new(pattern, flags);
    }

    EsRegex::new(regex_str, "")
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
    fn before_resolve_condition(&self) -> Vc<BeforeResolvePluginCondition> {
        BeforeResolvePluginCondition::from_request_glob(Glob::new("*".into()))
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

        // get request module name
        let module_name = match &*request_value {
            Request::Module { module, .. } => module,
            Request::Raw {
                path: Pattern::Constant(name),
                ..
            } => name,
            _ => return Ok(ResolveResultOption::none()),
        };

        tracing::debug!("before_resolve: checking module: {}", module_name);

        // check if the module exists in externals config.
        if let Some(external_config) = externals_config.get(module_name) {
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
                            return Ok(ResolveResultOption::some(*ResolveResult::primary(
                                ResolveResultItem::External {
                                    name: external_name.into(),
                                    ty: ExternalType::Script,
                                    traced: ExternalTraced::Traced,
                                },
                            )));
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

            return Ok(ResolveResultOption::some(*ResolveResult::primary(
                ResolveResultItem::External {
                    name: external_name,
                    ty: external_type,
                    traced: ExternalTraced::Traced,
                },
            )));
        }

        Ok(ResolveResultOption::none())
    }
}

#[turbo_tasks::value_impl]
impl AfterResolvePlugin for ExternalsPlugin {
    #[turbo_tasks::function]
    fn after_resolve_condition(&self) -> Vc<AfterResolvePluginCondition> {
        // We need to match files in node_modules to handle subpath externals
        AfterResolvePluginCondition::new(self.root.clone(), Glob::new("**/node_modules/**".into()))
    }

    #[turbo_tasks::function]
    async fn after_resolve(
        &self,
        _fs_path: FileSystemPath,
        _lookup_path: FileSystemPath,
        _reference_type: ReferenceType,
        request: ResolvedVc<Request>,
    ) -> Result<Vc<ResolveResultOption>> {
        tracing::debug!("execute externals plugins after resolve");
        let externals_config = self.externals_config.await?;
        let request_value = &*request.await?;

        let Request::Module {
            module: package,
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
                // Check exclude list first (highest priority)
                if let Some(exclude_list) = &sub_path_config.exclude {
                    for exclude_pattern in exclude_list {
                        let pattern = exclude_pattern.as_str();
                        let is_excluded = if pattern.starts_with('/') && pattern.ends_with('/') {
                            // Treat as regex pattern
                            if let Ok(regex) = parse_str_to_regex(pattern) {
                                regex.is_match(sub_path_str)
                            } else {
                                false
                            }
                        } else {
                            // Simple string matching - check if sub_path contains this pattern
                            sub_path_str.contains(pattern)
                        };

                        if is_excluded {
                            tracing::debug!(
                                "sub_path '{}' excluded by pattern '{}'",
                                sub_path_str,
                                pattern
                            );
                            return Ok(ResolveResultOption::none());
                        }
                    }
                }

                // Process each rule in order
                for rule in &sub_path_config.rules {
                    let rule_regex = parse_str_to_regex(rule.regex.as_str())?;
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
                                    // ignore will just skip this module and the code which import the moudle will be undefined.
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
                                            result = result.replace(&placeholder, &converted_value);
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
                            },
                        )));
                    }
                }
            }
        }

        Ok(ResolveResultOption::none())
    }
}
