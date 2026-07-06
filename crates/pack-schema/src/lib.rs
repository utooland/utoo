use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Top-level configuration for utoopack.json
/// This represents the JSON config file structure, aligned with pack-core's `Config` struct.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CompleteConfig {
    /// Build mode (development, production)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Build mode")]
    pub mode: Option<String>,

    /// Entry points for the build
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Entry points for the build")]
    pub entry: Option<Vec<SchemaEntryOptions>>,

    /// Module configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Module configuration")]
    pub module: Option<SchemaModuleConfig>,

    /// Resolve configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Resolve configuration")]
    pub resolve: Option<SchemaResolveConfig>,

    /// External dependencies configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "External dependencies configuration")]
    pub externals: Option<HashMap<String, SchemaExternalConfig>>,

    /// Output configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Output configuration")]
    pub output: Option<SchemaOutputConfig>,

    /// Target environment (e.g., "web", "node", or a browserslist query)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Target environment")]
    pub target: Option<String>,

    /// Enable source maps
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Enable source maps")]
    pub source_maps: Option<bool>,

    /// Define variables for build-time replacement
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Define variables for build-time replacement")]
    pub define: Option<HashMap<String, serde_json::Value>>,

    /// Provider (ProvidePlugin-style) configuration.
    /// Maps free variable names to module specifiers or [module, export] pairs.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Provider (ProvidePlugin) configuration")]
    pub provider: Option<HashMap<String, SchemaProviderConfigValue>>,

    /// Image processing configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Image processing configuration")]
    pub images: Option<SchemaImageConfig>,

    /// Style processing configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Style processing configuration")]
    pub styles: Option<SchemaStyleConfig>,

    /// React configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "React configuration")]
    pub react: Option<SchemaReactConfig>,

    /// Enable Rust MDX transform support
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Enable Rust MDX transform support")]
    pub mdx: Option<SchemaMdxConfigOrBoolean>,

    /// Build optimization settings
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Build optimization settings")]
    pub optimization: Option<SchemaOptimizationConfig>,

    /// Enable build statistics
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Enable build statistics")]
    pub stats: Option<bool>,

    /// Enable the Rust React Compiler transform
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "React compiler")]
    pub react_compiler: Option<SchemaReactCompilerConfig>,

    /// Enable persistent caching
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Enable persistent caching")]
    pub persistent_caching: Option<bool>,

    /// Turbopack memory eviction mode for the persistent cache
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Turbopack memory eviction mode for the persistent cache")]
    pub turbopack_memory_eviction: Option<SchemaTurbopackMemoryEviction>,

    /// Cache handler configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Cache handler configuration")]
    pub cache_handler: Option<String>,

    /// Enable Node.js polyfills for browser builds
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Enable Node.js polyfills for browser builds")]
    pub node_polyfill: Option<bool>,

    /// Development server configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Development server configuration")]
    pub dev_server: Option<SchemaDevServer>,

    /// Server-side configuration (server functions, future RSC)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Server-side configuration")]
    pub server: Option<SchemaServerConfig>,

    /// Experimental features
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Experimental features")]
    pub experimental: Option<SchemaExperimentalConfig>,
}

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

/// Provider configuration value.
/// Can be a simple module name string or a [module, export] tuple.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SchemaProviderConfigValue {
    /// Simple module import, e.g. "jquery"
    Module(String),
    /// Named export import, e.g. ["buffer", "Buffer"]
    NamedExport(Vec<String>),
}

// ---------------------------------------------------------------------------
// React
// ---------------------------------------------------------------------------

/// React configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaReactConfig {
    /// JSX runtime to use ("automatic" or "classic")
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "JSX runtime to use (automatic or classic)")]
    pub runtime: Option<String>,

    /// Custom JSX import source (e.g. "@emotion/react")
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Custom JSX import source")]
    pub import_source: Option<String>,
}

/// MDX configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SchemaMdxConfigOrBoolean {
    /// Simple boolean to enable/disable the Rust MDX transform
    Boolean(bool),
    /// Advanced MDX transform options
    Options(SchemaMdxConfig),
}

/// Turbopack memory eviction mode.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SchemaTurbopackMemoryEviction {
    Boolean(bool),
    Mode(SchemaTurbopackMemoryEvictionMode),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SchemaTurbopackMemoryEvictionMode {
    Full,
}

