use sha2::{Digest, Sha256};
use swc_core::{
    common::DUMMY_SP,
    ecma::{
        ast::{
            BindingIdent, CallExpr, Callee, Decl, ExportDecl, ExportSpecifier, Expr, ExprOrSpread,
            Ident, ImportDecl, ImportNamedSpecifier, ImportSpecifier, Lit, Module, ModuleDecl,
            ModuleExportName, ModuleItem, ObjectPatProp, Pat, Program, Str, VarDecl, VarDeclKind,
            VarDeclarator,
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
fn collect_binding_names(pat: &Pat, names: &mut Vec<(String, Option<Ident>)>) {
    match pat {
        Pat::Ident(ident) => {
            names.push((ident.id.sym.as_ref().to_string(), Some(ident.id.clone())))
        }
        Pat::Object(obj) => {
            for prop in &obj.props {
                match prop {
                    ObjectPatProp::Assign(assign) => {
                        names.push((
                            assign.key.sym.as_ref().to_string(),
                            Some(assign.key.id.clone()),
                        ));
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
/// Returns a list of (export_name, local_ident). Handles:
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
pub fn collect_exports(module: &Module) -> Vec<(String, Option<Ident>)> {
    let mut exports = Vec::new();
    for item in &module.body {
        match item {
            ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(ExportDecl { decl, .. })) => match decl {
                Decl::Fn(f) => {
                    exports.push((f.ident.sym.as_ref().to_string(), Some(f.ident.clone())))
                }
                Decl::Var(v) => {
                    for decl in &v.decls {
                        collect_binding_names(&decl.name, &mut exports);
                    }
                }
                Decl::Class(c) => {
                    exports.push((c.ident.sym.as_ref().to_string(), Some(c.ident.clone())))
                }
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
                        let local_ident = match &n.orig {
                            ModuleExportName::Ident(i) => Some(i.clone()),
                            ModuleExportName::Str(_) => None,
                        };
                        exports.push((name, local_ident));
                    }
                }
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultDecl(d)) => {
                let local_ident = match &d.decl {
                    swc_core::ecma::ast::DefaultDecl::Class(c) => c.ident.clone(),
                    swc_core::ecma::ast::DefaultDecl::Fn(f) => f.ident.clone(),
                    _ => None,
                };
                exports.push(("default".to_string(), local_ident));
            }
            ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultExpr(_)) => {
                exports.push(("default".to_string(), None));
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
/// import "./actions" with { __turbopack_transition__: "server-reference" };
/// import { createServerReference } from "@utoo/server-function/client";
/// export const createUser = createServerReference("a1b2c3...", "createUser");
/// export const deleteUser = createServerReference("d4e5f6...", "deleteUser");
/// ```
pub fn create_server_proxy_module(
    transition_name: &str,
    client_reference: &str,
    target_import: &str,
    module_id: &str,
    exports: &[(String, Option<Ident>)],
) -> Program {
    let create_ref_ident = Ident::new("createServerReference".into(), DUMMY_SP, Default::default());

    let mut body: Vec<ModuleItem> = Vec::new();

    // import "./self" with { __turbopack_transition__: "server-reference" };
    // This import is for module graph discovery only — the ServerReferenceModule
    // produces empty content in client chunks. Keep it side-effect-only so
    // TypeScript import elision cannot remove the transition edge.
    body.push(ModuleItem::ModuleDecl(ModuleDecl::Import(ImportDecl {
        specifiers: vec![],
        src: Box::new(target_import.into()),
        type_only: false,
        with: Some(with_clause(&[
            (TURBOPACK_HELPER.as_str(), "true"),
            (ANNOTATION_TRANSITION, transition_name),
        ])),
        span: DUMMY_SP,
        phase: Default::default(),
    })));

    // import { createServerReference } from "<client_reference>";
    body.push(ModuleItem::ModuleDecl(ModuleDecl::Import(ImportDecl {
        specifiers: vec![ImportSpecifier::Named(ImportNamedSpecifier {
            local: create_ref_ident.clone(),
            imported: None,
            span: DUMMY_SP,
            is_type_only: false,
        })],
        src: Box::new(client_reference.into()),
        type_only: false,
        with: None,
        span: DUMMY_SP,
        phase: Default::default(),
    })));

    // For each export, generate:
    //   export const <name> = createServerReference("<hashed_action_id>", "<export_name>");
    for (export_name, _) in exports {
        let action_id = generate_action_id(module_id, export_name);

        // createServerReference("<action_id>", "<export_name>")
        let call_expr = Expr::Call(CallExpr {
            callee: Callee::Expr(Box::new(Expr::Ident(create_ref_ident.clone()))),
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
                    expr: Box::new(Expr::Lit(Lit::Str(Str {
                        value: export_name.as_str().into(),
                        span: DUMMY_SP,
                        raw: None,
                    }))),
                },
            ],
            span: DUMMY_SP,
            type_args: None,
            ctxt: Default::default(),
        });

        if export_name == "default" {
            // export default createServerReference(...)
            body.push(ModuleItem::ModuleDecl(ModuleDecl::ExportDefaultExpr(
                swc_core::ecma::ast::ExportDefaultExpr {
                    expr: Box::new(call_expr),
                    span: DUMMY_SP,
                },
            )));
        } else {
            // export const <name> = createServerReference(...)
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
                        init: Some(Box::new(call_expr)),
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

pub fn create_server_registration_ast(
    program: &mut Program,
    register_module: &str,
    module_id: &str,
    exports: &[(String, Option<Ident>)],
) {
    let register_ident = private_ident!("registerServerReference");
    let mut stmts = Vec::new();

    // import { registerServerReference } from "<register_module>";
    stmts.push(ModuleItem::ModuleDecl(ModuleDecl::Import(ImportDecl {
        specifiers: vec![ImportSpecifier::Named(ImportNamedSpecifier {
            local: register_ident.clone(),
            imported: None,
            span: DUMMY_SP,
            is_type_only: false,
        })],
        src: Box::new(register_module.into()),
        type_only: false,
        with: None,
        span: DUMMY_SP,
        phase: Default::default(),
    })));

    for (export_name, local_ident_opt) in exports {
        let action_id = generate_action_id(module_id, export_name);

        let export_ident = match local_ident_opt {
            Some(ident) => ident.clone(),
            None => {
                if export_name == "default" {
                    // Cannot safely append `registerServerReference` for an anonymous default export
                    // because there is no local identifier to reference.
                    continue;
                }
                Ident::new(export_name.as_str().into(), DUMMY_SP, Default::default())
            }
        };

        // registerServerReference(fn, "<action_id>", "<export_name>")
        let call_expr = Expr::Call(CallExpr {
            callee: Callee::Expr(Box::new(Expr::Ident(register_ident.clone()))),
            args: vec![
                ExprOrSpread {
                    spread: None,
                    expr: Box::new(Expr::Ident(export_ident)),
                },
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
                    expr: Box::new(Expr::Lit(Lit::Str(Str {
                        value: export_name.as_str().into(),
                        span: DUMMY_SP,
                        raw: None,
                    }))),
                },
            ],
            span: DUMMY_SP,
            type_args: None,
            ctxt: Default::default(),
        });

        stmts.push(ModuleItem::Stmt(swc_core::ecma::ast::Stmt::Expr(
            swc_core::ecma::ast::ExprStmt {
                span: DUMMY_SP,
                expr: Box::new(call_expr),
            },
        )));
    }

    match program {
        Program::Module(m) => {
            m.body.extend(stmts);
        }
        Program::Script(_) => {}
    }
}
