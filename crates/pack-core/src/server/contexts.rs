use std::collections::BTreeSet;

use anyhow::Result;
use bincode::{Decode, Encode};
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{ResolvedVc, Vc, trace::TraceRawVcs};
use turbo_tasks_env::EnvMap;
use turbo_tasks_fs::FileSystemPath;
use turbopack::module_options::{
    CssOptionsContext, EcmascriptOptionsContext, JsxTransformOptions, ModuleOptionsContext,
    ModuleRule, TypescriptTransformOptions, side_effect_free_packages_glob,
};
use turbopack_core::{
    chunk::{
        ChunkingConfig, MangleType, MinifyType, SourceMapSourceType, SourceMapsType,
        UnusedReferences, chunk_id_strategy::ModuleIdStrategy,
    },
    compile_time_info::{
        CompileTimeDefines, CompileTimeInfo, DefinableNameSegment, FreeVarReferences,
    },
    environment::{Environment, ExecutionEnvironment, NodeJsEnvironment, NodeJsVersion},
    module_graph::binding_usage_info::OptionBindingUsageInfo,
};
use turbopack_css::chunk::CssChunkType;
use turbopack_ecmascript::{chunk::EcmascriptChunkType, transform::ReactCompilerTarget};
use turbopack_node::{
    execution_context::ExecutionContext,
    transforms::postcss::{PostCssConfigLocation, PostCssTransformOptions},
};
use turbopack_nodejs::NodeJsChunkingContext;
use turbopack_resolve::resolve_options_context::ResolveOptionsContext;