/// Rust MDX transform options
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaMdxConfig {
    /// Whether to compile in development mode
    #[serde(skip_serializing_if = "Option::is_none")]
    pub development: Option<bool>,

    /// Whether to preserve JSX in the MDX compiler output
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jsx: Option<bool>,

    /// JSX runtime to use
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jsx_runtime: Option<String>,

    /// JSX import source to use
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jsx_import_source: Option<String>,

    /// Module providing useMDXComponents
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_import_source: Option<String>,

    /// MDX parser mode
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mdx_type: Option<SchemaMdxParseConstructs>,
}

/// MDX parser mode
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SchemaMdxParseConstructs {
    Commonmark,
    Gfm,
}

// ---------------------------------------------------------------------------
// Development server
// ---------------------------------------------------------------------------

/// Development server configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaDevServer {
    /// Enable hot module replacement
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hot: Option<bool>,
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

/// Server-side configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaServerConfig {
    /// Entry point for the server runtime (e.g. "src/server.ts")
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Entry point for the server runtime (e.g. \"src/server.ts\")")]
    pub entry: Option<String>,

    /// Configuration for Server Functions (RPC)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Configuration for Server Functions (RPC) boundaries")]
    pub function: Option<SchemaServerFunctionConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaServerFunctionConfig {
    /// Module that exports `createServerReference` for client-side proxy generation.
    /// Expected signature:
    /// ```ts
    /// export function createServerReference(actionId: string, name: string) {
    ///   return async function (...args: any[]) { /* HTTP fetch to server */ }
    /// }
    /// ```
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "Module that exports createServerReference(actionId, name) for client proxy"
    )]
    pub client_proxy: Option<String>,

    /// Module that exports `registerServerReference` for the server bundle.
    /// Expected signature:
    /// ```ts
    /// export function registerServerReference(action: any, actionId: string, name: string) {
    ///   /* Register the action to a global router/map */
    /// }
    /// ```
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "Module that exports registerServerReference(action, actionId, name) for the server bundle"
    )]
    pub server_register: Option<String>,
}

// ---------------------------------------------------------------------------
// Entry
// ---------------------------------------------------------------------------

/// Entry point configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaEntryOptions {
    /// Entry name (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Entry name (optional)")]
    pub name: Option<String>,

    /// Import path for the entry point
    #[schemars(description = "Import path for the entry point")]
    pub import: String,

    /// Library configuration for this entry
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Library configuration for this entry")]
    pub library: Option<SchemaLibraryOptions>,

    /// HTML generation configuration for this entry
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "HTML generation configuration for this entry")]
    pub html: Option<SchemaHtmlConfig>,
}

/// HTML generation configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaHtmlConfig {
    /// Path to the HTML template file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,

    /// Inline HTML template content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template_content: Option<String>,

    /// Output filename for the generated HTML
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,

    /// Title for the generated HTML
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Where to inject scripts (true, false, "body", "head")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inject: Option<serde_json::Value>,

    /// Script loading strategy ("blocking", "defer", "module")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script_loading: Option<String>,

    /// Meta tags configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, serde_json::Value>>,
}

/// Library output configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaLibraryOptions {
    /// Library name (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Library name (optional)")]
    pub name: Option<String>,

    /// Export configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Export configuration")]
    pub export: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

/// Copy item configuration
/// Can be either a string (source path) or an object with from and optional to
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SchemaCopyItem {
    /// String variant: source path (destination will be same as source)
    String(String),
    /// Object variant: source path and optional destination path
    Object {
        /// Source path to copy from
        #[schemars(description = "Source path to copy from")]
        #[serde(rename = "from")]
        from: String,
        /// Destination path to copy to (optional, defaults to same as from)
        #[schemars(
            description = "Destination path to copy to (optional, defaults to same as from)"
        )]
        #[serde(rename = "to", skip_serializing_if = "Option::is_none")]
        to: Option<String>,
    },
}

