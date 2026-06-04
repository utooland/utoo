use std::sync::LazyLock;

use anyhow::{Context, Result};
use async_trait::async_trait;
use bincode::{Decode, Encode};
use either::Either;
use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value as JsonValue;
use turbo_esregex::EsRegex;
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{FxIndexMap, NonLocalValue, OperationValue, ResolvedVc, Vc, trace::TraceRawVcs};
use turbo_tasks_env::EnvMap;
use turbo_tasks_fs::{FileJsonContent, FileSystemPath};
use turbopack::module_options::{
    ConditionContentType, ConditionItem, ConditionPath, ConditionQuery, LoaderRuleItem,
    WebpackRules, module_options_context::MdxTransformOptions,
};
use turbopack_core::{
    chunk::{
        ChunkingConfig, CompressOptions as MinifyCompressOptions, CompressType,
        CrossOrigin as RuntimeCrossOriginLoading,
    },
    issue::{Issue, IssueExt, IssueStage, StyledString},
    resolve::ResolveAliasMap,
};
use turbopack_ecmascript::{OptionTreeShaking, TreeShakingMode};
use turbopack_ecmascript_plugins::transform::{
    emotion::EmotionTransformConfig, styled_components::StyledComponentsTransformConfig,
};
use turbopack_node::transforms::webpack::{WebpackLoaderItem, WebpackLoaderItems};

use crate::{
    mode::Mode,
    shared::{
        transforms::ModularizeImportPackageConfig, webpack_rules::WebpackLoaderBuiltinCondition,
    },
};

#[turbo_tasks::value(transparent)]
pub struct ModularizeImports(
    #[bincode(with = "turbo_bincode::indexmap")] FxIndexMap<String, ModularizeImportPackageConfig>,
);

#[turbo_tasks::value(transparent)]
pub struct OptionalJsonValue(
    #[bincode(with = "turbo_bincode::serde_self_describing")] Option<JsonValue>,
);

#[turbo_tasks::value]
#[derive(Clone, Debug, Default, Deserialize, OperationValue)]
#[serde(rename_all = "camelCase")]
pub struct EntryOptions {
    pub name: Option<RcStr>,
    pub import: RcStr,
    pub library: Option<LibraryOptions>,
}

#[turbo_tasks::value]
#[derive(Clone, Debug, Default, Deserialize, OperationValue)]
#[serde(rename_all = "camelCase")]
pub struct LibraryOptions {
    pub name: Option<RcStr>,
    pub export: Option<Vec<RcStr>>,
}

#[turbo_tasks::value(transparent)]
pub struct Entries(Vec<EntryOptions>);

#[turbo_tasks::value(eq = "manual")]
#[derive(Clone, Debug, PartialEq, Default, Deserialize, OperationValue)]
#[serde(rename_all = "camelCase")]
pub struct DevServer {
    pub hot: Option<bool>,
}

/// Provider configuration item - can be a module name string or [module, export] tuple.
#[turbo_tasks::value]
#[derive(Clone, Debug, Serialize, Deserialize, OperationValue)]
#[serde(rename_all = "camelCase")]
#[serde(untagged)]
pub enum ProviderConfigValue {
    /// Simple module import: "jquery" -> import $ from 'jquery'
    Module(RcStr),
    /// Named export import: ["buffer", "Buffer"] -> import { Buffer } from 'buffer'
    NamedExport(Vec<RcStr>),
}

#[turbo_tasks::value(transparent)]
pub struct ProviderConfig(
    #[bincode(with = "turbo_bincode::indexmap")] FxIndexMap<RcStr, ProviderConfigValue>,
);

#[turbo_tasks::value(eq = "manual")]
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, OperationValue)]
#[serde(rename_all = "camelCase")]
pub struct ServerConfig {
    /// Entry point for the server runtime (e.g. "src/server.ts")
    pub entry: Option<RcStr>,
    /// Configuration for Server Functions (RPC)
    pub function: Option<ServerFunctionConfig>,
    /*
    TODO: Support React Server Components (RSC) boundary mediation
    pub component: Option<ServerComponentConfig>,

    #[turbo_tasks::value(eq = "manual")]
    #[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, OperationValue)]
    #[serde(rename_all = "camelCase")]
    pub struct ServerComponentConfig {
        /// Module serving as the client registry for mapping RSC chunks and hydration
        pub client_registry: Option<RcStr>,
        /// Module handling the serialization of client references during SSR
        pub server_proxy: Option<RcStr>,
    }
    */
    /// Server output configuration
    pub output: Option<ServerOutputConfig>,
}

#[turbo_tasks::value(eq = "manual")]
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, OperationValue)]
#[serde(rename_all = "camelCase")]
pub struct ServerFunctionConfig {
    /// Module that exports the RPC transport (client-side proxy generation).
    /// Expected signature:
    /// ```ts
    /// export function createServerReference(actionId: string, name: string) {
    ///   return async function (...args: any[]) { /* HTTP fetch to server */ }
    /// }
    /// ```
    pub client_proxy: Option<RcStr>,

    /// Module that exports the handler registration for the server bundle.
    /// Expected signature:
    /// ```ts
    /// export function registerServerReference(action: any, actionId: string, name: string) {
    ///   /* Register the action to a global router/map */
    /// }
    /// ```
    pub server_register: Option<RcStr>,
}

#[turbo_tasks::value(eq = "manual")]
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, OperationValue)]
#[serde(rename_all = "camelCase")]
pub struct ServerOutputConfig {
    /// Output path for server chunks, relative to project root.
    /// Defaults to "{output.path}/server".
    pub path: Option<RcStr>,
    /// Server entry chunk filename template. Supports [name].
    /// Defaults to the app endpoint name.
    pub filename: Option<RcStr>,
    /// Non-entry chunk filename template. Supports [name] and [contenthash:N].
    /// Defaults to the standard chunk naming convention.
    pub chunk_filename: Option<RcStr>,
}

#[turbo_tasks::value(serialization = "custom", eq = "manual")]
#[derive(Clone, Debug, Default, PartialEq, Deserialize, OperationValue, Encode, Decode)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    mode: Option<Mode>,
    entry: Vec<EntryOptions>,
    module: Option<ModuleConfig>,
    resolve: Option<ResolveConfig>,
    #[bincode(with = "turbo_bincode::serde_self_describing")]
    externals: Option<FxIndexMap<RcStr, ExternalConfig>>,
    output: Option<OutputConfig>,
    target: Option<RcStr>,
    source_maps: Option<bool>,
    #[bincode(with = "turbo_bincode::serde_self_describing")]
    define: Option<FxIndexMap<String, JsonValue>>,
    #[bincode(with = "turbo_bincode::serde_self_describing")]
    provider: Option<FxIndexMap<RcStr, ProviderConfigValue>>,
    images: Option<ImageConfig>,
    pub styles: Option<StyleConfig>,
    react: Option<ReactConfig>,
    optimization: Option<OptimizationConfig>,
    stats: Option<bool>,
    #[bincode(with = "turbo_bincode::serde_self_describing")]
    swc_plugins: Option<Vec<(RcStr, serde_json::Value)>>,
    #[cfg(any(feature = "process_pool", feature = "worker_pool"))]
    plugin_runtime_strategy: Option<PluginRuntimeStrategy>,
    persistent_caching: Option<bool>,
    node_polyfill: Option<bool>,
    mdx: Option<MdxOptions>,
    dev_server: Option<DevServer>,
    #[bincode(with = "turbo_bincode::serde_self_describing")]
    server: Option<ServerConfig>,
    #[cfg(feature = "test")]
    #[serde(rename = "runtimeType")]
    runtime_type: Option<RcStr>,
}

