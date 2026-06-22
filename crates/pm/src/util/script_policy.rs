//! Install-time script execution policy (RFC #3145).
//!
//! utoo's historical control was binary: run every dependency lifecycle script
//! or skip them all (`--ignore-scripts`). This module adds the middle layer —
//! an auditable allow/deny policy that lets a project run `esbuild` and `sharp`
//! while denying everything else, with npm-compatible naming (`allowScripts` /
//! `allow-scripts`).
//!
//! Resolution funnels into [`InstallScriptMode`], which the package service
//! consults at queue-construction time. Until a project configures any policy,
//! the resolved mode is [`InstallScriptMode::AllowAllDangerously`] — i.e. the
//! pre-RFC behavior — so existing installs keep working. Configuring a single
//! `allowScripts` entry (or `strict-allow-scripts`) opts the project into
//! enforcement.
//!
//! Source-dependency controls (`allow-git` / `allow-remote` / …) from the RFC
//! are intentionally out of scope here; they gate at the resolver and land in a
//! follow-up.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::util::config_file::Config;

/// A package matcher used in `allowScripts` entries.
///
/// `sharp` matches every version of `sharp`; `esbuild@0.25.5` matches only that
/// resolved version. Scoped names keep their leading `@`, so `@scope/x` is
/// name-only while `@scope/x@1.2.3` is version-pinned.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PackageSelector {
    Name(String),
    NameVersion(String, String),
}

impl PackageSelector {
    /// Parse a selector string. A trailing `@<version>` (not the scope `@`)
    /// pins the version; otherwise the whole string is a bare name.
    pub fn parse(raw: &str) -> Self {
        let raw = raw.trim();
        match raw.rfind('@') {
            // `pos > 0` skips the scope marker in `@scope/x`. Require a
            // non-empty version after the `@` so a trailing `@` (e.g. `pkg@`)
            // stays a bare name instead of becoming `NameVersion(_, "")`, which
            // would otherwise only match a package whose resolved version is
            // empty (see `decide`'s version note).
            Some(pos) if pos > 0 && pos + 1 < raw.len() => {
                Self::NameVersion(raw[..pos].to_string(), raw[pos + 1..].to_string())
            }
            _ => Self::Name(raw.to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllowScriptDecision {
    Allow,
    Deny,
}

/// Why a package's install action was not run, for the skip/fail summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// No matching `allowScripts` entry.
    Unreviewed,
    /// Explicitly denied.
    Denied,
}

impl SkipReason {
    fn label(self) -> &'static str {
        match self {
            Self::Unreviewed => "unreviewed",
            Self::Denied => "denied",
        }
    }
}

/// One package whose install action was gated off, recorded for the summary.
#[derive(Debug, Clone)]
pub struct SkippedScript {
    pub name: String,
    pub version: String,
    pub reason: SkipReason,
    /// The gated action was an implicit `node-gyp rebuild` (binding.gyp present,
    /// no explicit `install` script).
    pub node_gyp: bool,
}

impl SkippedScript {
    fn id(&self) -> String {
        if self.version.is_empty() {
            self.name.clone()
        } else {
            format!("{}@{}", self.name, self.version)
        }
    }
}

/// Outcome of gating one package's install action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptGateDecision {
    /// Allowed — run the install action.
    Run,
    /// Skipped — record it and continue (non-strict).
    Skip(SkipReason),
    /// Unreviewed under strict mode — must fail the install.
    Error,
}

/// A resolved allow/deny policy. Deny wins over Allow at the same selector (and
/// [`AllowScriptsPolicy::decide`] checks deny at both name and `name@version`
/// granularity first).
#[derive(Debug, Clone, Default)]
pub struct AllowScriptsPolicy {
    entries: HashMap<PackageSelector, AllowScriptDecision>,
    pub strict: bool,
}

impl AllowScriptsPolicy {
    /// True when nothing is configured — neither entries nor strict. Such a
    /// policy carries no signal, so the resolver collapses it to
    /// [`InstallScriptMode::AllowAllDangerously`] (pre-RFC behavior).
    fn is_inert(&self) -> bool {
        self.entries.is_empty() && !self.strict
    }