use crate::{
    config::{Config, OptionCompressType, ProviderConfig},
    import_map::get_postcss_package_mapping,
    mode::Mode,
    server::{
        import_map::{get_server_fallback_import_map, get_server_import_map},
        transforms::get_server_transforms_rules,
    },
    shared::{
        contexts::{defines, free_vars},
        resolve::externals_plugin::ExternalsPlugin,
        transforms::{
            css_modules::get_auto_css_modules_rule,
            remove_console::get_remove_console_transform_rule,
            styled_components::get_styled_components_transform_rule,
            styled_jsx::get_styled_jsx_transform_rule,
            swc_ecma_transform_plugins::get_swc_ecma_transform_plugin_rule,
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

#[turbo_tasks::function]
pub async fn get_server_compile_time_info(
    browserslist_query: RcStr,
    define_env: Vc<EnvMap>,
    mode: Vc<Mode>,
    provider_config: Vc<ProviderConfig>,
) -> Result<Vc<CompileTimeInfo>> {
    let mut define_env = (*define_env.await?).clone();
    define_env.extend([(
        "process.env.NODE_ENV".into(),
        serde_json::to_string(mode.await?.node_env())
            .unwrap()
            .into(),
    )]);
    let define_env = Vc::cell(define_env);

    let distribs = browserslist::resolve(
        browserslist_query.split(","),
        &browserslist::Opts {
            ignore_unknown_versions: true,
            ..Default::default()
        },
    );

    let node_version = match distribs {
        Ok(distribs) => {
            if let Some(distrib) = distribs.first()
                && distrib.name() == "node"
            {
                NodeJsVersion::Static(ResolvedVc::cell(distrib.version().into()))
            } else {
                NodeJsVersion::default()
            }
        }
        Err(_) => NodeJsVersion::default(),
    };

    let environment = NodeJsEnvironment {
        node_version: node_version.resolved_cell(),
        ..Default::default()
    };

    // AMD's `define` is not available in Node.js unless it is explicitly provided. Marking the
    // free variable's type as undefined lets Turbopack eliminate AMD-first UMD branches before
    // resolving their dependency arrays. `free_vars` contains entries from both `define` and
    // `provider`, so checking it prevents this fallback from overriding either configuration.
    let mut server_defines = defines(define_env).owned().await?;
    let mut server_free_vars = free_vars(define_env, provider_config).owned().await?;
    let define = vec![DefinableNameSegment::Name(rcstr!("define"))];
    if !server_free_vars.contains_key(&define) {
        let typeof_define = vec![
            DefinableNameSegment::Name(rcstr!("define")),
            DefinableNameSegment::TypeOf,
        ];
        server_defines
            .entry(typeof_define.clone())
            .or_insert(rcstr!("undefined").into());
        server_free_vars
            .entry(typeof_define)
            .or_insert(rcstr!("undefined").into());
    }

    CompileTimeInfo::builder(
        Environment::new(ExecutionEnvironment::NodeJsLambda(
            environment.resolved_cell(),
        ))
        .to_resolved()
        .await?,
    )
    .defines(CompileTimeDefines(server_defines).resolved_cell())
    .free_var_references(FreeVarReferences(server_free_vars).resolved_cell())
    .cell()
    .await
}

#[turbo_tasks::function]
pub async fn get_server_module_options_context(
    project_path: FileSystemPath,
    execution_context: ResolvedVc<ExecutionContext>,
    env: ResolvedVc<Environment>,
    mode: Vc<Mode>,
    config: Vc<Config>,
) -> Result<Vc<ModuleOptionsContext>> {
    let mode_ref = mode.await?;

    let tsconfig = get_typescript_transform_options(project_path.clone())
        .to_resolved()
        .await?;
    let decorators_options = get_decorators_transform_options(project_path.clone());
    let enable_mdx_rs = *config.mdx().await?;

    let jsx_transform_options = get_jsx_transform_options(mode, config, false)
        .to_resolved()
        .await?;

    let mut loader_conditions = BTreeSet::new();
    loader_conditions.insert(WebpackLoaderBuiltinCondition::Node);
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

    let mut server_rules = get_server_transforms_rules(config, false).await?;
    let mut foreign_server_rules = get_server_transforms_rules(config, true).await?;

    // Ignore .d.ts files - they are TypeScript declaration files and should not be bundled
    let ignore_dts_rule = ModuleRule::new(
        turbopack::module_options::RuleCondition::ResourcePathEndsWith(".d.ts".to_string()),
        vec![turbopack::module_options::ModuleRuleEffect::Ignore],
    );
    server_rules.push(ignore_dts_rule.clone());
    foreign_server_rules.push(ignore_dts_rule);

    let styles = config.styles().await?;
    if styles.auto_css_modules.unwrap_or(true) {
        server_rules.push(get_auto_css_modules_rule());
    }

    let additional_rules: Vec<ModuleRule> = vec![
        get_swc_ecma_transform_plugin_rule(config, project_path.clone()).await?,
        get_styled_components_transform_rule(config).await?,
        get_styled_jsx_transform_rule(config, target_browsers).await?,
        get_remove_console_transform_rule(config).await?,
    ]
    .into_iter()
    .flatten()
    .collect();

    server_rules.extend(additional_rules);

    let server_config = config.server().await?;
    if server_config.function.is_some() {
        use crate::server_reference::server_directive_transformer::ServerDirectiveTransformer;
        use crate::shared::transforms::{EcmascriptTransformStage, get_ecma_transform_rule};

        server_rules.push(get_ecma_transform_rule(
            Box::new(ServerDirectiveTransformer::new(
                turbo_rcstr::rcstr!("server-reference"),
                turbo_rcstr::rcstr!("@utoo/server-function/client"),
                Some(turbo_rcstr::rcstr!("@utoo/server-function/server")),
                true,
            )),
            false,
            EcmascriptTransformStage::Preprocess,
        ));
    }

    let postcss_config_content = (*config.postcss_config_content().await?).clone();
    let postcss_transform_options = Some(PostCssTransformOptions {
        postcss_package: Some(get_postcss_package_mapping().to_resolved().await?),
        config_location: PostCssConfigLocation::ProjectPathOrLocalPath,
        config_content: postcss_config_content,
        ..Default::default()
    });

    let postcss_foreign_transform_options =
        postcss_transform_options
            .as_ref()
            .map(|postcss_transform_options| {
                PostCssTransformOptions {
                    // For node_modules we don't want to resolve postcss config relative to the file being
                    // compiled, instead it only uses the project root postcss config.
                    config_location: PostCssConfigLocation::ProjectPath,
                    ..postcss_transform_options.clone()
                }
            });

    let enable_postcss_transform = postcss_transform_options
        .map(|postcss_transform_options| postcss_transform_options.resolved_cell());
    let enable_foreign_postcss_transform = postcss_foreign_transform_options
        .map(|postcss_foreign_transform_options| postcss_foreign_transform_options.resolved_cell());

    let source_maps = if *config.source_maps().await? {
        SourceMapsType::Full
    } else {
        SourceMapsType::None
    };
    let css_modules_pattern = styles
        .css_modules
        .as_ref()
        .and_then(|css_modules| css_modules.local_ident_pattern());
    let enable_rust_react_compiler = *config.rust_react_compiler().await?;
    let rust_react_compiler_target: ReactCompilerTarget =
        *config.rust_react_compiler_target().await?;

    let module_options_context = ModuleOptionsContext {
        ecmascript: EcmascriptOptionsContext {
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
        module_rules: foreign_server_rules,
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
        module_rules: server_rules,
        ..module_options_context
    }
    .cell();

    Ok(module_options_context)
}

#[turbo_tasks::function]
pub async fn get_server_resolve_options_context(
    project_path: FileSystemPath,
    mode: Vc<Mode>,
    config: Vc<Config>,
    execution_context: Vc<ExecutionContext>,
    pack_path: FileSystemPath,
) -> Result<Vc<ResolveOptionsContext>> {
    let server_import_map =
        get_server_import_map(project_path.clone(), config, execution_context, pack_path)
            .to_resolved()
            .await?;
    let server_fallback_import_map = get_server_fallback_import_map().to_resolved().await?;

    let external_config = *config.externals_config().to_resolved().await?;

    let externals_plugin = ExternalsPlugin::new(
        project_path.clone(),
        project_path.root().owned().await?,
        external_config,
    )
    .to_resolved()
    .await?;

    let custom_conditions = vec!["node".into(), mode.await?.condition().into()];
    let resolve_options_context = ResolveOptionsContext {
        enable_node_modules: Some(project_path.root().owned().await?),
        enable_node_externals: true,
        enable_mjs_extension: true,
        enable_node_native_modules: true,
        custom_conditions,
        import_map: Some(server_import_map),
        fallback_import_map: Some(server_fallback_import_map),
        // Node honors neither the `module` main field nor the `module` exports condition; both are
        // bundler-only conventions. Enabling them on a node target makes a `require()` (or `import`)
        // of a package without an `exports` field resolve the ESM `module` build instead of the CJS
        // `main` Node would load — yielding the ESM namespace (`{ default, __esModule }`) so named
        // access becomes `undefined` (e.g. `require('json5').parse`). Keep it off to match Node.
        // See https://github.com/utooland/utoo/issues/3185
        module: false,
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
pub struct ServerChunkingContextOptions {
    pub mode: Vc<Mode>,
    pub config: Vc<Config>,
    pub root_path: FileSystemPath,
    pub node_root: FileSystemPath,
    pub node_root_to_root_path: RcStr,
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
}

// By default, assets are server assets, but the StructuredImageModuleType ones are on the server
#[turbo_tasks::function]
pub async fn get_server_chunking_context(
    options: ServerChunkingContextOptions,
) -> Result<Vc<NodeJsChunkingContext>> {
    let ServerChunkingContextOptions {
        mode,
        config,
        root_path,
        node_root,
        node_root_to_root_path,
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
    } = options;
    #[cfg(not(feature = "test"))]
    let _ = &config;
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
    let mut builder = NodeJsChunkingContext::builder(
        root_path,
        node_root.clone(),
        node_root_to_root_path,
        node_root.clone(),
        node_root.clone(),
        node_root.clone(),
        environment.to_resolved().await?,
        runtime_type,
    )
    .minify_type(if *minify.await? {
        MinifyType::Minify {
            mangle: (!*no_mangling.await?).then_some(MangleType::OptimalSize),
            compress: *compress.await?,
        }
    } else {
        MinifyType::NoMinify
    })
    .source_maps(*source_maps.await?)
    .module_id_strategy(module_id_strategy.to_resolved().await?)
    .export_usage(*export_usage.await?)
    .unused_references(unused_references.to_resolved().await?)
    .debug_ids(*debug_ids.await?)
    .nested_async_availability(*nested_async_chunking.await?);

    if mode.is_development() {
        builder = builder.source_map_source_type(SourceMapSourceType::AbsoluteFileUri);
    } else {
        let style_groups_algorithm = config.css_chunking_algorithm().owned().await?;

        builder = builder
            .source_map_source_type(SourceMapSourceType::RelativeUri)
            .chunking_config(
                Vc::<EcmascriptChunkType>::default().to_resolved().await?,
                ChunkingConfig {
                    min_chunk_size: 20_000,
                    max_chunk_count_per_group: 100,
                    max_merge_chunk_size: 100_000,
                    ..Default::default()
                },
            )
            .chunking_config(
                Vc::<CssChunkType>::default().to_resolved().await?,
                ChunkingConfig {
                    max_merge_chunk_size: 100_000,
                    style_groups_algorithm,
                    ..Default::default()
                },
            )
            .module_merging(*scope_hoisting.await?);
    }

    Ok(builder.build())
}
