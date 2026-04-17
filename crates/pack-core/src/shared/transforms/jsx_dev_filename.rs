use anyhow::Result;
use async_trait::async_trait;
use swc_core::atoms::Atom;
use swc_core::common::Spanned;
use swc_core::ecma::ast::*;
use swc_core::ecma::visit::{VisitMut, VisitMutWith};
use turbo_tasks_fs::to_sys_path;
use turbopack::module_options::ModuleRule;
use turbopack_ecmascript::{CustomTransformer, TransformContext};

use super::{EcmascriptTransformStage, get_ecma_transform_rule};

pub fn get_jsx_dev_filename_rule() -> ModuleRule {
    get_ecma_transform_rule(
        Box::new(JsxDevFilenameTransformer),
        false,
        EcmascriptTransformStage::Postprocess,
    )
}

#[derive(Debug)]
struct JsxDevFilenameTransformer;

#[async_trait]
impl CustomTransformer for JsxDevFilenameTransformer {
    #[tracing::instrument(level = "trace", name = "jsx_dev_filename", skip_all)]
    async fn transform(&self, program: &mut Program, ctx: &TransformContext<'_>) -> Result<()> {
        let Some(sys_path) = to_sys_path(ctx.file_path.clone()).await? else {
            return Ok(());
        };

        let mut visitor = JsxDevFilenameVisitor::new(sys_path.to_string_lossy().into_owned());
        program.visit_mut_with(&mut visitor);

        Ok(())
    }
}

struct JsxDevFilenameVisitor {
    file_name: Atom,
}

impl JsxDevFilenameVisitor {
    fn new(file_name: String) -> Self {
        Self {
            file_name: Atom::from(file_name),
        }
    }

    fn is_jsx_dev_callee(callee: &Callee) -> bool {
        matches!(
            callee,
            Callee::Expr(box Expr::Ident(Ident { sym, .. }))
                if sym == "jsxDEV" || sym == "_jsxDEV"
        )
    }

    fn rewrite_file_name_expr(&self, value: &mut Box<Expr>) {
        **value = Expr::Lit(Lit::Str(Str {
            span: value.span(),
            value: self.file_name.clone().into(),
            raw: None,
        }));
    }

    fn rewrite_object_file_name_prop(&self, source: &mut ObjectLit) {
        for prop in &mut source.props {
            let PropOrSpread::Prop(box Prop::KeyValue(KeyValueProp { key, value })) = prop else {
                continue;
            };

            let PropName::Ident(IdentName { sym, .. }) = key else {
                continue;
            };

            if sym == "fileName" {
                self.rewrite_file_name_expr(value);
            }
        }
    }

    fn rewrite_jsx_dev_source_file_name(&self, args: &mut [ExprOrSpread]) {
        let Some(ExprOrSpread {
            expr: box Expr::Object(source),
            ..
        }) = args.get_mut(4)
        else {
            return;
        };

        self.rewrite_object_file_name_prop(source);
    }

    fn rewrite_classic_runtime_source_file_name(&self, args: &mut [ExprOrSpread]) {
        let Some(ExprOrSpread {
            expr: box Expr::Object(props),
            ..
        }) = args.get_mut(1)
        else {
            return;
        };

        for prop in &mut props.props {
            let PropOrSpread::Prop(box Prop::KeyValue(KeyValueProp { key, value })) = prop else {
                continue;
            };

            let PropName::Ident(IdentName { sym, .. }) = key else {
                continue;
            };

            if sym != "__source" {
                continue;
            }

            let Expr::Object(source) = &mut **value else {
                continue;
            };

            self.rewrite_object_file_name_prop(source);
        }
    }
}

impl VisitMut for JsxDevFilenameVisitor {
    fn visit_mut_call_expr(&mut self, call_expr: &mut CallExpr) {
        call_expr.visit_mut_children_with(self);

        if !Self::is_jsx_dev_callee(&call_expr.callee) {
            self.rewrite_classic_runtime_source_file_name(&mut call_expr.args);
            return;
        }

        self.rewrite_jsx_dev_source_file_name(&mut call_expr.args);
    }
}
