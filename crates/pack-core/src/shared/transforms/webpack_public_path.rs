use anyhow::Result;
use async_trait::async_trait;
use swc_core::{
    atoms::{Atom, atom},
    common::SyntaxContext,
    ecma::{
        ast::*,
        visit::{Visit, VisitMut, VisitMutWith, VisitWith},
    },
};
use turbopack::module_options::ModuleRule;
use turbopack_ecmascript::{CustomTransformer, TransformContext};

use super::{EcmascriptTransformStage, get_ecma_transform_rule};

/// Returns a rule that maps webpack's runtime public path global onto utoopack's
/// existing runtime public path hook.
pub fn get_webpack_public_path_transform_rule() -> ModuleRule {
    get_ecma_transform_rule(
        Box::new(WebpackPublicPathTransformer {}),
        false,
        EcmascriptTransformStage::Preprocess,
    )
}

#[derive(Debug)]
struct WebpackPublicPathTransformer {}

#[async_trait]
impl CustomTransformer for WebpackPublicPathTransformer {
    #[tracing::instrument(level = "trace", name = "webpack_public_path", skip_all)]
    async fn transform(&self, program: &mut Program, ctx: &TransformContext<'_>) -> Result<()> {
        let unresolved_ctxt = SyntaxContext::empty().apply_mark(ctx.unresolved_mark);
        let mut checker = WebpackPublicPathChecker {
            unresolved_ctxt,
            should_work: false,
        };
        program.visit_with(&mut checker);
        if !checker.should_work {
            return Ok(());
        }

        program.visit_mut_with(&mut WebpackPublicPathVisitor { unresolved_ctxt });
        Ok(())
    }
}

struct WebpackPublicPathChecker {
    unresolved_ctxt: SyntaxContext,
    should_work: bool,
}

impl WebpackPublicPathChecker {
    fn is_webpack_public_path(&self, ident: &Ident) -> bool {
        ident.sym == atom!("__webpack_public_path__") && ident.ctxt == self.unresolved_ctxt
    }
}

impl Visit for WebpackPublicPathChecker {
    fn visit_ident(&mut self, ident: &Ident) {
        if self.is_webpack_public_path(ident) {
            self.should_work = true;
        }
    }

    fn visit_program(&mut self, program: &Program) {
        if !self.should_work {
            program.visit_children_with(self);
        }
    }
}

struct WebpackPublicPathVisitor {
    unresolved_ctxt: SyntaxContext,
}

impl WebpackPublicPathVisitor {
    fn is_webpack_public_path(&self, sym: &Atom, ctxt: SyntaxContext) -> bool {
        sym == &atom!("__webpack_public_path__") && ctxt == self.unresolved_ctxt
    }

    fn global_public_path_member(&self, span: swc_core::common::Span) -> MemberExpr {
        MemberExpr {
            span,
            obj: Box::new(Expr::Ident(Ident::new(
                atom!("globalThis"),
                span,
                self.unresolved_ctxt,
            ))),
            prop: MemberProp::Ident(IdentName::new(atom!("publicPath"), span)),
        }
    }

    fn global_public_path_expr(&self, span: swc_core::common::Span) -> Expr {
        Expr::Member(self.global_public_path_member(span))
    }
}

impl VisitMut for WebpackPublicPathVisitor {
    fn visit_mut_assign_expr(&mut self, assign_expr: &mut AssignExpr) {
        match &assign_expr.left {
            AssignTarget::Simple(SimpleAssignTarget::Ident(binding_ident))
                if self.is_webpack_public_path(&binding_ident.id.sym, binding_ident.id.ctxt) =>
            {
                assign_expr.left = AssignTarget::Simple(SimpleAssignTarget::Member(
                    self.global_public_path_member(binding_ident.id.span),
                ));
            }
            _ => {
                assign_expr.left.visit_mut_with(self);
            }
        }

        assign_expr.right.visit_mut_with(self);
    }

    fn visit_mut_expr(&mut self, expr: &mut Expr) {
        if let Expr::Ident(ident) = expr
            && self.is_webpack_public_path(&ident.sym, ident.ctxt)
        {
            *expr = self.global_public_path_expr(ident.span);
            return;
        }

        expr.visit_mut_children_with(self);
    }
}
