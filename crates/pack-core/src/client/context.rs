use std::collections::BTreeSet;

use anyhow::Result;
use bincode::{Decode, Encode};
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{ResolvedVc, TryJoinIterExt, ValueToString, Vc, trace::TraceRawVcs};
use turbo_tasks_env::EnvMap;
use turbo_tasks_fs::{File, FileContent, FileSystemPath};
use turbopack::{
    evaluate_context::{config_tracing_module_context, node_evaluate_asset_context},
    module_options::{
        CssOptionsContext, EcmascriptOptionsContext, JsxTransformOptions, ModuleOptionsContext,
        ModuleRule, TypescriptTransformOptions, side_effect_free_packages_glob,
    },
};
use turbopack_browser::{BrowserChunkingContext, CurrentChunkMethod};
use turbopack_core::{
    asset::AssetContent,
    chunk::{
        ChunkingConfig, ChunkingContext, MangleType, MinifyType, SourceMapSourceType,
        SourceMapsType, UnusedReferences, chunk_id_strategy::ModuleIdStrategy,
    },
    compile_time_info::CompileTimeInfo,
    environment::{BrowserEnvironment, Environment, ExecutionEnvironment},
    ident::Layer,
    module_graph::binding_usage_info::OptionBindingUsageInfo,
    resolve::options::{ImportMap, ImportMapping},
    virtual_source::VirtualSource,
};
use turbopack_css::chunk::CssChunkType;
use turbopack_ecmascript::{
    TypeofWindow, chunk::EcmascriptChunkType, transform::ReactCompilerTarget,
};
use turbopack_ecmascript_runtime::chunk_update_listeners_global_name;
use turbopack_node::{
    execution_context::ExecutionContext,
    transforms::postcss::{PostCssConfigLocation, PostCssTransform, PostCssTransformOptions},
};
use turbopack_resolve::resolve_options_context::ResolveOptionsContext;

use crate::{
    client::{
        import_map::{get_client_fallback_import_map, get_client_import_map},
        runtime_entry::RuntimeEntries,
    },
    config::{
        Config, OptionCompressType, ProviderConfig, default_max_chunk_count_per_group,
        default_max_merge_chunk_size, default_min_chunk_size,
    },
    embed_js::embed_file_path,
    import_map::get_postcss_package_mapping,
    mode::Mode,
    shared::{
        contexts::{defines, free_vars},
        resolve::externals_plugin::ExternalsPlugin,
        transforms::{
            css_modules::get_auto_css_modules_rule,
            default_export_namer::get_default_export_namer_rule,
            emotion::get_emotion_transform_rule, jsx_dev_filename::get_jsx_dev_filename_rule,
            remove_console::get_remove_console_transform_rule,
            styled_components::get_styled_components_transform_rule,
            styled_jsx::get_styled_jsx_transform_rule,
            swc_ecma_transform_plugins::get_swc_ecma_transform_plugin_rule,
            type_only_import::get_type_only_import_rule,
            webpack_public_path::get_webpack_public_path_transform_rule,
        },
        webpack_rules::{WebpackLoaderBuiltinCondition, webpack_loader_options},
    },
    transform_options::{
        get_decorators_transform_options, get_jsx_transform_options,
        get_typescript_transform_options,
    },
    util::{
        foreign_code_context_condition, internal_assets_conditions, module_styles_rule_condition,
    },
};

use super::{
    react_refresh::assert_can_resolve_react_refresh, runtime_entry::RuntimeEntry,
    transforms::get_client_transforms_rules,
};

#[turbo_tasks::function]
fn postcss_import_map(package_mapping: ResolvedVc<ImportMapping>) -> Vc<ImportMap> {
    let mut import_map = ImportMap::default();
    import_map.insert_exact_alias(RcStr::from("@vercel/turbopack/postcss"), package_mapping);
    import_map.cell()
}