#[turbo_tasks::value]
#[derive(Clone, Debug, Serialize, Deserialize, OperationValue)]
#[serde(rename_all = "camelCase")]
#[serde(untagged)]
pub enum ExternalConfig {
    Basic(RcStr),
    Umd(ExternalUmd),
    Advanced(ExternalAdvanced),
}

#[turbo_tasks::value]
#[derive(Clone, Debug, Serialize, Deserialize, OperationValue)]
#[serde(rename_all = "camelCase")]
pub enum ExternalType {
    #[serde(rename = "script")]
    Script,
    #[serde(rename = "commonjs")]
    CommonJs,
    #[serde(rename = "esm")]
    ESM,
    #[serde(rename = "global")]
    Global,
    #[serde(rename = "promise")]
    Promise,
}

#[turbo_tasks::value]
#[derive(Clone, Debug, OperationValue)]
pub enum ExternalSubPathTarget {
    Empty,
    Tpl(RcStr),
}

impl serde::Serialize for ExternalSubPathTarget {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            ExternalSubPathTarget::Empty => serializer.serialize_str("$empty"),
            ExternalSubPathTarget::Tpl(s) => serializer.serialize_str(s.as_str()),
        }
    }
}

impl<'de> Deserialize<'de> for ExternalSubPathTarget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if s == "$empty" {
            Ok(ExternalSubPathTarget::Empty)
        } else {
            Ok(ExternalSubPathTarget::Tpl(s.into()))
        }
    }
}

#[turbo_tasks::value]
#[derive(Clone, Debug, Serialize, Deserialize, OperationValue)]
#[serde(rename_all = "PascalCase")]
pub enum ExternalTargetConverter {
    PascalCase,
    CamelCase,
    KebabCase,
    SnakeCase,
}

#[turbo_tasks::value]
#[derive(Clone, Debug, Serialize, Deserialize, OperationValue)]
#[serde(rename_all = "camelCase")]
pub struct ExternalSubPathRule {
    pub regex: RcStr,
    pub target: ExternalSubPathTarget,
    pub target_converter: Option<ExternalTargetConverter>,
}

#[turbo_tasks::value]
#[derive(Clone, Debug, Serialize, Deserialize, OperationValue)]
pub struct ExternalSubPath {
    pub exclude: Option<Vec<RcStr>>,
    pub rules: Vec<ExternalSubPathRule>,
}

#[turbo_tasks::value]
#[derive(Clone, Debug, Serialize, Deserialize, OperationValue)]
#[serde(rename_all = "camelCase")]
pub struct ExternalUmd {
    /// Root global variable name
    pub root: RcStr,
    /// CommonJS module reference
    pub commonjs: RcStr,
}

#[turbo_tasks::value]
#[derive(Clone, Debug, Serialize, Deserialize, OperationValue)]
#[serde(rename_all = "camelCase")]
pub struct ExternalAdvanced {
    pub root: RcStr,
    #[serde(rename = "type")]
    pub r#type: Option<ExternalType>,
    pub script: Option<RcStr>,
    pub sub_path: Option<ExternalSubPath>,
}

#[turbo_tasks::value]
#[derive(Clone, Debug, Deserialize, OperationValue)]
#[serde(rename_all = "camelCase")]
pub enum ReactRuntime {
    Automatic,
    Classic,
}

impl ReactRuntime {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReactRuntime::Automatic => "automatic",
            ReactRuntime::Classic => "classic",
        }
    }
}

#[turbo_tasks::value(eq = "manual")]
#[derive(Clone, Debug, PartialEq, Default, Deserialize, OperationValue)]
#[serde(rename_all = "camelCase")]
pub struct ReactConfig {
    pub runtime: Option<ReactRuntime>,
    pub import_source: Option<RcStr>,
    pub absolute_source_filename: Option<bool>,
}

#[turbo_tasks::value(eq = "manual")]
#[derive(Clone, Debug, PartialEq, Default, Deserialize, OperationValue)]
#[serde(rename_all = "camelCase")]
pub struct StyleConfig {
    pub styled_components: Option<StyledComponentsTransformOptionsOrBoolean>,
    pub emotion: Option<EmotionTransformConfig>,
    pub auto_css_modules: Option<bool>,
    #[bincode(with = "turbo_bincode::serde_self_describing")]
    pub postcss: Option<serde_json::Value>,
    #[bincode(with = "turbo_bincode::serde_self_describing")]
    sass: Option<serde_json::Value>,
    #[bincode(with = "turbo_bincode::serde_self_describing")]
    less: Option<serde_json::Value>,
    #[bincode(with = "turbo_bincode::serde_self_describing")]
    inline_css: Option<serde_json::Value>,
}

#[turbo_tasks::value(eq = "manual")]
#[derive(Clone, Debug, PartialEq, Default, Deserialize, OperationValue)]
#[serde(rename_all = "camelCase")]
pub struct ResolveConfig {
    #[serde(rename = "alias")]
    #[bincode(with = "turbo_bincode::serde_self_describing")]
    resolve_alias: Option<FxIndexMap<RcStr, JsonValue>>,
    #[serde(rename = "extensions")]
    #[bincode(with = "turbo_bincode::serde_self_describing")]
    resolve_extensions: Option<Vec<RcStr>>,
}

#[turbo_tasks::value]
#[derive(Clone, Debug, Default, Deserialize, OperationValue)]
#[serde(rename_all = "camelCase")]
pub struct ImageConfig {
    pub inline_limit: Option<u64>,
}

#[turbo_tasks::value(transparent)]
pub struct OptionImageConfig(Option<ImageConfig>);

#[turbo_tasks::value(eq = "manual")]
#[derive(Clone, Debug, PartialEq, Default, Deserialize, OperationValue)]
#[serde(rename_all = "camelCase")]
pub struct OptimizationConfig {
    pub module_ids: Option<ModuleIds>,
    /// When the code is minified, this opts out of the default mangling of
    /// local names for variables, functions etc., which can be useful for
    /// debugging/profiling purposes.
    pub no_mangling: Option<bool>,
    /// Whether to enable compression when minifying.
    #[bincode(with = "turbo_bincode::serde_self_describing")]
    pub compress: Option<JsonValue>,
    pub minify: Option<bool>,
    pub tree_shaking: Option<bool>,
    pub package_imports: Option<Vec<RcStr>>,
    #[bincode(with = "turbo_bincode::serde_self_describing")]
    pub modularize_imports: Option<FxIndexMap<String, ModularizeImportPackageConfig>>,
    pub transpile_packages: Option<Vec<RcStr>>,
    pub remove_console: Option<RemoveConsoleConfig>,
    #[bincode(with = "turbo_bincode::serde_self_describing")]
    pub split_chunks: Option<FxIndexMap<RcStr, SplitChunkConfig>>,
    /// Concatenate modules when possible to reduce the number of chunks.
    /// This can improve performance by reducing the number of requests and
    /// improving caching.
    #[serde(default)]
    pub concatenate_modules: Option<bool>,
    /// Defaults to false in development mode, true in production mode.
    pub remove_unused_exports: Option<bool>,
    /// Defaults to false in development mode, true in production mode.
    pub remove_unused_imports: Option<bool>,
    pub nested_async_chunking: Option<bool>,
    /// Whether to inline WASM files into the bundle. Defaults to false.
    /// When false, WASM files will be output as static assets.
    #[serde(default)]
    pub wasm_as_asset: Option<bool>,
}

