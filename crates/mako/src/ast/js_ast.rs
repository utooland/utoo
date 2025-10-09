use std::fmt;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use swc_core::common::util::take::Take;
use swc_core::common::{FileName, Mark, Spanned, GLOBALS};
use swc_core::ecma::ast::{EsVersion, Module};
use swc_core::ecma::codegen::text_writer::JsWriter;
use swc_core::ecma::codegen::{Config as JsCodegenConfig, Emitter};
use swc_core::ecma::parser::error::SyntaxError;
use swc_core::ecma::parser::lexer::Lexer;
use swc_core::ecma::parser::{EsSyntax, Parser, StringInput, Syntax, TsSyntax};
use swc_core::ecma::transforms::base::helpers::inject_helpers;
use swc_core::ecma::utils::contains_top_level_await;
use swc_core::ecma::visit;
use swc_core::ecma::visit::{VisitMutWith, VisitWith};

use crate::ast::file::{Content, File, JsContent};
use crate::ast::sourcemap::build_source_map_to_buf;
use crate::ast::{error, utils};
use crate::compiler::Context;
use crate::config::{DevtoolConfig, Mode, OutputMode};
use crate::module::Dependency;
use crate::utils::base64_encode;
use crate::visitors::dep_analyzer::DepAnalyzer;

#[derive(Clone)]
pub struct JsAst {
    pub ast: Module,
    pub unresolved_mark: Mark,
    pub top_level_mark: Mark,
    pub path: String,
    pub contains_top_level_await: bool,
}

impl fmt::Debug for JsAst {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "JsAst")
    }
}

impl JsAst {
    pub fn new(file: &File, context: Arc<Context>) -> Result<Self> {
        let fm = context.meta.script.cm.new_source_file(
            FileName::Real(file.relative_path.to_path_buf()).into(),
            file.get_content_raw(),
        );
        let comments = context.meta.script.origin_comments.read().unwrap();
        let extname = &file.extname;
        let syntax = if extname == "ts" || extname == "tsx" {
            Syntax::Typescript(TsSyntax {
                tsx: extname == "tsx",
                decorators: true,
                ..Default::default()
            })
        } else {
            let jsx = file.is_content_jsx()
                || extname == "jsx"
                || (extname == "js" && !file.is_under_node_modules);
            Syntax::Es(EsSyntax {
                jsx,
                decorators: true,
                decorators_before_export: true,
                // temporarily parse import xx.json with { type: "json" }
                // e.g. https://github.com/zen-fs/core/blob/80d32c4807df4a0e887f5c33afae82fde8e62191/src/index.ts#L19
                import_attributes: true,
                ..Default::default()
            })
        };
        let lexer = Lexer::new(
            syntax,
            EsVersion::Es2015,
            StringInput::from(&*fm),
            Some(comments.get_swc_comments()),
        );
        let mut parser = Parser::new_from(lexer);
        let ast = parser.parse_module();

        // handle ast errors
        let mut ast_errors = parser.take_errors();
        // ignore with syntax error in strict mode
        ast_errors.retain(|error| {
            !matches!(
                error.kind(),
                SyntaxError::WithInStrict | SyntaxError::LegacyOctal
            )
        });
        if ast.is_err() {
            ast_errors.push(ast.clone().unwrap_err());
        }
        if !ast_errors.is_empty() {
            let errors = ast_errors
                .iter()
                .map(|err| {
                    error::code_frame(
                        error::ErrorSpan::Js(err.span()),
                        err.kind().msg().to_string().as_str(),
                        context.clone(),
                    )
                })
                .collect::<Vec<String>>();
            return Err(anyhow!(error::ParseError::JsParseError {
                messages: errors.join("\n")
            }));
        }
        let ast = ast./*safe*/unwrap();

        // top level mark and unresolved mark need to be persisted for transform usage
        GLOBALS.set(&context.meta.script.globals, || {
            let top_level_mark = Mark::new();
            let unresolved_mark = Mark::new();
            let contains_top_level_await = contains_top_level_await(&ast);
            Ok(JsAst {
                ast,
                unresolved_mark,
                top_level_mark,
                path: file.relative_path.to_string_lossy().to_string(),
                contains_top_level_await,
            })
        })
    }

    pub fn build(path: &str, content: &str, context: Arc<Context>) -> Result<Self> {
        let is_jsx = path.ends_with(".jsx") || path.ends_with(".tsx");
        JsAst::new(
            &File::with_content(
                path.to_string(),
                Content::Js(JsContent {
                    content: content.to_string(),
                    is_jsx,
                }),
                context.clone(),
            ),
            context.clone(),
        )
    }