#[turbo_tasks::function]
pub async fn get_client_compile_time_info(
    browserslist_query: RcStr,
    define_env: Vc<EnvMap>,
    mode: Vc<Mode>,
    provider_config: Vc<ProviderConfig>,
    watch: Vc<bool>,
    hot: Vc<bool>,
) -> Result<Vc<CompileTimeInfo>> {
    let mode_ref = mode.await?;
    let mut define_env = (*define_env.await?).clone();
    define_env.extend([(
        "process.env.NODE_ENV".into(),
        serde_json::to_string(mode_ref.node_env()).unwrap().into(),
    )]);
    let define_env = Vc::cell(define_env);
    let environment = BrowserEnvironment {
        dom: true,
        web_worker: true,
        service_worker: true,
        browserslist_query: browserslist_query.to_owned(),
    }
    .resolved_cell();

    CompileTimeInfo::builder(
        Environment::new(ExecutionEnvironment::Browser(environment))
            .to_resolved()
            .await?,
    )
    .defines(defines(define_env).to_resolved().await?)
    .free_var_references(free_vars(define_env, provider_config).to_resolved().await?)
    .hot_module_replacement_enabled(mode_ref.is_development() && *watch.await? && *hot.await?)
    .cell()
    .await
}

#[turbo_tasks::function]
pub async fn get_client_runtime_entries(
    project_root: FileSystemPath,
    mode: Vc<Mode>,
    config: Vc<Config>,
    execution_context: Vc<ExecutionContext>,
    pack_path: FileSystemPath,
    watch: Vc<bool>,
    hot: Vc<bool>,
) -> Result<Vc<RuntimeEntries>> {
    let mut runtime_entries = vec![];
    let resolve_options_context = get_client_resolve_options_context(
        project_root.clone(),
        mode,
        config,
        execution_context,
        pack_path,
    );

    let is_development = mode.await?.is_development();
    let watch = *watch.await?;
    let hot = *hot.await?;

    if is_development && watch {
        let enable_react_refresh =
            assert_can_resolve_react_refresh(project_root.clone(), resolve_options_context)
                .await?
                .as_request();

        // It's important that React Refresh come before the regular bootstrap file,
        // because the bootstrap contains JSX which requires Refresh's global
        // functions to be available.
        if let Some(request) = enable_react_refresh {
            runtime_entries.push(
                RuntimeEntry::Request(request.to_resolved().await?, project_root.join("_")?)
                    .resolved_cell(),
            )
        };
    }

    if is_development && watch && hot {
        #[cfg(all(target_family = "wasm", target_os = "unknown"))]
        let (hmr_bootstrap_path, hmr_client_path) = (
            rcstr!("hmr/bootstrap-messageport.ts"),
            "./client-messageport",
        );
        #[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
        let (hmr_bootstrap_path, hmr_client_path) = (rcstr!("hmr/bootstrap.ts"), "./client");

        let chunk_loading_global = config
            .client_chunk_loading_global(project_root.clone())
            .await?;
        let chunk_update_listeners_global = chunk_update_listeners_global_name(
            chunk_loading_global
                .as_ref()
                .map_or("TURBOPACK", RcStr::as_str),
        );
        let hmr_bootstrap = format!(
            "import {{ initHMR }} from {hmr_client_path:?};\n\ninitHMR({});\n",
            serde_json::to_string(&chunk_update_listeners_global)?,
        );

        runtime_entries.push(
            RuntimeEntry::Source(ResolvedVc::upcast(
                VirtualSource::new(
                    embed_file_path(hmr_bootstrap_path).owned().await?,
                    AssetContent::file(FileContent::Content(File::from(hmr_bootstrap)).cell()),
                )
                .to_resolved()
                .await?,
            ))
            .resolved_cell(),
        );
    }

    Ok(Vc::cell(runtime_entries))
}