    /// Merge one entry. Deny is sticky: an existing Deny is never downgraded to
    /// Allow, and an incoming Deny overrides an existing Allow — regardless of
    /// source order — so a team-level deny survives a project-level allow.
    fn merge_entry(&mut self, selector: PackageSelector, decision: AllowScriptDecision) {
        self.entries
            .entry(selector)
            .and_modify(|cur| {
                if decision == AllowScriptDecision::Deny {
                    *cur = AllowScriptDecision::Deny;
                }
            })
            .or_insert(decision);
    }

    /// Decide whether `name@version`'s install action may run, following the RFC
    /// precedence: deny > exact `name@version` allow > bare-name allow >
    /// unreviewed (skip, or fail under strict).
    pub fn decide(&self, name: &str, version: &str) -> ScriptGateDecision {
        let name_key = PackageSelector::Name(name.to_string());
        let pinned_key = PackageSelector::NameVersion(name.to_string(), version.to_string());

        let is =
            |key: &PackageSelector, want: AllowScriptDecision| self.entries.get(key) == Some(&want);

        if is(&name_key, AllowScriptDecision::Deny) || is(&pinned_key, AllowScriptDecision::Deny) {
            return ScriptGateDecision::Skip(SkipReason::Denied);
        }
        if is(&pinned_key, AllowScriptDecision::Allow) || is(&name_key, AllowScriptDecision::Allow)
        {
            return ScriptGateDecision::Run;
        }
        if self.strict {
            ScriptGateDecision::Error
        } else {
            ScriptGateDecision::Skip(SkipReason::Unreviewed)
        }
    }

    #[cfg(test)]
    pub fn from_entries(entries: &[(&str, bool)], strict: bool) -> Self {
        let mut policy = Self {
            entries: HashMap::new(),
            strict,
        };
        for (raw, allow) in entries {
            policy.merge_entry(
                PackageSelector::parse(raw),
                if *allow {
                    AllowScriptDecision::Allow
                } else {
                    AllowScriptDecision::Deny
                },
            );
        }
        policy
    }
}

/// How install-time dependency scripts are handled for one install run.
///
/// Built once per run by [`InstallScriptMode::resolve`] and consulted by the
/// package service. The [`Self::IgnoreAll`] / [`Self::AllowAllDangerously`]
/// variants preserve the two pre-RFC `ScriptPolicy` behaviors verbatim.
#[derive(Debug, Clone)]
pub enum InstallScriptMode {
    /// `--ignore-scripts`: skip every install-time script (bin linking only).
    IgnoreAll,
    /// Run every dependency install script unconditionally. Both the migration
    /// escape hatch (`--dangerously-allow-all-scripts`) and the no-policy
    /// default, so projects that never configure a policy are unaffected.
    AllowAllDangerously,
    /// Gate each dependency against an [`AllowScriptsPolicy`].
    Policy(AllowScriptsPolicy),
}

impl InstallScriptMode {
    /// Whether scripts may run at all (everything but [`Self::IgnoreAll`]).
    pub fn collects_scripts(&self) -> bool {
        !matches!(self, Self::IgnoreAll)
    }

    pub fn is_ignore_all(&self) -> bool {
        matches!(self, Self::IgnoreAll)
    }

    /// True only under a strict policy, where unreviewed scripts fail the install.
    pub fn is_strict(&self) -> bool {
        matches!(self, Self::Policy(p) if p.strict)
    }