/// Output configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaOutputConfig {
    /// Output directory path
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Output directory path")]
    pub path: Option<String>,

    /// Filename pattern for main JS files
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Filename pattern for main JS files")]
    pub filename: Option<String>,

    /// Filename pattern for JS chunk files
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Filename pattern for JS chunk files")]
    pub chunk_filename: Option<String>,

    /// Filename pattern for main CSS files
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Filename pattern for main CSS files")]
    pub css_filename: Option<String>,

    /// Filename pattern for CSS chunk files
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Filename pattern for CSS chunk files")]
    pub css_chunk_filename: Option<String>,

    /// Filename pattern for asset modules (images, fonts, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Filename pattern for asset modules (images, fonts, etc.)")]
    pub asset_module_filename: Option<String>,

    /// Output type
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Output type")]
    pub output_type: Option<SchemaOutputType>,

    /// Whether to clean output directory before build
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Whether to clean output directory before build")]
    pub clean: Option<bool>,

    /// Copy files configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Copy files configuration")]
    pub copy: Option<Vec<SchemaCopyItem>>,

    /// URL prefix prepended to all chunk and asset URLs when loading them.
    /// Examples: "/", "/assets/", "https://cdn.example.com/", "runtime", "auto"
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "URL prefix prepended to all chunk and asset URLs. Use 'runtime' for globalThis.publicPath or 'auto' for current-script inference."
    )]
    pub public_path: Option<String>,

    /// Controls the `crossorigin` attribute for dynamically loaded chunks.
    /// Webpack-compatible values: false, "anonymous", "use-credentials".
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "Controls crossorigin for dynamically loaded chunks. Supports false, 'anonymous', or 'use-credentials'."
    )]
    pub cross_origin_loading: Option<SchemaCrossOriginLoading>,

    /// The global variable name used by the runtime for loading chunks.
    /// This is similar to webpack's `output.chunkLoadingGlobal`.
    /// Default: "TURBOPACK"
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "The global variable name used by the runtime for loading chunks. Default: 'TURBOPACK'"
    )]
    pub chunk_loading_global: Option<String>,

    /// Expose entry module exports to global scope with the specified name
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "Expose entry module exports to global scope with the specified name. When set, all named exports from the entry module will be available on window/globalThis under the specified name. If set to empty string, will use package.json name. Default: None (no exposure)"
    )]
    pub entry_root_export: Option<String>,
}

/// Cross-origin loading mode for dynamic chunks.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SchemaCrossOriginLoadingMode {
    Anonymous,
    UseCredentials,
}

/// Webpack-compatible `output.crossOriginLoading`.
/// Supports `false`, `"anonymous"`, and `"use-credentials"`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SchemaCrossOriginLoading {
    Boolean(bool),
    Mode(SchemaCrossOriginLoadingMode),
}

/// Output type
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SchemaOutputType {
    Standalone,
    Export,
}

// ---------------------------------------------------------------------------
// Optimization
// ---------------------------------------------------------------------------

/// Optimization configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaOptimizationConfig {
    /// Module ID generation strategy
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Module ID generation strategy")]
    pub module_ids: Option<SchemaModuleIds>,

    /// Whether to disable name mangling
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Whether to disable name mangling")]
    pub no_mangling: Option<bool>,

    /// Whether to enable compression when minifying
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Whether to enable compression when minifying")]
    pub compress: Option<SchemaCompressConfig>,

    /// Whether to minify the output
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Whether to minify the output")]
    pub minify: Option<bool>,

    /// Whether to enable tree shaking
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Whether to enable tree shaking")]
    pub tree_shaking: Option<bool>,

    /// Packages to optimize imports for
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Packages to optimize imports for")]
    pub package_imports: Option<Vec<String>>,

    /// Modularize imports configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Modularize imports configuration")]
    pub modularize_imports: Option<HashMap<String, SchemaModularizeImportPackageConfig>>,

    /// Packages to transpile
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Packages to transpile")]
    pub transpile_packages: Option<Vec<String>>,

    /// Console removal configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Console removal configuration")]
    pub remove_console: Option<SchemaRemoveConsoleConfig>,

    /// Split chunks configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Split chunks configuration")]
    pub split_chunks: Option<HashMap<String, SchemaSplitChunkConfig>>,

    /// Whether to concatenate modules when possible to reduce the number of chunks
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "Whether to concatenate modules when possible to reduce the number of chunks. This can improve performance by reducing the number of requests and improving caching."
    )]
    pub concatenate_modules: Option<bool>,

    /// Whether to remove unused exports
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "Whether to remove unused exports. Defaults to false in development, true in production."
    )]
    pub remove_unused_exports: Option<bool>,

    /// Whether to remove unused imports
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "Whether to remove unused imports. Defaults to false in development, true in production."
    )]
    pub remove_unused_imports: Option<bool>,

    /// Whether to enable nested async chunking
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Whether to enable nested async chunking")]
    pub nested_async_chunking: Option<bool>,

    /// Whether to bundle WASM as asset
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "Whether to bundle WASM as asset. Defaults to false. When false, WASM files will be output as static assets."
    )]
    pub wasm_as_asset: Option<bool>,
}

