use sha2::{Digest, Sha256};
use swc_core::{
    common::DUMMY_SP,
    ecma::{
        ast::{
            ArrowExpr, BindingIdent, BlockStmtOrExpr, CallExpr, Callee, Decl, ExportDecl,
            ExportSpecifier, Expr, ExprOrSpread, Ident, ImportDecl, ImportNamedSpecifier,
            ImportSpecifier, ImportStarAsSpecifier, Lit, Module, ModuleDecl, ModuleExportName,
            ModuleItem, ObjectPatProp, Pat, Program, Str, VarDecl, VarDeclKind, VarDeclarator,
        },
        utils::private_ident,
    },
};
use turbopack_ecmascript::{
    TURBOPACK_HELPER,
    annotations::{ANNOTATION_TRANSITION, with_clause},
};

/// Generates a stable, content-based action ID by hashing the module path and
/// export name with SHA-256.
///
/// The output is a 64-character hex string: `hex(SHA-256(module_id + '#' + export_name))`.
///
/// This ensures IDs are:
/// - **Unique**: different modules/exports always produce different IDs
/// - **Stable**: the same source produces the same ID across rebuilds
/// - **Opaque**: internal file paths are not leaked to the client
pub fn generate_action_id(module_id: &str, export_name: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(module_id.as_bytes());
    hasher.update(b"#");
    hasher.update(export_name.as_bytes());
    // Truncate to 16 hex chars (64 bits) — sufficient for uniqueness
    // across any realistic project while keeping bundle size minimal.
    format!("{:x}", hasher.finalize())[..16].to_string()
}

/// Recursively collects all binding names from a destructuring pattern.
///
/// Handles identifiers, object destructuring (`{ a, b: c }`), array
/// destructuring (`[a, , b]`), rest patterns (`...rest`), and defaults
/// (`a = expr`).
fn collect_binding_names(pat: &Pat, names: &mut Vec<String>) {
    match pat {
        Pat::Ident(ident) => names.push(ident.id.sym.as_ref().to_string()),
        Pat::Object(obj) => {
            for prop in &obj.props {
                match prop {
                    ObjectPatProp::Assign(assign) => {
                        names.push(assign.key.sym.as_ref().to_string());
                    }
                    ObjectPatProp::KeyValue(kv) => {
                        collect_binding_names(&kv.value, names);
                    }
                    ObjectPatProp::Rest(rest) => {
                        collect_binding_names(&rest.arg, names);
                    }
                }
            }
        }
        Pat::Array(arr) => {
            for elem in arr.elems.iter().flatten() {
                collect_binding_names(elem, names);
            }
        }
        Pat::Rest(rest) => {
            collect_binding_names(&rest.arg, names);
        }
        Pat::Assign(assign) => {
            collect_binding_names(&assign.left, names);
        }
        Pat::Expr(_) | Pat::Invalid(_) => {}
    }
}

/// Extracts all named exports from a `"use server"` module.
///
/// Returns a list of export names (strings). Handles:
/// - `export function foo() {}`
/// - `export async function foo() {}`
/// - `export const foo = ...`
/// - `export const { a, b } = ...` (object destructuring)
/// - `export const [a, b] = ...` (array destructuring)
/// - `export { foo, bar }`
/// - `export default ...` → represented as `"default"`
///
/// Note: `export * from '...'` is not supported because resolving the
/// re-exported names requires module graph analysis at this stage.
pub fn collect_exports(module: &Module) -> Vec<String> {
    let mut exports = Vec::new();
    for item in &module.body {
        match item {
            ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(ExportDecl { decl, .. })) => match decl {
                Decl::Fn(f) => exports.push(f.ident.sym.as_ref().to_string()),
                Decl::Var(v) => {
                    for decl in &v.decls {
                        collect_binding_names(&decl.name, &mut exports);
                    }
                }
                Decl::Class(c) => exports.push(c.ident.sym.as_ref().to_string()),
                _ => {}
            },
            ModuleItem::ModuleDecl(ModuleDecl::ExportNamed(named)) => {
                for spec in &named.specifiers {
                    if let ExportSpecifier::Named(n) = spec {
                        let name = match &n.exported {
                            Some(ModuleExportName::Ident(i)) => i.sym.as_ref().to_string(),
                            Some(ModuleExportName::Str(s)) => {
                                s.value.to_string_lossy().into_owned()
                            }
                            None => match &n.orig {
                                ModuleExportName::Ident(i) => i.sym.as_ref().to_string(),
                                ModuleExportName::Str(s) => s.value.to_string_lossy().into_owned(),
                            },
                        };
                        exports.push(name);
                    }
                }
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultDecl(_))
            | ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultExpr(_)) => {
                exports.push("default".to_string());
            }
            // TODO: support `export * from '...'` — requires module graph resolution
            _ => {}
        }
    }
    exports
}