    /// Resolve the effective mode from CLI args, utoo config, and (for project
    /// installs) the root `package.json` policy.
    ///
    /// Sources are layered in precedence order — global config → project config
    /// → root `package.json` → CLI `--allow-scripts` — and [`merge_entry`] keeps
    /// deny sticky ACROSS them, so a team-level (global) deny survives a
    /// project-level allow. Global installs pass `root_path = None`: they have no
    /// project context, so the CWD's `.utoo.toml` and `package.json` are ignored
    /// and only the global config + CLI apply.
    ///
    /// [`merge_entry`]: AllowScriptsPolicy::merge_entry
    pub async fn resolve(args: &ScriptPolicyArgs, root_path: Option<&Path>) -> Result<Self> {
        // 1. `--ignore-scripts` has the highest precedence.
        if args.ignore_scripts {
            return Ok(Self::IgnoreAll);
        }

        // Load global + project-local config SEPARATELY (not the merged view) so
        // deny precedence can be applied across sources. A genuine parse error is
        // surfaced rather than swallowed — a broken policy file must not silently
        // fail open and run every install script.
        let (global, local) = Config::load_layers()
            .await
            .context("failed to load utoo config for the install-script policy")?;
        // Global installs (root_path = None) ignore the CWD project config.
        let local = root_path.and(local.as_ref());

        // For scalar flags, a local value overrides global; absence is false.
        // Trim so a stray space (e.g. `ut config set strict-allow-scripts " true"`)
        // does not silently disable the flag.
        let config_flag = |key: &str| -> bool {
            local
                .and_then(|c| c.get(key).ok().flatten())
                .or_else(|| global.get(key).ok().flatten())
                .is_some_and(|v| v.trim().eq_ignore_ascii_case("true"))
        };

        // 2. Precedence: an explicit CLI `--dangerously-allow-all-scripts`
        // bypasses the policy. A *config* dangerously flag yields only when the
        // user did NOT explicitly pass `--strict-allow-scripts`, so a stale
        // config escape hatch can't silently override an explicit CLI strict.
        let dangerously = args.dangerously_allow_all
            || (config_flag("dangerously-allow-all-scripts") && !args.strict);
        if dangerously {
            eprintln!(
                "warning: dangerously-allow-all-scripts is set — running ALL dependency install \
                 scripts without review"
            );
            tracing::warn!("dangerously-allow-all-scripts: bypassing allowScripts policy");
            return Ok(Self::AllowAllDangerously);
        }

        let strict = args.strict || config_flag("strict-allow-scripts");
        let mut policy = AllowScriptsPolicy {
            entries: HashMap::new(),
            strict,
        };

        // Layer config sources in order; deny stays sticky across them.
        feed_config(&mut policy, &global);
        if let Some(local) = local {
            feed_config(&mut policy, local);
        }

        // Root `package.json` `allowScripts` (project installs only).
        if let Some(root) = root_path {
            for (name, allow) in read_package_json_allow_scripts(root).await {
                policy.merge_entry(PackageSelector::parse(&name), decision(allow));
            }
        }

        // CLI `--allow-scripts` one-shot allows (never persisted). These cannot
        // override an explicit deny — `merge_entry` keeps deny sticky.
        for raw in &args.allow {
            for sel in split_selectors(raw) {
                policy.merge_entry(sel, AllowScriptDecision::Allow);
            }
        }

        // No policy configured anywhere → preserve pre-RFC behavior (run all).
        if policy.is_inert() {
            return Ok(Self::AllowAllDangerously);
        }
        Ok(Self::Policy(policy))
    }
}

/// Merge one config layer's `allowScripts` table + `allow-scripts`/`deny-scripts`
/// list and comma-string forms into `policy`. Called per source (global, then
/// local) so [`AllowScriptsPolicy::merge_entry`]'s deny-stickiness applies across
/// sources.
fn feed_config(policy: &mut AllowScriptsPolicy, config: &Config) {
    // `[allowScripts]` table form: name -> bool.
    for (name, allow) in config.allow_scripts() {
        policy.merge_entry(PackageSelector::parse(name), decision(*allow));
    }
    // `[arrays]` list form, plus the comma-separated string form written by
    // `ut config set allow-scripts "a,b"`.
    merge_list(policy, config.get_array("allow-scripts"), true);
    merge_list(policy, config.get_array("deny-scripts"), false);
    merge_csv(policy, config.get("allow-scripts").ok().flatten(), true);
    merge_csv(policy, config.get("deny-scripts").ok().flatten(), false);
}

fn decision(allow: bool) -> AllowScriptDecision {
    if allow {
        AllowScriptDecision::Allow
    } else {
        AllowScriptDecision::Deny
    }
}

/// Split a comma-separated selector list (`"esbuild@0.25.5,sharp"`).
fn split_selectors(raw: &str) -> impl Iterator<Item = PackageSelector> + '_ {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PackageSelector::parse)
}

