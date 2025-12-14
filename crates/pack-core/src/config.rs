use anyhow::{Context, Result, bail};
use either::Either;
use rustc_hash::FxHashSet;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value as JsonValue;
use turbo_esregex::EsRegex;
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{
    FxIndexMap, NonLocalValue, OperationValue, ResolvedVc, Vc, debug::ValueDebugFormat,
    trace::TraceRawVcs,
};
use turbo_tasks_env::EnvMap;
use turbo_tasks_fs::FileSystemPath;
use turbopack::module_options::{
    ConditionItem, ConditionPath, LoaderRuleItem, WebpackRules,
    module_options_context::MdxTransformOptions,
};
use turbopack_core::{
    chunk::ChunkingConfig,
    issue::{Issue, IssueExt, IssueStage, OptionStyledString, StyledString},
    resolve::ResolveAliasMap,
};
use turbopack_ecmascript::{OptionTreeShaking, TreeShakingMode};
use turbopack_ecmascript_plugins::transform::{
    emotion::EmotionTransformConfig, styled_components::StyledComponentsTransformConfig,
};
use turbopack_node::transforms::webpack::{WebpackLoaderItem, WebpackLoaderItems};

use crate::{
    import_map::mdx_import_source_file,
    mode::Mode,
    shared::{
        transforms::ModularizeImportPackageConfig, webpack_rules::WebpackLoaderBuiltinCondition,
    },
};

#[turbo_tasks::value(transparent)]
pub struct ModularizeImports(FxIndexMap<String, ModularizeImportPackageConfig>);

#[turbo_tasks::value(transparent)]
#[derive(Clone, Debug)]
pub struct CacheKinds(FxHashSet<RcStr>);

impl CacheKinds {
    pub fn extend<I: IntoIterator<Item = RcStr>>(&mut self, iter: I) {
        self.0.extend(iter);
    }
}

impl Default for CacheKinds {
    fn default() -> Self {
        CacheKinds(["default", "remote"].iter().map(|&s| s.into()).collect())
    }
}

#[turbo_tasks::value(transparent)]
pub struct OptionalJsonValue(Option<JsonValue>);

#[derive(
    Debug,
    Default,
    Serialize,
    Deserialize,
    Clone,
    PartialEq,
    Eq,
    Hash,
    TraceRawVcs,
    NonLocalValue,
    OperationValue,
)]
#[serde(rename_all = "camelCase")]
pub struct EntryOptions {
    pub name: Option<RcStr>,
    pub import: RcStr,
    pub library: Option<LibraryOptions>,
}

#[derive(
    Debug,
    Default,
    Serialize,
    Deserialize,
    Clone,
    PartialEq,
    Eq,
    Hash,
    TraceRawVcs,
    NonLocalValue,
    OperationValue,
)]
#[serde(rename_all = "camelCase")]
pub struct LibraryOptions {
    pub name: Option<RcStr>,
    pub export: Option<Vec<RcStr>>,
}

#[turbo_tasks::value(transparent)]
pub struct Entries(Vec<EntryOptions>);

#[turbo_tasks::value(serialization = "custom", eq = "manual")]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, OperationValue)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    mode: Option<Mode>,
    entry: Vec<EntryOptions>,
    module: Option<ModuleConfig>,
    resolve: Option<ResolveConfig>,
    externals: Option<FxIndexMap<RcStr, ExternalConfig>>,
    output: Option<OutputConfig>,
    target: Option<RcStr>,
    source_maps: Option<bool>,
    define: Option<FxIndexMap<String, JsonValue>>,
    images: Option<ImageConfig>,
    styles: Option<StyleConfig>,
    optimization: Option<OptimizationConfig>,
    stats: Option<bool>,
    #[serde(default)]
    experimental: ExperimentalConfig,
    persistent_caching: Option<bool>,
    cache_handler: Option<RcStr>,
    node_polyfill: Option<bool>,
    #[cfg(feature = "test")]
    #[serde(rename = "runtimeType")]
    runtime_type: Option<RcStr>,
}

