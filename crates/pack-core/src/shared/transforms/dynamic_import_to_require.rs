use anyhow::Result;
use async_trait::async_trait;
use swc_core::{
    common::{DUMMY_SP, SyntaxContext},
    ecma::{
        ast::{CallExpr, Callee, Expr, ExprOrSpread, Ident, Lit, Pass, Program},
        utils::{ExprFactory, member_expr, quote_str},
        visit::{VisitMut, VisitMutWith, visit_mut_pass},
    },
};
use turbo_tasks::ResolvedVc;
use turbopack::module_options::{ModuleRule, ModuleRuleEffect};
use turbopack_ecmascript::{CustomTransformer, EcmascriptInputTransform, TransformContext};

use super::module_rule_match_js_no_url;

pub fn get_dynamic_import_to_require_rule() -> ModuleRule {
    let dynamic_import_to_require_transform = EcmascriptInputTransform::Plugin(ResolvedVc::cell(
        Box::new(DynamicImportToRequireTransformer {}) as _,
    ));

    ModuleRule::new(
        module_rule_match_js_no_url(false),
        vec![ModuleRuleEffect::ExtendEcmascriptTransforms {
            preprocess: ResolvedVc::cell(vec![dynamic_import_to_require_transform]),
            main: ResolvedVc::cell(vec![]),
            postprocess: ResolvedVc::cell(vec![]),
        }],
    )
}

pub fn dynamic_import_to_require(unresolved_ctxt: SyntaxContext) -> impl VisitMut + Pass {
    visit_mut_pass(DynamicImportToRequire { unresolved_ctxt })
}

struct DynamicImportToRequire {
    unresolved_ctxt: SyntaxContext,
}

impl VisitMut for DynamicImportToRequire {
    fn visit_mut_call_expr(&mut self, call_expr: &mut CallExpr) {
        if let Callee::Import(..) = &call_expr.callee {
            if let ExprOrSpread {
                expr: box Expr::Lit(Lit::Str(source)),
                ..
            } = &mut call_expr.args[0]
            {
                let require_call = Ident::new("require".into(), DUMMY_SP, self.unresolved_ctxt)
                    .as_call(DUMMY_SP, vec![quote_str!(source.value.clone()).as_arg()])
                    .into_lazy_arrow(vec![]);

                // Promise.resolve()
                let promise_resolve: Box<Expr> =
                    member_expr!(Default::default(), DUMMY_SP, Promise.resolve)
                        .as_call(DUMMY_SP, vec![])
                        .into();

                *call_expr = CallExpr {
                    span: DUMMY_SP,
                    args: vec![require_call.as_arg()],
                    callee: member_expr!(@EXT, DUMMY_SP, promise_resolve, then).as_callee(),
                    ..Default::default()
                };
            }
        } else {
            call_expr.visit_mut_children_with(self);
        }
    }
}

#[derive(Debug)]
struct DynamicImportToRequireTransformer {}

#[async_trait]
impl CustomTransformer for DynamicImportToRequireTransformer {
    #[tracing::instrument(level = tracing::Level::TRACE, name = "dynamic_import_to_require", skip_all)]
    async fn transform(&self, program: &mut Program, ctx: &TransformContext<'_>) -> Result<()> {
        program.visit_mut_with(&mut dynamic_import_to_require(
            SyntaxContext::empty().apply_mark(ctx.unresolved_mark),
        ));
        Ok(())
    }
}
