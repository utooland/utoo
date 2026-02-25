use anyhow::Result;
use bincode::{Decode, Encode};
use turbo_rcstr::RcStr;
use turbo_tasks::{ResolvedVc, TaskInput, Vc, trace::TraceRawVcs};
use turbo_tasks_env::EnvMap;
use turbo_tasks_fs::FileSystemPath;
use turbopack_core::{
    chunk::{
        MangleType, MinifyType, SourceMapsType,
        UnusedReferences, chunk_id_strategy::ModuleIdStrategy,
    },
    compile_time_info::CompileTimeInfo,
    environment::{Environment, ExecutionEnvironment, NodeJsEnvironment},
    module_graph::binding_usage_info::OptionBindingUsageInfo,
};
use turbopack_css::chunk::CssChunkType;
use turbopack_ecmascript::chunk::EcmascriptChunkType;
use turbopack_nodejs::NodeJsChunkingContext;

use crate::{
    client::context::{compile_time_defines, free_var_references},
    config::{Config, ProviderConfig, resolve_split_chunks_config},
    mode::Mode,
};

#[derive(Clone, Debug, PartialEq, Eq, Hash, TaskInput, TraceRawVcs, Encode, Decode)]
pub struct ServerChunkingContextOptions {
    pub mode: Vc<Mode>,
    pub root_path: FileSystemPath,
    pub output_root: FileSystemPath,
    pub output_root_to_root_path: RcStr,
    pub environment: ResolvedVc<Environment>,
    pub module_id_strategy: Vc<ModuleIdStrategy>,
    pub no_mangling: Vc<bool>,
    pub config: Vc<Config>,
    pub export_usage: Vc<OptionBindingUsageInfo>,
    pub unused_references: Vc<UnusedReferences>,
}

#[turbo_tasks::function]
pub async fn get_server_chunking_context(
    options: ServerChunkingContextOptions,
) -> Result<Vc<NodeJsChunkingContext>> {
    let ServerChunkingContextOptions {
        mode,
        root_path,
        output_root,
        output_root_to_root_path,
        environment,
        module_id_strategy,
        no_mangling,
        config,
        export_usage,
        unused_references,
    } = options;

    let minify = config.minify(mode);
    let concatenate_modules = config.concatenate_modules(mode);
    let mode = mode.await?;

    let runtime_type = mode.runtime_type();

    let mut builder = NodeJsChunkingContext::builder(
        root_path,
        output_root.clone(),
        output_root_to_root_path,
        output_root.clone(),
        output_root.clone(),
        output_root,
        environment,
        runtime_type,
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
    .unused_references(unused_references.to_resolved().await?);

    if mode.is_development() {
        builder = builder.source_map_source_type(
            turbopack_core::chunk::SourceMapSourceType::AbsoluteFileUri,
        );
    } else {
        let split_chunks = &config.optimization().await?.split_chunks;
        let (ecmascript_chunking_config, css_chunking_config) =
            resolve_split_chunks_config(split_chunks);

        builder = builder
            .chunking_config(
                Vc::<EcmascriptChunkType>::default().to_resolved().await?,
                ecmascript_chunking_config,
            )
            .chunking_config(
                Vc::<CssChunkType>::default().to_resolved().await?,
                css_chunking_config,
            )
            .module_merging(*concatenate_modules.await?);
    }

    Ok(builder.build())
}

#[turbo_tasks::function]
pub async fn get_server_compile_time_info(
    define_env: Vc<EnvMap>,
    mode: Vc<Mode>,
    provider_config: Vc<ProviderConfig>,
) -> Result<Vc<CompileTimeInfo>> {
    let mut define_env_map = (*define_env.await?).clone();
    define_env_map.extend([(
        "process.env.NODE_ENV".into(),
        serde_json::to_string(mode.await?.node_env())
            .unwrap()
            .into(),
    )]);
    let define_env = Vc::cell(define_env_map);

    let environment = Environment::new(ExecutionEnvironment::NodeJsBuildTime(
        NodeJsEnvironment::default().resolved_cell(),
    ))
    .to_resolved()
    .await?;

    CompileTimeInfo::builder(environment)
        .defines(compile_time_defines(define_env).to_resolved().await?)
        .free_var_references(
            free_var_references(define_env, provider_config)
                .to_resolved()
                .await?,
        )
        .cell()
        .await
}
