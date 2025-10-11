use anyhow::Result;
use serde::{Deserialize, Serialize};
use turbo_rcstr::RcStr;
use turbo_tasks::{TaskInput, Vc, trace::TraceRawVcs};
use turbo_tasks_fs::FileSystemPath;
use turbopack_core::{
    chunk::{
        ChunkingContext, MangleType, MinifyType, SourceMapsType,
        module_id_strategies::ModuleIdStrategy,
    },
    environment::Environment,
    module_graph::export_usage::OptionExportUsageInfo,
};

use crate::{config::Config, mode::Mode};

use super::LibraryChunkingContext;

#[derive(Clone, Debug, PartialEq, Eq, Hash, TaskInput, TraceRawVcs, Serialize, Deserialize)]
pub struct LibraryChunkingContextOptions {
    pub mode: Vc<Mode>,
    pub root_path: FileSystemPath,
    pub output_root: FileSystemPath,
    pub output_root_to_root_path: RcStr,
    pub environment: Vc<Environment>,
    pub module_id_strategy: Vc<Box<dyn ModuleIdStrategy>>,
    pub no_mangling: Vc<bool>,
    pub runtime_root: Vc<Option<RcStr>>,
    pub runtime_export: Vc<Vec<RcStr>>,
    pub config: Vc<Config>,
    pub export_usage: Vc<OptionExportUsageInfo>,
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
        environment,
        module_id_strategy,
        no_mangling,
        runtime_root,
        runtime_export,
        config,
        export_usage,
    } = options;
    let minify = config.minify(mode);
    let concatenate_modules = config.concatenate_modules(mode);
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

    let mut builder = LibraryChunkingContext::builder(
        root_path,
        output_root,
        output_root_to_root_path,
        environment.to_resolved().await?,
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
    .asset_base_path(output.public_path.clone())
    .module_id_strategy(module_id_strategy.to_resolved().await?)
    .export_usage(*export_usage.await?);

    if !mode.is_development()
        && let Some(filename) = &output.filename
    {
        builder = builder.filename(filename.clone());
    }

    if mode.is_development() {
        builder = builder.use_file_source_map_uris();
    } else {
        builder = builder.module_merging(*concatenate_modules.await?)
    }

    Ok(Vc::upcast(builder.build()))
}
