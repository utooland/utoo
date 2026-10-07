use anyhow::{Result, bail};
use turbo_rcstr::rcstr;
use turbo_tasks::{ResolvedVc, Vc};
use turbopack::{ModuleAssetContext, transition::Transition};
use turbopack_core::{compile_time_info::CompileTimeInfo, ident::Layer, module::Module};
use turbopack_ecmascript::chunk::EcmascriptChunkPlaceable;
use turbopack_resolve::resolve_options_context::ResolveOptionsContext;

use turbopack::module_options::ModuleOptionsContext;
use turbopack::transition::TransitionOptions;

use super::server_reference_module::ServerReferenceModule;

/// Transition that processes a `"use server"` module in a server-side
/// (Node.js) `ModuleAssetContext`.
///
/// When the client graph encounters a `"use server"` module, the
/// `ServerDirectiveTransformer` replaces it with a proxy and attaches
/// a `__turbopack_transition__: "server-reference"` annotation.
/// Turbopack looks up this named transition and calls `process()`,
/// which re-processes the *original* source in the server context
/// and wraps it in a `ServerReferenceModule`.
#[turbo_tasks::value(shared)]
pub struct ServerReferenceTransition {
    server_compile_time_info: ResolvedVc<CompileTimeInfo>,
    server_module_options_context: ResolvedVc<ModuleOptionsContext>,
    server_resolve_options_context: ResolvedVc<ResolveOptionsContext>,
}

#[turbo_tasks::value_impl]
impl ServerReferenceTransition {
    #[turbo_tasks::function]
    pub fn new(
        server_compile_time_info: ResolvedVc<CompileTimeInfo>,
        server_module_options_context: ResolvedVc<ModuleOptionsContext>,
        server_resolve_options_context: ResolvedVc<ResolveOptionsContext>,
    ) -> Vc<Self> {
        ServerReferenceTransition {
            server_compile_time_info,
            server_module_options_context,
            server_resolve_options_context,
        }
        .cell()
    }
}

#[turbo_tasks::value_impl]
impl Transition for ServerReferenceTransition {
    /// Override process_context to use a server-specific layer.
    ///
    /// The default impl inherits the caller's layer (e.g. `[client]`), which
    /// would cause duplicate idents since the proxy module also lives in
    /// `[client]`. We switch to `[server]` so the server-processed module
    /// gets a distinct ident like `actions.ts [server] (ecmascript)`.
    #[turbo_tasks::function]
    async fn process_context(
        self: Vc<Self>,
        _module_asset_context: Vc<ModuleAssetContext>,
    ) -> Result<Vc<ModuleAssetContext>> {
        let this = self.await?;

        // Use a server-specific layer instead of inheriting the caller's layer
        let layer = Layer::new_with_user_friendly_name(rcstr!("server"), rcstr!("Nodejs"));

        // Use empty transitions — the server context should NOT inherit the
        // client's "server-reference" transition to avoid infinite recursion
        Ok(ModuleAssetContext::new(
            TransitionOptions::default().cell(),
            *this.server_compile_time_info,
            *this.server_module_options_context,
            *this.server_resolve_options_context,
            layer,
        ))
    }

    #[turbo_tasks::function]
    async fn process_module(
        self: Vc<Self>,
        module: Vc<Box<dyn Module>>,
        _context: Vc<ModuleAssetContext>,
    ) -> Result<Vc<Box<dyn Module>>> {
        let module = module.to_resolved().await?;

        let Some(server_module) =
            ResolvedVc::try_sidecast::<Box<dyn EcmascriptChunkPlaceable>>(module)
        else {
            bail!("server reference module is not ecmascript chunk placeable");
        };

        let ident = module.ident().to_resolved().await?;

        Ok(Vc::upcast(ServerReferenceModule::new(
            *ident,
            *server_module,
        )))
    }
}
