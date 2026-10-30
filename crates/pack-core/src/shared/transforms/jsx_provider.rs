use anyhow::Result;
use async_trait::async_trait;
use swc_core::common::{Mark, SyntaxContext};
use swc_core::ecma::{
    ast::{Expr, Id, Program},
    utils::collect_decls_with_ctxt,
    visit::{VisitMut, VisitMutWith},
};
use turbopack::module_options::ModuleRule;
use turbopack_ecmascript::{CustomTransformer, TransformContext};

use super::{EcmascriptTransformStage, get_ecma_transform_rule};

/// Runs after JSX and TypeScript transforms so providers can resolve classic
/// JSX factories whose value bindings were absent or erased.
pub fn get_jsx_provider_rule(enable_mdx_rs: bool) -> ModuleRule {
    get_ecma_transform_rule(
        Box::new(JsxProviderTransformer),
        enable_mdx_rs,
        EcmascriptTransformStage::Postprocess,
    )
}

#[derive(Debug)]
struct JsxProviderTransformer;

#[async_trait]
impl CustomTransformer for JsxProviderTransformer {
    async fn transform(&self, program: &mut Program, ctx: &TransformContext<'_>) -> Result<()> {
        resolve_jsx_provider(program, ctx.top_level_mark, ctx.unresolved_mark);
        Ok(())
    }
}

fn resolve_jsx_provider(program: &mut Program, top_level_mark: Mark, unresolved_mark: Mark) {
    let top_level_ctxt = SyntaxContext::empty().apply_mark(top_level_mark);
    let bindings = collect_decls_with_ctxt::<Id, _>(program, top_level_ctxt);
    if bindings.iter().any(|(name, _)| name == "React") {
        return;
    }

    // SWC creates classic JSX pragma expressions after the resolver has run and
    // marks their identifiers as top-level bindings. Without a surviving React
    // declaration, provider analysis must instead see an unresolved reference.
    program.visit_mut_with(&mut JsxProviderVisitor {
        top_level_ctxt,
        unresolved_ctxt: SyntaxContext::empty().apply_mark(unresolved_mark),
    });
}

struct JsxProviderVisitor {
    top_level_ctxt: SyntaxContext,
    unresolved_ctxt: SyntaxContext,
}

impl VisitMut for JsxProviderVisitor {
    fn visit_mut_expr(&mut self, expr: &mut Expr) {
        expr.visit_mut_children_with(self);
        if let Expr::Ident(ident) = expr
            && ident.sym == "React"
            && ident.span.is_dummy()
            && ident.ctxt == self.top_level_ctxt
        {
            ident.ctxt = self.unresolved_ctxt;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use swc_core::{
        common::{FileName, GLOBALS, SourceMap, comments::SingleThreadedComments},
        ecma::{
            ast::EsVersion,
            parser::{Syntax, TsSyntax, parse_file_as_program},
            transforms::{
                base::resolver,
                react::{Options, Runtime, react},
                typescript::{Config, typescript},
            },
            visit::{Visit, VisitWith},
        },
    };

    use super::*;

    #[derive(Default)]
    struct ReactReferences {
        generated: Vec<SyntaxContext>,
        source: Vec<SyntaxContext>,
    }

    impl Visit for ReactReferences {
        fn visit_expr(&mut self, expr: &Expr) {
            expr.visit_children_with(self);
            if let Expr::Ident(ident) = expr
                && ident.sym == "React"
            {
                if ident.span.is_dummy() {
                    self.generated.push(ident.ctxt);
                } else {
                    self.source.push(ident.ctxt);
                }
            }
        }
    }

    fn check_references(
        source: &str,
        check: impl FnOnce(ReactReferences, SyntaxContext, SyntaxContext),
    ) {
        GLOBALS.set(&Default::default(), || {
            let source_map = Arc::new(SourceMap::default());
            let file = source_map.new_source_file(
                FileName::Custom("test.tsx".into()).into(),
                source.to_owned(),
            );
            let comments = SingleThreadedComments::default();
            let mut errors = Vec::new();
            let mut program = parse_file_as_program(
                &file,
                Syntax::Typescript(TsSyntax {
                    tsx: true,
                    ..Default::default()
                }),
                EsVersion::EsNext,
                Some(&comments),
                &mut errors,
            )
            .expect("valid JSX fixture");
            assert!(errors.is_empty(), "unexpected parser recovery: {errors:?}");

            let unresolved_mark = Mark::new();
            let top_level_mark = Mark::new();
            program.mutate(resolver(unresolved_mark, top_level_mark, true));
            program.mutate(typescript(
                Config {
                    // The import-preserver rule already retains JSX value imports
                    // in the real pipeline; keep them for this focused pass test.
                    verbatim_module_syntax: true,
                    ..Default::default()
                },
                unresolved_mark,
                top_level_mark,
            ));
            program.mutate(react(
                source_map,
                Some(&comments),
                Options {
                    runtime: Some(Runtime::Classic),
                    ..Default::default()
                },
                top_level_mark,
                unresolved_mark,
            ));
            resolve_jsx_provider(&mut program, top_level_mark, unresolved_mark);

            let mut references = ReactReferences::default();
            program.visit_with(&mut references);
            check(
                references,
                SyntaxContext::empty().apply_mark(top_level_mark),
                SyntaxContext::empty().apply_mark(unresolved_mark),
            );
        });
    }

    #[test]
    fn exposes_unbound_classic_factories_and_fragments_to_providers() {
        for source in [
            "export default <div />;",
            "export default <><span /></>;",
            "import type React from 'react'; export default <div />;",
            "import { type React } from 'types'; export default <div />;",
            "namespace React { export interface Props {} } export default <div />;",
            "declare const React: any; export default <div />;",
        ] {
            check_references(source, |references, _, unresolved| {
                assert!(!references.generated.is_empty(), "{source}");
                assert!(
                    references.generated.iter().all(|ctxt| *ctxt == unresolved),
                    "{source}"
                );
            });
        }
    }

    #[test]
    fn retains_existing_top_level_value_bindings() {
        for source in [
            "import React from 'react'; export default <div />;",
            "import * as React from 'custom-react'; export default <div />;",
            "const React = custom; export default <div />;",
            "function React() {} export default <div />;",
            "class React {} export default <div />;",
            "namespace React { export const createElement = custom; } export default <div />;",
        ] {
            check_references(source, |references, top_level, _| {
                assert!(!references.generated.is_empty(), "{source}");
                assert!(
                    references.generated.iter().all(|ctxt| *ctxt == top_level),
                    "{source}"
                );
            });
        }
    }

    #[test]
    fn leaves_automatic_runtime_and_custom_factories_unchanged() {
        for source in [
            "/** @jsxRuntime automatic */ export default <div />;",
            "/** @jsx h */ const h = custom; export default <div />;",
        ] {
            check_references(source, |references, _, _| {
                assert!(references.generated.is_empty(), "{source}");
            });
        }
    }

    #[test]
    fn preserves_source_references_to_nested_bindings() {
        check_references(
            "export function render(React) { return [React.createElement('span'), <div />]; }",
            |references, top_level, unresolved| {
                assert_eq!(references.source.len(), 1);
                assert_ne!(references.source[0], top_level);
                assert_ne!(references.source[0], unresolved);
                assert_eq!(references.generated, [unresolved]);
            },
        );
    }
}