#[turbo_tasks::function]
pub async fn get_client_module_options_context(
    project_path: FileSystemPath,
    execution_context: ResolvedVc<ExecutionContext>,
    env: ResolvedVc<Environment>,
    mode: Vc<Mode>,
    config: Vc<Config>,
    watch: Vc<bool>,
    pack_path: FileSystemPath,
) -> Result<Vc<ModuleOptionsContext>> {
    let mode_ref = mode.await?;

    // resolve context
    let resolve_options_context = get_client_resolve_options_context(
        project_path.clone(),
        mode,
        config,
        *execution_context,
        pack_path.clone(),
    );

    let tsconfig = get_typescript_transform_options(project_path.clone())
        .to_resolved()
        .await?;
    let decorators_options = get_decorators_transform_options(project_path.clone());
    let enable_mdx_rs = *config.mdx().await?;
    let react_config = config.react().await?;
    let is_react_development = mode.await?.is_react_development();
    let enable_react_refresh = if *watch.await? && is_react_development {
        assert_can_resolve_react_refresh(project_path.clone(), resolve_options_context)
            .await?
            .is_found()
    } else {
        false
    };
    let jsx_transform_options = get_jsx_transform_options(mode, config, enable_react_refresh)
        .to_resolved()
        .await?;

    let mut loader_conditions = BTreeSet::new();
    loader_conditions.insert(WebpackLoaderBuiltinCondition::Browser);
    loader_conditions.extend(mode.await?.webpack_loader_conditions());

    // A separate webpack rules will be applied to codes matching foreign_code_context_condition.
    // This allows to import codes from node_modules that requires webpack loaders, which dev
    // implicitly does by default.
    let mut foreign_conditions = loader_conditions.clone();
    foreign_conditions.insert(WebpackLoaderBuiltinCondition::Foreign);

    let foreign_enable_webpack_loaders =
        *webpack_loader_options(project_path.clone(), config, foreign_conditions).await?;

    // Now creates a webpack rules that applies to all codes.
    let enable_webpack_loaders =
        *webpack_loader_options(project_path.clone(), config, loader_conditions).await?;

    let tree_shaking_mode_for_user_code = *config
        .tree_shaking_mode_for_user_code(mode_ref.is_development())
        .await?;
    let tree_shaking_mode_for_foreign_code = *config
        .tree_shaking_mode_for_foreign_code(mode_ref.is_development())
        .await?;
    let target_browsers = env.runtime_versions();

    let source_maps = if *config.source_maps().await? {
        SourceMapsType::Full
    } else {
        SourceMapsType::None
    };
    let postcss_config_content = (*config.postcss_config_content().await?).clone();
    let postcss_package_mapping = get_postcss_package_mapping().to_resolved().await?;
    let postcss_transform_options = Some(PostCssTransformOptions {
        postcss_package: Some(postcss_package_mapping),
        config_location: PostCssConfigLocation::ProjectPathOrLocalPath,
        config_content: postcss_config_content,
        ..Default::default()
    });

    let postcss_foreign_transform_options =
        postcss_transform_options
            .as_ref()
            .map(|postcss_transform_options| PostCssTransformOptions {
                // For node_modules we don't want to resolve postcss config relative to the file being
                // compiled, instead it only uses the project root postcss config.
                config_location: PostCssConfigLocation::ProjectPath,
                ..postcss_transform_options.clone()
            });

    let postcss_import_map = postcss_import_map(*postcss_package_mapping);
    let create_inline_postcss_transform = |options: &PostCssTransformOptions| {
        PostCssTransform::new(
            node_evaluate_asset_context(
                *execution_context,
                Some(postcss_import_map),
                None,
                Layer::new(rcstr!("webpack_loaders")),
                cfg!(all(target_family = "wasm", target_os = "unknown")),
            ),
            config_tracing_module_context(*execution_context),
            *execution_context,
            options.config_location,
            options.config_content.clone(),
            matches!(source_maps, SourceMapsType::Full),
        )
    };

    let inline_postcss_transform = if let Some(options) = postcss_transform_options.as_ref() {
        Some(ResolvedVc::upcast(
            create_inline_postcss_transform(options)
                .to_resolved()
                .await?,
        ))
    } else {
        None
    };
    let inline_foreign_postcss_transform =
        if let Some(options) = postcss_foreign_transform_options.as_ref() {
            Some(ResolvedVc::upcast(
                create_inline_postcss_transform(options)
                    .to_resolved()
                    .await?,
            ))
        } else {
            None
        };

    let mut client_rules =
        get_client_transforms_rules(config, false, inline_postcss_transform).await?;
    let mut foreign_client_rules =
        get_client_transforms_rules(config, true, inline_foreign_postcss_transform).await?;

    client_rules.push(get_type_only_import_rule(enable_mdx_rs.is_some()));
    foreign_client_rules.push(get_type_only_import_rule(enable_mdx_rs.is_some()));

    // Ignore .d.ts files - they are TypeScript declaration files and should not be bundled
    let ignore_dts_rule = ModuleRule::new(
        turbopack::module_options::RuleCondition::ResourcePathEndsWith(".d.ts".to_string()),
        vec![turbopack::module_options::ModuleRuleEffect::Ignore],
    );
    client_rules.push(ignore_dts_rule.clone());
    foreign_client_rules.push(ignore_dts_rule);

    let styles = config.styles().await?;
    if styles.auto_css_modules.unwrap_or(true) {
        client_rules.push(get_auto_css_modules_rule());
    }

    if enable_react_refresh {
        // This transformer just to solve the react-refresh not work for no named jsx function component.
        // Refer to: https://github.com/utooland/utoo/issues/2439
        client_rules.push(get_default_export_namer_rule());
    }

    if is_react_development && react_config.absolute_source_filename.unwrap_or(false) {
        client_rules.push(get_jsx_dev_filename_rule());
    }

    let additional_rules: Vec<ModuleRule> = vec![
        get_swc_ecma_transform_plugin_rule(config, project_path.clone()).await?,
        get_emotion_transform_rule(config).await?,
        get_styled_components_transform_rule(config).await?,
        get_styled_jsx_transform_rule(config, target_browsers).await?,
        get_remove_console_transform_rule(config).await?,
        Some(get_webpack_public_path_transform_rule()),
    ]
    .into_iter()
    .flatten()
    .collect();

    client_rules.extend(additional_rules);

    // Register "use server" directive transformer when server.function is configured
    let server_config = config.server().await?;
    if server_config.function.is_some() {
        use crate::server_reference::server_directive_transformer::ServerDirectiveTransformer;
        use crate::shared::transforms::{EcmascriptTransformStage, get_ecma_transform_rule};

        client_rules.push(get_ecma_transform_rule(
            Box::new(ServerDirectiveTransformer::new(
                rcstr!("server-reference"),
                rcstr!("@utoo/server-function/client"),
                Some(rcstr!("@utoo/server-function/server")),
                false,
            )),
            false,
            EcmascriptTransformStage::Preprocess,
        ));
    }

    let enable_postcss_transform = postcss_transform_options
        .map(|postcss_transform_options| postcss_transform_options.resolved_cell());
    let enable_foreign_postcss_transform = postcss_foreign_transform_options
        .map(|postcss_foreign_transform_options| postcss_foreign_transform_options.resolved_cell());
    let css_modules_pattern = styles
        .css_modules
        .as_ref()
        .and_then(|css_modules| css_modules.local_ident_pattern());
    let enable_rust_react_compiler = *config.rust_react_compiler().await?;
    let rust_react_compiler_target: ReactCompilerTarget =
        *config.rust_react_compiler_target().await?;

    let module_options_context = ModuleOptionsContext {
        ecmascript: EcmascriptOptionsContext {
            enable_typeof_window_inlining: Some(TypeofWindow::Object),
            source_maps,
            import_externals: true,
            enable_typescript_transform: Some(
                TypescriptTransformOptions::default().resolved_cell(),
            ),
            ignore_dynamic_requests: true,
            ..Default::default()
        },
        css: CssOptionsContext {
            source_maps,
            module_css_condition: Some(module_styles_rule_condition()),
            css_modules_pattern,
            ..Default::default()
        },
        environment: Some(env),
        execution_context: Some(execution_context),
        tree_shaking_mode: tree_shaking_mode_for_user_code,
        enable_postcss_transform,
        side_effect_free_packages: Some(
            side_effect_free_packages_glob(config.optimize_package_imports())
                .to_resolved()
                .await?,
        ),
        keep_last_successful_parse: mode_ref.is_development(),
        ..Default::default()
    };

    // node_modules context
    let foreign_codes_options_context = ModuleOptionsContext {
        ecmascript: EcmascriptOptionsContext {
            enable_typeof_window_inlining: None,
            enable_jsx: Some(jsx_transform_options),
            ..module_options_context.ecmascript
        },
        enable_webpack_loaders: foreign_enable_webpack_loaders,
        enable_postcss_transform: enable_foreign_postcss_transform,
        module_rules: foreign_client_rules,
        tree_shaking_mode: tree_shaking_mode_for_foreign_code,
        ..module_options_context.clone()
    };

    let internal_context = ModuleOptionsContext {
        ecmascript: EcmascriptOptionsContext {
            enable_jsx: Some(JsxTransformOptions::default().resolved_cell()),
            ..module_options_context.ecmascript.clone()
        },
        enable_postcss_transform: None,
        ..module_options_context.clone()
    };

    let module_options_context = ModuleOptionsContext {
        // We don't need to resolve React Refresh for each module. Instead,
        // we try resolve it once at the root and pass down a context to all
        // the modules.
        ecmascript: EcmascriptOptionsContext {
            enable_jsx: Some(jsx_transform_options),
            enable_typescript_transform: Some(tsconfig),
            enable_decorators: Some(decorators_options.to_resolved().await?),
            enable_rust_react_compiler,
            rust_react_compiler_target,
            ..module_options_context.ecmascript.clone()
        },
        enable_webpack_loaders,
        enable_mdx_rs,
        rules: vec![
            (
                foreign_code_context_condition(config).await?,
                foreign_codes_options_context.resolved_cell(),
            ),
            (
                internal_assets_conditions().await?,
                internal_context.resolved_cell(),
            ),
        ],
        module_rules: client_rules,
        ..module_options_context
    }
    .cell();

    Ok(module_options_context)
}