#[turbo_tasks::value]
#[derive(Clone, Debug, Deserialize, OperationValue)]
#[serde(untagged)]
pub enum CopyItem {
    String(RcStr),
    Object {
        #[serde(rename = "from")]
        from: RcStr,
        #[serde(rename = "to", skip_serializing_if = "Option::is_none")]
        to: Option<RcStr>,
    },
}

impl CopyItem {
    pub fn from(&self) -> &RcStr {
        match self {
            CopyItem::String(s) => s,
            CopyItem::Object { from, .. } => from,
        }
    }

    pub fn to(&self) -> Option<&RcStr> {
        match self {
            CopyItem::String(_) => None,
            CopyItem::Object { to, .. } => to.as_ref(),
        }
    }
}

#[turbo_tasks::value]
#[derive(Clone, Debug, Default, Deserialize, OperationValue)]
#[serde(rename_all = "camelCase")]
pub struct OutputConfig {
    pub path: Option<RcStr>,
    pub filename: Option<RcStr>,
    pub chunk_filename: Option<RcStr>,
    pub css_filename: Option<RcStr>,
    pub css_chunk_filename: Option<RcStr>,
    pub asset_module_filename: Option<RcStr>,
    // TODO: make sure this is needed
    pub r#type: Option<OutputType>,
    pub clean: Option<bool>,
    pub copy: Option<Vec<CopyItem>>,
    /// URL prefix that will be prepended to all chunk and asset URLs when loading them.
    /// This is used to configure CDN URLs or serve assets from a different path.
    /// Examples: "/", "/assets/", "https://cdn.example.com/", "runtime", "auto"
    /// Note: This path will not appear in chunk paths or chunk data on disk,
    /// it only affects the URLs used by the browser to fetch resources.
    pub public_path: Option<RcStr>,
    /// Controls the `crossorigin` attribute for dynamically loaded JS chunks.
    /// Webpack-compatible values: false, "anonymous", "use-credentials".
    pub cross_origin_loading: Option<OutputCrossOriginLoading>,
    /// The global variable name used by the runtime for loading chunks.
    /// This is similar to webpack's `output.chunkLoadingGlobal`.
    /// Default: "TURBOPACK"
    pub chunk_loading_global: Option<RcStr>,
    /// Expose entry module exports to global scope with the specified name.
    /// When set, all named exports from the entry module will be available on `window`/`globalThis`
    /// under the specified name. If set to empty string, will use package.json name.
    /// Default: None (no exposure)
    pub entry_root_export: Option<RcStr>,
}

#[turbo_tasks::value]
#[derive(Clone, Debug, Deserialize, OperationValue)]
#[serde(rename_all = "kebab-case")]
pub enum OutputType {
    Standalone,
    Export,
}

#[turbo_tasks::value]
#[derive(Clone, Debug, Deserialize, OperationValue)]
#[serde(rename_all = "kebab-case")]
pub enum OutputCrossOriginLoadingMode {
    Anonymous,
    UseCredentials,
}

#[turbo_tasks::value]
#[derive(Clone, Debug, Deserialize, OperationValue)]
#[serde(untagged)]
pub enum OutputCrossOriginLoading {
    Boolean(bool),
    Mode(OutputCrossOriginLoadingMode),
}

impl OutputCrossOriginLoading {
    fn to_runtime(&self) -> RuntimeCrossOriginLoading {
        match self {
            Self::Mode(OutputCrossOriginLoadingMode::Anonymous) | Self::Boolean(true) => {
                RuntimeCrossOriginLoading::Anonymous
            }
            Self::Mode(OutputCrossOriginLoadingMode::UseCredentials) => {
                RuntimeCrossOriginLoading::UseCredentials
            }
            // Webpack-compatible: false disables crossorigin attribute.
            Self::Boolean(false) => RuntimeCrossOriginLoading::None,
        }
    }
}

#[derive(
    Clone,
    PartialEq,
    Eq,
    Debug,
    Deserialize,
    TraceRawVcs,
    NonLocalValue,
    OperationValue,
    Encode,
    Decode,
)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ConfigConditionQuery {
    Constant(RcStr),
    Regex(RegexComponents),
}

impl TryFrom<ConfigConditionQuery> for ConditionQuery {
    type Error = anyhow::Error;

    fn try_from(config: ConfigConditionQuery) -> Result<ConditionQuery> {
        Ok(match config {
            ConfigConditionQuery::Constant(value) => ConditionQuery::Constant(value),
            ConfigConditionQuery::Regex(regex) => {
                ConditionQuery::Regex(EsRegex::try_from(regex)?.resolved_cell())
            }
        })
    }
}

#[derive(
    Clone,
    PartialEq,
    Eq,
    Debug,
    Deserialize,
    TraceRawVcs,
    NonLocalValue,
    OperationValue,
    Encode,
    Decode,
)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ConfigConditionContentType {
    Glob(RcStr),
    Regex(RegexComponents),
}

impl TryFrom<ConfigConditionContentType> for ConditionContentType {
    type Error = anyhow::Error;

    fn try_from(config: ConfigConditionContentType) -> Result<ConditionContentType> {
        Ok(match config {
            ConfigConditionContentType::Glob(value) => ConditionContentType::Glob(value),
            ConfigConditionContentType::Regex(regex) => {
                ConditionContentType::Regex(EsRegex::try_from(regex)?.resolved_cell())
            }
        })
    }
}

#[derive(
    Deserialize,
    Clone,
    PartialEq,
    Eq,
    Debug,
    TraceRawVcs,
    NonLocalValue,
    OperationValue,
    Encode,
    Decode,
)]
// We can end up with confusing behaviors if we silently ignore extra properties, since `Base` will
// match nearly every object, since it has no required field.
#[serde(deny_unknown_fields)]
pub enum ConfigConditionItem {
    #[serde(rename = "all")]
    All(Box<[ConfigConditionItem]>),
    #[serde(rename = "any")]
    Any(Box<[ConfigConditionItem]>),
    #[serde(rename = "not")]
    Not(Box<ConfigConditionItem>),
    #[serde(untagged)]
    Builtin(WebpackLoaderBuiltinCondition),
    #[serde(untagged)]
    Base {
        #[serde(default)]
        path: Option<ConfigConditionPath>,
        #[serde(default)]
        content: Option<RegexComponents>,
        #[serde(default)]
        query: Option<ConfigConditionQuery>,
        #[serde(default, rename = "contentType")]
        content_type: Option<ConfigConditionContentType>,
    },
}

impl TryFrom<ConfigConditionItem> for ConditionItem {
    type Error = anyhow::Error;

    fn try_from(config: ConfigConditionItem) -> Result<Self> {
        let try_from_vec = |conds: Box<[_]>| {
            conds
                .into_iter()
                .map(ConditionItem::try_from)
                .collect::<Result<_>>()
        };
        Ok(match config {
            ConfigConditionItem::All(conds) => ConditionItem::All(try_from_vec(conds)?),
            ConfigConditionItem::Any(conds) => ConditionItem::Any(try_from_vec(conds)?),
            ConfigConditionItem::Not(cond) => ConditionItem::Not(Box::new((*cond).try_into()?)),
            ConfigConditionItem::Builtin(cond) => {
                ConditionItem::Builtin(RcStr::from(cond.as_str()))
            }
            ConfigConditionItem::Base {
                path,
                content,
                query,
                content_type,
            } => ConditionItem::Base {
                path: path.map(ConditionPath::try_from).transpose()?,
                content: content
                    .map(EsRegex::try_from)
                    .transpose()?
                    .map(EsRegex::resolved_cell),
                query: query.map(ConditionQuery::try_from).transpose()?,
                content_type: content_type
                    .map(ConditionContentType::try_from)
                    .transpose()?,
            },
        })
    }
}