#[derive(
    Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TraceRawVcs, NonLocalValue, OperationValue,
)]
#[serde(untagged)]
pub enum ExternalConfig {
    Basic(RcStr),
    Umd(ExternalUmd),
    Advanced(ExternalAdvanced),
}

#[derive(
    Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TraceRawVcs, NonLocalValue, OperationValue,
)]
pub enum ExternalType {
    #[serde(rename = "script")]
    Script,
    #[serde(rename = "commonjs")]
    CommonJs,
    #[serde(rename = "esm")]
    ESM,
    #[serde(rename = "global")]
    Global,
}

#[derive(Clone, Debug, PartialEq, Eq, TraceRawVcs, NonLocalValue, OperationValue)]
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

#[derive(
    Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TraceRawVcs, NonLocalValue, OperationValue,
)]
#[serde(rename_all = "PascalCase")]
pub enum ExternalTargetConverter {
    PascalCase,
    CamelCase,
    KebabCase,
    SnakeCase,
}

#[derive(
    Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TraceRawVcs, NonLocalValue, OperationValue,
)]
#[serde(rename_all = "camelCase")]
pub struct ExternalSubPathRule {
    pub regex: RcStr,
    pub target: ExternalSubPathTarget,
    pub target_converter: Option<ExternalTargetConverter>,
}

#[derive(
    Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TraceRawVcs, NonLocalValue, OperationValue,
)]
pub struct ExternalSubPath {
    pub exclude: Option<Vec<RcStr>>,
    pub rules: Vec<ExternalSubPathRule>,
}

#[derive(
    Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TraceRawVcs, NonLocalValue, OperationValue,
)]
#[serde(rename_all = "camelCase")]
pub struct ExternalUmd {
    /// Root global variable name
    pub root: RcStr,
    /// CommonJS module reference
    pub commonjs: RcStr,
}

#[derive(
    Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TraceRawVcs, NonLocalValue, OperationValue,
)]
#[serde(rename_all = "camelCase")]
pub struct ExternalAdvanced {
    pub root: RcStr,
    #[serde(rename = "type")]
    pub r#type: Option<ExternalType>,
    pub script: Option<RcStr>,
    pub sub_path: Option<ExternalSubPath>,
}

#[turbo_tasks::value(eq = "manual")]
#[derive(Clone, Debug, PartialEq, Default, OperationValue)]
#[serde(rename_all = "camelCase")]
pub struct StyleConfig {
    pub emotion: Option<EmotionTransformOptionsOrBoolean>,
    pub styled_components: Option<StyledComponentsTransformOptionsOrBoolean>,
    sass: Option<serde_json::Value>,
    less: Option<serde_json::Value>,
    inline_css: Option<serde_json::Value>,
}

#[derive(
    Clone,
    Debug,
    Eq,
    Default,
    PartialEq,
    Serialize,
    Deserialize,
    TraceRawVcs,
    ValueDebugFormat,
    NonLocalValue,
    OperationValue,
)]
pub struct ResolveConfig {
    #[serde(rename = "alias")]
    resolve_alias: Option<FxIndexMap<RcStr, JsonValue>>,
    #[serde(rename = "extensions")]
    resolve_extensions: Option<Vec<RcStr>>,
}

#[derive(
    Clone,
    Debug,
    Eq,
    Default,
    PartialEq,
    Serialize,
    Deserialize,
    TraceRawVcs,
    ValueDebugFormat,
    NonLocalValue,
    OperationValue,
)]
#[serde(rename_all = "camelCase")]
pub struct ImageConfig {
    pub inline_limit: Option<u64>,
}

#[turbo_tasks::value(transparent)]
pub struct OptionImageConfig(Option<ImageConfig>);