#[turbo_tasks::function]
pub async fn get_client_resolve_options_context(
    project_path: FileSystemPath,
    mode: Vc<Mode>,
    config: Vc<Config>,
    execution_context: Vc<ExecutionContext>,
    pack_path: FileSystemPath,
) -> Result<Vc<ResolveOptionsContext>> {
    let client_import_map =
        get_client_import_map(project_path.clone(), config, execution_context, pack_path)
            .to_resolved()
            .await?;
    let enable_node_polyfill = *config.node_polyfill().await?;
    let client_fallback_import_map = get_client_fallback_import_map(enable_node_polyfill)
        .to_resolved()
        .await?;

    let external_config = *config.externals_config().to_resolved().await?;

    let externals_plugin = ExternalsPlugin::new(
        project_path.clone(),
        project_path.root().owned().await?,
        external_config,
    )
    .to_resolved()
    .await?;
    let custom_conditions = vec![mode.await?.condition().into()];
    let resolve_options_context = ResolveOptionsContext {
        enable_node_modules: Some(project_path.root().owned().await?),
        custom_conditions,
        import_map: Some(client_import_map),
        fallback_import_map: Some(client_fallback_import_map),
        browser: true,
        module: true,
        before_resolve_plugins: vec![ResolvedVc::upcast(externals_plugin)],
        after_resolve_plugins: vec![ResolvedVc::upcast(externals_plugin)],
        ..Default::default()
    };

    // For node_modules: manually specify extensions to avoid parsing their tsconfig.json
    let foreign_resolve_options = ResolveOptionsContext {
        custom_extensions: Some(vec![
            rcstr!(".js"),
            rcstr!(".mjs"),
            rcstr!(".json"),
            rcstr!(".jsx"),
            rcstr!(".ts"),
            rcstr!(".tsx"),
        ]),
        ..resolve_options_context.clone()
    };

    Ok(ResolveOptionsContext {
        enable_typescript: true,
        enable_react: true,
        enable_mjs_extension: true,
        custom_extensions: config.resolve_extension().owned().await?,
        rules: vec![(
            foreign_code_context_condition(config).await?,
            foreign_resolve_options.resolved_cell(),
        )],
        ..resolve_options_context
    }
    .cell())
}