#[turbo_tasks::value(operation)]
#[derive(Copy, Clone, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TurbopackModuleType {
    Asset,
    Ecmascript,
    Typescript,
    Css,
    CssModule,
    Json,
    Wasm,
    Raw,
    Node,
    Bytes,
}

impl From<&TurbopackModuleType> for RcStr {
    fn from(value: &TurbopackModuleType) -> Self {
        match value {
            TurbopackModuleType::Asset => rcstr!("asset"),
            TurbopackModuleType::Ecmascript => rcstr!("ecmascript"),
            TurbopackModuleType::Typescript => rcstr!("typescript"),
            TurbopackModuleType::Css => rcstr!("css"),
            TurbopackModuleType::CssModule => rcstr!("css-module"),
            TurbopackModuleType::Json => rcstr!("json"),
            TurbopackModuleType::Wasm => rcstr!("wasm"),
            TurbopackModuleType::Raw => rcstr!("raw"),
            TurbopackModuleType::Node => rcstr!("node"),
            TurbopackModuleType::Bytes => rcstr!("bytes"),
        }
    }
}

#[turbo_tasks::value]
#[derive(Clone, Debug, Deserialize, OperationValue)]
#[serde(rename_all = "camelCase")]
pub struct RuleConfigItem {
    #[serde(default)]
    pub loaders: Vec<LoaderItem>,
    #[serde(default, alias = "as")]
    pub rename_as: Option<RcStr>,
    #[serde(default, alias = "type")]
    pub module_type: Option<TurbopackModuleType>,
    #[serde(default)]
    pub condition: Option<ConfigConditionItem>,
}

#[turbo_tasks::value]
#[derive(Clone, Debug, OperationValue)]
pub struct RuleConfigCollection(Vec<RuleConfigCollectionItem>);

impl<'de> Deserialize<'de> for RuleConfigCollection {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match either::serde_untagged::deserialize::<Vec<RuleConfigCollectionItem>, RuleConfigItem, D>(
            deserializer,
        )? {
            Either::Left(collection) => Ok(RuleConfigCollection(collection)),
            Either::Right(item) => Ok(RuleConfigCollection(vec![RuleConfigCollectionItem::Full(
                item,
            )])),
        }
    }
}

#[turbo_tasks::value]
#[derive(Clone, Debug, Deserialize, OperationValue)]
#[serde(untagged)]
pub enum RuleConfigCollectionItem {
    Shorthand(LoaderItem),
    Full(RuleConfigItem),
}

#[turbo_tasks::value]
#[derive(Clone, Debug, Deserialize, OperationValue)]
#[serde(untagged)]
pub enum LoaderItem {
    LoaderName(RcStr),
    LoaderOptions(WebpackLoaderItem),
}

#[turbo_tasks::value(operation)]
#[derive(Copy, Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModuleIds {
    Named,
    Deterministic,
}

#[turbo_tasks::value(transparent)]
pub struct OptionModuleIds(pub Option<ModuleIds>);

#[turbo_tasks::value(transparent)]
pub struct OptionCompressType(pub Option<CompressType>);

// PluginRuntimeStrategy only makes sense when at least one pool backend is enabled.
// On WASM targets (no pool features), skip this type entirely to avoid empty-enum
// issues with derived macros (e.g. turbo_tasks::ShrinkToFit generates a non-exhaustive match).
#[cfg(any(feature = "process_pool", feature = "worker_pool"))]
#[turbo_tasks::value(operation)]
#[derive(Copy, Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginRuntimeStrategy {
    #[cfg(feature = "worker_pool")]
    WorkerThreads,
    #[cfg(feature = "process_pool")]
    ChildProcesses,
}

#[turbo_tasks::value(shared, operation)]
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReactCompilerMode {
    #[default]
    Infer,
    Annotation,
    All,
}

/// Subset of react compiler options
#[turbo_tasks::value(shared, operation)]
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReactCompilerOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compilation_mode: Option<ReactCompilerMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub panic_threshold: Option<RcStr>,
}

#[turbo_tasks::value]
#[derive(Clone, Debug, Deserialize, OperationValue)]
#[serde(untagged)]
pub enum ReactCompilerOptionsOrBoolean {
    Boolean(bool),
    Option(ReactCompilerOptions),
}

#[turbo_tasks::value(transparent)]
pub struct OptionalReactCompilerOptions(Option<ResolvedVc<ReactCompilerOptions>>);

#[turbo_tasks::value]
#[derive(Clone, Debug, Deserialize, OperationValue)]
#[serde(rename_all = "camelCase")]
pub struct ModuleConfig {
    #[bincode(with = "turbo_bincode::indexmap")]
    pub rules: FxIndexMap<RcStr, RuleConfigCollection>,
}

#[turbo_tasks::value]
#[derive(Clone, Debug, Serialize, Deserialize, OperationValue)]
#[serde(deny_unknown_fields)]
pub struct RegexComponents {
    source: RcStr,
    flags: RcStr,
}

#[turbo_tasks::value]
#[derive(Clone, Debug, Serialize, Deserialize, OperationValue)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum ConfigConditionPath {
    Glob(RcStr),
    Regex(RegexComponents),
}

impl TryFrom<ConfigConditionPath> for ConditionPath {
    type Error = anyhow::Error;

    fn try_from(config: ConfigConditionPath) -> Result<ConditionPath> {
        Ok(match config {
            ConfigConditionPath::Glob(path) => ConditionPath::Glob(path),
            ConfigConditionPath::Regex(path) => {
                ConditionPath::Regex(EsRegex::try_from(path)?.resolved_cell())
            }
        })
    }
}

impl TryFrom<RegexComponents> for EsRegex {
    type Error = anyhow::Error;

    fn try_from(components: RegexComponents) -> Result<EsRegex> {
        EsRegex::new(&components.source, &components.flags)
    }
}

#[turbo_tasks::value]
#[derive(Clone, Debug, Deserialize, OperationValue)]
#[serde(untagged)]
pub enum StyledComponentsTransformOptionsOrBoolean {
    Boolean(bool),
    Options(StyledComponentsTransformConfig),
}

impl StyledComponentsTransformOptionsOrBoolean {
    pub fn is_enabled(&self) -> bool {
        match self {
            Self::Boolean(enabled) => *enabled,
            _ => true,
        }
    }
}

#[turbo_tasks::value]
#[derive(Clone, Debug, Deserialize, OperationValue)]
#[serde(untagged, rename_all = "camelCase")]
pub enum ReactRemoveProperties {
    Boolean(bool),
    Config { properties: Option<Vec<String>> },
}

impl ReactRemoveProperties {
    pub fn is_enabled(&self) -> bool {
        match self {
            Self::Boolean(enabled) => *enabled,
            _ => true,
        }
    }
}

#[turbo_tasks::value]
#[derive(Clone, Debug, Deserialize, OperationValue)]
#[serde(untagged)]
pub enum RemoveConsoleConfig {
    Boolean(bool),
    Config { exclude: Option<Vec<String>> },
}

impl RemoveConsoleConfig {
    pub fn is_enabled(&self) -> bool {
        match self {
            Self::Boolean(enabled) => *enabled,
            _ => true,
        }
    }
}

#[turbo_tasks::value]
#[derive(Clone, Debug, Serialize, Deserialize, OperationValue)]
#[serde(rename_all = "camelCase")]
pub struct SplitChunkConfig {
    /// Try to avoid creating more than 1 chunk smaller than this size.
    /// It merges multiple small chunks into bigger ones to avoid that.
    #[serde(default = "default_min_chunk_size")]
    pub min_chunk_size: usize,