#[turbo_tasks::value(eq = "manual")]
#[derive(Clone, Debug, PartialEq, Default, OperationValue)]
#[serde(rename_all = "camelCase")]
pub struct OptimizationConfig {
    pub module_ids: Option<ModuleIds>,
    /// When the code is minified, this opts out of the default mangling of
    /// local names for variables, functions etc., which can be useful for
    /// debugging/profiling purposes.
    pub no_mangling: Option<bool>,
    pub minify: Option<bool>,
    pub tree_shaking: Option<bool>,
    pub package_imports: Option<Vec<RcStr>>,
    pub modularize_imports: Option<FxIndexMap<String, ModularizeImportPackageConfig>>,
    pub transpile_packages: Option<Vec<RcStr>>,
    pub remove_console: Option<RemoveConsoleConfig>,
    #[serde(default)]
    pub split_chunks: FxIndexMap<RcStr, SplitChunkConfig>,
    /// Concatenate modules when possible to reduce the number of chunks.
    /// This can improve performance by reducing the number of requests and
    /// improving caching.
    #[serde(default)]
    pub concatenate_modules: Option<bool>,
    /// Defaults to false in development mode, true in production mode.
    pub remove_unused_exports: Option<bool>,
    pub nested_async_chunking: Option<bool>,
    /// Whether to inline WASM files into the bundle. Defaults to true.
    /// When false, WASM files will be output as static assets.
    #[serde(default)]
    pub inline_wasm: Option<bool>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, TraceRawVcs, NonLocalValue, OperationValue,
)]
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

#[turbo_tasks::value(eq = "manual")]
#[derive(Clone, Debug, PartialEq, Default, OperationValue)]
#[serde(rename_all = "camelCase")]
pub struct OutputConfig {
    pub path: Option<RcStr>,
    pub filename: Option<RcStr>,
    pub chunk_filename: Option<RcStr>,
    // TODO: make sure this is needed
    pub r#type: Option<OutputType>,
    pub clean: Option<bool>,
    pub copy: Option<Vec<CopyItem>>,
    /// URL prefix that will be prepended to all chunk and asset URLs when loading them.
    /// This is used to configure CDN URLs or serve assets from a different path.
    /// Examples: "/", "/assets/", "https://cdn.example.com/"
    /// Note: This path will not appear in chunk paths or chunk data on disk,
    /// it only affects the URLs used by the browser to fetch resources.
    pub public_path: Option<RcStr>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, TraceRawVcs, NonLocalValue, OperationValue,
)]
#[serde(rename_all = "kebab-case")]
pub enum OutputType {
    Standalone,
    Export,
}

#[derive(
    Serialize, Deserialize, Clone, PartialEq, Eq, Debug, TraceRawVcs, NonLocalValue, OperationValue,
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
            ConfigConditionItem::Base { path, content } => ConditionItem::Base {
                path: path.map(ConditionPath::try_from).transpose()?,
                content: content
                    .map(EsRegex::try_from)
                    .transpose()?
                    .map(EsRegex::resolved_cell),
            },
        })
    }
}

#[derive(
    Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TraceRawVcs, NonLocalValue, OperationValue,
)]
#[serde(rename_all = "camelCase")]
pub struct RuleConfigItem {
    pub loaders: Vec<LoaderItem>,
    #[serde(default, alias = "as")]
    pub rename_as: Option<RcStr>,
    #[serde(default)]
    pub condition: Option<ConfigConditionItem>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TraceRawVcs, NonLocalValue, OperationValue)]
#[serde(transparent)]
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

#[derive(
    Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TraceRawVcs, NonLocalValue, OperationValue,
)]
#[serde(untagged)]
pub enum RuleConfigCollectionItem {
    Shorthand(LoaderItem),
    Full(RuleConfigItem),
}

#[derive(
    Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TraceRawVcs, NonLocalValue, OperationValue,
)]
#[serde(untagged)]
pub enum LoaderItem {
    LoaderName(RcStr),
    LoaderOptions(WebpackLoaderItem),
}

#[turbo_tasks::value(operation)]
#[derive(Copy, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub enum ModuleIds {
    Named,
    Deterministic,
}

