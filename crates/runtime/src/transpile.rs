use std::path::Path;

use anyhow::{Context, Result};
use swc_core::{
    common::{
        FileName, Globals, Mark, SourceMap,
        sync::Lrc,
    },
    ecma::{
        ast::{EsVersion, Pass, Program},
        codegen::{self, Emitter, text_writer::JsWriter},
        parser::{EsSyntax, Syntax, TsSyntax, parse_file_as_module},
        transforms::{
            base::{
                fixer::fixer,
                helpers::{HELPERS, Helpers, inject_helpers},
                hygiene::hygiene,
                resolver,
            },
            module::common_js,
            proposal::decorators::{self, decorators},
            typescript::strip_type,
        },
        visit::VisitMutWith,
    },
};

/// Transpile TypeScript/TSX/JSX source to JavaScript by stripping types.
/// Also transforms legacy TypeScript decorators into __decorate() calls,
/// and converts ES module imports/exports to CommonJS require/module.exports.
pub fn transpile_to_js(source: &str, path: &Path) -> Result<String> {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(FileName::Real(path.to_path_buf()).into(), source.to_string());

    let syntax = syntax_for_path(path);
    let is_ts = matches!(syntax, Syntax::Typescript(_));

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
    let module = swc_core::common::GLOBALS.set(&globals, || {
        let mut module = module;
        let unresolved_mark = Mark::new();
        let top_level_mark = Mark::new();

        // Resolver pass: marks identifiers with correct scope marks
        // (required by decorator and module transforms)
        module.visit_mut_with(&mut resolver(unresolved_mark, top_level_mark, is_ts));

        HELPERS.set(&Helpers::new(false), || {
            // Transform legacy TypeScript decorators into __decorate() calls
            if is_ts {
                let mut program = Program::Module(module);
                decorators(decorators::Config {
                    legacy: true,
                    emit_metadata: false,
                    ..Default::default()
                }).process(&mut program);
                module = match program {
                    Program::Module(m) => m,
                    _ => unreachable!(),
                };
            }

            // Strip TypeScript types
            module.visit_mut_with(&mut strip_type());

            // Convert ES module imports/exports to CommonJS require/module.exports
            let mut program = Program::Module(module);
            common_js::common_js(
                Default::default(), // Resolver::Default - paths unchanged
                unresolved_mark,
                common_js::Config::default(),
                common_js::FeatureFlag {
                    support_arrow: true,
                    support_block_scoping: true,
                },
            ).process(&mut program);
            module = match program {
                Program::Module(m) => m,
                _ => unreachable!(),
            };

            // Inject SWC runtime helper functions.
            // inject_helpers emits ESM `import` from "@swc/helpers/..."
            // which we post-process into require() calls below.
            module.visit_mut_with(&mut inject_helpers(unresolved_mark));

            // Hygiene renames identifiers that collide only by SyntaxContext
            // (e.g. two imports whose specifiers end in the same segment both
            // generated as `_promises`) so codegen emits distinct names. Fixer
            // re-adds parens the AST implies but the printer would otherwise drop
            // (e.g. the `(0, mod.fn)(...)` indirect-call sequence).
            let mut program = Program::Module(module);
            hygiene().process(&mut program);
            fixer(None).process(&mut program);
            module = match program {
                Program::Module(m) => m,
                _ => unreachable!(),
            };

            module
        })
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

    let code = String::from_utf8(buf).context("Emitted code is not valid UTF-8")?;

    // Post-process the generated code to fix two SWC codegen issues:
    // 1. inject_helpers emits ESM imports after common_js: convert to require()
    // 2. SeqExpr (0, _mod.default) loses parentheses in const declarations
    let code = fix_swc_codegen_issues(&code);
    Ok(code)
}

/// Fix two SWC codegen issues in CJS output:
/// 1. Convert remaining `import { _ as X } from "@swc/helpers/_/Y";` to `const X = require("@swc/helpers/_/Y")._;`
/// 2. Fix missing parentheses: `= 0, _mod.default(` → `= (0, _mod.default)(`
fn fix_swc_codegen_issues(code: &str) -> String {
    let mut result = String::with_capacity(code.len());
    for line in code.lines() {
        let trimmed = line.trim();

        // Fix 1: Convert @swc/helpers ESM imports to CJS require()
        if trimmed.starts_with("import ") && trimmed.contains("@swc/helpers/") {
            if let (Some(as_pos), Some(from_pos)) = (trimmed.find(" as "), trimmed.find("from ")) {
                let name_end = trimmed[as_pos + 4..].find(|c: char| !c.is_alphanumeric() && c != '_');
                if let Some(end) = name_end {
                    let name = &trimmed[as_pos + 4..as_pos + 4 + end];
                    let after_from = &trimmed[from_pos + 5..];
                    let quote = after_from.chars().next().unwrap_or('"');
                    if let Some(close) = after_from[1..].find(quote) {
                        let module_path = &after_from[1..1 + close];
                        result.push_str(&format!(
                            "const {} = require(\"{}\")._;",
                            name, module_path
                        ));
                        result.push('\n');
                        continue;
                    }
                }
            }
        }

        result.push_str(line);
        result.push('\n');
    }
    if !code.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }
    // Fix 2: SWC codegen sometimes drops the parens around the `(0, obj.method)`
    // indirect-call sequence (used to call a method without `this` binding),
    // emitting e.g. `()=>0, _mod.fn(x)`. V8 then parses the identifier after the
    // comma as a new binding (`Identifier already declared`) or mis-scopes it.
    // Re-wrap any `0, _ident.member(` that isn't already parenthesized.
    fix_indirect_call_parens(&result)
}

/// Re-wrap dropped-paren SWC indirect calls: `0, _ident.member(` -> `(0, _ident.member)(`.
/// Only matches the SWC-generated marker (a bare `0, _<ident>.<chain>` immediately
/// followed by a call `(`, not already preceded by `(`). UTF-8 safe: slices only at
/// the ASCII byte positions it matched.
fn fix_indirect_call_parens(code: &str) -> String {
    let bytes = code.as_bytes();
    let len = bytes.len();
    let mut result = String::with_capacity(len + 16);
    let mut last = 0usize;
    let mut i = 0usize;
    while i < len {
        // marker: `0, _` where the `0` isn't already wrapped in `(`
        if bytes[i] == b'0'
            && i + 3 < len
            && bytes[i + 1] == b','
            && bytes[i + 2] == b' '
            && bytes[i + 3] == b'_'
            && (i == 0 || bytes[i - 1] != b'(')
        {
            // parse `_ident(.ident)*`
            let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'$';
            let mut j = i + 3;
            while j < len && is_ident(bytes[j]) {
                j += 1;
            }
            loop {
                if j < len && bytes[j] == b'.' {
                    let mut k = j + 1;
                    while k < len && is_ident(bytes[k]) {
                        k += 1;
                    }
                    if k > j + 1 {
                        j = k;
                        continue;
                    }
                }
                break;
            }
            // Only an indirect *call*: the member expression is immediately invoked.
            if j > i + 4 && j < len && bytes[j] == b'(' {
                result.push_str(&code[last..i]);
                result.push('(');
                result.push_str(&code[i..j]);
                result.push(')');
                last = j;
                i = j;
                continue;
            }
        }
        i += 1;
    }
    result.push_str(&code[last..]);
    result
}

fn syntax_for_path(path: &Path) -> Syntax {
    match path.extension().and_then(|e| e.to_str()) {
        Some("ts") => Syntax::Typescript(TsSyntax {
            tsx: false,
            decorators: true,
            ..Default::default()
        }),
        Some("tsx") => Syntax::Typescript(TsSyntax {
            tsx: true,
            decorators: true,
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
    fn indirect_call_parens_preserved() {
        // The SWC `(0, mod.fn)(...)` indirect-call parens must survive codegen,
        // otherwise V8 reparses the identifier after the comma as a new binding.
        let src = "import { debuglog } from \"node:util\";\nimport path from \"node:path\";\nexport const foo = () => debuglog(\"x\")(path.sep);\n";
        let out = transpile_to_js(src, Path::new("test.js")).unwrap();
        assert!(
            out.contains("(0, _nodeutil.debuglog)"),
            "indirect-call parens dropped:\n{out}"
        );
        assert!(!out.contains("=>0, _nodeutil"), "unwrapped seq in arrow body:\n{out}");
    }

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

    #[test]
    fn transform_decorators() {
        let src = r#"
            function MyDec() { return function(target: any) {}; }
            @MyDec()
            class Foo {
                bar: string = "hello";
            }
        "#;
        let out = transpile_to_js(src, Path::new("test.ts")).unwrap();
        assert!(!out.contains("@MyDec"));
        assert!(out.contains("Foo"));
    }

    #[test]
    fn transform_esm_to_cjs() {
        let src = r#"
            import { foo } from './bar';
            export const baz = foo + 1;
        "#;
        let out = transpile_to_js(src, Path::new("test.ts")).unwrap();
        // Should not contain import/export statements
        assert!(!out.contains("import "));
        assert!(!out.contains("export "));
        // Should contain require
        assert!(out.contains("require"));
    }

    #[test]
    fn transform_class_with_readonly_props() {
        let src = r#"
            import util from 'node:util';
            import { AccessLevel, SingletonProto, Inject, Logger, HttpClient } from 'chair/tegg';

            @SingletonProto({ accessLevel: AccessLevel.PUBLIC })
            export class LogService {
                @Inject()
                private readonly logger: Logger;

                @Inject()
                private httpclient: HttpClient;

                public appendUrl = '';
            }
        "#;
        let out = transpile_to_js(src, Path::new("test.ts")).unwrap();
        eprintln!("=== READONLY PROPS OUTPUT ===\n{}\n=== END ===", out);
        assert!(!out.contains("import "), "Output still contains 'import': {}", out);
    }

    #[test]
    fn transform_default_import_to_cjs() {
        let src = r#"
            import os from 'node:os';
            import { foo } from './bar';
            console.log(os.hostname(), foo);
        "#;
        let out = transpile_to_js(src, Path::new("test.ts")).unwrap();
        eprintln!("=== DEFAULT IMPORT OUTPUT ===\n{}\n=== END ===", out);
        assert!(!out.contains("import "), "Output still contains 'import ': {}", out);
        assert!(out.contains("require"), "Output missing 'require': {}", out);
    }

    #[test]
    fn transform_default_import_called_directly() {
        // Regression test: `dayjs()` (default import called as function)
        // SWC emits `(0, _dayjs.default)()` but codegen may omit parens,
        // producing `const time = 0, _dayjs.default()...` which is a SyntaxError.
        let src = r#"
            import dayjs from 'dayjs';
            const time = dayjs().format('YYYY');
            console.log(time);
        "#;
        let out = transpile_to_js(src, Path::new("test.ts")).unwrap();
        eprintln!("=== DIRECT DEFAULT CALL OUTPUT ===\n{}\n=== END ===", out);
        // The output should NOT have bare `= 0, _dayjs.default` (missing parens)
        assert!(!out.contains("= 0, _dayjs"), "Missing parens in comma expr: {}", out);
        assert!(!out.contains("import "), "Output still contains 'import': {}", out);
    }

    #[test]
    fn transform_decorators_with_esm() {
        // Mirrors UtooController.ts: decorators + ESM imports + export class
        let src = r#"
            import { WebGWController, Inject, Middleware } from 'chair/tegg';
            import { CommonResult, SUCCESS, handleError } from 'chair/tegg/errorcode';
            import { UtooService } from '@/src/service/UtooService';

            function WebGWMethod() { return function(t: any, k: string, d: any) { return d; }; }

            @WebGWController()
            @Middleware(handleError)
            export class UtooController {
                @Inject()
                private utooService: UtooService;

                @WebGWMethod()
                async deps(): Promise<void> {
                    console.log("deps");
                }
            }
        "#;
        let out = transpile_to_js(src, Path::new("test.ts")).unwrap();
        eprintln!("=== TRANSPILED OUTPUT ===\n{}\n=== END ===", out);
        // Should NOT contain ES module syntax
        assert!(!out.contains("import "), "Output still contains 'import ': {}", out);
        assert!(!out.contains("export "), "Output still contains 'export ': {}", out);
        // Should contain require (CJS)
        assert!(out.contains("require"), "Output missing 'require': {}", out);
        // Should contain the class
        assert!(out.contains("UtooController"));
    }
}
