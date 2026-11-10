//! Protocol-independent browser module references. The framework owns discovery
//! and transport semantics; the bundler only substitutes explicit module exports.

use std::{
    collections::{HashMap, HashSet},
    path::{Component, Path},
};

use anyhow::{Context, Result, bail, ensure};
use turbo_rcstr::RcStr;
use turbo_tasks::{ResolvedVc, Vc};
use turbo_tasks_fs::{File, FileContent, FileSystemEntryType, FileSystemPath, to_sys_path};
use turbopack::{
    ModuleAssetContext,
    module_options::{RuleCondition, transition_rule::TransitionRule},
    transition::Transition,
};
use turbopack_core::{
    asset::AssetContent,
    context::{AssetContext, ProcessResult},
    reference_type::{EcmaScriptModulesReferenceSubType, ReferenceType},
    source::Source,
    virtual_source::VirtualSource,
};

use crate::config::{Config, ModuleReferenceConfig};

/// Generate only the supplied references. The original module is never read,
/// evaluated, parsed, or included in source-map sourcesContent.
fn generate_reference_module(config: &ModuleReferenceConfig) -> Result<String> {
    ensure!(
        !config.references.is_empty(),
        "module.references must contain a reference"
    );
    ensure!(
        !config.exports.is_empty(),
        "module.references must contain an export"
    );
    let mut ids = HashMap::new();
    let mut output = String::new();
    for (index, reference) in config.references.iter().enumerate() {
        ensure!(
            !reference.id.is_empty(),
            "module reference id must not be empty"
        );
        ensure!(
            ids.insert(reference.id.as_str(), index).is_none(),
            "duplicate module reference id: {}",
            reference.id
        );
        ensure!(
            !reference.factory.module.is_empty(),
            "module reference factory.module must not be empty"
        );
        ensure!(
            !reference.factory.export.is_empty(),
            "module reference factory.export must not be empty"
        );
        output.push_str(&format!(
            "import {{ {} as __factory{index} }} from {};\n",
            serde_json::to_string(&reference.factory.export)?,
            serde_json::to_string(&reference.factory.module)?,
        ));
        // Parse JSON instead of emitting an object literal: an own __proto__
        // property must remain data rather than changing the object's prototype.
        output.push_str(&format!(
            "const __reference{index} = __factory{index}(JSON.parse({}));\n",
            serde_json::to_string(&serde_json::to_string(&reference.data)?)?,
        ));
    }
    let mut names = HashSet::new();
    for export in &config.exports {
        ensure!(
            !export.name.is_empty(),
            "module reference export name must not be empty"
        );
        ensure!(
            names.insert(export.name.as_str()),
            "duplicate module reference export: {}",
            export.name
        );
        let index = ids
            .get(export.reference.as_str())
            .with_context(|| format!("unknown module reference id: {}", export.reference))?;
        output.push_str(&format!(
            "export {{ __reference{index} as {} }};\n",
            serde_json::to_string(&export.name)?,
        ));
    }
    Ok(output)
}

