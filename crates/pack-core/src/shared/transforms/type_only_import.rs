use anyhow::Result;
use async_trait::async_trait;
use swc_core::ecma::ast::{
    ExportSpecifier, ImportDecl, ImportSpecifier, ModuleDecl, ModuleItem, NamedExport, Program,
};
use turbopack::module_options::ModuleRule;
use turbopack_ecmascript::{CustomTransformer, TransformContext};

use super::{EcmascriptTransformStage, get_ecma_transform_rule};

pub fn get_type_only_import_rule(enable_mdx_rs: bool) -> ModuleRule {
    get_ecma_transform_rule(
        Box::new(TypeOnlyImportTransformer),
        enable_mdx_rs,
        EcmascriptTransformStage::Preprocess,
    )
}

/// Normalizes inline type-only imports and exports to their declaration-level
/// equivalents when every specifier is type-only.
///
/// Utoopack enables SWC's `verbatimModuleSyntax` behavior by default so unused value
/// imports survive classic JSX transforms. Without this normalization, SWC preserves
/// all-type named imports and exports as `import {}` or `export {}` and creates
/// unintended runtime dependencies.
#[derive(Debug)]
struct TypeOnlyImportTransformer;

#[async_trait]
impl CustomTransformer for TypeOnlyImportTransformer {
    #[tracing::instrument(
        level = tracing::Level::TRACE,
        name = "type_only_import",
        skip_all
    )]
    async fn transform(&self, program: &mut Program, _ctx: &TransformContext<'_>) -> Result<()> {
        let Program::Module(module) = program else {
            return Ok(());
        };

        for item in &mut module.body {
            match item {
                ModuleItem::ModuleDecl(ModuleDecl::Import(import)) => {
                    normalize_type_only_import(import);
                }
                ModuleItem::ModuleDecl(ModuleDecl::ExportNamed(export)) => {
                    normalize_type_only_export(export);
                }
                _ => {}
            }
        }

        Ok(())
    }
}

fn normalize_type_only_export(export: &mut NamedExport) {
    if export.type_only || export.specifiers.is_empty() {
        return;
    }

    let all_specifiers_are_type_only = export.specifiers.iter().all(|specifier| {
        matches!(
            specifier,
            ExportSpecifier::Named(named) if named.is_type_only
        )
    });

    if all_specifiers_are_type_only {
        export.type_only = true;
        for specifier in &mut export.specifiers {
            if let ExportSpecifier::Named(named) = specifier {
                named.is_type_only = false;
            }
        }
    }
}

fn normalize_type_only_import(import: &mut ImportDecl) {
    if import.type_only || import.specifiers.is_empty() {
        return;
    }

    let all_specifiers_are_type_only = import.specifiers.iter().all(|specifier| {
        matches!(
            specifier,
            ImportSpecifier::Named(named) if named.is_type_only
        )
    });

    if all_specifiers_are_type_only {
        import.type_only = true;
        for specifier in &mut import.specifiers {
            if let ImportSpecifier::Named(named) = specifier {
                named.is_type_only = false;
            }
        }
    }
}
