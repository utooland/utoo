use anyhow::Result;
use turbo_rcstr::RcStr;
use turbo_tasks::{ResolvedVc, Vc};
use turbo_tasks_fs::FileSystemPath;
use turbopack_core::{
    chunk::{
        ChunkingContext, MangleType, MinifyType, SourceMapsType,
        module_id_strategies::ModuleIdStrategy,
    },
    environment::Environment,
};

use crate::{config::Config, mode::Mode};

use super::LibraryChunkingContext;

#[turbo_tasks::function]
pub async fn get_library_chunking_context(
    root_path: FileSystemPath,
    output_root: FileSystemPath,
    output_root_to_root_path: RcStr,
    environment: ResolvedVc<Environment>,
    mode: Vc<Mode>,
    module_id_strategy: ResolvedVc<Box<dyn ModuleIdStrategy>>,
    no_mangling: Vc<bool>,
    runtime_root: Vc<Option<RcStr>>,
    runtime_export: Vc<Vec<RcStr>>,
    config: ResolvedVc<Config>,
) -> Result<Vc<Box<dyn ChunkingContext>>> {
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

    let mut builder = LibraryChunkingContext::builder(
        root_path,
        output_root,
        output_root_to_root_path,
        environment,
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
    .module_id_strategy(module_id_strategy);

    if !mode.is_development()
        && let Some(filename) = &config.output().await?.filename
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
