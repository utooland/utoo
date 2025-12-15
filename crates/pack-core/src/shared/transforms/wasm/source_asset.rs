use std::io::Write;

use anyhow::Result;
use turbo_rcstr::rcstr;
use turbo_tasks::{ResolvedVc, Vc};
use turbo_tasks_fs::rope::RopeBuilder;
use turbopack_core::{
    asset::{Asset, AssetContent},
    ident::AssetIdent,
    source::Source,
};

/// A source asset that transforms a WASM file into javascript code which exports
/// the URL of the WASM file.
#[turbo_tasks::value(shared)]
pub struct StaticWasmFileSource {
    pub wasm: ResolvedVc<Box<dyn Source>>,
}

#[turbo_tasks::value_impl]
impl Source for StaticWasmFileSource {
    #[turbo_tasks::function]
    fn ident(&self) -> Vc<AssetIdent> {
        self.wasm
            .ident()
            .with_modifier(rcstr!("static wasm url"))
            .rename_as("*.mjs".into())
    }
}

#[turbo_tasks::value_impl]
impl Asset for StaticWasmFileSource {
    #[turbo_tasks::function]
    async fn content(&self) -> Result<Vc<AssetContent>> {
        let mut result = RopeBuilder::from("");

        // Import the WASM URL from the static asset
        writeln!(result, "import wasmUrl from \"WASM\";")?;
        // Export the URL as default export
        writeln!(result, "export default wasmUrl;")?;

        Ok(AssetContent::File(
            turbo_tasks_fs::FileContent::Content(result.build().into()).resolved_cell(),
        )
        .cell())
    }
}