#[turbo_tasks::value(transparent)]
pub struct OptionModuleIds(pub Option<ModuleIds>);

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, TraceRawVcs, NonLocalValue, OperationValue,
)]
#[serde(untagged)]
pub enum MdxRsOptions {
    Boolean(bool),
    Option(MdxTransformOptions),
}

#[turbo_tasks::value(shared, operation)]
#[derive(Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub enum ReactCompilerMode {
    Infer,
    Annotation,
    All,
}

/// Subset of react compiler options
#[turbo_tasks::value(shared, operation)]
#[derive(Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ReactCompilerOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compilation_mode: Option<ReactCompilerMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub panic_threshold: Option<RcStr>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, TraceRawVcs, NonLocalValue, OperationValue,
)]
#[serde(untagged)]
pub enum ReactCompilerOptionsOrBoolean {
    Boolean(bool),
    Option(ReactCompilerOptions),
}

#[turbo_tasks::value(transparent)]
pub struct OptionalReactCompilerOptions(Option<ResolvedVc<ReactCompilerOptions>>);

#[derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    Serialize,
    Deserialize,
    TraceRawVcs,
    NonLocalValue,
    OperationValue,
)]
#[serde(rename_all = "camelCase")]
pub struct ModuleConfig {
    pub rules: Option<FxIndexMap<RcStr, RuleConfigCollection>>,
}

#[derive(
    Serialize, Deserialize, Clone, PartialEq, Eq, Debug, TraceRawVcs, NonLocalValue, OperationValue,
)]
#[serde(deny_unknown_fields)]
pub struct RegexComponents {
    source: RcStr,
    flags: RcStr,
}

#[derive(
    Clone, PartialEq, Eq, Debug, Serialize, Deserialize, TraceRawVcs, NonLocalValue, OperationValue,
)]
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

#[derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    Serialize,
    Deserialize,
    TraceRawVcs,
    ValueDebugFormat,
    NonLocalValue,
    OperationValue,
)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentalConfig {
    swc_plugins: Option<Vec<(RcStr, serde_json::Value)>>,
    mdx_rs: Option<MdxRsOptions>,
    #[serde(rename = "dynamicIO")]
    dynamic_io: Option<bool>,
    use_cache: Option<bool>,
    cache_handlers: Option<FxIndexMap<RcStr, RcStr>>,
    esm_externals: Option<EsmExternals>,
    /// Using this feature will enable the `react@experimental` for the `app`
    /// directory.
    ppr: Option<ExperimentalPartialPrerendering>,
    taint: Option<bool>,
    react_compiler: Option<ReactCompilerOptionsOrBoolean>,
    view_transition: Option<bool>,
    server_actions: Option<ServerActionsOrLegacyBool>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, TraceRawVcs, NonLocalValue, OperationValue,
)]
#[serde(rename_all = "lowercase")]
pub enum ExperimentalPartialPrerenderingIncrementalValue {
    Incremental,
}

#[derive(
    Clone, Debug, PartialEq, Deserialize, Serialize, TraceRawVcs, NonLocalValue, OperationValue,
)]
#[serde(untagged)]
pub enum ExperimentalPartialPrerendering {
    Boolean(bool),
    Incremental(ExperimentalPartialPrerenderingIncrementalValue),
}

#[derive(
    Clone, Debug, PartialEq, Deserialize, Serialize, TraceRawVcs, NonLocalValue, OperationValue,
)]
#[serde(untagged)]
pub enum ServerActionsOrLegacyBool {
    /// The current way to configure server actions sub behaviors.
    ServerActionsConfig(ServerActions),

    /// The legacy way to disable server actions. This is no longer used, server
    /// actions is always enabled.
    LegacyBool(bool),
}

#[derive(
    Clone, Debug, PartialEq, Deserialize, Serialize, TraceRawVcs, NonLocalValue, OperationValue,
)]
#[serde(rename_all = "kebab-case")]
pub enum EsmExternalsValue {
    Loose,
}

