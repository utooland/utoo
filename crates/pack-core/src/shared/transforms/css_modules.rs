use anyhow::Result;
use async_trait::async_trait;
use swc_core::ecma::{
    ast::{ImportDecl, ModuleDecl, ModuleItem, Program},
    utils::quote_str,
    visit::{VisitMut, VisitMutWith, visit_mut_pass},
};

use turbo_tasks::ResolvedVc;
use turbopack::module_options::{ModuleRule, ModuleRuleEffect};
use turbopack_ecmascript::{CustomTransformer, EcmascriptInputTransform, TransformContext};

use super::module_rule_match_js_no_url;

pub fn get_auto_css_modules_rule() -> ModuleRule {
    let auto_css_modules_transform = EcmascriptInputTransform::Plugin(ResolvedVc::cell(Box::new(
        CssModulesImportTransformer {},
    ) as _));

    ModuleRule::new(
        module_rule_match_js_no_url(false),
        vec![ModuleRuleEffect::ExtendEcmascriptTransforms {
            preprocess: ResolvedVc::cell(vec![auto_css_modules_transform]),
            main: ResolvedVc::cell(vec![]),
            postprocess: ResolvedVc::cell(vec![]),
        }],
    )
}

pub fn auto_css_modules_transform() -> impl VisitMut {
    visit_mut_pass(CssModulesImportVisitor::new())
}

struct CssModulesImportVisitor {}

impl CssModulesImportVisitor {
    fn new() -> Self {
        Self {}
    }

    fn should_add_modules_suffix(&self, source: &str) -> bool {
        // Don't add `?modules` if it already exists
        if source.contains("?modules")
            || source.ends_with(".module.css")
            || source.ends_with(".module.less")
            || source.ends_with(".module.sass")
            || source.ends_with(".module.scss")
        {
            return false;
        }

        // Check if it matches our CSS extension pattern
        source.ends_with(".css")
            || source.ends_with(".less")
            || source.ends_with(".sass")
            || source.ends_with(".scss")
    }
}

impl VisitMut for CssModulesImportVisitor {
    fn visit_mut_program(&mut self, program: &mut Program) {
        program.visit_mut_children_with(self);
    }

    fn visit_mut_module_item(&mut self, item: &mut ModuleItem) {
        if let ModuleItem::ModuleDecl(ModuleDecl::Import(import_decl)) = item {
            self.visit_mut_import_decl(import_decl);
        }
        item.visit_mut_children_with(self);
    }

    fn visit_mut_import_decl(&mut self, import_decl: &mut ImportDecl) {
        let source_value = import_decl.src.value.to_string();

        // Check if this is a default import of a CSS-like file
        let has_default_import = import_decl
            .specifiers
            .iter()
            .any(|spec| matches!(spec, swc_core::ecma::ast::ImportSpecifier::Default(_)));

        if has_default_import && self.should_add_modules_suffix(&source_value) {
            let new_source = format!("{source_value}?modules");
            *import_decl.src = quote_str!(new_source);
        }

        import_decl.visit_mut_children_with(self);
    }
}

#[derive(Debug)]
struct CssModulesImportTransformer {}

#[async_trait]
impl CustomTransformer for CssModulesImportTransformer {
    #[tracing::instrument(level = "trace", name = "auto_css_modules", skip_all)]
    async fn transform(&self, program: &mut Program, _ctx: &TransformContext<'_>) -> Result<()> {
        program.visit_mut_with(&mut auto_css_modules_transform());
        Ok(())
    }
}
