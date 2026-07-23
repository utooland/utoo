use anyhow::Result;
use async_trait::async_trait;
use swc_core::common::DUMMY_SP;
use swc_core::ecma::ast::*;
use swc_core::ecma::utils::private_ident;
use swc_core::ecma::visit::{VisitMut, VisitMutWith};
use turbo_tasks::ResolvedVc;
use turbopack::module_options::{ModuleRule, ModuleRuleEffect, RuleCondition};
use turbopack_core::reference_type::ReferenceTypeCondition;
use turbopack_ecmascript::{CustomTransformer, EcmascriptInputTransform, TransformContext};

const DEFAULT_COMPONENT_NAME: &str = "Component$$";

pub fn get_default_export_namer_rule() -> ModuleRule {
    let default_export_namer_transform = EcmascriptInputTransform::Plugin(ResolvedVc::cell(
        Box::new(DefaultExportNamerTransformer {}) as _,
    ));

    let condition = RuleCondition::all(vec![
        RuleCondition::not(RuleCondition::ReferenceType(ReferenceTypeCondition::Url(
            None,
        ))),
        RuleCondition::any(vec![
            RuleCondition::ResourcePathEndsWith(".jsx".to_string()),
            RuleCondition::ResourcePathEndsWith(".js".to_string()),
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
            preprocess: ResolvedVc::cell(vec![default_export_namer_transform]),
            main: ResolvedVc::cell(vec![]),
            postprocess: ResolvedVc::cell(vec![]),
        }],
    )
}

struct DefaultExportNamerVisitor {}

impl DefaultExportNamerVisitor {
    fn new() -> Self {
        Self {}
    }
}

#[derive(Debug)]
struct DefaultExportNamerTransformer {}

#[async_trait]
impl CustomTransformer for DefaultExportNamerTransformer {
    #[tracing::instrument(level = "trace", name = "default_export_namer", skip_all)]
    async fn transform(&self, program: &mut Program, _ctx: &TransformContext<'_>) -> Result<()> {
        let mut visitor = DefaultExportNamerVisitor::new();
        program.visit_mut_with(&mut visitor);
        Ok(())
    }
}

impl VisitMut for DefaultExportNamerVisitor {
    fn visit_mut_module_item(&mut self, item: &mut ModuleItem) {
        if let ModuleItem::ModuleDecl(module_decl) = item {
            match module_decl {
                ModuleDecl::ExportDefaultExpr(ExportDefaultExpr {
                    expr: box Expr::Arrow(arrow_expr),
                    ..
                }) => {
                    if arrow_expr.is_async || arrow_expr.is_generator {
                        return;
                    }
                    let ArrowExpr {
                        params,
                        body,
                        is_async,
                        is_generator,
                        return_type,
                        type_params,
                        span,
                        ctxt,
                        ..
                    } = arrow_expr.clone();
                    *item = ModuleDecl::ExportDefaultDecl(ExportDefaultDecl {
                        span: DUMMY_SP,
                        decl: DefaultDecl::Fn(FnExpr {
                            ident: Some(private_ident!(DEFAULT_COMPONENT_NAME)),
                            function: Function {
                                params: params
                                    .into_iter()
                                    .map(|pat| pat.into())
                                    .collect::<Vec<_>>(),
                                body: Some(match *body {
                                    BlockStmtOrExpr::BlockStmt(block_stmt) => block_stmt,
                                    BlockStmtOrExpr::Expr(expr) => BlockStmt {
                                        span,
                                        ctxt,
                                        stmts: vec![Stmt::Return(ReturnStmt {
                                            span,
                                            arg: Some(expr),
                                        })],
                                    },
                                }),
                                is_async,
                                is_generator,
                                span,
                                ctxt,
                                return_type,
                                type_params,
                                decorators: vec![],
                            }
                            .into(),
                        }),
                    })
                    .into();
                }
                ModuleDecl::ExportDefaultDecl(decl) => {
                    if let DefaultDecl::Fn(fn_expr) = &mut decl.decl
                        && fn_expr.ident.is_none()
                    {
                        fn_expr.ident = Some(private_ident!(DEFAULT_COMPONENT_NAME));
                    }
                }
                _ => (),
            }
        }
    }
}
