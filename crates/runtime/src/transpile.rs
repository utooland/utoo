use std::path::Path;

use anyhow::{Context, Result};
use swc_core::{
    common::{
        FileName, Globals, SourceMap,
        sync::Lrc,
    },
    ecma::{
        ast::EsVersion,
        codegen::{self, Emitter, text_writer::JsWriter},
        parser::{EsSyntax, Syntax, TsSyntax, parse_file_as_module},
        transforms::typescript::strip_type,
        visit::VisitMutWith,
    },
};

/// Transpile TypeScript/TSX/JSX source to JavaScript by stripping types.
pub fn transpile_to_js(source: &str, path: &Path) -> Result<String> {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(FileName::Real(path.to_path_buf()).into(), source.to_string());

    let syntax = syntax_for_path(path);

    let mut errors = vec![];
    let module = parse_file_as_module(
        &fm,
        syntax,
        EsVersion::Es2022,
        None,
        &mut errors,
    )
    .map_err(|e| anyhow::anyhow!("Parse error: {:?}", e))
    .context("Failed to parse module")?;

    if !errors.is_empty() {
        let msgs: Vec<String> = errors.iter().map(|e| format!("{e:?}")).collect();
        anyhow::bail!("Parse errors:\n{}", msgs.join("\n"));
    }

    let globals = Globals::default();
    let mut module = module;
    swc_core::common::GLOBALS.set(&globals, || {
        module.visit_mut_with(&mut strip_type());
    });

    let mut buf = vec![];
    {
        let wr = JsWriter::new(cm.clone(), "\n", &mut buf, None);
        let mut emitter = Emitter {
            cfg: codegen::Config::default().with_target(EsVersion::Es2022),
            cm,
            comments: None,
            wr: Box::new(wr),
        };
        emitter
            .emit_module(&module)
            .context("Failed to emit JavaScript")?;
    }

    String::from_utf8(buf).context("Emitted code is not valid UTF-8")
}

fn syntax_for_path(path: &Path) -> Syntax {
    match path.extension().and_then(|e| e.to_str()) {
        Some("ts") => Syntax::Typescript(TsSyntax {
            tsx: false,
            ..Default::default()
        }),
        Some("tsx") => Syntax::Typescript(TsSyntax {
            tsx: true,
            ..Default::default()
        }),
        Some("jsx") => Syntax::Es(EsSyntax {
            jsx: true,
            ..Default::default()
        }),
        _ => Syntax::Es(EsSyntax::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_basic_types() {
        let src = r#"const x: number = 42; console.log(x);"#;
        let out = transpile_to_js(src, Path::new("test.ts")).unwrap();
        assert!(!out.contains(": number"));
        assert!(out.contains("42"));
        assert!(out.contains("console.log"));
    }

    #[test]
    fn strip_interface() {
        let src = r#"
            interface Foo { bar: string }
            const obj: Foo = { bar: "hello" };
            console.log(obj.bar);
        "#;
        let out = transpile_to_js(src, Path::new("test.ts")).unwrap();
        assert!(!out.contains("interface"));
        assert!(out.contains("hello"));
    }

    #[test]
    fn passthrough_plain_js() {
        let src = r#"const x = 42;"#;
        let out = transpile_to_js(src, Path::new("test.js")).unwrap();
        assert!(out.contains("42"));
    }
}
