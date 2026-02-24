use anyhow::Result;
use async_trait::async_trait;
use swc_core::common::DUMMY_SP;
use swc_core::ecma::ast::*;
use swc_core::ecma::visit::{Visit, VisitWith};
use turbo_tasks::ResolvedVc;
use turbopack::module_options::{ModuleRule, ModuleRuleEffect, RuleCondition};
use turbopack_core::reference_type::{ReferenceType, UrlReferenceSubType};
use turbopack_ecmascript::{CustomTransformer, EcmascriptInputTransform, TransformContext};

pub fn get_classic_jsx_react_import_rule() -> ModuleRule {
    let transform = EcmascriptInputTransform::Plugin(ResolvedVc::cell(
        Box::new(ClassicJsxReactImportTransformer) as _,
    ));

    let condition = RuleCondition::all(vec![
        RuleCondition::not(RuleCondition::ReferenceType(ReferenceType::Url(
            UrlReferenceSubType::Undefined,
        ))),
        RuleCondition::any(vec![
            RuleCondition::ResourcePathEndsWith(".jsx".to_string()),
            RuleCondition::All(vec![
                RuleCondition::ResourcePathEndsWith(".ts".to_string()),
                RuleCondition::Not(Box::new(RuleCondition::ResourcePathEndsWith(
                    ".d.ts".to_string(),
                ))),
            ]),
            RuleCondition::ResourcePathEndsWith(".tsx".to_string()),
            RuleCondition::ResourcePathEndsWith(".mjs".to_string()),
            RuleCondition::ResourcePathEndsWith(".cjs".to_string()),
        ]),
    ]);

    ModuleRule::new(
        condition,
        vec![ModuleRuleEffect::ExtendEcmascriptTransforms {
            preprocess: ResolvedVc::cell(vec![transform]),
            main: ResolvedVc::cell(vec![]),
            postprocess: ResolvedVc::cell(vec![]),
        }],
    )
}

#[derive(Debug)]
struct ClassicJsxReactImportTransformer;

#[async_trait]
impl CustomTransformer for ClassicJsxReactImportTransformer {
    #[tracing::instrument(
        level = tracing::Level::TRACE,
        name = "classic_jsx_react_import",
        skip_all
    )]
    async fn transform(&self, program: &mut Program, _ctx: &TransformContext<'_>) -> Result<()> {
        let Program::Module(module) = program else {
            return Ok(());
        };

        let mut scanner = ReactJsxScanner::default();
        module.visit_with(&mut scanner);

        let Some(react_ident) = scanner.react_ident else {
            return Ok(());
        };
        if !scanner.has_jsx {
            return Ok(());
        }

        let synthetic_stmt = ModuleItem::Stmt(Stmt::Expr(ExprStmt {
            span: DUMMY_SP,
            expr: Box::new(Expr::Unary(UnaryExpr {
                span: DUMMY_SP,
                op: UnaryOp::Void,
                arg: Box::new(Expr::Ident(react_ident)),
            })),
        }));

        let insert_after = module
            .body
            .iter()
            .rposition(|item| matches!(item, ModuleItem::ModuleDecl(ModuleDecl::Import(_))))
            .unwrap_or(0);

        module.body.insert(insert_after + 1, synthetic_stmt);

        Ok(())
    }
}

#[derive(Default)]
struct ReactJsxScanner {
    react_ident: Option<Ident>,
    has_jsx: bool,
}

impl Visit for ReactJsxScanner {
    fn visit_import_decl(&mut self, decl: &ImportDecl) {
        if &*decl.src.value != "react" {
            return;
        }
        for spec in &decl.specifiers {
            match spec {
                // `import React from "react"`
                ImportSpecifier::Default(default_spec) => {
                    if self.react_ident.is_none() {
                        self.react_ident = Some(default_spec.local.clone());
                    }
                }
                // `import * as React from "react"`
                ImportSpecifier::Namespace(ns_spec) => {
                    if self.react_ident.is_none() {
                        self.react_ident = Some(ns_spec.local.clone());
                    }
                }
                ImportSpecifier::Named(_) => {}
            }
        }
    }

    fn visit_jsx_element(&mut self, node: &JSXElement) {
        self.has_jsx = true;
        node.visit_children_with(self);
    }

    fn visit_jsx_fragment(&mut self, node: &JSXFragment) {
        self.has_jsx = true;
        node.visit_children_with(self);
    }
}