/// Module ID generation strategy
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SchemaModuleIds {
    Named,
    Deterministic,
}

/// Compress configuration (boolean or options object)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SchemaCompressConfig {
    Boolean(bool),
    Options(SchemaCompressOptions),
}

/// Compress options for minification
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaCompressOptions {
    /// Number of compress passes
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Number of compress passes")]
    pub passes: Option<u8>,

    /// Sequence optimization level
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Sequence optimization level")]
    pub sequences: Option<u8>,

    /// Keep class names during compression
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Keep class names during compression")]
    pub keep_classnames: Option<bool>,

    /// Keep function names during compression
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Keep function names during compression")]
    pub keep_fnames: Option<bool>,
}

/// Transform configuration for modularize imports
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SchemaTransform {
    /// String transform template
    String(String),
    /// Vector of transformation pairs
    Vec(Vec<(String, String)>),
}

/// Modularize import package configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaModularizeImportPackageConfig {
    /// Transform configuration
    #[schemars(description = "Transform configuration")]
    pub transform: SchemaTransform,

    /// Prevent full import of the package
    #[serde(default)]
    #[schemars(description = "Prevent full import of the package")]
    pub prevent_full_import: bool,

    /// Skip default conversion
    #[serde(default)]
    #[schemars(description = "Skip default conversion")]
    pub skip_default_conversion: bool,

    /// Handle default import
    #[serde(default)]
    #[schemars(description = "Handle default import")]
    pub handle_default_import: bool,

    /// Handle namespace import
    #[serde(default)]
    #[schemars(description = "Handle namespace import")]
    pub handle_namespace_import: bool,

    /// Style import configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Style import configuration")]
    pub style: Option<String>,
}

/// Console removal configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SchemaRemoveConsoleConfig {
    /// Simple boolean to enable/disable
    Boolean(bool),
    /// Advanced configuration
    Config {
        /// Methods to exclude from removal
        #[serde(skip_serializing_if = "Option::is_none")]
        #[schemars(description = "Methods to exclude from removal")]
        exclude: Option<Vec<String>>,
    },
}

/// Split chunk configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaSplitChunkConfig {
    /// Minimum chunk size
    #[serde(default = "default_min_chunk_size")]
    #[schemars(description = "Minimum chunk size")]
    pub min_chunk_size: usize,

    /// Maximum chunk count per group
    #[serde(default = "default_max_chunk_count_per_group")]
    #[schemars(description = "Maximum chunk count per group")]
    pub max_chunk_count_per_group: usize,

    /// Maximum merge chunk size
    #[serde(default = "default_max_merge_chunk_size")]
    #[schemars(description = "Maximum merge chunk size")]
    pub max_merge_chunk_size: usize,
}

// Import defaults from pack-core
pub use pack_core::config::{
    default_max_chunk_count_per_group, default_max_merge_chunk_size, default_min_chunk_size,
};

// ---------------------------------------------------------------------------
// Externals
// ---------------------------------------------------------------------------

/// External dependency configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SchemaExternalConfig {
    /// Simple string external (e.g., "react" -> "React")
    Basic(String),
    /// UMD external configuration
    Umd(SchemaExternalUmd),
    /// Subpath external configuration (for complex path handling)
    Advanced(SchemaExternalAdvanced),
}

/// Subpath external configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaExternalAdvanced {
    /// Root name for the external
    #[schemars(description = "Root name for the external")]
    pub root: String,

    /// Type of external
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Type of external")]
    pub external_type: Option<SchemaExternalType>,

    /// Script URL for script type externals
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Script URL for script type externals")]
    pub script: Option<String>,

    /// Sub-path configuration for complex externals
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Sub-path configuration for complex externals")]
    pub sub_path: Option<SchemaExternalSubPath>,
}

/// External type
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SchemaExternalType {
    Script,
    #[serde(rename = "commonjs")]
    CommonJs,
    #[serde(rename = "esm")]
    ESM,
    Global,
    Promise,
}