#[turbo_tasks::task_input(contains_unresolved_vcs)]
#[derive(Clone, Debug, PartialEq, Eq, Hash, TraceRawVcs, Encode, Decode)]
pub struct ClientChunkingContextOptions {
    pub mode: Vc<Mode>,
    pub watch: Vc<bool>,
    pub hot: Vc<bool>,
    pub root_path: FileSystemPath,
    pub client_root: FileSystemPath,
    pub client_root_to_root_path: RcStr,
    pub public_path: Vc<RcStr>,
    pub environment: Vc<Environment>,
    pub module_id_strategy: Vc<ModuleIdStrategy>,
    pub export_usage: Vc<OptionBindingUsageInfo>,
    pub unused_references: Vc<UnusedReferences>,
    pub minify: Vc<bool>,
    pub compress: Vc<OptionCompressType>,
    pub source_maps: Vc<SourceMapsType>,
    pub no_mangling: Vc<bool>,
    pub scope_hoisting: Vc<bool>,
    pub nested_async_chunking: Vc<bool>,
    pub debug_ids: Vc<bool>,
    pub should_use_absolute_url_references: Vc<bool>,
    pub config: Vc<Config>,
}

#[turbo_tasks::function]
pub async fn get_client_chunking_context(
    options: ClientChunkingContextOptions,
) -> Result<Vc<Box<dyn ChunkingContext>>> {
    let ClientChunkingContextOptions {
        mode,
        watch,
        hot,
        root_path,
        client_root,
        client_root_to_root_path,
        public_path,
        environment,
        module_id_strategy,
        export_usage,
        unused_references,
        minify,
        compress,
        source_maps,
        no_mangling,
        scope_hoisting,
        nested_async_chunking,
        debug_ids,
        should_use_absolute_url_references,
        config,
    } = options;

    let mode = mode.await?;
    let public_path = public_path.owned().await?;

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

    let mut builder = BrowserChunkingContext::builder(
        root_path.clone(),
        client_root.clone(),
        client_root_to_root_path,
        client_root.clone(),
        client_root.clone(),
        client_root.clone(),
        environment.to_resolved().await?,
        runtime_type,
    )
    .minify_type(if mode.is_production() && *minify.await? {
        MinifyType::Minify {
            mangle: (!*no_mangling.await?).then_some(MangleType::OptimalSize),
            compress: *compress.await?,
        }
    } else {
        MinifyType::NoMinify
    })
    .source_maps(*source_maps.await?)
    .chunk_base_path(Some(public_path.clone()))
    .asset_base_path(Some(public_path))
    .current_chunk_method(CurrentChunkMethod::DocumentCurrentScript)
    .module_id_strategy(module_id_strategy.to_resolved().await?)
    .export_usage(*export_usage.await?)
    .unused_references(unused_references.to_resolved().await?)
    .debug_ids(*debug_ids.await?)
    .should_use_absolute_url_references(*should_use_absolute_url_references.await?)
    .nested_async_availability(*nested_async_chunking.await?);

    if let Some(chunk_loading_global) = &*config
        .client_chunk_loading_global(root_path.clone())
        .await?
    {
        builder = builder.chunk_loading_global(chunk_loading_global.clone());
    }

    builder = builder.cross_origin(*config.cross_origin_loading().await?);

    // Read entry_root_export from config
    if let Some(entry_root_export) = &*config.entry_root_export().await? {
        builder = builder.entry_root_export(Some(entry_root_export.clone()));
    }

    let output = config.output().await?;

    if let Some(filename) = &output.filename {
        builder = builder.filename(filename.clone());
    }

    if let Some(chunk_filename) = &output.chunk_filename {
        builder = builder.chunk_filename(chunk_filename.clone());
    }

    if let Some(css_filename) = &output.css_filename {
        builder = builder.css_filename(css_filename.clone());
    }

    if let Some(css_chunk_filename) = &output.css_chunk_filename {
        builder = builder.css_chunk_filename(css_chunk_filename.clone());
    }

    if let Some(asset_module_filename) = &output.asset_module_filename {
        builder = builder.asset_module_filename(asset_module_filename.clone());
    }

    if mode.is_development() {
        builder = builder
            .source_map_source_type(SourceMapSourceType::AbsoluteFileUri)
            .dynamic_chunk_content_loading(true);

        if *watch.await? && *hot.await? {
            builder = builder.hot_module_replacement().dynamic_hmr_chunk_lists();
        }
    } else {
        let split_chunks = &config.optimization().await?.split_chunks;
        let style_groups_algorithm = config.css_chunking_algorithm().owned().await?;

        let (ecmascript_chunking_config, css_chunking_config) = (
            split_chunks.as_ref().and_then(|sc| sc.get("js")).map_or(
                ChunkingConfig {
                    min_chunk_size: default_min_chunk_size(),
                    max_chunk_count_per_group: default_max_chunk_count_per_group(),
                    max_merge_chunk_size: default_max_merge_chunk_size(),
                    ..Default::default()
                },
                Into::into,
            ),
            match split_chunks.as_ref().and_then(|sc| sc.get("css")) {
                None => ChunkingConfig {
                    max_merge_chunk_size: 100_000,
                    style_groups_algorithm,
                    ..Default::default()
                },
                Some(config) => {
                    let mut config = ChunkingConfig::from(config);
                    config.style_groups_algorithm = style_groups_algorithm;
                    config
                }
            },
        );

        builder = builder
            .chunking_config(
                Vc::<EcmascriptChunkType>::default().to_resolved().await?,
                ecmascript_chunking_config,
            )
            .chunking_config(
                Vc::<CssChunkType>::default().to_resolved().await?,
                css_chunking_config,
            )
            .module_merging(*scope_hoisting.await?);
    }

    let chunking_context = builder.build();

    // TODO: split chunks not worked as we expect now, check the implementation in
    // turbopack_browser
    tracing::debug!(
        "client chunking config {:?}\n",
        chunking_context
            .chunking_configs()
            .await?
            .iter()
            .map(|(ty, config)| async { Ok((ty.to_string().await?, config.clone())) })
            .try_join()
            .await?,
    );

    Ok(Vc::upcast(chunking_context))
}
