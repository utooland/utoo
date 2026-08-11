use anyhow::{Result, anyhow};
use pack_api::project::Project;
use rustc_hash::FxHashSet;
use turbo_tasks::{
    NonLocalValue, OperationVc, ResolvedVc, TryFlatJoinIterExt, TryJoinIterExt, Vc,
    trace::TraceRawVcs,
};

use turbopack_core::chunk::{ChunkableModule, EvaluatableAsset};
use turbopack_dev_server::{
    SourceProvider,
    html::{DevHtmlAsset, DevHtmlEntry},
    introspect::IntrospectionSource,
    source::{
        ContentSource, asset_graph::AssetGraphContentSource, combined::CombinedContentSource,
        router::PrefixedRouterContentSource,
    },
};

#[turbo_tasks::function]
pub async fn create_web_entry_source(
    project: ResolvedVc<Project>,
) -> Result<Vc<Box<dyn ContentSource>>> {
    let entries = match &*project.app_project().await? {
        Some(app_project) => {
            let asset_context = Vc::upcast(app_project.app_module_context());
            let runtime_entries = app_project.app_runtime_entries();
            let chunking_context = app_project
                .project()
                .client_chunking_context()
                .to_resolved()
                .await?;

            app_project
                .resolved_entrypoints()
                .await?
                .iter()
                .map(async |app| {
                    let module_graph = app
                        .module_graph_for_entry(asset_context, runtime_entries)
                        .to_resolved()
                        .await?;

                    let entry_modules = app.app_entry_modules(asset_context).await?.to_vec();

                    entry_modules
                        .into_iter()
                        .map(async |m| {
                            if let (Some(chunkable_module), Some(entry)) = (
                                ResolvedVc::try_sidecast::<Box<dyn ChunkableModule>>(m),
                                ResolvedVc::try_sidecast::<Box<dyn EvaluatableAsset>>(m),
                            ) {
                                Ok(DevHtmlEntry {
                                    chunkable_module,
                                    module_graph,
                                    chunking_context,
                                    runtime_entries: Some(
                                        runtime_entries.with_entry(*entry).to_resolved().await?,
                                    ),
                                })
                            } else if let Some(chunkable_module) =
                                ResolvedVc::try_sidecast::<Box<dyn ChunkableModule>>(m)
                            {
                                // TODO this is missing runtime code, so it's probably broken and we should also
                                // add an ecmascript chunk with the runtime code
                                Ok(DevHtmlEntry {
                                    chunkable_module,
                                    module_graph,
                                    chunking_context,
                                    runtime_entries: None,
                                })
                            } else {
                                Err(anyhow!(
                            "Entry module is not chunkable, so it can't be used to bootstrap the \
                     application"
                        ))
                            }
                        })
                        .try_join()
                        .await
                })
                .try_flat_join()
                .await?
        }
        None => vec![],
    };

    let client_root = project.client_root().owned().await?;

    let entry_asset = Vc::upcast(DevHtmlAsset::new_with_body(
        client_root.join("index.html")?,
        entries,
        // Just add this root node for test
        r#"<div id="root"></div>"#.into(),
    ));

    let graph = Vc::upcast(AssetGraphContentSource::new_lazy(client_root, entry_asset));

    Ok(graph)
}

#[turbo_tasks::function(operation)]
async fn source(
    web_source: ResolvedVc<Box<dyn ContentSource>>,
) -> Result<Vc<Box<dyn ContentSource>>> {
    let main_source = CombinedContentSource::new(vec![web_source])
        .to_resolved()
        .await?;

    let introspect = ResolvedVc::upcast(
        IntrospectionSource {
            roots: FxHashSet::from_iter([ResolvedVc::upcast(main_source)]),
        }
        .resolved_cell(),
    );

    let main_source = ResolvedVc::upcast(main_source);

    Ok(Vc::upcast(PrefixedRouterContentSource::new(
        Default::default(),
        vec![("__turbopack__".into(), introspect)],
        *main_source,
    )))
}

#[derive(Clone, TraceRawVcs, NonLocalValue)]
pub struct ServerSourceProvider {
    pub web_source: ResolvedVc<Box<dyn ContentSource>>,
}

impl SourceProvider for ServerSourceProvider {
    fn get_source(&self) -> OperationVc<Box<dyn ContentSource>> {
        source(self.web_source)
    }
}