    pub fn transform(
        &mut self,
        mut_visitors: &mut Vec<Box<dyn visit::VisitMut>>,
        folders: &mut Vec<Box<dyn visit::Fold>>,
        should_inject_helpers: bool,
        _context: Arc<Context>,
    ) -> Result<()> {
        let ast = &mut self.ast;

        // visitors
        for visitor in mut_visitors {
            ast.visit_mut_with(visitor.as_mut());
        }

        // folders
        let mut module = ast.take();
        for folder in folders {
            module = folder.as_mut().fold_module(module);
        }
        *ast = module;

        // FIXME: remove this, it's special logic
        // inject helpers
        // why need to handle cjs specially?
        // because the ast is currently a module, not a program
        // if not handled specially, the injected helpers will all be in esm format
        // which is not as expected in the cjs scenario
        // ref: https://github.com/umijs/mako/pull/831
        if should_inject_helpers {
            if utils::is_esm(ast) {
                ast.visit_mut_with(&mut inject_helpers(self.unresolved_mark));
            } else {
                let body = ast.body.take();
                let mut script_ast = swc_core::ecma::ast::Script {
                    span: ast.span,
                    shebang: ast.shebang.clone(),
                    body: body.into_iter().map(|i| i.stmt().unwrap()).collect(),
                };
                script_ast.visit_mut_with(&mut inject_helpers(self.unresolved_mark));
                ast.body = script_ast.body.into_iter().map(|i| i.into()).collect();
            }
        }

        Ok(())
    }

    pub fn analyze_deps(&self, context: Arc<Context>) -> Vec<Dependency> {
        let mut visitor = DepAnalyzer::new(self.unresolved_mark, context.clone());
        GLOBALS.set(&context.meta.script.globals, || {
            self.ast.visit_with(&mut visitor);
            visitor.dependencies
        })
    }

    pub fn generate(&self, context: Arc<Context>) -> Result<JSAstGenerated> {
        let mut buf = vec![];
        let mut source_map_buf = vec![];
        let cm = context.meta.script.cm.clone();
        {
            let comments = context.meta.script.origin_comments.read().unwrap();
            let swc_comments = comments.get_swc_comments();
            let is_prod = matches!(context.config.mode, Mode::Production);
            let minify = context.config.minify && is_prod;
            let ascii_only = if context.config.output.mode == OutputMode::Bundless {
                false
            } else {
                minify
            };
            let mut emitter = Emitter {
                cfg: JsCodegenConfig::default()
                    .with_minify(minify)
                    .with_target(context.config.output.es_version)
                    .with_ascii_only(ascii_only)
                    .with_omit_last_semi(true),
                cm: cm.clone(),
                comments: if minify { None } else { Some(swc_comments) },
                wr: Box::new(JsWriter::new(
                    cm.clone(),
                    "\n",
                    &mut buf,
                    Some(&mut source_map_buf),
                )),
            };
            emitter.emit_module(&self.ast).map_err(|err| {
                anyhow!(error::GenerateError::JsGenerateError {
                    message: err.to_string()
                })
            })?;
        }

        let sourcemap = match context.config.devtool {
            Some(DevtoolConfig::SourceMap | DevtoolConfig::InlineSourceMap) => {
                let src_buf = build_source_map_to_buf(&source_map_buf, &cm);
                String::from_utf8(src_buf).unwrap()
            }
            None => "".to_string(),
        };
        if matches!(context.config.devtool, Some(DevtoolConfig::SourceMap)) {
            let filename = &self.path;
            buf.append(
                &mut format!("\n//# sourceMappingURL={filename}.map")
                    .as_bytes()
                    .to_vec(),
            );
        } else if matches!(context.config.devtool, Some(DevtoolConfig::InlineSourceMap)) {
            buf.append(
                &mut format!(
                    "\n//# sourceMappingURL=data:application/json;charset=utf-8;base64,{}",
                    base64_encode(&sourcemap)
                )
                .as_bytes()
                .to_vec(),
            );
        }

        let code = String::from_utf8(buf)?;
        Ok(JSAstGenerated { code, sourcemap })
    }
}

pub struct JSAstGenerated {
    pub code: String,
    pub sourcemap: String,
}

#[cfg(test)]
mod tests {
    use crate::ast::tests::TestUtils;

    #[test]
    #[ignore]
    fn test_chinese_ascii() {
        assert_eq!(run(r#"log("中文")"#), r#"log("\u4E2D\u6587");"#);
    }

    #[test]
    fn test_decorator() {
        // no panic
        run(r#"
@foo()
class Bar {}
        "#);
    }

    #[test]
    fn test_legacy_octal() {
        // no panic
        run(r#"
console.log("\002F");
        "#);
    }

    fn run(js_code: &str) -> String {
        let mut test_utils = TestUtils::gen_js_ast(js_code);
        let code = test_utils.js_ast_to_code();
        println!("{}", code);
        code
    }
}
