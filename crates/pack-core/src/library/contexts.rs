use anyhow::Result;
use bincode::{Decode, Encode};
use turbo_rcstr::RcStr;
use turbo_tasks::{TaskInput, Vc, trace::TraceRawVcs};
use turbo_tasks_fs::FileSystemPath;
use turbopack_core::{
    chunk::{
        ChunkingContext, MangleType, MinifyType, SourceMapSourceType, SourceMapsType,
        UnusedReferences, chunk_id_strategy::ModuleIdStrategy,
    },
    environment::{EdgeWorkerEnvironment, Environment, ExecutionEnvironment, NodeJsVersion},
    module_graph::binding_usage_info::OptionBindingUsageInfo,
};

use crate::{config::Config, mode::Mode};

use super::LibraryChunkingContext;

#[derive(Clone, Debug, PartialEq, Eq, Hash, TaskInput, TraceRawVcs, Encode, Decode)]
pub struct LibraryChunkingContextOptions {
    pub mode: Vc<Mode>,
    pub root_path: FileSystemPath,
    pub output_root: FileSystemPath,
    pub output_root_to_root_path: RcStr,
    /// The environment provided by the caller. Note that this is ignored in favor of
    /// an EdgeWorker environment to disable async chunk splitting. See
    /// `get_library_chunking_context` for details.
    pub environment: Vc<Environment>,
    pub module_id_strategy: Vc<ModuleIdStrategy>,
    pub no_mangling: Vc<bool>,
    pub runtime_root: Vc<Option<RcStr>>,
    pub runtime_export: Vc<Vec<RcStr>>,
    pub config: Vc<Config>,
    pub export_usage: Vc<OptionBindingUsageInfo>,
    pub unused_references: Vc<UnusedReferences>,
}

#[turbo_tasks::function]
pub async fn get_library_chunking_context(
    options: LibraryChunkingContextOptions,
) -> Result<Vc<Box<dyn ChunkingContext>>> {
    let LibraryChunkingContextOptions {
        mode,
        root_path,
        output_root,
        output_root_to_root_path,
        // Note: We ignore the provided environment and use EdgeWorker instead.
        environment: _provided_environment,
        module_id_strategy,
        no_mangling,
        runtime_root,
        runtime_export,
        config,
        export_usage,
        unused_references,
    } = options;
    let minify = config.minify(mode);
    let concatenate_modules = config.concatenate_modules(mode);
    let nested_async_chunking = config.nested_async_chunking(mode);
    let mode = mode.await?;

    let runtime_type = {
        #[cfg(feature = "test")]
        {
            use turbopack_ecmascript_runtime::RuntimeType;
            match config.runtime_type_str().await?.as_deref() {
                Some(rt) if rt.eq_ignore_ascii_case("Development") => RuntimeType::Development,
                Some(rt) if rt.eq_ignore_ascii_case("Production") => RuntimeType::Production,
                _ => RuntimeType::Dummy,
            }
        }
        #[cfg(not(feature = "test"))]
        {
            mode.runtime_type()
        }
    };

    let output = config.output().await?;
    // Use an EdgeWorker environment to disable async chunk splitting for library builds.
    // This ensures the library is bundled as a single, self-contained file.
    let library_environment = Environment::new(ExecutionEnvironment::EdgeWorker(
        EdgeWorkerEnvironment {
            node_version: NodeJsVersion::default().resolved_cell(),
        }
        .resolved_cell(),
    ))
    .to_resolved()
    .await?;

    let mut builder = LibraryChunkingContext::builder(
        root_path,
        output_root,
        output_root_to_root_path,
        library_environment,
        runtime_type,
        (*runtime_root.await?).clone(),
        (*runtime_export.await?).clone(),
    )
    .minify_type(if mode.is_production() && *minify.await? {
        MinifyType::Minify {
            mangle: (!*no_mangling.await?).then_some(MangleType::OptimalSize),
        }
    } else {
        MinifyType::NoMinify
    })
    .source_maps(if *config.source_maps().await? {
        SourceMapsType::Full
    } else {
        SourceMapsType::None
    })
    .module_id_strategy(module_id_strategy.to_resolved().await?)
    .export_usage(*export_usage.await?)
    .unused_references(unused_references.to_resolved().await?)
    .nested_async_availability(*nested_async_chunking.await?);

    if !mode.is_development() {
        if let Some(filename) = &output.filename {
            builder = builder.filename(filename.clone());
        }

        if let Some(css_filename) = &output.css_filename {
            builder = builder.css_filename(css_filename.clone());
        }

        if let Some(asset_module_filename) = &output.asset_module_filename {
            builder = builder.asset_module_filename(asset_module_filename.clone());
        }
    }

    if mode.is_development() {
        builder = builder.source_map_source_type(SourceMapSourceType::AbsoluteFileUri);
    } else {
        builder = builder.module_merging(*concatenate_modules.await?)
    }

    Ok(Vc::upcast(builder.build()))
}
