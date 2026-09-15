use std::sync::Arc;

use anyhow::{Context, Result, bail};
use rustc_hash::FxHashMap;
use swc_core::{
    base::try_with_handler,
    common::{
        DUMMY_SP, EqIgnoreSpan, FileName, FilePathMapping, GLOBALS, Mark, SourceMap, SyntaxContext,
        comments::{Comments, SingleThreadedComments},
    },
    ecma::{
        ast::{
            BinExpr, BinaryOp, CondExpr, EsVersion, Expr, Ident, Lit, Program, Stmt, Str,
            UnaryExpr, UnaryOp, VarDecl, VarDeclKind, VarDeclarator,
        },
        codegen::{Emitter, text_writer::JsWriter},
        parser::{Parser, StringInput, Syntax, lexer::Lexer},
        preset_env::{Config, Targets, transform_from_env},
        transforms::base::{
            assumptions::Assumptions,
            fixer::fixer,
            helpers::{HELPERS, Helpers, inject_helpers},
            hygiene::{self, hygiene_with_config},
            rename::rename,
            resolver,
        },
        utils::private_ident,
        visit::{Visit, VisitWith},
    },
};
use turbo_tasks::Vc;
use turbo_tasks_fs::rope::Rope;
use turbopack_core::{
    code_builder::{Code, CodeBuilder},
    environment::Environment,
};
use turbopack_ecmascript::parse::{IdentCollector, generate_js_source_map};

pub(super) enum CodegenStage {
    Lower,
    EmitMinified,
}

/// Module transforms run before code generation. Lower the complete library as well so that
/// runtime assets, module factories and generated async-module wrappers obey the same target.
pub(super) async fn generate_library_code(
    code: Code,
    environment: Vc<Environment>,
    source_maps: bool,
    stage: CodegenStage,
) -> Result<Code> {
    let versions = *environment.runtime_versions().await?;
    let target = if *environment
        .runtime_versions()
        .supports_arrow_functions()
        .await?
    {
        EsVersion::latest()
    } else {
        EsVersion::Es5
    };
    let lower = matches!(stage, CodegenStage::Lower);
    let lower_global_this = lower
        && !*environment
            .runtime_versions()
            .supports_global_this()
            .await?;
    let is_node_target = *environment.node_externals().await?;
    let source = code.source_code().to_str()?.into_owned();
    let cm = Arc::new(SourceMap::new(FilePathMapping::empty()));
    let fm = cm.new_source_file(FileName::Anon.into(), source);
    let comments = SingleThreadedComments::default();
    let lexer = Lexer::new(
        Syntax::default(),
        EsVersion::latest(),
        StringInput::from(&*fm),
        Some(&comments),
    );
    let mut parser = Parser::new_from(lexer);

    let transformed = try_with_handler(cm.clone(), Default::default(), |handler| {
        GLOBALS.set(&Default::default(), || {
            let mut program = match parser.parse_program() {
                Ok(program) => program,
                Err(error) => {
                    error.into_diagnostic(handler).emit();
                    bail!("failed to parse library output");
                }
            };
            let errors = parser.take_errors();
            if !errors.is_empty() {
                for error in errors {
                    error.into_diagnostic(handler).emit();
                }
                bail!("failed to parse library output");
            }
            let names = if source_maps {
                let mut collector = IdentCollector::default();
                program.visit_with(&mut collector);
                collector.into_map()
            } else {
                Default::default()
            };

            let unresolved_mark = Mark::new();
            let top_level_mark = Mark::new();
            program.mutate(resolver(unresolved_mark, top_level_mark, false));
            // Compare after resolving bindings, before hygiene/fixer can change formatting.
            // ES5 still needs emission to normalize raw literals and statement separators.
            let original_program = (lower && target != EsVersion::Es5).then(|| program.clone());
            // Library output is self-contained: new transform helpers cannot be imports.
            let has_helpers = if lower {
                HELPERS.set(&Helpers::new(false), || {
                    program.mutate(transform_from_env::<&dyn Comments>(
                        unresolved_mark,
                        Some(&comments),
                        Config {
                            targets: Some(Targets::Versions(versions)),
                            ..Default::default()
                        }
                        .into(),
                        Assumptions::default(),
                    ));
                    let statements_before = statement_count(&program);
                    program.mutate(inject_helpers(unresolved_mark));
                    statement_count(&program) > statements_before
                })
            } else {
                false
            };
            // Include any free global references introduced by transform helpers as well.
            let has_global_alias = lower_global_this
                && lower_global_this_references(&mut program, unresolved_mark, is_node_target);
            if original_program
                .as_ref()
                .is_some_and(|original| program.eq_ignore_span(original))
            {
                return Ok(None);
            }
            program.mutate(hygiene_with_config(hygiene::Config {
                top_level_mark,
                ..Default::default()
            }));
            program.mutate(fixer(Some(&comments)));
            Ok(Some((program, names, has_helpers || has_global_alias)))
        })
    })
    .map_err(|error| error.to_pretty_error())?;

    let Some((program, names, needs_wrapper)) = transformed else {
        // Preserve the original chunk layout, comments and sectioned source map when no
        // compatibility transform was needed. Reprinting would only add debugging noise.
        return Ok(code);
    };
    let original_map = source_maps.then(|| code.generate_source_map_ref(None));
    let generate_debug_id = code.should_generate_debug_id();

    let mut source = Vec::new();
    let mut mappings = Vec::new();
    Emitter {
        cfg: swc_core::ecma::codegen::Config::default()
            .with_target(target)
            .with_minify(!lower),
        comments: Some(&comments),
        cm: cm.clone(),
        wr: JsWriter::new(
            cm.clone(),
            "\n",
            &mut source,
            source_maps.then_some(&mut mappings),
        ),
    }
    .emit_program(&program)
    .context("failed to emit library output")?;

    let source: Rope = String::from_utf8(source)?.into();
    let mut builder = CodeBuilder::new(source_maps, generate_debug_id);
    // Keep helpers and the legacy global-object alias private to the complete library,
    // including the module factories passed as arguments to its runtime IIFE.
    if needs_wrapper {
        builder += "(function() {\n";
    }
    if let Some(original_map) = &original_map {
        builder.push_source(
            &source,
            Some(generate_js_source_map(
                &*cm,
                mappings,
                Some(original_map),
                true,
                false,
                names,
            )?),
        );
    } else {
        builder.push_source(&source, None::<Rope>);
    }
    if needs_wrapper {
        builder += "\n}).call(this);\n";
    }
    Ok(builder.build())
}