fn merge_list(policy: &mut AllowScriptsPolicy, list: Option<&[String]>, allow: bool) {
    for entry in list.unwrap_or_default() {
        for sel in split_selectors(entry) {
            policy.merge_entry(sel, decision(allow));
        }
    }
}

fn merge_csv(policy: &mut AllowScriptsPolicy, value: Option<String>, allow: bool) {
    if let Some(value) = value {
        for sel in split_selectors(&value) {
            policy.merge_entry(sel, decision(allow));
        }
    }
}

/// Read the `allowScripts` object from a project's root `package.json`.
///
/// Returns `(name, allow)` pairs. A missing field, unreadable file, or wrong
/// shape yields no entries — policy never fails the install on a malformed
/// `allowScripts` (the rest of the manifest is validated elsewhere).
async fn read_package_json_allow_scripts(root_path: &Path) -> Vec<(String, bool)> {
    let path = root_path.join("package.json");
    let Ok(content) = crate::fs::read_to_string(&path).await else {
        return Vec::new();
    };
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&content) else {
        return Vec::new();
    };
    match map.get("allowScripts") {
        Some(Value::Object(entries)) => entries
            .iter()
            .filter_map(|(name, value)| Some((name.clone(), value.as_bool()?)))
            .collect(),
        _ => Vec::new(),
    }
}

/// CLI-level inputs for the install-time script policy, captured at the CLI
/// boundary and resolved into an [`InstallScriptMode`] once the project context
/// (root `package.json` + config) is known.
#[derive(Debug, Clone, Default)]
pub struct ScriptPolicyArgs {
    pub ignore_scripts: bool,
    pub dangerously_allow_all: bool,
    pub strict: bool,
    /// `--allow-scripts pkg[,pkg...]`, possibly repeated.
    pub allow: Vec<String>,
}

impl ScriptPolicyArgs {
    /// `--ignore-scripts` only — the form used by paths that expose no other
    /// policy flags (uninstall, bare `utoo`). Other call sites build the struct
    /// with named fields directly (the two `bool`s are easy to transpose
    /// positionally, so there is no positional constructor).
    pub fn ignore_only(ignore_scripts: bool) -> Self {
        Self {
            ignore_scripts,
            ..Self::default()
        }
    }
}