/// Sub-path configuration for externals
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaExternalSubPath {
    /// Paths to exclude
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Paths to exclude")]
    pub exclude: Option<Vec<String>>,

    /// Transformation rules
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Transformation rules")]
    pub rules: Option<Vec<SchemaExternalSubPathRule>>,
}

/// Sub-path transformation rule
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaExternalSubPathRule {
    /// Regular expression to match
    #[schemars(description = "Regular expression to match")]
    pub regex: String,

    /// Target replacement pattern (supports $empty and template strings)
    #[schemars(description = "Target replacement pattern (supports $empty and template strings)")]
    pub target: String,

    /// Target case converter
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Target case converter")]
    pub target_converter: Option<SchemaExternalTargetConverter>,
}

/// Target case converter
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum SchemaExternalTargetConverter {
    PascalCase,
    CamelCase,
    KebabCase,
    SnakeCase,
}

/// UMD external configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaExternalUmd {
    /// Root global variable name
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Root global variable name")]
    pub root: Option<String>,
    /// CommonJS module reference
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "CommonJS module reference")]
    pub commonjs: Option<String>,
}

// ---------------------------------------------------------------------------
// Module / Loaders
// ---------------------------------------------------------------------------

/// Module configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SchemaModuleConfig {
    /// Module rules configuration — keyed by glob pattern (e.g. "*.svg")
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Module rules configuration")]
    pub rules: Option<HashMap<String, SchemaModuleRule>>,
}

/// Module rule configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SchemaModuleRule {
    /// Shorthand for a single loader
    Shorthand(String),
    /// Full rule configuration
    Full(Box<SchemaRuleConfigItem>),
    /// Multiple rule configurations
    Array(Vec<SchemaModuleRuleItem>),
}

/// Item in a module rule array
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SchemaModuleRuleItem {
    /// Shorthand for a single loader
    Shorthand(String),
    /// Full rule configuration
    Full(Box<SchemaRuleConfigItem>),
}

/// Module type for a module rule (`type` / `moduleType` field).
///
/// Values must match pack-core / Turbopack's `ConfiguredModuleType::parse()`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SchemaTurbopackModuleType {
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

/// Full module rule configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaRuleConfigItem {
    /// Loaders to apply
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loaders: Vec<SchemaLoaderItem>,
    /// Rename the module as another extension
    #[serde(default, alias = "as")]
    pub rename_as: Option<String>,
    /// Optional configured module type (`type` / `moduleType`).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "type",
        alias = "moduleType"
    )]
    pub module_type: Option<SchemaTurbopackModuleType>,
    /// Condition for applying the rule
    #[serde(default)]
    pub condition: Option<SchemaConfigConditionItem>,
}

/// Loader configuration item
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SchemaLoaderItem {
    /// Loader name
    LoaderName(String),
    /// Loader with options
    LoaderOptions(serde_json::Value),
}

// ---------------------------------------------------------------------------
// Conditions
// ---------------------------------------------------------------------------

/// Configuration condition item — supports compound conditions (all/any/not)
/// and base conditions with path, content, query, and contentType matchers.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SchemaConfigConditionItem {
    /// All conditions must match
    All { all: Vec<SchemaConfigConditionItem> },
    /// Any condition must match
    Any { any: Vec<SchemaConfigConditionItem> },
    /// Negate a condition
    Not { not: Box<SchemaConfigConditionItem> },
    /// Base condition with path/content/query/contentType matchers
    Base {
        /// Path condition
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<SchemaConfigConditionPath>,
        /// Content regex
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<SchemaRegexComponents>,
        /// Query condition
        #[serde(default, skip_serializing_if = "Option::is_none")]
        query: Option<SchemaConfigConditionQuery>,
        /// Content type condition
        #[serde(
            default,
            rename = "contentType",
            skip_serializing_if = "Option::is_none"
        )]
        content_type: Option<SchemaConfigConditionContentType>,
    },
    /// Built-in condition (e.g., "server", "client", "edge")
    Builtin(String),
}

/// Configuration condition path
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum SchemaConfigConditionPath {
    /// Glob pattern for path matching
    #[schemars(description = "Glob pattern for path matching")]
    Glob(String),
    /// Regular expression for path matching
    #[schemars(description = "Regular expression for path matching")]
    Regex(SchemaRegexComponents),
}

