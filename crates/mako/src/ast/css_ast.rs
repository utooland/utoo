use std::fmt;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use md5;
use swc_core::common::FileName;
use swc_core::css::ast::Stylesheet;
use swc_core::css::codegen::writer::basic::{BasicCssWriter, BasicCssWriterConfig};
use swc_core::css::codegen::{CodeGenerator, CodegenConfig, Emit};
use swc_core::css::modules::{compile, CssClassName, TransformConfig, TransformResult};
use swc_core::css::visit::{VisitMutWith, VisitWith};
use swc_core::css::{parser, visit};
use swc_core::ecma::atoms;
use swc_core::ecma::parser::StringInput;

use crate::ast::error;
use crate::ast::file::{Content, File};
use crate::ast::sourcemap::build_source_map_to_buf;
use crate::compiler::Context;
use crate::config::{DevtoolConfig, Mode};
use crate::module::Dependency;
use crate::utils::{base64_encode, url_safe_base64_encode};
use crate::visitors::css_dep_analyzer::CSSDepAnalyzer;

#[derive(Clone)]
pub struct CssAst {
    pub ast: Stylesheet,
    pub path: String,
}

impl fmt::Debug for CssAst {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CssAst")
    }
}

impl CssAst {
    pub fn new(file: &File, context: Arc<Context>, css_modules: bool) -> Result<Self> {
        let fm = context.meta.css.cm.new_source_file(
            FileName::Real(file.relative_path.clone()).into(),
            file.get_content_raw(),
        );
        let config = parser::parser::ParserConfig {
            css_modules,
            legacy_ie: true,
            legacy_nesting: true,
            ..Default::default()
        };

        let comments = context.meta.css.comments.clone();

        let lexer = parser::lexer::Lexer::new(StringInput::from(&*fm), Some(&comments), config);
        let mut parser = parser::parser::Parser::new(lexer, config);
        let parse_result = parser.parse_all();
        let mut ast_errors = parser.take_errors();
        if parse_result.is_err() {
            ast_errors.push(parse_result.clone().unwrap_err());
        };
        if !ast_errors.is_empty() && !file.is_under_node_modules {
            let errors = ast_errors
                .iter()
                .map(|err| {
                    error::code_frame(
                        error::ErrorSpan::Css((*err.clone().into_inner()).0),
                        err.message().to_string().as_str(),
                        context.clone(),
                    )
                })
                .collect::<Vec<String>>();
            return Err(anyhow!(error::ParseError::CSSParseError {
                messages: errors.join("\n")
            }));
        }
        let ast = parse_result./*safe*/unwrap();
        Ok(Self {
            ast,
            path: file.relative_path.to_string_lossy().to_string(),
        })
    }

    pub fn build(
        path: &str,
        content: &str,
        context: Arc<Context>,
        css_modules: bool,
    ) -> Result<Self> {
        CssAst::new(
            &File::with_content(
                path.to_string(),
                Content::Css(content.to_string()),
                context.clone(),
            ),
            context.clone(),
            css_modules,
        )
    }

    pub fn analyze_deps(&self) -> Vec<Dependency> {
        let mut visitor = CSSDepAnalyzer::new();
        self.ast.visit_with(&mut visitor);
        visitor.dependencies
    }

    pub fn transform(&mut self, mut_visitors: &mut Vec<Box<dyn visit::VisitMut>>) -> Result<()> {
        let ast = &mut self.ast;
        for visitor in mut_visitors {
            ast.visit_mut_with(visitor);
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn generate(&self, context: Arc<Context>) -> Result<CSSAstGenerated> {
        let mut code = String::new();
        let mut source_map = Vec::new();
        let writer = BasicCssWriter::new(
            &mut code,
            Some(&mut source_map),
            BasicCssWriterConfig::default(),
        );
        let mut gen = CodeGenerator::new(
            writer,
            CodegenConfig {
                minify: context.config.minify && matches!(context.config.mode, Mode::Production),
            },
        );
        gen.emit(&self.ast).map_err(|err| {
            anyhow!(error::GenerateError::CSSGenerateError {
                message: err.to_string()
            })
        })?;

        let buf = build_source_map_to_buf(&source_map, &context.meta.css.cm);
        let sourcemap = String::from_utf8(buf).unwrap();
        if matches!(context.config.devtool, Some(DevtoolConfig::SourceMap)) {
            let filename = &self.path;
            code.push_str(format!("\n/*# sourceMappingURL={filename}.map*/").as_str());
        } else if matches!(context.config.devtool, Some(DevtoolConfig::InlineSourceMap)) {
            code.push_str(
                format!(
                    "\n/*# sourceMappingURL=data:application/json;charset=utf-8;base64,{}*/",
                    base64_encode(&sourcemap)
                )
                .as_str(),
            );
        }

        Ok(CSSAstGenerated { code, sourcemap })
    }

    pub fn compile_css_modules(path: &str, ast: &mut Stylesheet) -> TransformResult {
        compile(
            ast,
            CssModuleRename {
                path: path.to_string(),
            },
        )
    }

    pub fn generate_css_modules_exports(
        path: &str,
        ast: &mut Stylesheet,
        export_only: bool,
    ) -> String {
        let result = Self::compile_css_modules(path, ast);
        let mut export_names = Vec::new();
        for (name, classes) in result.renamed.iter() {
            let mut after_transform_classes = Vec::new();
            for v in classes {
                match v {
                    CssClassName::Local { name } => {
                        after_transform_classes.push(name.value.to_string());
                    }
                    CssClassName::Global { name } => {
                        // e.g. composes foo from global
                        after_transform_classes.push(name.value.to_string());
                    }
                    CssClassName::Import { name, from: _ } => {
                        // TODO: support composes from external files
                        after_transform_classes.push(name.value.to_string());
                    }
                }
            }
            export_names.push((name, after_transform_classes));
        }
        let export_names = export_names
            .iter()
            .map(|(name, classes)| format!("\"{}\": `{}`", name, classes.join(" ").trim()))
            .collect::<Vec<String>>()
            .join(",");

        if export_only {
            format!(
                r#"
export default {{{}}}
"#,
                export_names
            )
        } else {
            format!(
                r#"
import "{}?modules";
export default {{{}}}
"#,
                path, export_names
            )
        }
    }
}

pub struct CSSAstGenerated {
    pub code: String,
    pub sourcemap: String,
}

struct CssModuleRename {
    pub path: String,
}

impl TransformConfig for CssModuleRename {
    fn new_name_for(&self, local: &atoms::JsWord) -> atoms::JsWord {
        let name = local.to_string();
        let new_name = ident_name(&self.path, &name);
        new_name.into()
    }
}

fn ident_name(path: &str, name: &str) -> String {
    let source = format!("{}__{}", path, name);
    let digest = md5::compute(source);
    let hash = url_safe_base64_encode(digest.0);
    let hash_slice = hash[..8].to_string();
    format!("{}-{}", name, hash_slice)
}