    /// Try to avoid creating more than this number of chunks per group.
    /// It merges multiple chunks into bigger ones to avoid that.
    #[serde(default = "default_max_chunk_count_per_group")]
    pub max_chunk_count_per_group: usize,

    /// Never merges chunks bigger than this size with other chunks.
    /// This makes sure that code in big chunks is not duplicated in multiple chunks.
    #[serde(default = "default_max_merge_chunk_size")]
    pub max_merge_chunk_size: usize,
}

impl From<&SplitChunkConfig> for ChunkingConfig {
    fn from(value: &SplitChunkConfig) -> Self {
        ChunkingConfig {
            min_chunk_size: value.min_chunk_size,
            max_chunk_count_per_group: value.max_chunk_count_per_group,
            max_merge_chunk_size: value.max_merge_chunk_size,
            ..Default::default()
        }
    }
}

pub fn default_min_chunk_size() -> usize {
    50_000
}

pub fn default_max_chunk_count_per_group() -> usize {
    40
}

pub fn default_max_merge_chunk_size() -> usize {
    200_000
}

#[turbo_tasks::value(transparent)]
pub struct SplitChunksConfig(
    #[bincode(with = "turbo_bincode::indexmap")] FxIndexMap<RcStr, SplitChunkConfig>,
);

#[turbo_tasks::value(transparent)]
pub struct ResolveExtensions(Option<Vec<RcStr>>);

#[turbo_tasks::value(transparent)]
pub struct SwcPlugins(
    #[bincode(with = "turbo_bincode::serde_self_describing")] Vec<(RcStr, serde_json::Value)>,
);

#[turbo_tasks::value(transparent)]
pub struct OptionalMdxTransformOptions(Option<ResolvedVc<MdxTransformOptions>>);

#[derive(
    Clone, Debug, PartialEq, Deserialize, TraceRawVcs, NonLocalValue, OperationValue, Encode, Decode,
)]
#[serde(untagged)]
pub enum MdxOptions {
    Boolean(bool),
    Option(MdxTransformOptions),
}

#[turbo_tasks::value(transparent)]
pub struct ExternalsConfig(
    #[bincode(with = "turbo_bincode::indexmap")] FxIndexMap<RcStr, ExternalConfig>,
);

#[turbo_tasks::value(shared)]
pub enum Platform {
    Web,
    Node,
}

fn turbopack_config_documentation_link() -> RcStr {
    rcstr!(
        "https://nextjs.org/docs/app/api-reference/config/next-config-js/turbopack#configuring-webpack-loaders"
    )
}

#[turbo_tasks::value(shared)]
struct InvalidLoaderRuleRenameAsIssue {
    glob: RcStr,
    rename_as: RcStr,
    config_file_path: FileSystemPath,
}

#[async_trait]
#[turbo_tasks::value_impl]
impl Issue for InvalidLoaderRuleRenameAsIssue {
    async fn file_path(&self) -> Result<FileSystemPath> {
        Ok(self.config_file_path.clone())
    }

    fn stage(&self) -> IssueStage {
        IssueStage::Config
    }

    async fn title(&self) -> Result<StyledString> {
        Ok(StyledString::Text(
            format!("Invalid loader rule for extension: {}", self.glob).into(),
        ))
    }

    async fn description(&self) -> Result<Option<StyledString>> {
        Ok(Some(StyledString::Text(RcStr::from(format!(
            "The extension {} contains a wildcard, but the `as` option does not: {}",
            self.glob, self.rename_as,
        )))))
    }

    fn documentation_link(&self) -> RcStr {
        turbopack_config_documentation_link()
    }
}

#[turbo_tasks::value(shared)]
struct InvalidLoaderRuleConditionIssue {
    error_string: RcStr,
    condition: ConfigConditionItem,
    config_file_path: FileSystemPath,
}

#[async_trait]
#[turbo_tasks::value_impl]
impl Issue for InvalidLoaderRuleConditionIssue {
    async fn file_path(&self) -> Result<FileSystemPath> {
        Ok(self.config_file_path.clone())
    }

    fn stage(&self) -> IssueStage {
        IssueStage::Config
    }

    async fn title(&self) -> Result<StyledString> {
        Ok(StyledString::Text(rcstr!(
            "Invalid condition for Turbopack loader rule"
        )))
    }

    async fn description(&self) -> Result<Option<StyledString>> {
        Ok(Some(StyledString::Stack(vec![
            StyledString::Line(vec![
                StyledString::Text(rcstr!("Encountered the following error: ")),
                StyledString::Code(self.error_string.clone()),
            ]),
            StyledString::Text(rcstr!("While processing the condition:")),
            StyledString::Code(RcStr::from(format!("{:#?}", self.condition))),
        ])))
    }

    fn documentation_link(&self) -> RcStr {
        turbopack_config_documentation_link()
    }
}

static IDENTIFIER_START_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([^a-zA-Z$_])").unwrap());
static IDENTIFIER_INVALID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[^a-zA-Z0-9$]+").unwrap());

// port from: https://github.com/webpack/webpack/blob/main/lib/Template.js#L104-L109
fn to_identifier(s: &str) -> String {
    let result = IDENTIFIER_START_RE.replace(s, "_$1");
    IDENTIFIER_INVALID_RE.replace_all(&result, "_").into_owned()
}

#[turbo_tasks::value_impl]
impl Config {
    #[turbo_tasks::function]
    pub async fn from_string(string: Vc<RcStr>) -> Result<Vc<Self>> {
        let string = string.await?;
        let mut jdeserializer = serde_json::Deserializer::from_str(&string);
        let config: Config = serde_path_to_error::deserialize(&mut jdeserializer)
            .with_context(|| format!("failed to parse utoopack config: {string}"))?;
        Ok(config.cell())
    }

