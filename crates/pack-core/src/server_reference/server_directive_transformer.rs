use anyhow::Result;
use async_trait::async_trait;
use swc_core::ecma::{ast::Program, transforms::base::resolver, visit::VisitMutWith};
use turbo_rcstr::RcStr;
use turbopack_ecmascript::{CustomTransformer, TransformContext};

use super::proxy::{collect_exports, create_server_proxy_module};

/// Detects `"use server"` directive in a module and replaces the program
/// with a client-side proxy containing `callServer` stubs and a
/// transition import for server-side module graph discovery.
#[derive(Debug)]
pub struct ServerDirectiveTransformer {
    transition_name: RcStr,
    call_server_module: RcStr,
}

impl ServerDirectiveTransformer {
    pub fn new(transition_name: RcStr, call_server_module: RcStr) -> Self {
        Self {
            transition_name,
            call_server_module,
        }
    }
}

/// Check if a sequence of statements begins with a `"use server"` directive.
///
/// Per the ECMAScript spec, directives are string literal expression statements
/// that appear before any other kind of statement. We stop scanning as soon as
/// we encounter a non-string-literal statement.
fn has_server_directive<'a>(stmts: impl Iterator<Item = &'a swc_core::ecma::ast::Stmt>) -> bool {
    for stmt in stmts {
        if let Some(expr) = stmt.as_expr()
            && let Some(swc_core::ecma::ast::Lit::Str(str)) = expr.expr.as_lit()
        {
            if &*str.value == "use server" {
                return true;
            }
            // Another string literal directive (e.g. "use strict") — keep scanning
            continue;
        }
        // Not a string literal expression — directive prologue is over
        break;
    }
    false
}

fn is_server_module(program: &Program) -> bool {
    match program {
        Program::Module(m) => has_server_directive(m.body.iter().filter_map(|item| item.as_stmt())),
        Program::Script(s) => has_server_directive(s.body.iter()),
    }
}

#[async_trait]
impl CustomTransformer for ServerDirectiveTransformer {
    #[tracing::instrument(level = tracing::Level::TRACE, name = "server_directive", skip_all)]
    async fn transform(&self, program: &mut Program, ctx: &TransformContext<'_>) -> Result<()> {
        if !is_server_module(program) {
            return Ok(());
        }

        // Extract exports from the original module BEFORE replacing it
        let exports = match program {
            Program::Module(m) => collect_exports(m),
            Program::Script(_) => vec![],
        };

        // Use the project-relative file path as the module ID for action dispatch.
        // file_path_str is unique across the project (e.g. "src/auth/actions.ts"),
        // whereas file_name_str would collide for same-named files in different dirs.
        let module_id = ctx.file_path_str.to_string();
        let target_import = format!("./{}", ctx.file_name_str);

        *program = create_server_proxy_module(
            self.transition_name.as_str(),
            self.call_server_module.as_str(),
            &target_import,
            &module_id,
            &exports,
        );
        program.visit_mut_with(&mut resolver(
            ctx.unresolved_mark,
            ctx.top_level_mark,
            false,
        ));

        Ok(())
    }
}
