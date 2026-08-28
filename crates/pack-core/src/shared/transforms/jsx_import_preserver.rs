use anyhow::Result;
use async_trait::async_trait;
use swc_core::common::{
    DUMMY_SP, Spanned,
    comments::{Comment, CommentKind, Comments},
};
use swc_core::ecma::ast::*;
use swc_core::ecma::visit::{Visit, VisitWith};
use turbo_tasks::ResolvedVc;
use turbopack::module_options::{ModuleRule, ModuleRuleEffect, RuleCondition};
use turbopack_core::reference_type::{ReferenceTypeCondition, UrlReferenceSubType};
use turbopack_ecmascript::{CustomTransformer, EcmascriptInputTransform, TransformContext};

const REACT_USE_MARKER: &str = "__UTOOPACK_JSX_IMPORT_PRESERVER__";

/// Preserves inputs needed by the later JSX transform while TypeScript import
/// elision runs.
///
/// Turbopack runs TypeScript in preprocess and React in postprocess. With
/// `verbatimModuleSyntax` disabled, SWC cannot yet see the value references that
/// the React transform will generate. This rule temporarily uses the classic
/// `React` binding and moves JSX directives to the module so neither is removed
/// with an otherwise-unused import.
pub fn get_jsx_import_preserver_rule(classic_runtime: bool) -> ModuleRule {
    let preserve = EcmascriptInputTransform::Plugin(ResolvedVc::cell(Box::new(
        JsxImportPreserverTransformer { classic_runtime },
    ) as _));
    let cleanup = EcmascriptInputTransform::Plugin(ResolvedVc::cell(Box::new(
        JsxImportPreserverCleanupTransformer,
    ) as _));

    let condition = RuleCondition::all(vec![
        RuleCondition::not(RuleCondition::ReferenceType(ReferenceTypeCondition::Url(
            Some(UrlReferenceSubType::Undefined),
        ))),
        RuleCondition::ResourcePathEndsWith(".tsx".to_string()),
    ]);

    ModuleRule::new(
        condition,
        vec![ModuleRuleEffect::ExtendEcmascriptTransforms {
            preprocess: ResolvedVc::cell(vec![preserve]),
            main: ResolvedVc::cell(vec![]),
            postprocess: ResolvedVc::cell(vec![cleanup]),
        }],
    )
}

#[derive(Debug)]
struct JsxImportPreserverTransformer {
    classic_runtime: bool,
}

#[async_trait]
impl CustomTransformer for JsxImportPreserverTransformer {
    #[tracing::instrument(
        level = tracing::Level::TRACE,
        name = "jsx_import_preserver",
        skip_all
    )]
    async fn transform(&self, program: &mut Program, ctx: &TransformContext<'_>) -> Result<()> {
        let Program::Module(module) = program else {
            return Ok(());
        };

        preserve_jsx_directives(module, ctx.comments);

        let classic_runtime =
            jsx_runtime_directive(module, ctx.comments).unwrap_or(self.classic_runtime);
        if !classic_runtime {
            return Ok(());
        }

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
                arg: Box::new(Expr::Seq(SeqExpr {
                    span: DUMMY_SP,
                    exprs: vec![
                        Box::new(Expr::Ident(react_ident)),
                        Box::new(Expr::Lit(Lit::Str(Str {
                            span: DUMMY_SP,
                            value: REACT_USE_MARKER.into(),
                            raw: None,
                        }))),
                    ],
                })),
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

/// Moves the first JSX directive block to the module span. The React transform
/// looks there before scanning item spans, while the TypeScript transform may
/// remove the item that originally owned the comment.
fn preserve_jsx_directives(module: &Module, comments: &dyn Comments) {
    if comments
        .get_leading(module.span.lo)
        .is_some_and(|comments| comments.iter().any(is_jsx_directive))
    {
        return;
    }

    for item in &module.body {
        let item_pos = item.span_lo();
        let Some(item_comments) = comments.get_leading(item_pos) else {
            continue;
        };
        if !item_comments.iter().any(is_jsx_directive) {
            continue;
        }

        let Some(item_comments) = comments.take_leading(item_pos) else {
            return;
        };
        let (directives, remaining): (Vec<_>, Vec<_>) =
            item_comments.into_iter().partition(is_jsx_directive);

        if !remaining.is_empty() {
            comments.add_leading_comments(item_pos, remaining);
        }
        comments.add_leading_comments(module.span.lo, directives);
        return;
    }
}

fn is_jsx_directive(comment: &Comment) -> bool {
    comment.kind == CommentKind::Block
        && comment.text.lines().any(|line| {
            let line = line.trim().trim_start_matches('*').trim();
            line.starts_with("@jsx")
        })
}

/// Returns the per-file JSX runtime override, when present. This takes
/// precedence over the project-level runtime in the React transform.
fn jsx_runtime_directive(module: &Module, comments: &dyn Comments) -> Option<bool> {
    comments
        .get_leading(module.span.lo)?
        .iter()
        .find_map(|comment| {
            if comment.kind != CommentKind::Block {
                return None;
            }

            comment.text.lines().find_map(|line| {
                let line = line.trim().trim_start_matches('*').trim();
                let runtime = line.strip_prefix("@jsxRuntime")?.trim();
                match runtime.split_whitespace().next()? {
                    "classic" => Some(true),
                    "automatic" => Some(false),
                    _ => None,
                }
            })
        })
}

#[derive(Debug)]
struct JsxImportPreserverCleanupTransformer;

#[async_trait]
impl CustomTransformer for JsxImportPreserverCleanupTransformer {
    #[tracing::instrument(
        level = tracing::Level::TRACE,
        name = "jsx_import_preserver_cleanup",
        skip_all
    )]
    async fn transform(&self, program: &mut Program, _ctx: &TransformContext<'_>) -> Result<()> {
        let Program::Module(module) = program else {
            return Ok(());
        };

        module.body.retain(|item| !is_synthetic_react_use(item));
        Ok(())
    }
}

fn is_synthetic_react_use(item: &ModuleItem) -> bool {
    let ModuleItem::Stmt(Stmt::Expr(ExprStmt { span, expr })) = item else {
        return false;
    };
    let Expr::Unary(UnaryExpr {
        span: unary_span,
        op: UnaryOp::Void,
        arg,
    }) = &**expr
    else {
        return false;
    };

    let Expr::Seq(SeqExpr {
        span: seq_span,
        exprs,
    }) = &**arg
    else {
        return false;
    };

    span.is_dummy()
        && unary_span.is_dummy()
        && seq_span.is_dummy()
        && matches!(
            exprs.as_slice(),
            [react, marker]
                if matches!(&**react, Expr::Ident(_))
                    && matches!(
                        &**marker,
                        Expr::Lit(Lit::Str(Str { value, .. })) if value == REACT_USE_MARKER
                    )
        )
}

#[derive(Default)]
struct ReactJsxScanner {
    react_ident: Option<Ident>,
    has_jsx: bool,
}

impl Visit for ReactJsxScanner {
    fn visit_import_decl(&mut self, decl: &ImportDecl) {
        for spec in &decl.specifiers {
            match spec {
                // `import React from "react"` or a framework facade such as
                // `import React from "@alipay/bigfish/react"`.
                ImportSpecifier::Default(default_spec) => {
                    if self.react_ident.is_none() && &*default_spec.local.sym == "React" {
                        self.react_ident = Some(default_spec.local.clone());
                    }
                }
                // `import * as React from "react"`
                ImportSpecifier::Namespace(ns_spec) => {
                    if self.react_ident.is_none() && &*ns_spec.local.sym == "React" {
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
