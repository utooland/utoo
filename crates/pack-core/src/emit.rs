use anyhow::Result;
use tracing::Instrument;
use turbo_tasks::{TryFlatJoinIterExt, ValueToString, Vc};
use turbo_tasks_fs::{FileSystemPath, rebase};
use turbopack_core::{
    asset::Asset,
    output::{ExpandedOutputAssets, OutputAsset},
};

/// Emits all assets transitively reachable from the given chunks, that are
/// inside the node root or the client root.
///
/// Assets inside the given client root are rebased to the given client output
/// path.
#[turbo_tasks::function]
pub async fn emit_assets(
    assets: Vc<ExpandedOutputAssets>,
    _node_root: FileSystemPath,
    client_relative_path: FileSystemPath,
    client_output_path: FileSystemPath,
) -> Result<()> {
    let _: Vec<Vc<()>> = assets
        .await?
        .iter()
        .copied()
        .map(|asset| {
            let client_relative_path = client_relative_path.clone();
            let client_output_path = client_output_path.clone();

            async move {
                let path = asset.path();
                let span = tracing::trace_span!("emit asset", name = %path.to_string().await?);
                // We allow to write output out of dist path, this is different with next.js
                async move {
                    let path = path.await?;
                    Ok(if path.is_inside_ref(&client_relative_path) {
                        // Client assets are emitted to the client output path, which is prefixed
                        // with _next. We need to rebase them to remove that
                        // prefix.
                        Some(emit_rebase(
                            *asset,
                            client_relative_path,
                            client_output_path,
                        ))
                    } else {
                        Some(emit(*asset))
                    })
                }
                .instrument(span)
                .await
            }
        })
        .try_flat_join()
        .await?;
    Ok(())
}

#[turbo_tasks::function]
async fn emit(asset: Vc<Box<dyn OutputAsset>>) -> Result<()> {
    let _ = asset
        .content()
        .write(asset.path().owned().await?)
        .resolve()
        .await?;
    Ok(())
}

#[turbo_tasks::function]
async fn emit_rebase(
    asset: Vc<Box<dyn OutputAsset>>,
    from: FileSystemPath,
    to: FileSystemPath,
) -> Result<()> {
    let path = rebase(asset.path().owned().await?, from, to);
    let content = asset.content();
    let _ = content
        .resolve()
        .await?
        .write(path.owned().await?)
        .resolve()
        .await?;
    Ok(())
}