/// Configuration condition for query strings
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum SchemaConfigConditionQuery {
    /// Constant string match
    #[schemars(description = "Constant string match for query")]
    Constant(String),
    /// Regex match
    #[schemars(description = "Regex match for query")]
    Regex(SchemaRegexComponents),
}

/// Configuration condition for content type
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum SchemaConfigConditionContentType {
    /// Glob pattern
    #[schemars(description = "Glob pattern for content type matching")]
    Glob(String),
    /// Regex match
    #[schemars(description = "Regex match for content type")]
    Regex(SchemaRegexComponents),
}

/// Regular expression components
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaRegexComponents {
    /// Regular expression source
    #[schemars(description = "Regular expression source")]
    pub source: String,
    /// Regular expression flags
    #[schemars(description = "Regular expression flags")]
    pub flags: String,
}

// ---------------------------------------------------------------------------
// Resolve
// ---------------------------------------------------------------------------

/// Resolve configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaResolveConfig {
    /// Resolve alias mapping
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Resolve alias mapping")]
    #[serde(rename = "alias")]
    pub resolve_alias: Option<HashMap<String, serde_json::Value>>,

    /// Resolve extensions
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Resolve extensions")]
    #[serde(rename = "extensions")]
    pub resolve_extensions: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// Image
// ---------------------------------------------------------------------------

/// Image configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaImageConfig {
    /// Inline limit for images in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Inline limit for images in bytes")]
    pub inline_limit: Option<u64>,
}

// ---------------------------------------------------------------------------
// Style
// ---------------------------------------------------------------------------

/// Style configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaStyleConfig {
    /// Styled components configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Styled components configuration")]
    pub styled_components: Option<serde_json::Value>,

    /// Enable @emotion/react transform support
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Enable @emotion/react transform support with boolean or options")]
    pub emotion: Option<SchemaEmotionConfigOrBoolean>,

    /// Enable automatic CSS Modules transform (defaults to true)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Enable automatic CSS Modules transform")]
    pub auto_css_modules: Option<bool>,

    /// CSS Modules configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "CSS Modules configuration")]
    pub css_modules: Option<SchemaCssModulesConfig>,

    /// Inline PostCSS configuration passed directly to the PostCSS transform
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Inline PostCSS configuration")]
    pub postcss: Option<serde_json::Value>,

    /// Sass configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Sass configuration")]
    pub sass: Option<serde_json::Value>,

    /// Less configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Less configuration")]
    pub less: Option<serde_json::Value>,

    /// Inline CSS configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Inline CSS configuration")]
    pub inline_css: Option<serde_json::Value>,
}