/// Print the compact "install scripts skipped" summary to stderr.
///
/// ```text
/// install scripts skipped:
///   esbuild@0.25.5     unreviewed
///   telemetry-pkg@1.0  denied
///   native-addon@2.1   unreviewed node-gyp
/// ```
pub fn report_skipped_scripts(skipped: &[SkippedScript]) {
    if skipped.is_empty() {
        return;
    }
    let width = skipped.iter().map(|s| s.id().len()).max().unwrap_or(0);
    let mut out = String::from("install scripts skipped:\n");
    for s in skipped {
        let suffix = if s.node_gyp { " node-gyp" } else { "" };
        out.push_str(&format!(
            "  {id:width$}  {reason}{suffix}\n",
            id = s.id(),
            reason = s.reason.label(),
        ));
    }
    eprint!("{out}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_pinned_and_scoped_selectors() {
        assert_eq!(
            PackageSelector::parse("sharp"),
            PackageSelector::Name("sharp".into())
        );
        assert_eq!(
            PackageSelector::parse("esbuild@0.25.5"),
            PackageSelector::NameVersion("esbuild".into(), "0.25.5".into())
        );
        assert_eq!(
            PackageSelector::parse("@scope/native-addon"),
            PackageSelector::Name("@scope/native-addon".into())
        );
        assert_eq!(
            PackageSelector::parse("@scope/native-addon@1.2.3"),
            PackageSelector::NameVersion("@scope/native-addon".into(), "1.2.3".into())
        );
    }

    #[test]
    fn deny_wins_over_allow_at_same_name() {
        // Allow then deny, and deny then allow, both end up denied.
        let p = AllowScriptsPolicy::from_entries(&[("sharp", true), ("sharp", false)], false);
        assert_eq!(
            p.decide("sharp", "1.0.0"),
            ScriptGateDecision::Skip(SkipReason::Denied)
        );
        let p = AllowScriptsPolicy::from_entries(&[("sharp", false), ("sharp", true)], false);
        assert_eq!(
            p.decide("sharp", "1.0.0"),
            ScriptGateDecision::Skip(SkipReason::Denied)
        );
    }

    #[test]
    fn pinned_allow_only_matches_that_version() {
        let p = AllowScriptsPolicy::from_entries(&[("esbuild@0.25.5", true)], false);
        assert_eq!(p.decide("esbuild", "0.25.5"), ScriptGateDecision::Run);
        assert_eq!(
            p.decide("esbuild", "0.26.0"),
            ScriptGateDecision::Skip(SkipReason::Unreviewed)
        );
    }

    #[test]
    fn bare_name_allow_matches_any_version() {
        let p = AllowScriptsPolicy::from_entries(&[("sharp", true)], false);
        assert_eq!(p.decide("sharp", "1.0.0"), ScriptGateDecision::Run);
        assert_eq!(p.decide("sharp", "2.0.0"), ScriptGateDecision::Run);
    }

    #[test]
    fn deny_beats_pinned_allow() {
        // Bare deny + a pinned allow for the same package: deny still wins.
        let p = AllowScriptsPolicy::from_entries(&[("sharp", false), ("sharp@1.0.0", true)], false);
        assert_eq!(
            p.decide("sharp", "1.0.0"),
            ScriptGateDecision::Skip(SkipReason::Denied)
        );
    }

    #[test]
    fn unreviewed_skips_when_lax_errors_when_strict() {
        let lax = AllowScriptsPolicy::from_entries(&[("sharp", true)], false);
        assert_eq!(
            lax.decide("telemetry", "1.0.0"),
            ScriptGateDecision::Skip(SkipReason::Unreviewed)
        );
        let strict = AllowScriptsPolicy::from_entries(&[("sharp", true)], true);
        assert_eq!(
            strict.decide("telemetry", "1.0.0"),
            ScriptGateDecision::Error
        );
    }

    #[tokio::test]
    async fn ignore_scripts_takes_precedence() {
        let mode = InstallScriptMode::resolve(&ScriptPolicyArgs::ignore_only(true), None)
            .await
            .unwrap();
        assert!(mode.is_ignore_all());
    }

    #[tokio::test]
    async fn cli_allow_opts_into_policy_mode() {
        let args = ScriptPolicyArgs {
            allow: vec!["sharp,esbuild".into()],
            ..Default::default()
        };
        let mode = InstallScriptMode::resolve(&args, None).await.unwrap();
        match mode {
            InstallScriptMode::Policy(p) => {
                assert_eq!(p.decide("sharp", "1.0.0"), ScriptGateDecision::Run);
                assert_eq!(p.decide("esbuild", "9.9.9"), ScriptGateDecision::Run);
                assert_eq!(
                    p.decide("other", "1.0.0"),
                    ScriptGateDecision::Skip(SkipReason::Unreviewed)
                );
            }
            other => panic!("expected Policy mode, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dangerously_beats_allow_entries() {
        let args = ScriptPolicyArgs {
            dangerously_allow_all: true,
            allow: vec!["sharp".into()],
            ..Default::default()
        };
        let mode = InstallScriptMode::resolve(&args, None).await.unwrap();
        assert!(matches!(mode, InstallScriptMode::AllowAllDangerously));
    }
}
