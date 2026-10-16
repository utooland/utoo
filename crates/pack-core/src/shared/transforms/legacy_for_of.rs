use anyhow::Result;
use async_trait::async_trait;
use swc_core::{
    common::{DUMMY_SP, Mark, SyntaxContext, util::take::Take},
    ecma::{
        ast::*,
        transforms::base::resolver,
        utils::{prepend_stmt, private_ident},
        visit::{VisitMut, VisitMutWith},
    },
    quote,
};
use turbopack::module_options::ModuleRule;
use turbopack_ecmascript::{CustomTransformer, TransformContext};

use super::{EcmascriptTransformStage, get_ecma_transform_rule};

pub fn get_legacy_for_of_rule(enable_mdx_rs: bool) -> ModuleRule {
    get_ecma_transform_rule(
        Box::new(LegacyForOfTransformer),
        enable_mdx_rs,
        EcmascriptTransformStage::Postprocess,
    )
}

#[derive(Debug)]
struct LegacyForOfTransformer;

#[async_trait]
impl CustomTransformer for LegacyForOfTransformer {
    async fn transform(&self, program: &mut Program, ctx: &TransformContext<'_>) -> Result<()> {
        add_legacy_for_of_fallback(program, ctx.unresolved_mark);
        Ok(())
    }
}

/// SWC's strict for-of lowering assumes Symbol.iterator exists. Supply Babel-compatible
/// fallbacks while retaining SWC's iterator closing, labels and error handling unchanged.
/// Run immediately after preset-env, before emission loses the generated-node provenance.
pub(crate) fn add_legacy_for_of_fallback(program: &mut Program, unresolved_mark: Mark) -> bool {
    let helper = private_ident!("__utoo_iterator__");
    let mut visitor = LegacyForOfVisitor {
        helper: helper.clone(),
        changed: false,
    };
    program.visit_mut_with(&mut visitor);
    if !visitor.changed {
        return false;
    }

    // Keep the real iterator first, including overrides on arrays. Only the fallback copies
    // strings/Arguments/typed arrays; ordinary arrays retain their live length semantics.
    let statement = quote!(
        "function iterator(value) {
            var method = typeof Symbol !== 'undefined' && value[Symbol.iterator] || value['@@iterator'];
            if (method) return method.call(value);
            if (!Array.isArray(value)) {
                var name;
                if (typeof value === 'string') name = 'Arguments';
                else {
                    name = Object.prototype.toString.call(value).slice(8, -1);
                    if (name === 'Object' && value.constructor) name = value.constructor.name;
                }
                if (name === 'Map' || name === 'Set') value = Array.from(value);
                else if (name === 'Arguments' || /^(?:Ui|I)nt(?:8|16|32)(?:Clamped)?Array$/.test(name)) {
                    var copy = Array(value.length);
                    for (var i = 0; i < copy.length; i++) copy[i] = value[i];
                    value = copy;
                } else throw new TypeError('Invalid attempt to iterate non-iterable instance.');
            }
            var index = 0;
            return { next: function() {
                return index >= value.length ? { done: true } : { done: false, value: value[index++] };
            } };
        }" as Stmt
    );
    let mut helper_program = Program::Script(Script {
        body: vec![statement],
        ..Default::default()
    });
    helper_program.mutate(resolver(unresolved_mark, Mark::new(), false));
    let Program::Script(mut script) = helper_program else {
        unreachable!();
    };
    let mut statement = script.body.pop().unwrap();
    let Stmt::Decl(Decl::Fn(function)) = &mut statement else {
        unreachable!();
    };
    function.ident = helper;
    match program {
        Program::Module(module) => prepend_stmt(&mut module.body, statement.into()),
        Program::Script(script) => prepend_stmt(&mut script.body, statement),
    }
    true
}

struct LegacyForOfVisitor {
    helper: Ident,
    changed: bool,
}

impl VisitMut for LegacyForOfVisitor {
    fn visit_mut_for_stmt(&mut self, statement: &mut ForStmt) {
        statement.visit_mut_children_with(self);
        let Some(VarDeclOrExpr::VarDecl(declaration)) = &mut statement.init else {
            return;
        };
        let [iterator, step] = declaration.decls.as_mut_slice() else {
            return;
        };
        // Source-written loops/calls have non-dummy spans. The two fresh bindings and
        // generated call identify SWC's strict lowering, not arbitrary Symbol.iterator use.
        if declaration.span != DUMMY_SP || iterator.span != DUMMY_SP || step.span != DUMMY_SP {
            return;
        }
        let (Pat::Ident(iterator_ident), Pat::Ident(step_ident)) = (&iterator.name, &step.name)
        else {
            return;
        };
        if iterator_ident.id.sym != "_iterator"
            || step_ident.id.sym != "_step"
            || iterator_ident.id.ctxt == SyntaxContext::empty()
            || iterator_ident.id.ctxt != step_ident.id.ctxt
            || step.init.is_some()
        {
            return;
        }
        let Some(box Expr::Call(call)) = &mut iterator.init else {
            return;
        };
        if call.span != DUMMY_SP || !call.args.is_empty() {
            return;
        }
        let Callee::Expr(box Expr::Member(member)) = &mut call.callee else {
            return;
        };
        let MemberProp::Computed(property) = &member.prop else {
            return;
        };
        if !matches!(property.expr.as_ref(), Expr::Member(MemberExpr {
            obj: box Expr::Ident(Ident { sym, .. }),
            prop: MemberProp::Ident(IdentName { sym: property, .. }),
            ..
        }) if sym == "Symbol" && property == "iterator")
        {
            return;
        }
        call.args.push(ExprOrSpread {
            spread: None,
            expr: member.obj.take(),
        });
        call.callee = Callee::Expr(Box::new(Expr::Ident(self.helper.clone())));
        self.changed = true;
    }
}