/// Resolve free globalThis references after assembly so runtime templates and external module
/// factories use the same global object. Local bindings and property names must stay intact.
fn lower_global_this_references(
    program: &mut Program,
    unresolved_mark: Mark,
    is_node_target: bool,
) -> bool {
    let unresolved_ctxt = SyntaxContext::empty().apply_mark(unresolved_mark);
    let mut finder = GlobalThisFinder {
        unresolved_ctxt,
        found: false,
    };
    program.visit_with(&mut finder);
    if !finder.found {
        return false;
    }
    let global = private_ident!("__utoo_global__");
    let replacements =
        FxHashMap::from_iter([(("globalThis".into(), unresolved_ctxt), global.to_id())]);
    program.mutate(rename(&replacements));

    let self_ident = Ident::new("self".into(), DUMMY_SP, unresolved_ctxt);
    // Browser-targeted UMD can also be loaded through CommonJS on Node.js.
    let fallback = Ident::new("global".into(), DUMMY_SP, unresolved_ctxt);
    let init = if is_node_target {
        Expr::Ident(fallback)
    } else {
        Expr::Cond(CondExpr {
            span: DUMMY_SP,
            test: Box::new(Expr::Bin(BinExpr {
                span: DUMMY_SP,
                op: BinaryOp::NotEqEq,
                left: Box::new(Expr::Unary(UnaryExpr {
                    span: DUMMY_SP,
                    op: UnaryOp::TypeOf,
                    arg: Box::new(Expr::Ident(self_ident.clone())),
                })),
                right: Box::new(Expr::Lit(Lit::Str(Str {
                    span: DUMMY_SP,
                    value: "undefined".into(),
                    raw: None,
                }))),
            })),
            cons: Box::new(Expr::Ident(self_ident)),
            alt: Box::new(Expr::Ident(fallback)),
        })
    };
    let declaration: Stmt = VarDecl {
        span: DUMMY_SP,
        ctxt: SyntaxContext::empty(),
        kind: VarDeclKind::Var,
        declare: false,
        decls: vec![VarDeclarator {
            span: DUMMY_SP,
            name: global.into(),
            init: Some(Box::new(init)),
            definite: false,
        }],
    }
    .into();
    match program {
        Program::Module(module) => module.body.insert(0, declaration.into()),
        Program::Script(script) => script.body.insert(0, declaration),
    }
    true
}

struct GlobalThisFinder {
    unresolved_ctxt: SyntaxContext,
    found: bool,
}

impl Visit for GlobalThisFinder {
    fn visit_ident(&mut self, ident: &Ident) {
        if ident.sym == "globalThis" && ident.ctxt == self.unresolved_ctxt {
            self.found = true;
        }
    }
}

fn statement_count(program: &Program) -> usize {
    match program {
        Program::Module(module) => module.body.len(),
        Program::Script(script) => script.body.len(),
    }
}