fn relative_resource(resource: &str, project: Option<&Path>) -> Result<String> {
    ensure!(
        !resource.is_empty(),
        "module reference resource must not be empty"
    );
    let resource = Path::new(resource);
    let relative = if resource.is_absolute() {
        resource
            .strip_prefix(
                project.context("absolute module reference resources require a disk filesystem")?,
            )
            .context("module reference resource must be inside the project")?
    } else {
        resource
    };
    ensure!(
        !relative.components().any(|part| matches!(
            part,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )),
        "module reference resource must not escape the project"
    );
    let relative = relative
        .components()
        .filter_map(|part| match part {
            Component::Normal(value) => Some(value.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    ensure!(
        !relative.is_empty(),
        "module reference resource must identify a file"
    );
    Ok(relative)
}

fn factory_specifier(module: &str, resource: &str, project: Option<&Path>) -> Result<String> {
    let module_path = Path::new(module);
    if !module_path.is_absolute() {
        return Ok(module.to_owned());
    }
    let resource_path = project
        .context("absolute reference factories require a disk filesystem")?
        .join(resource);
    let directory = resource_path
        .parent()
        .context("module reference resource must identify a file")?;
    let relative = pathdiff::diff_paths(module_path, directory)
        .context("reference factory must be on the same filesystem as the resource")?;
    let relative = relative.to_string_lossy().replace('\\', "/");
    Ok(
        if relative.starts_with("./") || relative.starts_with("../") {
            relative
        } else {
            format!("./{relative}")
        },
    )
}

pub async fn rules(project: FileSystemPath, config: Vc<Config>) -> Result<Vec<TransitionRule>> {
    let references = config.module_references().await?;
    let disk_project = to_sys_path(project.clone()).await?;
    let mut seen = HashSet::new();
    let mut rules = Vec::with_capacity(references.len());
    for reference in references.iter() {
        let resource = relative_resource(&reference.resource, disk_project.as_deref())?;
        if !seen.insert(resource.clone()) {
            bail!("duplicate module reference resource: {resource}");
        }
        let mut normalized = reference.clone();
        for item in &mut normalized.references {
            // A leading slash in an ECMAScript request means server-relative,
            // not an absolute OS path. Convert explicit disk paths before emit.
            item.factory.module =
                factory_specifier(&item.factory.module, &resource, disk_project.as_deref())?.into();
        }
        let code = generate_reference_module(&normalized)
            .with_context(|| format!("invalid module reference for {resource}"))?;
        let path = project.join(&resource)?;
        // Reject symlinks in any segment before exact matching. Checking only the
        // final entry type can otherwise silently miss a symlinked directory.
        let real_path = path.realpath().await?.with_context(|| {
            format!("module reference resource must be an existing file: {resource}")
        })?;
        ensure!(
            real_path == path,
            "module reference resource must use its canonical path: {resource}"
        );
        ensure!(
            matches!(*path.get_type().await?, FileSystemEntryType::File),
            "module reference resource must be an existing file: {resource}"
        );
        rules.push(TransitionRule::new_all(
            RuleCondition::ResourcePathEquals(path),
            ResolvedVc::upcast(ModuleReferenceTransition { code: code.into() }.resolved_cell()),
        ));
    }
    Ok(rules)
}

#[turbo_tasks::value]
struct ModuleReferenceTransition {
    code: RcStr,
}

#[turbo_tasks::value_impl]
impl ModuleReferenceTransition {
    #[turbo_tasks::function]
    async fn generated_source(&self, source: Vc<Box<dyn Source>>) -> Result<Vc<Box<dyn Source>>> {
        // A distinct .mjs identity prevents the exact-resource rule from applying
        // again and gives source maps only generated reference content.
        let ident = source
            .ident()
            .owned()
            .await?
            .rename_as("*.[module-reference].mjs");
        Ok(Vc::upcast(VirtualSource::new_with_ident(
            ident.into_vc(),
            AssetContent::file(FileContent::Content(File::from(self.code.to_string())).cell()),
        )))
    }
}

#[turbo_tasks::value_impl]
impl Transition for ModuleReferenceTransition {
    #[turbo_tasks::function]
    async fn process(
        self: Vc<Self>,
        source: Vc<Box<dyn Source>>,
        module_asset_context: Vc<ModuleAssetContext>,
        reference_type: ReferenceType,
    ) -> Result<Vc<ProcessResult>> {
        ensure!(
            supported_reference_type(&reference_type),
            "module.references resources cannot be imported as assets or with inline loaders"
        );
        // Share the generated source across import kinds/export parts so aliases
        // cannot create distinct module objects with an identical asset id.
        Ok(module_asset_context.process(self.generated_source(source), reference_type))
    }
}

fn supported_reference_type(reference_type: &ReferenceType) -> bool {
    matches!(
        reference_type,
        ReferenceType::CommonJs(_)
            | ReferenceType::Entry(_)
            | ReferenceType::EcmaScriptModules(
                EcmaScriptModulesReferenceSubType::Import
                    | EcmaScriptModulesReferenceSubType::ImportPart(_)
                    | EcmaScriptModulesReferenceSubType::DynamicImport
                    | EcmaScriptModulesReferenceSubType::LazyDynamicImport
                    | EcmaScriptModulesReferenceSubType::Undefined
            )
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config() -> ModuleReferenceConfig {
        serde_json::from_value(json!({
            "resource": "src/greeting.ts",
            "references": [{ "id": "greeting", "factory": { "module": "./runtime.js", "export": "createReference" }, "data": { "operation": "greet", "__proto__": { "value": 1 } } }],
            "exports": [{ "name": "Greeting", "reference": "greeting" }, { "name": "default", "reference": "greeting" }]
        })).unwrap()
    }

    #[test]
    fn aliases_share_one_factory_call_and_json_preserves_data() {
        let output = generate_reference_module(&config()).unwrap();
        assert_eq!(output.matches("= __factory0(").count(), 1);
        assert!(output.contains("export { __reference0 as \"Greeting\" }"));
        assert!(output.contains("export { __reference0 as \"default\" }"));
        assert!(output.contains("JSON.parse("));
        assert!(output.contains("__proto__"));
    }

    #[test]
    fn rejects_ambiguous_or_missing_references() {
        let mut duplicate = config();
        duplicate.references.push(duplicate.references[0].clone());
        assert!(
            generate_reference_module(&duplicate)
                .unwrap_err()
                .to_string()
                .contains("duplicate module reference id")
        );
        let mut duplicate = config();
        duplicate.exports.push(duplicate.exports[0].clone());
        assert!(
            generate_reference_module(&duplicate)
                .unwrap_err()
                .to_string()
                .contains("duplicate module reference export")
        );
        let mut missing = config();
        missing.exports[0].reference = "missing".into();
        assert!(
            generate_reference_module(&missing)
                .unwrap_err()
                .to_string()
                .contains("unknown module reference id")
        );
    }

    #[test]
    fn resources_are_exact_project_files() {
        assert_eq!(
            relative_resource("./src/greeting.ts", None).unwrap(),
            "src/greeting.ts"
        );
        assert_eq!(
            relative_resource("/app/src/greeting.ts", Some(Path::new("/app"))).unwrap(),
            "src/greeting.ts"
        );
        assert!(relative_resource("../other.ts", None).is_err());
        assert!(relative_resource("/other/greeting.ts", Some(Path::new("/app"))).is_err());
    }

    #[test]
    fn rejects_asset_import_modes_before_reading_source() {
        for kind in ["text", "bytes", "json"] {
            assert!(!supported_reference_type(
                &ReferenceType::EcmaScriptModules(
                    EcmaScriptModulesReferenceSubType::ImportWithType(kind.into())
                )
            ));
        }
        assert!(supported_reference_type(&ReferenceType::EcmaScriptModules(
            EcmaScriptModulesReferenceSubType::Import
        )));
    }

    #[test]
    fn absolute_factory_paths_become_module_relative_imports() {
        assert_eq!(
            factory_specifier(
                "/app/runtime/client.js",
                "src/greeting.ts",
                Some(Path::new("/app"))
            )
            .unwrap(),
            "../runtime/client.js"
        );
        assert_eq!(
            factory_specifier(
                "/app/src/runtime.js",
                "src/greeting.ts",
                Some(Path::new("/app"))
            )
            .unwrap(),
            "./runtime.js"
        );
        assert_eq!(
            factory_specifier("example-client", "src/greeting.ts", None).unwrap(),
            "example-client"
        );
    }
}