/// Creates a self-contained client-side proxy module for a `"use server"` module.
///
/// For each exported function, generates a `callServer` wrapper that invokes
/// the configured transport module.
///
/// Also includes a "ghost" transition import (`import * as _server from "./self"
/// with { __turbopack_transition__: "server-reference" }`) that doesn't produce
/// any runtime code but allows the build system to discover server modules in
/// the module graph and build them as Node.js targets.
///
/// Generated code for a module with `createUser` and `deleteUser` exports:
/// ```js
/// import * as _server from "./actions" with { __turbopack_transition__: "server-reference" };
/// import { callServer } from "@evjs/client/transport";
/// export const createUser = (...args) => callServer("a1b2c3...", args);
/// export const deleteUser = (...args) => callServer("d4e5f6...", args);
/// ```
pub fn create_server_proxy_module(
    transition_name: &str,
    call_server_module: &str,
    target_import: &str,
    module_id: &str,
    exports: &[String],
) -> Program {
    let call_server_ident = Ident::new("callServer".into(), DUMMY_SP, Default::default());

    let mut body: Vec<ModuleItem> = Vec::new();

    // import * as _server from "./self" with { __turbopack_transition__: "server-reference" };
    // This import is for module graph discovery only — the ServerReferenceModule
    // produces empty content in client chunks.
    let server_ref_ident = private_ident!("_server");
    body.push(ModuleItem::ModuleDecl(ModuleDecl::Import(ImportDecl {
        specifiers: vec![ImportSpecifier::Namespace(ImportStarAsSpecifier {
            local: server_ref_ident,
            span: DUMMY_SP,
        })],
        src: Box::new(target_import.into()),
        type_only: false,
        with: Some(with_clause(&[
            (TURBOPACK_HELPER.as_str(), "true"),
            (ANNOTATION_TRANSITION, transition_name),
        ])),
        span: DUMMY_SP,
        phase: Default::default(),
    })));

    // import { callServer } from "<call_server_module>";
    body.push(ModuleItem::ModuleDecl(ModuleDecl::Import(ImportDecl {
        specifiers: vec![ImportSpecifier::Named(ImportNamedSpecifier {
            local: call_server_ident.clone(),
            imported: None,
            span: DUMMY_SP,
            is_type_only: false,
        })],
        src: Box::new(call_server_module.into()),
        type_only: false,
        with: None,
        span: DUMMY_SP,
        phase: Default::default(),
    })));

    // For each export, generate:
    //   export const <name> = (...args) => callServer("<hashed_action_id>", args);
    for export_name in exports {
        let action_id = generate_action_id(module_id, export_name);
        let args_ident = Ident::new("args".into(), DUMMY_SP, Default::default());

        // callServer("<action_id>", args)
        let call_expr = Expr::Call(CallExpr {
            callee: Callee::Expr(Box::new(Expr::Ident(call_server_ident.clone()))),
            args: vec![
                ExprOrSpread {
                    spread: None,
                    expr: Box::new(Expr::Lit(Lit::Str(Str {
                        value: action_id.into(),
                        span: DUMMY_SP,
                        raw: None,
                    }))),
                },
                ExprOrSpread {
                    spread: None,
                    expr: Box::new(Expr::Ident(args_ident.clone())),
                },
            ],
            span: DUMMY_SP,
            type_args: None,
            ctxt: Default::default(),
        });

        // (...args) => callServer(...)
        let arrow = Expr::Arrow(ArrowExpr {
            params: vec![Pat::Rest(swc_core::ecma::ast::RestPat {
                dot3_token: DUMMY_SP,
                arg: Box::new(Pat::Ident(BindingIdent {
                    id: args_ident,
                    type_ann: None,
                })),
                span: DUMMY_SP,
                type_ann: None,
            })],
            body: Box::new(BlockStmtOrExpr::Expr(Box::new(call_expr))),
            is_async: false,
            is_generator: false,
            span: DUMMY_SP,
            type_params: None,
            return_type: None,
            ctxt: Default::default(),
        });

        if export_name == "default" {
            // export default (...args) => callServer(...)
            body.push(ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultExpr(
                swc_core::ecma::ast::ExportDefaultExpr {
                    expr: Box::new(arrow),
                    span: DUMMY_SP,
                },
            )));
        } else {
            // export const <name> = (...args) => callServer(...)
            let export_ident =
                Ident::new(export_name.as_str().into(), DUMMY_SP, Default::default());
            body.push(ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(ExportDecl {
                decl: Decl::Var(Box::new(VarDecl {
                    kind: VarDeclKind::Const,
                    decls: vec![VarDeclarator {
                        name: Pat::Ident(BindingIdent {
                            id: export_ident,
                            type_ann: None,
                        }),
                        init: Some(Box::new(arrow)),
                        span: DUMMY_SP,
                        definite: false,
                    }],
                    span: DUMMY_SP,
                    ctxt: Default::default(),
                    declare: false,
                })),
                span: DUMMY_SP,
            })));
        }
    }

    Program::Module(Module {
        body,
        shebang: None,
        span: DUMMY_SP,
    })
}