    #[turbo_tasks::function]
    pub fn is_standalone(&self) -> Vc<bool> {
        Vc::cell(
            self.output
                .as_ref()
                .is_some_and(|o| o.r#type == Some(OutputType::Standalone)),
        )
    }

    #[turbo_tasks::function]
    pub fn externals_config(&self) -> Vc<ExternalsConfig> {
        let externals = self.externals.clone().unwrap_or_default();

        ExternalsConfig(externals).cell()
    }

    #[turbo_tasks::function]
    pub fn optimization(&self) -> Vc<OptimizationConfig> {
        self.optimization.clone().unwrap_or_default().cell()
    }

    #[turbo_tasks::function]
    pub fn styles(&self) -> Vc<StyleConfig> {
        self.styles.clone().unwrap_or_default().cell()
    }

    #[turbo_tasks::function]
    pub fn postcss_config_content(&self) -> Result<Vc<Option<RcStr>>> {
        let postcss_config_content = self
            .styles
            .as_ref()
            .and_then(|styles| styles.postcss.as_ref())
            .map(serde_json::to_string)
            .transpose()?
            .map(RcStr::from);

        Ok(Vc::cell(postcss_config_content))
    }

    #[turbo_tasks::function]
    pub fn react(&self) -> Vc<ReactConfig> {
        self.react.clone().unwrap_or_default().cell()
    }

    #[turbo_tasks::function]
    pub fn output(&self) -> Vc<OutputConfig> {
        self.output.clone().unwrap_or_default().cell()
    }

    // refer to: https://github.com/utooland/utoo/issues/2526
    #[turbo_tasks::function]
    pub async fn client_chunk_loading_global(
        &self,
        project_path: FileSystemPath,
    ) -> Result<Vc<Option<RcStr>>> {
        // 1. Check if user explicitly configured chunkLoadingGlobal
        if let Some(chunk_loading_global) = self
            .output
            .as_ref()
            .and_then(|o| o.chunk_loading_global.as_ref())
        {
            return Ok(Vc::cell(Some(
                format!("utooChunk_{}", chunk_loading_global).into(),
            )));
        }

        // TODO: support it when this feature is stable
        // // 2. Check entry[].name from the first entry
        // if let Some(entry_name) = self.entry.first().and_then(|e| e.name.as_ref()) {
        //     let global_name = to_identifier(entry_name);
        //     return Ok(Vc::cell(Some(global_name.into())));
        // }

        // 3. Read package.json and get name field
        let package_json_path = project_path.join("package.json")?;
        let package_json_content = package_json_path.read_json().await?;

        if let FileJsonContent::Content(json) = &*package_json_content
            && let Some(name) = json.get("name").and_then(|n| n.as_str())
        {
            let global_name = to_identifier(name);
            return Ok(Vc::cell(Some(format!("utooChunk_{}", global_name).into())));
        }

        // 4. No name found, return None to let the runtime use its default
        Ok(Vc::cell(None))
    }

    #[turbo_tasks::function]
    pub async fn entry_root_export(&self) -> Result<Vc<Option<RcStr>>> {
        // Check if entry_root_export is configured
        let entry_root_export = self
            .output
            .as_ref()
            .and_then(|o| o.entry_root_export.as_ref());

        match entry_root_export {
            Some(name) if !name.is_empty() => Ok(Vc::cell(Some(name.clone()))),
            _ => Ok(Vc::cell(None)),
        }
    }

    #[turbo_tasks::function]
    pub async fn cross_origin_loading(&self) -> Result<Vc<RuntimeCrossOriginLoading>> {
        let cross_origin_loading = self
            .output
            .as_ref()
            .and_then(|o| o.cross_origin_loading.as_ref())
            .map_or(
                RuntimeCrossOriginLoading::None,
                OutputCrossOriginLoading::to_runtime,
            );

        Ok(cross_origin_loading.cell())
    }

    #[turbo_tasks::function]
    pub fn mode(&self) -> Vc<Mode> {
        self.mode.unwrap_or_default().cell()
    }

    #[turbo_tasks::function]
    pub fn dev_server(&self) -> Vc<DevServer> {
        self.dev_server.clone().unwrap_or_default().cell()
    }

    #[turbo_tasks::function]
    pub fn server(&self) -> Vc<ServerConfig> {
        self.server.clone().unwrap_or_default().cell()
    }

    // Almost align to https://webpack.js.org/configuration/target/#target,
    // support configured via browserslist query, support target web or node
    #[turbo_tasks::function]
    pub fn target(&self) -> Vc<RcStr> {
        Vc::cell(self.target.clone().unwrap_or(
            "last 1 Chrome versions, last 1 Firefox versions, last 1 Safari versions, last 1 Edge versions".into()
        ))
    }

    #[turbo_tasks::function]
    pub fn platform(&self) -> Vc<Platform> {
        let target = if let Some(target) = self.target.as_ref() {
            target
        } else {
            return Platform::Web.cell();
        };

        let distribs = browserslist::resolve(
            target.split(","),
            &browserslist::Opts {
                ignore_unknown_versions: true,
                ..Default::default()
            },
        );

        match distribs {
            Ok(distribs) => match distribs.first() {
                Some(distrib) => {
                    if distrib.name() == "node" {
                        Platform::Node.cell()
                    } else {
                        Platform::Web.cell()
                    }
                }
                None => Platform::Web.cell(),
            },
            Err(_) => {
                if target == "node" {
                    Platform::Node.cell()
                } else {
                    Platform::Web.cell()
                }
            }
        }
    }

    #[turbo_tasks::function]
    pub fn define_env(&self) -> Vc<EnvMap> {
        let define_env = self
            .define
            .as_ref()
            .unwrap_or(&FxIndexMap::default())
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().into(),
                    if let JsonValue::String(s) = v {
                        // A string value is kept, calling `to_string` would wrap in to quotes.
                        s.as_str().into()
                    } else {
                        v.to_string().into()
                    },
                )
            })
            .collect();

        Vc::cell(define_env)
    }

    #[turbo_tasks::function]
    pub fn provider_config(&self) -> Vc<ProviderConfig> {
        Vc::cell(self.provider.clone().unwrap_or_default())
    }

    #[turbo_tasks::function]
    pub fn runtime_type_str(&self) -> Vc<Option<RcStr>> {
        #[cfg(feature = "test")]
        {
            Vc::cell(self.runtime_type.clone())
        }
        #[cfg(not(feature = "test"))]
        {
            Vc::cell(None)
        }
    }

    #[turbo_tasks::function]
    pub fn entries(&self) -> Vc<Entries> {
        Vc::cell(self.entry.clone())
    }

    #[turbo_tasks::function]
    pub fn webpack_rules(&self, project_path: FileSystemPath) -> Result<Vc<WebpackRules>> {
        let Some(turbo_rules) = self.module.as_ref().map(|t| &t.rules) else {
            return Ok(Vc::cell(Vec::new()));
        };
        if turbo_rules.is_empty() {
            return Ok(Vc::cell(Vec::new()));
        }
        let mut rules = Vec::new();
        for (glob, rule_collection) in turbo_rules.iter() {
            fn transform_loaders(
                loaders: &mut dyn Iterator<Item = &LoaderItem>,
            ) -> ResolvedVc<WebpackLoaderItems> {
                ResolvedVc::cell(
                    loaders
                        .map(|item| match item {
                            LoaderItem::LoaderName(name) => WebpackLoaderItem {
                                loader: name.clone(),
                                options: Default::default(),
                            },
                            LoaderItem::LoaderOptions(options) => options.clone(),
                        })
                        .collect(),
                )
            }
            // let config_file_path = || project_path.join(&self.config_file_name);
            for item in &rule_collection.0 {
                match item {
                    RuleConfigCollectionItem::Shorthand(loaders) => {
                        rules.push((
                            glob.clone(),
                            LoaderRuleItem {
                                loaders: transform_loaders(&mut [loaders].into_iter()),
                                rename_as: None,
                                condition: None,
                                module_type: None,
                            },
                        ));
                    }
                    RuleConfigCollectionItem::Full(RuleConfigItem {
                        loaders,
                        rename_as,
                        module_type,
                        condition,
                    }) => {
                        // If the extension contains a wildcard, and the rename_as does not,
                        // emit an issue to prevent users from encountering duplicate module
                        // names.
                        if glob.contains("*")
                            && let Some(rename_as) = rename_as.as_ref()
                            && !rename_as.contains("*")
                        {
                            InvalidLoaderRuleRenameAsIssue {
                                glob: glob.clone(),
                                config_file_path: project_path.clone(),
                                rename_as: rename_as.clone(),
                            }
                            .resolved_cell()
                            .emit();
                        }

                        let condition = if let Some(condition) = condition {
                            match ConditionItem::try_from(condition.clone()) {
                                Ok(cond) => Some(cond),
                                Err(err) => {
                                    InvalidLoaderRuleConditionIssue {
                                        error_string: RcStr::from(err.to_string()),
                                        condition: condition.clone(),
                                        config_file_path: project_path.clone(),
                                    }
                                    .resolved_cell()
                                    .emit();
                                    None
                                }
                            }
                        } else {
                            None
                        };
                        rules.push((
                            glob.clone(),
                            LoaderRuleItem {
                                loaders: transform_loaders(&mut loaders.iter()),
                                rename_as: rename_as.clone(),
                                condition,
                                // `module_type` is optional and is configured in userland as a string.
                                // Turbopack consumes it as `Option<RcStr>`.
                                module_type: module_type.as_ref().map(RcStr::from),
                            },
                        ));
                    }
                }
            }
        }
        Ok(Vc::cell(rules))
    }

    #[turbo_tasks::function]
    pub fn persistent_caching_enabled(&self) -> Result<Vc<bool>> {
        Ok(Vc::cell(self.persistent_caching.unwrap_or_default()))
    }

    #[turbo_tasks::function]
    pub fn resolve_alias_options(&self) -> Result<Vc<ResolveAliasMap>> {
        let Some(resolve_alias) = self.resolve.as_ref().and_then(|t| t.resolve_alias.as_ref())
        else {
            return Ok(ResolveAliasMap::cell(ResolveAliasMap::default()));
        };
        let alias_map: ResolveAliasMap = resolve_alias.try_into()?;
        Ok(alias_map.cell())
    }

    #[turbo_tasks::function]
    pub fn resolve_extension(&self) -> Vc<ResolveExtensions> {
        let Some(resolve_extensions) = self
            .resolve
            .as_ref()
            .and_then(|t| t.resolve_extensions.as_ref())
        else {
            return Vc::cell(None);
        };
        Vc::cell(Some(resolve_extensions.clone()))
    }

    #[turbo_tasks::function]
    pub fn node_polyfill(&self) -> Vc<bool> {
        Vc::cell(self.node_polyfill.unwrap_or(false))
    }

    #[turbo_tasks::function]
    pub fn mdx(&self) -> Vc<OptionalMdxTransformOptions> {
        let options = match &self.mdx {
            Some(MdxOptions::Boolean(true)) => Some(MdxTransformOptions::default().resolved_cell()),
            Some(MdxOptions::Option(options)) => Some(options.clone().resolved_cell()),
            _ => None,
        };

        OptionalMdxTransformOptions(options).cell()
    }

    #[turbo_tasks::function]
    pub fn image_config(&self) -> Vc<OptionImageConfig> {
        Vc::cell(self.images.clone())
    }

    #[turbo_tasks::function]
    pub fn modularize_imports(&self) -> Vc<ModularizeImports> {
        Vc::cell(
            self.optimization
                .as_ref()
                .map(|op| op.modularize_imports.clone().unwrap_or_default())
                .unwrap_or_default(),
        )
    }

    #[turbo_tasks::function]
    pub fn swc_plugins(&self) -> Vc<SwcPlugins> {
        Vc::cell(self.swc_plugins.clone().unwrap_or_default())
    }

    #[turbo_tasks::function]
    pub fn sass_config(&self) -> Vc<JsonValue> {
        Vc::cell(
            self.styles
                .as_ref()
                .map(|styles| {
                    styles
                        .sass
                        .clone()
                        .unwrap_or(JsonValue::Object(serde_json::Map::new()))
                })
                .unwrap_or(JsonValue::Object(serde_json::Map::new())),
        )
    }

    #[turbo_tasks::function]
    pub fn less_config(&self) -> Vc<JsonValue> {
        Vc::cell(
            self.styles
                .as_ref()
                .map(|styles| {
                    styles
                        .less
                        .clone()
                        .unwrap_or(JsonValue::Object(serde_json::Map::new()))
                })
                .unwrap_or(JsonValue::Object(serde_json::Map::new())),
        )
    }

    #[turbo_tasks::function]
    pub fn inline_css(&self) -> Vc<OptionalJsonValue> {
        Vc::cell(self.styles.as_ref().and_then(|op| op.inline_css.clone()))
    }

    #[turbo_tasks::function]
    pub fn optimize_package_imports(&self) -> Vc<Vec<RcStr>> {
        Vc::cell(
            self.optimization
                .as_ref()
                .map(|op| op.package_imports.clone().unwrap_or_default())
                .unwrap_or_default(),
        )
    }

    #[turbo_tasks::function]
    pub fn tree_shaking_mode_for_foreign_code(
        &self,
        _is_development: bool,
    ) -> Vc<OptionTreeShaking> {
        let tree_shaking = self
            .optimization
            .as_ref()
            .map(|op| op.tree_shaking.unwrap_or_default());

        OptionTreeShaking(match tree_shaking {
            Some(false) => Some(TreeShakingMode::ReexportsOnly),
            Some(true) => Some(TreeShakingMode::ModuleFragments),
            None => Some(TreeShakingMode::ReexportsOnly),
        })
        .cell()
    }

    #[turbo_tasks::function]
    pub fn tree_shaking_mode_for_user_code(&self, _is_development: bool) -> Vc<OptionTreeShaking> {
        let tree_shaking = self
            .optimization
            .as_ref()
            .map(|op| op.tree_shaking.unwrap_or_default());

        OptionTreeShaking(match tree_shaking {
            Some(false) => Some(TreeShakingMode::ReexportsOnly),
            Some(true) => Some(TreeShakingMode::ModuleFragments),
            None => Some(TreeShakingMode::ReexportsOnly),
        })
        .cell()
    }

    #[turbo_tasks::function]
    pub async fn remove_unused_exports(&self, mode: Vc<Mode>) -> Result<Vc<bool>> {
        Ok(Vc::cell(match *mode.await? {
            Mode::Development => false,
            Mode::Production => self
                .optimization
                .as_ref()
                .and_then(|op| op.remove_unused_exports)
                .unwrap_or(true),
        }))
    }

    #[turbo_tasks::function]
    pub async fn remove_unused_imports(&self, mode: Vc<Mode>) -> Result<Vc<bool>> {
        Ok(Vc::cell(match *mode.await? {
            Mode::Development => false,
            Mode::Production => self
                .optimization
                .as_ref()
                .and_then(|op| op.remove_unused_imports)
                .unwrap_or(true),
        }))
    }

    #[turbo_tasks::function]
    pub fn module_ids(&self) -> Vc<OptionModuleIds> {
        let Some(module_ids) = self.optimization.as_ref().and_then(|t| t.module_ids) else {
            return Vc::cell(None);
        };
        Vc::cell(Some(module_ids))
    }

    #[turbo_tasks::function]
    pub async fn minify(&self, mode: Vc<Mode>) -> Result<Vc<bool>> {
        let minify = self
            .optimization
            .as_ref()
            .map(|op| op.minify.is_none_or(|minify| minify));

        Ok(Vc::cell(
            minify.unwrap_or(matches!(*mode.await?, Mode::Production)),
        ))
    }

    #[turbo_tasks::function]
    pub fn no_mangling(&self) -> Vc<bool> {
        Vc::cell(
            self.optimization
                .as_ref()
                .map(|op| op.no_mangling.is_some_and(|no_mangling| no_mangling))
                .unwrap_or(false),
        )
    }

    #[turbo_tasks::function]
    pub fn compress(&self) -> Vc<OptionCompressType> {
        let compress = match self
            .optimization
            .as_ref()
            .and_then(|op| op.compress.as_ref())
        {
            Some(JsonValue::Bool(false)) => None,
            Some(JsonValue::Bool(true)) | None => Some(CompressType::Default),
            Some(JsonValue::Object(options)) => {
                let parse_u8 = |key: &str| {
                    options
                        .get(key)
                        .and_then(|v| v.as_u64())
                        .and_then(|v| u8::try_from(v).ok())
                };
                Some(CompressType::Options(MinifyCompressOptions {
                    passes: parse_u8("passes"),
                    sequences: parse_u8("sequences"),
                    keep_classnames: options.get("keepClassnames").and_then(|v| v.as_bool()),
                    keep_fnames: options.get("keepFnames").and_then(|v| v.as_bool()),
                }))
            }
            Some(_) => Some(CompressType::Default),
        };
        Vc::cell(compress)
    }

    #[turbo_tasks::function]
    pub async fn concatenate_modules(&self, mode: Vc<Mode>) -> Result<Vc<bool>> {
        Ok(Vc::cell(match *mode.await? {
            // Ignore configuration in development mode to not break HMR
            Mode::Development => false,
            Mode::Production => self
                .optimization
                .as_ref()
                .map(|op| op.concatenate_modules.unwrap_or(false))
                .unwrap_or(false),
        }))
    }

    #[turbo_tasks::function]
    pub async fn nested_async_chunking(&self, mode: Vc<Mode>) -> Result<Vc<bool>> {
        let option = self
            .optimization
            .as_ref()
            .and_then(|op| op.nested_async_chunking);
        Ok(Vc::cell(if let Some(val) = option {
            val
        } else {
            match *mode.await? {
                Mode::Development => false,
                Mode::Production => true,
            }
        }))
    }

    #[turbo_tasks::function]
    pub async fn source_maps(&self) -> Result<Vc<bool>> {
        Ok(Vc::cell(self.source_maps.unwrap_or(true)))
    }

    #[turbo_tasks::function]
    pub fn stats(&self) -> Vc<bool> {
        Vc::cell(self.stats.unwrap_or(false))
    }

    // TODO: extend this function.
    // publicPath 要写成 "/", 用于运行时 chunkPath 的替换
    #[turbo_tasks::function]
    pub async fn computed_public_path(self: Vc<Self>) -> Result<Vc<RcStr>> {
        let this = self.await?;

        let public_path = this
            .output
            .as_ref()
            .and_then(|o| o.public_path.clone())
            .unwrap_or("".into());

        // Special publicPath modes are represented as markers and resolved by
        // the browser runtime when chunk or asset URLs are constructed.
        match public_path.as_str() {
            "runtime" => return Ok(Vc::cell("__RUNTIME_PUBLIC_PATH__".into())),
            "auto" => return Ok(Vc::cell("__AUTO_PUBLIC_PATH__".into())),
            _ => {}
        }

        Ok(Vc::cell(
            format!("{}/", public_path.trim_end_matches("/")).into(),
        ))
    }
}

