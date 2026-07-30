//! Stable, typed contract for `utoo --json`.

use std::collections::BTreeMap;

use schemars::{JsonSchema, schema_for};
use serde::Serialize;
use serde_json::Value;

use crate::error::ErrorKind;

pub const SCHEMA_VERSION: u8 = 1;
pub const CAPTURED_OUTPUT_TAIL_LIMIT: usize = 64 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuccessEnvelope<T> {
    pub schema_version: u8,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subcommand: Option<String>,
    pub ok: bool,
    pub duration_ms: u64,
    pub result: T,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FailureEnvelope {
    pub schema_version: u8,
    #[schemars(required)]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subcommand: Option<String>,
    #[schemars(schema_with = "false_schema")]
    pub ok: bool,
    pub duration_ms: u64,
    pub error: ErrorOutput,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ErrorOutput {
    pub category: ErrorKind,
    #[schemars(length(min = 1))]
    pub code: String,
    pub exit_code: u8,
    pub message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub causes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partial_result: Option<PartialResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<ErrorDetails>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CapturedOutput {
    pub tail: String,
    pub bytes: u64,
    pub truncated: bool,
}

impl CapturedOutput {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let truncated = bytes.len() > CAPTURED_OUTPUT_TAIL_LIMIT;
        let start = bytes.len().saturating_sub(CAPTURED_OUTPUT_TAIL_LIMIT);
        Self {
            tail: String::from_utf8_lossy(&bytes[start..]).into_owned(),
            bytes: bytes.len() as u64,
            truncated,
        }
    }

    pub fn empty() -> Self {
        Self {
            tail: String::new(),
            bytes: 0,
            truncated: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Succeeded,
    Failed,
    FailedToStart,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProcessExecution {
    pub command: String,
    pub cwd: String,
    pub status: ExecutionStatus,
    #[schemars(required)]
    pub exit_code: Option<u32>,
    pub stdout: CapturedOutput,
    pub stderr: CapturedOutput,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleExecution {
    #[schemars(required)]
    pub package: Option<String>,
    #[schemars(required)]
    pub workspace: Option<String>,
    pub event: String,
    pub command: String,
    pub cwd: String,
    pub status: ExecutionStatus,
    #[schemars(required)]
    pub exit_code: Option<u32>,
    pub stdout: CapturedOutput,
    pub stderr: CapturedOutput,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ErrorDetails {
    Lifecycle {
        executions: Vec<LifecycleExecution>,
    },
    Dependency {
        package: RequestedPackage,
        #[serde(rename = "requiredBy")]
        required_by: Vec<RequiredBy>,
    },
    Registry {
        registry: String,
        #[schemars(required)]
        status: Option<u16>,
        #[serde(rename = "durationMs")]
        duration_ms: u64,
    },
    Filesystem {
        path: String,
    },
    Process {
        execution: ProcessExecution,
    },
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RequestedPackage {
    pub name: String,
    pub spec: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RequiredBy {
    #[schemars(required)]
    pub name: Option<String>,
    #[schemars(required)]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum PartialResult {
    Publish(PublishPartialResult),
    Run(RunPartialResult),
    Link(LinkPartialResult),
    Clean(CleanPartialResult),
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PublishPartialResult {
    pub packages: Vec<PublishedPackage>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RunPartialResult {
    pub executions: Vec<LifecycleExecution>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LinkPartialResult {
    pub links: Vec<LinkEntry>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CleanPartialResult {
    pub deleted: Vec<PackageVersion>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct VersionResult {
    pub version: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct HelpResult {
    #[schemars(required)]
    pub target: Option<HelpTarget>,
    pub text: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct HelpTarget {
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subcommand: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DependencyOperation {
    Install,
    Add,
    Remove,
    Update,
    Rebuild,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DependencyScope {
    Local,
    Global,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PackageVersion {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DependencySummary {
    pub added: u64,
    pub removed: u64,
    pub changed: u64,
    pub reused: u64,
    pub downloaded_bytes: u64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct InstallResult {
    pub operation: DependencyOperation,
    pub scope: DependencyScope,
    #[schemars(required)]
    pub workspace: Option<String>,
    pub requested: Vec<String>,
    pub resolved: Vec<PackageVersion>,
    pub summary: DependencySummary,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct UninstallResult {
    pub operation: DependencyOperation,
    pub scope: DependencyScope,
    #[schemars(required)]
    pub workspace: Option<String>,
    pub requested: Vec<String>,
    pub removed: Vec<PackageVersion>,
    pub summary: DependencySummary,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct UpdatedPackage {
    pub name: String,
    #[serde(rename = "fromVersion")]
    pub from_version: String,
    #[serde(rename = "toVersion")]
    pub to_version: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct UpdateResult {
    pub operation: DependencyOperation,
    pub scope: DependencyScope,
    #[schemars(required)]
    pub workspace: Option<String>,
    pub force: bool,
    pub updated: Vec<UpdatedPackage>,
    pub summary: DependencySummary,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RebuildResult {
    pub operation: DependencyOperation,
    pub summary: RebuildSummary,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RebuildSummary {
    pub packages: u64,
    pub scripts: u64,
    pub bins: u64,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CleanResult {
    pub pattern: String,
    pub deleted: Vec<PackageVersion>,
    pub summary: CleanSummary,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CleanSummary {
    pub matched: u64,
    pub deleted: u64,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum DepsResult {
    Dependencies {
        #[serde(rename = "outputPath")]
        output_path: String,
        summary: DependenciesSummary,
    },
    Workspace {
        #[serde(rename = "outputPath")]
        output_path: String,
        summary: WorkspaceSummary,
    },
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DependenciesSummary {
    pub packages: u64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct WorkspaceSummary {
    pub workspaces: u64,
    pub edges: u64,
    pub layers: u64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ListPathNode {
    pub name: String,
    pub version: String,
    pub path: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ListResult {
    pub package: String,
    pub paths: Vec<Vec<ListPathNode>>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SkippedExecution {
    #[schemars(required)]
    pub package: Option<String>,
    #[schemars(required)]
    pub workspace: Option<String>,
    pub cwd: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RunResult {
    pub script: String,
    pub executions: Vec<LifecycleExecution>,
    pub skipped: Vec<SkippedExecution>,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecutableSource {
    Local,
    Cache,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ExecuteResult {
    pub requested: String,
    pub source: ExecutableSource,
    pub executable: String,
    pub execution: ProcessExecution,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ViewResult {
    pub requested: String,
    pub name: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    pub dependencies: BTreeMap<String, String>,
    pub dist_tags: BTreeMap<String, String>,
    pub dist: ViewDist,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ViewDist {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tarball: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shasum: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integrity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unpacked_size: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LinkDirection {
    GlobalToLocal,
    LocalToGlobal,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LinkEntry {
    pub package: String,
    pub source: String,
    pub target: String,
    pub bins: u64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LinkResult {
    pub direction: LinkDirection,
    pub links: Vec<LinkEntry>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PackFile {
    pub path: String,
    pub size: u64,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PackResult {
    pub name: String,
    pub version: String,
    pub filename: String,
    #[schemars(required)]
    pub tarball_path: Option<String>,
    pub dry_run: bool,
    pub files: Vec<PackFile>,
    pub unpacked_size: u64,
    pub packed_size: u64,
    pub integrity: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PublishedPackage {
    pub name: String,
    pub version: String,
    pub registry: String,
    pub tag: String,
    pub access: String,
    pub provenance: bool,
    pub files: Vec<PackFile>,
    pub packed_size: u64,
    pub integrity: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PublishResult {
    pub dry_run: bool,
    pub packages: Vec<PublishedPackage>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PingResult {
    pub registry: String,
    pub latency_ms: u64,
    pub supports_semver: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct WhoamiResult {
    pub username: String,
    pub registry: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogoutResult {
    pub registry: String,
    pub remote_revoked: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ConfigSetResult {
    pub values: BTreeMap<String, Value>,
    pub scope: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ConfigGetResult {
    pub values: BTreeMap<String, Value>,
    pub source: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ConfigListResult {
    pub values: BTreeMap<String, Value>,
    pub scope: String,
    pub path: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct InitResult {
    pub path: String,
    pub name: String,
    pub version: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CompletionsResult {
    pub shell: String,
    pub script: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CustomResult {
    pub name: String,
    pub configured_command: String,
    pub execution: ProcessExecution,
}

fn false_schema(_: &mut schemars::r#gen::SchemaGenerator) -> schemars::schema::Schema {
    let mut schema = schemars::schema::SchemaObject::default();
    schema.instance_type = Some(schemars::schema::InstanceType::Boolean.into());
    schema.const_value = Some(Value::Bool(false));
    schema.into()
}

struct SchemaVersionOne;

impl JsonSchema for SchemaVersionOne {
    fn schema_name() -> String {
        "SchemaVersionOne".to_string()
    }

    fn json_schema(_: &mut schemars::r#gen::SchemaGenerator) -> schemars::schema::Schema {
        let mut schema = schemars::schema::SchemaObject::default();
        schema.instance_type = Some(schemars::schema::InstanceType::Integer.into());
        schema.const_value = Some(Value::from(SCHEMA_VERSION));
        schema.into()
    }
}

struct SuccessMarker;

impl JsonSchema for SuccessMarker {
    fn schema_name() -> String {
        "SuccessMarker".to_string()
    }

    fn json_schema(_: &mut schemars::r#gen::SchemaGenerator) -> schemars::schema::Schema {
        let mut schema = schemars::schema::SchemaObject::default();
        schema.instance_type = Some(schemars::schema::InstanceType::Boolean.into());
        schema.const_value = Some(Value::Bool(true));
        schema.into()
    }
}

macro_rules! success_schema {
    ($schema:ident, $command_type:ident, $command:literal, $result:ty) => {
        #[derive(JsonSchema)]
        #[allow(dead_code)]
        enum $command_type {
            #[serde(rename = $command)]
            Value,
        }

        #[derive(JsonSchema)]
        #[serde(rename_all = "camelCase")]
        #[allow(dead_code)]
        struct $schema {
            schema_version: SchemaVersionOne,
            command: $command_type,
            ok: SuccessMarker,
            duration_ms: u64,
            result: $result,
        }
    };
}

success_schema!(
    VersionSuccessSchema,
    VersionCommand,
    "version",
    VersionResult
);
success_schema!(HelpSuccessSchema, HelpCommand, "help", HelpResult);
success_schema!(
    InstallSuccessSchema,
    InstallCommand,
    "install",
    InstallResult
);
success_schema!(
    UninstallSuccessSchema,
    UninstallCommand,
    "uninstall",
    UninstallResult
);
success_schema!(UpdateSuccessSchema, UpdateCommand, "update", UpdateResult);
success_schema!(
    RebuildSuccessSchema,
    RebuildCommand,
    "rebuild",
    RebuildResult
);
success_schema!(CleanSuccessSchema, CleanCommand, "clean", CleanResult);
success_schema!(DepsSuccessSchema, DepsCommand, "deps", DepsResult);
success_schema!(ListSuccessSchema, ListCommand, "list", ListResult);
success_schema!(RunSuccessSchema, RunCommand, "run", RunResult);
success_schema!(
    ExecuteSuccessSchema,
    ExecuteCommand,
    "execute",
    ExecuteResult
);
success_schema!(ViewSuccessSchema, ViewCommand, "view", ViewResult);
success_schema!(LinkSuccessSchema, LinkCommand, "link", LinkResult);
success_schema!(PackSuccessSchema, PackCommand, "pack", PackResult);
success_schema!(
    PublishSuccessSchema,
    PublishCommand,
    "publish",
    PublishResult
);
success_schema!(PingSuccessSchema, PingCommand, "ping", PingResult);
success_schema!(WhoamiSuccessSchema, WhoamiCommand, "whoami", WhoamiResult);
success_schema!(LogoutSuccessSchema, LogoutCommand, "logout", LogoutResult);
success_schema!(InitSuccessSchema, InitCommand, "init", InitResult);
success_schema!(
    CompletionsSuccessSchema,
    CompletionsCommand,
    "completions",
    CompletionsResult
);
success_schema!(CustomSuccessSchema, CustomCommand, "custom", CustomResult);

#[derive(JsonSchema)]
#[allow(dead_code)]
enum ConfigCommand {
    #[serde(rename = "config")]
    Value,
}

macro_rules! config_success_schema {
    ($schema:ident, $subcommand_type:ident, $subcommand:literal, $result:ty) => {
        #[derive(JsonSchema)]
        #[allow(dead_code)]
        enum $subcommand_type {
            #[serde(rename = $subcommand)]
            Value,
        }

        #[derive(JsonSchema)]
        #[serde(rename_all = "camelCase")]
        #[allow(dead_code)]
        struct $schema {
            schema_version: SchemaVersionOne,
            command: ConfigCommand,
            subcommand: $subcommand_type,
            ok: SuccessMarker,
            duration_ms: u64,
            result: $result,
        }
    };
}

config_success_schema!(
    ConfigSetSuccessSchema,
    ConfigSetSubcommand,
    "set",
    ConfigSetResult
);
config_success_schema!(
    ConfigGetSuccessSchema,
    ConfigGetSubcommand,
    "get",
    ConfigGetResult
);
config_success_schema!(
    ConfigListSuccessSchema,
    ConfigListSubcommand,
    "list",
    ConfigListResult
);

#[derive(JsonSchema)]
#[serde(untagged)]
#[allow(dead_code)]
enum CliOutputSchema {
    Version(VersionSuccessSchema),
    Help(HelpSuccessSchema),
    Install(InstallSuccessSchema),
    Uninstall(UninstallSuccessSchema),
    Update(UpdateSuccessSchema),
    Rebuild(RebuildSuccessSchema),
    Clean(CleanSuccessSchema),
    Deps(DepsSuccessSchema),
    List(ListSuccessSchema),
    Run(RunSuccessSchema),
    Execute(ExecuteSuccessSchema),
    View(ViewSuccessSchema),
    Link(LinkSuccessSchema),
    Pack(PackSuccessSchema),
    Publish(PublishSuccessSchema),
    Ping(PingSuccessSchema),
    Whoami(WhoamiSuccessSchema),
    Logout(LogoutSuccessSchema),
    ConfigSet(ConfigSetSuccessSchema),
    ConfigGet(ConfigGetSuccessSchema),
    ConfigList(ConfigListSuccessSchema),
    Init(InitSuccessSchema),
    Completions(CompletionsSuccessSchema),
    Custom(CustomSuccessSchema),
    Failure(FailureEnvelope),
}

#[allow(dead_code)]
pub fn generate_schema_string() -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&schema_for!(CliOutputSchema)).map(|schema| schema + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_output_keeps_the_final_64_kib() {
        let mut bytes = b"BEGIN".to_vec();
        bytes.resize(CAPTURED_OUTPUT_TAIL_LIMIT + 8, b'x');
        bytes.extend_from_slice(b"END");

        let output = CapturedOutput::from_bytes(&bytes);

        assert_eq!(output.bytes, bytes.len() as u64);
        assert!(output.truncated);
        assert!(!output.tail.contains("BEGIN"));
        assert!(output.tail.ends_with("END"));
    }

    #[test]
    fn cli_output_schema() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("schema/cli-output-v1.schema.json");
        let generated = generate_schema_string().unwrap();
        if std::env::var_os("UPDATE").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, generated).unwrap();
            return;
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!(
                "failed to read {} ({error}); run UPDATE=1 cargo test -p utoo-pm cli_output_schema",
                path.display()
            )
        });
        let committed = committed.replace("\r\n", "\n");
        assert_eq!(
            committed, generated,
            "CLI JSON Schema changed; run UPDATE=1 cargo test -p utoo-pm cli_output_schema"
        );
    }
}