#[derive(
    Clone, Debug, PartialEq, Deserialize, Serialize, TraceRawVcs, NonLocalValue, OperationValue,
)]
#[serde(untagged)]
pub enum EsmExternals {
    Loose(EsmExternalsValue),
    Bool(bool),
}

#[derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
    Deserialize,
    Serialize,
    TraceRawVcs,
    NonLocalValue,
    OperationValue,
)]
#[serde(rename_all = "camelCase")]
pub struct ServerActions {
    /// Allows adjusting body parser size limit for server actions.
    pub body_size_limit: Option<SizeLimit>,
}

#[derive(Clone, Debug, Serialize, Deserialize, TraceRawVcs, NonLocalValue, OperationValue)]
#[serde(untagged)]
pub enum SizeLimit {
    Number(f64),
    WithUnit(String),
}

// Manual implementation of PartialEq and Eq for SizeLimit because f64 doesn't
// implement Eq.
impl PartialEq for SizeLimit {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (SizeLimit::Number(a), SizeLimit::Number(b)) => a.to_bits() == b.to_bits(),
            (SizeLimit::WithUnit(a), SizeLimit::WithUnit(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for SizeLimit {}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, TraceRawVcs, NonLocalValue, OperationValue,
)]
#[serde(untagged)]
pub enum EmotionTransformOptionsOrBoolean {
    Boolean(bool),
    Options(EmotionTransformConfig),
}

impl EmotionTransformOptionsOrBoolean {
    pub fn is_enabled(&self) -> bool {
        match self {
            Self::Boolean(enabled) => *enabled,
            _ => true,
        }
    }
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, TraceRawVcs, NonLocalValue, OperationValue,
)]
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

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, TraceRawVcs, NonLocalValue, OperationValue,
)]
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

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, TraceRawVcs, NonLocalValue, OperationValue,
)]
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

#[derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    TraceRawVcs,
    NonLocalValue,
    OperationValue,
)]
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
pub struct SplitChunksConfig(FxIndexMap<RcStr, SplitChunkConfig>);

#[turbo_tasks::value(transparent)]
pub struct ResolveExtensions(Option<Vec<RcStr>>);

#[turbo_tasks::value(transparent)]
pub struct SwcPlugins(Vec<(RcStr, serde_json::Value)>);

#[turbo_tasks::value(transparent)]
pub struct OptionalMdxTransformOptions(Option<ResolvedVc<MdxTransformOptions>>);

#[turbo_tasks::value(transparent)]
pub struct OptionServerActions(Option<ServerActions>);

#[turbo_tasks::value(transparent)]
pub struct ExternalsConfig(FxIndexMap<RcStr, ExternalConfig>);

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

#[turbo_tasks::value_impl]
impl Issue for InvalidLoaderRuleRenameAsIssue {
    #[turbo_tasks::function]
    async fn file_path(&self) -> Result<Vc<FileSystemPath>> {
        Ok(self.config_file_path.clone().cell())
    }

    #[turbo_tasks::function]
    fn stage(&self) -> Vc<IssueStage> {
        IssueStage::Config.cell()
    }

    #[turbo_tasks::function]
    async fn title(&self) -> Result<Vc<StyledString>> {
        Ok(
            StyledString::Text(format!("Invalid loader rule for extension: {}", self.glob).into())
                .cell(),
        )
    }

    #[turbo_tasks::function]
    async fn description(&self) -> Result<Vc<OptionStyledString>> {
        Ok(Vc::cell(Some(
            StyledString::Text(RcStr::from(format!(
                "The extension {} contains a wildcard, but the `as` option does not: {}",
                self.glob, self.rename_as,
            )))
            .resolved_cell(),
        )))
    }

    #[turbo_tasks::function]
    fn documentation_link(&self) -> Vc<RcStr> {
        Vc::cell(turbopack_config_documentation_link())
    }
}

#[turbo_tasks::value(shared)]
struct InvalidLoaderRuleConditionIssue {
    condition: ConfigConditionItem,
    config_file_path: FileSystemPath,
}