// Separate value_impl block so the cfg gate can exclude the entire block (including
// turbo_tasks-generated symbols) when no pool feature is enabled.
#[cfg(any(feature = "process_pool", feature = "worker_pool"))]
#[turbo_tasks::value_impl]
impl Config {
    #[turbo_tasks::function]
    pub fn plugin_runtime_strategy(&self) -> Vc<PluginRuntimeStrategy> {
        #[cfg(feature = "process_pool")]
        let default = PluginRuntimeStrategy::ChildProcesses;
        #[cfg(all(feature = "worker_pool", not(feature = "process_pool")))]
        let default = PluginRuntimeStrategy::WorkerThreads;

        self.plugin_runtime_strategy.unwrap_or(default).cell()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_externals_deserialization() {
        let json = serde_json::json!({
            "entry": [{"import": "./index.js"}],
            "externals": {
                "foo": "foo",
                "foo_require": "commonjs foo",
                "foo_require2": {
                    "root": "foo",
                    "type": "commonjs"
                },
                "foo_import": "esm foo",
                "foo_import2": {
                    "root": "foo",
                    "type": "esm"
                },
                "foo_promise": "promise foo",
                "foo_promise2": {
                    "root": "foo",
                    "type": "promise"
                },
                "react": {
                    "root": "React",
                    "commonjs": "react"
                },
                "antd": {
                    "root": "antd",
                    "subPath": {
                        "exclude": ["style"],
                        "rules": [
                            {
                              "regex": "/(version|message|notification)$",
                              "target": "$1"
                            },
                            {
                              "regex": "/locale/.+$",
                              "target": "$empty"
                            },
                            {
                              "regex": "/es\\/([^\\/]+)(?:\\/.*)?$/",
                              "target": "$1",
                              "targetConverter": "PascalCase"
                            }
                        ]
                    }
                }
            }
        });

        let config: Config = serde_json::from_value(json).unwrap();
        let externals = config.externals.unwrap();

        // test basic external config
        assert!(
            matches!(externals.get("foo"), Some(ExternalConfig::Basic(name)) if name.as_str() == "foo")
        );
        assert!(
            matches!(externals.get("foo_require"), Some(ExternalConfig::Basic(name)) if name.as_str() == "commonjs foo")
        );
        assert!(
            matches!(externals.get("foo_import"), Some(ExternalConfig::Basic(name)) if name.as_str() == "esm foo")
        );
        assert!(
            matches!(externals.get("foo_promise"), Some(ExternalConfig::Basic(name)) if name.as_str() == "promise foo")
        );

        // test advanced external config
        if let Some(ExternalConfig::Advanced(advanced)) = externals.get("foo_require2") {
            assert_eq!(advanced.root.as_str(), "foo");
            assert_eq!(advanced.r#type, Some(ExternalType::CommonJs));
        } else {
            panic!("Expected ExternalConfig::Advanced for foo_require2");
        }

        if let Some(ExternalConfig::Advanced(advanced)) = externals.get("foo_import2") {
            assert_eq!(advanced.root.as_str(), "foo");
            assert_eq!(advanced.r#type, Some(ExternalType::ESM));
        } else {
            panic!("Expected ExternalConfig::Advanced for foo_import2");
        }

        if let Some(ExternalConfig::Advanced(advanced)) = externals.get("foo_promise2") {
            assert_eq!(advanced.root.as_str(), "foo");
            assert_eq!(advanced.r#type, Some(ExternalType::Promise));
        } else {
            panic!("Expected ExternalConfig::Advanced for foo_promise2");
        }

        if let Some(ExternalConfig::Umd(umd_config)) = externals.get("react") {
            assert_eq!(umd_config.root.as_str(), "React");
            assert_eq!(umd_config.commonjs.as_str(), "react");
        } else {
            panic!("Expected ExternalConfig::Umd for react");
        }

        if let Some(ExternalConfig::Advanced(advanced)) = externals.get("antd") {
            assert_eq!(advanced.root.as_str(), "antd");
            assert_eq!(advanced.sub_path.as_ref().unwrap().rules.len(), 3);

            let rule1 = &advanced.sub_path.as_ref().unwrap().rules[0];
            assert_eq!(rule1.regex.as_str(), "/(version|message|notification)$");
            assert_eq!(rule1.target, ExternalSubPathTarget::Tpl("$1".into()));

            let rule2 = &advanced.sub_path.as_ref().unwrap().rules[1];
            assert_eq!(rule2.regex.as_str(), "/locale/.+$");
            assert_eq!(rule2.target, ExternalSubPathTarget::Empty);

            let rule3 = &advanced.sub_path.as_ref().unwrap().rules[2];
            assert_eq!(rule3.regex.as_str(), "/es\\/([^\\/]+)(?:\\/.*)?$/");
            assert_eq!(rule3.target, ExternalSubPathTarget::Tpl("$1".into()));
            assert_eq!(
                rule3.target_converter,
                Some(ExternalTargetConverter::PascalCase)
            );
        } else {
            panic!("Expected ExternalConfig::Advanced for antd");
        }
    }
}
