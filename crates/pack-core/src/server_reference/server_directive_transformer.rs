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

fn is_server_module(program: &Program) -> bool {
    match program {
        Program::Module(m) => m
            .body
            .iter()
            .filter_map(|item| item.as_stmt())
            .filter_map(|stmt| {
                if let swc_core::ecma::ast::Lit::Str(str) = stmt.as_expr()?.expr.as_lit()? {
                    Some(str)
                } else {
                    None
                }
            })
            .take_while(|_| true)
            .any(|s| &*s.value == "use server"),
        Program::Script(s) => s
            .body
            .iter()
            .filter_map(|stmt| {
                if let swc_core::ecma::ast::Lit::Str(str) = stmt.as_expr()?.expr.as_lit()? {
                    Some(str)
                } else {
                    None
                }
            })
            .take_while(|_| true)
            .any(|s| &*s.value == "use server"),
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

        // Use the file path as the module ID for action dispatch
        let module_id = ctx.file_name_str.to_string();
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