#[turbo_tasks::value_impl]
impl Issue for InvalidLoaderRuleConditionIssue {
    #[turbo_tasks::function]
    async fn file_path(self: Vc<Self>) -> Result<Vc<FileSystemPath>> {
        Ok(self.await?.config_file_path.clone().cell())
    }

    #[turbo_tasks::function]
    fn stage(self: Vc<Self>) -> Vc<IssueStage> {
        IssueStage::Config.cell()
    }

    #[turbo_tasks::function]
    async fn title(&self) -> Result<Vc<StyledString>> {
        Ok(StyledString::Text(rcstr!("Invalid condition for Turbopack loader rule")).cell())
    }

    #[turbo_tasks::function]
    async fn description(&self) -> Result<Vc<OptionStyledString>> {
        Ok(Vc::cell(Some(
            StyledString::Text(RcStr::from(
                serde_json::to_string_pretty(&self.condition)
                    .expect("condition must be serializable"),
            ))
            .resolved_cell(),
        )))
    }

    #[turbo_tasks::function]
    fn documentation_link(&self) -> Vc<RcStr> {
        Vc::cell(turbopack_config_documentation_link())
    }
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
    pub fn cache_handler(&self) -> Vc<Option<RcStr>> {
        Vc::cell(self.cache_handler.clone())
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
    pub fn output(&self) -> Vc<OutputConfig> {
        self.output.clone().unwrap_or_default().cell()
    }

    #[turbo_tasks::function]
    pub fn mode(&self) -> Vc<Mode> {
        self.mode.unwrap_or_default().cell()
    }

    #[turbo_tasks::function]
    pub fn target(&self) -> Vc<RcStr> {
        Vc::cell(self.target.clone().unwrap_or(
            "last 1 Chrome versions, last 1 Firefox versions, last 1 Safari versions, last 1 Edge versions".into()
        ))
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
        let Some(turbo_rules) = self.module.as_ref().and_then(|t| t.rules.as_ref()) else {
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
                            },
                        ));
                    }
                    RuleConfigCollectionItem::Full(RuleConfigItem {
                        loaders,
                        rename_as,
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
                            if let Ok(cond) = ConditionItem::try_from(condition.clone()) {
                                Some(cond)
                            } else {
                                InvalidLoaderRuleConditionIssue {
                                    condition: condition.clone(),
                                    config_file_path: project_path.clone(),
                                }
                                .resolved_cell()
                                .emit();
                                None
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
    pub async fn import_externals(&self) -> Result<Vc<bool>> {
        Ok(Vc::cell(match self.experimental.esm_externals {
            Some(EsmExternals::Bool(b)) => b,
            Some(EsmExternals::Loose(_)) => bail!("esmExternals = \"loose\" is not supported"),
            None => true,
        }))
    }

    #[turbo_tasks::function]
    pub fn mdx_rs(&self) -> Vc<OptionalMdxTransformOptions> {
        let options = &self.experimental.mdx_rs;

        let options = match options {
            Some(MdxRsOptions::Boolean(true)) => OptionalMdxTransformOptions(Some(
                MdxTransformOptions {
                    provider_import_source: Some(mdx_import_source_file()),
                    ..Default::default()
                }
                .resolved_cell(),
            )),
            Some(MdxRsOptions::Option(options)) => OptionalMdxTransformOptions(Some(
                MdxTransformOptions {
                    provider_import_source: Some(
                        options
                            .provider_import_source
                            .clone()
                            .unwrap_or(mdx_import_source_file()),
                    ),
                    ..options.clone()
                }
                .resolved_cell(),
            )),
            _ => OptionalMdxTransformOptions(None),
        };

        options.cell()
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
    pub fn experimental_swc_plugins(&self) -> Vc<SwcPlugins> {
        Vc::cell(self.experimental.swc_plugins.clone().unwrap_or_default())
    }

    #[turbo_tasks::function]
    pub fn experimental_server_actions(&self) -> Vc<OptionServerActions> {
        Vc::cell(match self.experimental.server_actions.as_ref() {
            Some(ServerActionsOrLegacyBool::ServerActionsConfig(server_actions)) => {
                Some(server_actions.clone())
            }
            Some(ServerActionsOrLegacyBool::LegacyBool(true)) => Some(ServerActions::default()),
            _ => None,
        })
    }

    #[turbo_tasks::function]
    pub fn react_compiler(&self) -> Vc<OptionalReactCompilerOptions> {
        let options = &self.experimental.react_compiler;

        let options = match options {
            Some(ReactCompilerOptionsOrBoolean::Boolean(true)) => {
                OptionalReactCompilerOptions(Some(
                    ReactCompilerOptions {
                        compilation_mode: None,
                        panic_threshold: None,
                    }
                    .resolved_cell(),
                ))
            }
            Some(ReactCompilerOptionsOrBoolean::Option(options)) => OptionalReactCompilerOptions(
                Some(ReactCompilerOptions { ..options.clone() }.resolved_cell()),
            ),
            _ => OptionalReactCompilerOptions(None),
        };

        options.cell()
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
    pub fn enable_ppr(&self) -> Vc<bool> {
        Vc::cell(
            self.experimental
                .ppr
                .as_ref()
                .map(|ppr| match ppr {
                    ExperimentalPartialPrerendering::Incremental(
                        ExperimentalPartialPrerenderingIncrementalValue::Incremental,
                    ) => true,
                    ExperimentalPartialPrerendering::Boolean(b) => *b,
                })
                .unwrap_or(false),
        )
    }

    #[turbo_tasks::function]
    pub fn enable_taint(&self) -> Vc<bool> {
        Vc::cell(self.experimental.taint.unwrap_or(false))
    }

    #[turbo_tasks::function]
    pub fn enable_view_transition(&self) -> Vc<bool> {
        Vc::cell(self.experimental.view_transition.unwrap_or(false))
    }

    #[turbo_tasks::function]
    pub fn enable_dynamic_io(&self) -> Vc<bool> {
        Vc::cell(self.experimental.dynamic_io.unwrap_or(false))
    }

    #[turbo_tasks::function]
    pub fn enable_use_cache(&self) -> Vc<bool> {
        Vc::cell(
            self.experimental
                .use_cache
                // "use cache" was originally implicitly enabled with the
                // dynamicIO flag, so we transfer the value for dynamicIO to the
                // explicit useCache flag to ensure backwards compatibility.
                .unwrap_or(self.experimental.dynamic_io.unwrap_or(false)),
        )
    }

    #[turbo_tasks::function]
    pub fn cache_kinds(&self) -> Vc<CacheKinds> {
        let mut cache_kinds = CacheKinds::default();

        if let Some(handlers) = self.experimental.cache_handlers.as_ref() {
            cache_kinds.extend(handlers.keys().cloned());
        }

        cache_kinds.cell()
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
        let is_prod = matches!(*mode.await?, Mode::Production);
        let remove_unused_exports = self
            .optimization
            .as_ref()
            .and_then(|op| op.remove_unused_exports)
            .unwrap_or(is_prod);
        Ok(Vc::cell(remove_unused_exports))
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

        // Special handling for "runtime" value - return a marker that will be
        // replaced at runtime with window.publicPath
        if public_path.as_str() == "runtime" {
            return Ok(Vc::cell("__RUNTIME_PUBLIC_PATH__".into()));
        }

        Ok(Vc::cell(
            format!("{}/", public_path.trim_end_matches("/")).into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_esm_externals_deserialization() {
        let json = serde_json::json!({
            "esmExternals": true
        });
        let config: ExperimentalConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.esm_externals, Some(EsmExternals::Bool(true)));

        let json = serde_json::json!({
            "esmExternals": "loose"
        });
        let config: ExperimentalConfig = serde_json::from_value(json).unwrap();
        assert_eq!(
            config.esm_externals,
            Some(EsmExternals::Loose(EsmExternalsValue::Loose))
        );
    }

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
