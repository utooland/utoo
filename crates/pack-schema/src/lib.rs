use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Top-level project options configuration
/// This represents the root structure of project_options.json
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOptions {
    /// Root path of the project
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Root path of the project")]
    pub root_path: Option<String>,

    /// Project path relative to root
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Project path relative to root")]
    pub project_path: Option<String>,

    /// Main configuration object
    #[schemars(description = "Main configuration object")]
    pub config: SchemaConfig,
}

/// Main configuration structure that mirrors pack_core::config::Config
/// All fields are derived from the original Config structure in pack-core
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaConfig {
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

    /// Target environment (e.g., \"web\", \"node\")
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

    /// Image processing configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Image processing configuration")]
    pub images: Option<SchemaImageConfig>,

    /// Style processing configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Style processing configuration")]
    pub styles: Option<SchemaStyleConfig>,

    /// Build optimization settings
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Build optimization settings")]
    pub optimization: Option<SchemaOptimizationConfig>,

    /// Experimental features
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Experimental features")]
    pub experimental: Option<SchemaExperimentalConfig>,

    /// Enable persistent caching
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Enable persistent caching")]
    pub persistent_caching: Option<bool>,

    /// Cache handler configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Cache handler configuration")]
    pub cache_handler: Option<String>,
}

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

/// Copy item configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaCopyItem {
    /// Source path to copy from
    #[schemars(description = "Source path to copy from")]
    pub from: String,

    /// Destination path to copy to
    #[schemars(description = "Destination path to copy to")]
    pub to: String,
}

/// Output configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaOutputConfig {
    /// Output directory path
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Output directory path")]
    pub path: Option<String>,

    /// Filename pattern for main files
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Filename pattern for main files")]
    pub filename: Option<String>,

    /// Filename pattern for chunk files
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Filename pattern for chunk files")]
    pub chunk_filename: Option<String>,

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
}

/// Output type
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum SchemaOutputType {
    #[serde(rename = "standalone")]
    Standalone,
    #[serde(rename = "export")]
    Export,
}

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

    /// Modularize imports configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Modularize imports configuration")]
    pub modularize_imports: Option<HashMap<String, SchemaModularizeImportPackageConfig>>,

    /// Whether to concatenate modules when possible to reduce the number of chunks
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "Whether to concatenate modules when possible to reduce the number of chunks. This can improve performance by reducing the number of requests and improving caching."
    )]
    pub concatenate_modules: Option<bool>,
}

/// Module ID generation strategy
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SchemaModuleIds {
    Named,
    Deterministic,
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

/// External dependency configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SchemaExternalConfig {
    /// Simple string external (e.g., \"react\" -> \"React\")
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

/// Module configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SchemaModuleConfig {
    /// Module rules configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Module rules configuration")]
    pub rules: Option<HashMap<String, serde_json::Value>>,

    /// Module conditions configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Module conditions configuration")]
    pub conditions: Option<HashMap<String, SchemaConfigConditionItem>>,
}

/// Configuration condition item
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaConfigConditionItem {
    /// Condition path configuration
    #[schemars(description = "Condition path configuration")]
    pub path: SchemaConfigConditionPath,
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

/// Image configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaImageConfig {
    /// Inline limit for images in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Inline limit for images in bytes")]
    pub inline_limit: Option<u64>,
}

/// Style configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaStyleConfig {
    /// Emotion transform configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Emotion transform configuration")]
    pub emotion: Option<serde_json::Value>,

    /// Styled components configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Styled components configuration")]
    pub styled_components: Option<serde_json::Value>,

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

/// Experimental features configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaExperimentalConfig {
    /// SWC plugins
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "SWC plugins")]
    pub swc_plugins: Option<Vec<serde_json::Value>>,

    /// MDX-RS options
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "MDX-RS options")]
    pub mdx_rs: Option<serde_json::Value>,

    /// Dynamic IO
    #[serde(rename = "dynamicIO", skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Dynamic IO")]
    pub dynamic_io: Option<bool>,

    /// Use cache
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Use cache")]
    pub use_cache: Option<bool>,

    /// Cache handlers
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Cache handlers")]
    pub cache_handlers: Option<HashMap<String, String>>,

    /// ESM externals
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "ESM externals")]
    pub esm_externals: Option<serde_json::Value>,

    /// Partial prerendering
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Partial prerendering")]
    pub ppr: Option<serde_json::Value>,

    /// Taint
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Taint")]
    pub taint: Option<bool>,

    /// React compiler
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "React compiler")]
    pub react_compiler: Option<serde_json::Value>,

    /// View transition
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "View transition")]
    pub view_transition: Option<bool>,

    /// Server actions
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Server actions")]
    pub server_actions: Option<serde_json::Value>,
}

/// Generate JSON Schema for ProjectOptions
pub fn generate_schema() -> serde_json::Value {
    let schema = schema_for!(ProjectOptions);
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
        assert!(schema_str.contains("config"));
        assert!(schema_str.contains("entry"));
        assert!(schema_str.contains("externals"));
        assert!(schema_str.contains("optimization"));
        assert!(schema_str.contains("concatenateModules"));
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
        assert!(externals.contains_key("antd"));

        // Test serialization back to JSON
        let serialized = serde_json::to_string(&externals).unwrap();
        assert!(serialized.contains("bar"));
        assert!(serialized.contains("antd"));
    }

    #[test]
    fn test_deserialize_complete_example() {
        // Test the complete project options configuration
        let json = r#"
        {
          "rootPath": "../../",
          "projectPath": "./",
          "config": {
            "entry": [
              {
                "import": "./index.js"       
              }
            ],
            "output": {
              "path": "./dist",
              "filename": "[name].[contenthash:6].js",
              "chunkFilename": "[name].[contenthash:8].js",
              "clean": true
            },
            "optimization": {
              "moduleIds": "named",
              "minify": false,
              "concatenateModules": true
            },
            "externals": {
              "foo": "bar"
            }
          }
        }
        "#;

        let config: ProjectOptions = serde_json::from_str(json).unwrap();
        assert_eq!(config.root_path, Some("../../".to_string()));
        assert_eq!(config.project_path, Some("./".to_string()));
        assert!(config.config.entry.is_some());
        assert!(config.config.output.is_some());
        assert!(config.config.optimization.is_some());
        assert!(config.config.externals.is_some());

        // Test concatenateModules configuration
        let optimization = config.config.optimization.as_ref().unwrap();
        assert_eq!(optimization.concatenate_modules, Some(true));
    }

    #[test]
    fn test_concatenate_modules_configuration() {
        // Test with concatenateModules: true
        let json_true = r#"
        {
          "config": {
            "optimization": {
              "concatenateModules": true
            }
          }
        }
        "#;
        let config: ProjectOptions = serde_json::from_str(json_true).unwrap();
        let optimization = config.config.optimization.as_ref().unwrap();
        assert_eq!(optimization.concatenate_modules, Some(true));

        // Test with concatenateModules: false
        let json_false = r#"
        {
          "config": {
            "optimization": {
              "concatenateModules": false
            }
          }
        }
        "#;
        let config: ProjectOptions = serde_json::from_str(json_false).unwrap();
        let optimization = config.config.optimization.as_ref().unwrap();
        assert_eq!(optimization.concatenate_modules, Some(false));

        // Test without concatenateModules (should be None)
        let json_none = r#"
        {
          "config": {
            "optimization": {
              "minify": true
            }
          }
        }
        "#;
        let config: ProjectOptions = serde_json::from_str(json_none).unwrap();
        let optimization = config.config.optimization.as_ref().unwrap();
        assert_eq!(optimization.concatenate_modules, None);
    }
}