/// CSS Modules configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaCssModulesConfig {
    /// CSS Modules local class name pattern
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "CSS Modules local class name pattern")]
    pub local_ident_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SchemaEmotionConfigOrBoolean {
    Boolean(bool),
    Options(SchemaEmotionConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaEmotionConfig {
    /// Enable source maps in Emotion transform
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Enable source maps in Emotion transform")]
    pub sourcemap: Option<bool>,

    /// Classname label format
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Emotion label format")]
    pub label_format: Option<String>,

    /// Auto label strategy
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Emotion auto label strategy")]
    pub auto_label: Option<SchemaEmotionLabelKind>,

    /// Emotion import map configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Emotion import map configuration")]
    pub import_map: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SchemaEmotionLabelKind {
    DevOnly,
    Always,
    Never,
}

// ---------------------------------------------------------------------------
// Experimental
// ---------------------------------------------------------------------------

/// Experimental features configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaExperimentalConfig {
    /// SWC plugins — each element is a [plugin_path, plugin_options] tuple
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "SWC plugins as [path, options] tuples")]
    pub swc_plugins: Option<Vec<(String, serde_json::Value)>>,

    /// React compiler
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "React compiler")]
    pub react_compiler: Option<SchemaReactCompilerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SchemaReactCompilerConfig {
    Boolean(bool),
    Options(SchemaReactCompilerOptions),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaReactCompilerOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compilation_mode: Option<SchemaReactCompilerCompilationMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<SchemaReactCompilerTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SchemaReactCompilerCompilationMode {
    Infer,
    Annotation,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum SchemaReactCompilerTarget {
    #[serde(rename = "18")]
    React18,
    #[serde(rename = "19")]
    React19,
}

// ---------------------------------------------------------------------------
// Schema generation
// ---------------------------------------------------------------------------

/// Generate JSON Schema for CompleteConfig
pub fn generate_schema() -> serde_json::Value {
    let schema = schema_for!(CompleteConfig);
    serde_json::to_value(schema).unwrap()
}

/// Generate JSON Schema as a formatted string
pub fn generate_schema_string() -> Result<String, serde_json::Error> {
    let schema = generate_schema();
    serde_json::to_string_pretty(&schema)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_generation() {
        let schema = generate_schema();
        assert!(schema.is_object());

        let schema_obj = schema.as_object().unwrap();
        assert!(schema_obj.contains_key("$schema"));
        assert!(schema_obj.contains_key("title"));
        assert!(schema_obj.contains_key("properties"));
    }

    #[test]
    fn test_schema_contains_expected_fields() {
        let schema = generate_schema();
        let schema_str = serde_json::to_string(&schema).unwrap();

        // Check for key configuration fields
        assert!(schema_str.contains("entry"));
        assert!(schema_str.contains("externals"));
        assert!(schema_str.contains("optimization"));
        assert!(schema_str.contains("concatenateModules"));
        assert!(schema_str.contains("html"));
        assert!(schema_str.contains("react"));
        assert!(schema_str.contains("provider"));
        assert!(schema_str.contains("postcss"));
        assert!(schema_str.contains("publicPath"));
        assert!(schema_str.contains("crossOriginLoading"));
        assert!(schema_str.contains("cssFilename"));
        assert!(schema_str.contains("assetModuleFilename"));
    }

    #[test]
    fn test_deserialize_externals_example() {
        // Test the actual externals configuration from the example file
        let externals_json = r#"
        {
          "foo": "bar",
          "foo_require": "commonjs bar",
          "foo_require2": {
            "root": "bar_require2",
            "type": "commonjs"
          },
          "foo_promise2": {
            "root": "bar_promise2",
            "type": "promise"
          },
          "antd": {
            "root": "antd",
            "subPath": {
              "exclude": ["style"],
              "rules": [
                {
                  "regex": "/(version|message|notification)$/",
                  "target": "$1"
                },
                {
                  "regex": "/zh_CN$/",
                  "target": "$empty"
                }
              ]
            }
          }
        }
        "#;

        let externals: HashMap<String, SchemaExternalConfig> =
            serde_json::from_str(externals_json).unwrap();

        // Verify basic external
        assert!(
            matches!(externals.get("foo"), Some(SchemaExternalConfig::Basic(name)) if name == "bar")
        );

        // Verify we can deserialize advanced externals
        assert!(externals.contains_key("foo_require2"));
        assert!(externals.contains_key("foo_promise2"));
        assert!(externals.contains_key("antd"));

        // Test serialization back to JSON
        let serialized = serde_json::to_string(&externals).unwrap();
        assert!(serialized.contains("bar"));
        assert!(serialized.contains("antd"));
    }

    #[test]
    fn test_deserialize_complete_example() {
        // Test the complete project options configuration (config-level fields only)
        let json = r#"
        {
          "entry": [
            {
              "import": "./index.js"
            }
          ],
          "output": {
            "path": "./dist",
            "filename": "[name].[contenthash:6].js",
            "chunkFilename": "[name].[contenthash:8].js",
            "cssFilename": "[name].[contenthash:6].css",
            "publicPath": "/assets/",
            "crossOriginLoading": "anonymous",
            "clean": true
          },
          "optimization": {
            "moduleIds": "named",
            "minify": false,
            "concatenateModules": true
          },
          "externals": {
            "foo": "bar"
          },
          "react": {
            "runtime": "automatic"
          },
          "provider": {
            "$": "jquery",
            "Buffer": ["buffer", "Buffer"]
          }
        }
        "#;

        let config: CompleteConfig = serde_json::from_str(json).unwrap();
        assert!(config.entry.is_some());
        assert!(config.output.is_some());
        assert!(config.optimization.is_some());
        assert!(config.externals.is_some());
        assert!(config.react.is_some());
        assert!(config.provider.is_some());

        // Test output fields
        let output = config.output.as_ref().unwrap();
        assert_eq!(
            output.css_filename,
            Some("[name].[contenthash:6].css".to_string())
        );
        assert_eq!(output.public_path, Some("/assets/".to_string()));
        assert!(matches!(
            output.cross_origin_loading,
            Some(SchemaCrossOriginLoading::Mode(
                SchemaCrossOriginLoadingMode::Anonymous
            ))
        ));

        // Test concatenateModules configuration
        let optimization = config.optimization.as_ref().unwrap();
        assert_eq!(optimization.concatenate_modules, Some(true));

        // Test react config
        let react = config.react.as_ref().unwrap();
        assert_eq!(react.runtime, Some("automatic".to_string()));

        // Test provider config
        let provider = config.provider.as_ref().unwrap();
        assert!(matches!(
            provider.get("$"),
            Some(SchemaProviderConfigValue::Module(m)) if m == "jquery"
        ));
        assert!(matches!(
            provider.get("Buffer"),
            Some(SchemaProviderConfigValue::NamedExport(v)) if v == &["buffer", "Buffer"]
        ));
    }

    #[test]
    fn test_concatenate_modules_configuration() {
        // Test with concatenateModules: true
        let json_true = r#"
        {
          "optimization": {
            "concatenateModules": true
          }
        }
        "#;
        let config: CompleteConfig = serde_json::from_str(json_true).unwrap();
        let optimization = config.optimization.as_ref().unwrap();
        assert_eq!(optimization.concatenate_modules, Some(true));

        // Test with concatenateModules: false
        let json_false = r#"
        {
          "optimization": {
            "concatenateModules": false
          }
        }
        "#;
        let config: CompleteConfig = serde_json::from_str(json_false).unwrap();
        let optimization = config.optimization.as_ref().unwrap();
        assert_eq!(optimization.concatenate_modules, Some(false));

        // Test without concatenateModules (should be None)
        let json_none = r#"
        {
          "optimization": {
            "minify": true
          }
        }
        "#;
        let config: CompleteConfig = serde_json::from_str(json_none).unwrap();
        let optimization = config.optimization.as_ref().unwrap();
        assert_eq!(optimization.concatenate_modules, None);
    }

    #[test]
    fn test_module_rules_deserialization() {
        let json = r#"
        {
          "module": {
            "rules": {
              "*.txt": {
                "loaders": ["./test-file-loader.js"],
                "as": "*.js",
                "type": "css-module"
              },
              "*.svg": "svg-loader"
            }
          }
        }
        "#;
        let config: CompleteConfig = serde_json::from_str(json).unwrap();
        let rules = config.module.as_ref().unwrap().rules.as_ref().unwrap();

        // Check full rule
        let txt_rule = rules.get("*.txt").unwrap();
        if let SchemaModuleRule::Full(full) = txt_rule {
            let full = full.as_ref();
            assert_eq!(full.rename_as, Some("*.js".to_string()));
            assert_eq!(full.loaders.len(), 1);
        } else {
            panic!("Expected Full rule for *.txt");
        }

        // Check shorthand rule
        let svg_rule = rules.get("*.svg").unwrap();
        if let SchemaModuleRule::Shorthand(s) = svg_rule {
            assert_eq!(s, "svg-loader");
        } else {
            panic!("Expected Shorthand rule for *.svg");
        }
    }

    #[test]
    fn test_output_type_kebab_case() {
        let json = r#"{ "output": { "type": "standalone" } }"#;
        let config: CompleteConfig = serde_json::from_str(json).unwrap();
        assert!(matches!(
            config.output.as_ref().unwrap().output_type,
            Some(SchemaOutputType::Standalone)
        ));

        let json = r#"{ "output": { "type": "export" } }"#;
        let config: CompleteConfig = serde_json::from_str(json).unwrap();
        assert!(matches!(
            config.output.as_ref().unwrap().output_type,
            Some(SchemaOutputType::Export)
        ));
    }

    #[test]
    fn test_swc_plugins_tuple_format() {
        let json = r#"
        {
          "experimental": {
            "swcPlugins": [
              ["@swc/plugin-emotion", {}]
            ]
          }
        }
        "#;
        let config: CompleteConfig = serde_json::from_str(json).unwrap();
        let plugins = config
            .experimental
            .as_ref()
            .unwrap()
            .swc_plugins
            .as_ref()
            .unwrap();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].0, "@swc/plugin-emotion");
    }
}
