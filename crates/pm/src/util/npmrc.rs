//! Minimal npm `.npmrc` reader (compat layer).
//!
//! utoo's native config lives in `.utoo.toml` / `~/.utoo/config.toml`, but the
//! npm ecosystem configures clients through `.npmrc` files (registry bridges,
//! private registries, corporate proxies all drop one). Reading them lets `ut`
//! work in such projects without utoo-specific setup.
//!
//! Supported subset:
//! - `registry=<url>`
//! - `//host[/path]/:_authToken=<token>` credentials, matched with npm's
//!   walk-up ("nerf dart") rules
//! - `${VAR}` environment interpolation in values
//!
//! Not supported (yet): `@scope:registry=` routing (resolution currently uses
//! a single registry per install), `_auth`/`username`/`_password` credentials,
//! and npm's global/builtin config files. Where these interact with utoo's own
//! config, `.npmrc` sits below the same-level `.utoo.toml` (see
//! `user_config::init_registry`).

use std::collections::HashMap;
use std::path::Path;

use crate::service::auth::parse_registry;

/// Parsed `.npmrc` values, kept per level so callers can interleave them with
/// utoo's own project/user config in precedence order.
#[derive(Debug, Default)]
pub struct Npmrc {
    /// `<cwd>/.npmrc` (project level).
    project: HashMap<String, String>,
    /// `~/.npmrc` (user level).
    user: HashMap<String, String>,
}

static NPMRC: tokio::sync::OnceCell<Npmrc> = tokio::sync::OnceCell::const_new();

/// The process-wide `.npmrc` view (project + user), loaded once. Missing or
/// unreadable files simply contribute no values.
pub async fn load() -> &'static Npmrc {
    NPMRC
        .get_or_init(|| async { Npmrc::from_disk().await })
        .await
}

impl Npmrc {
    async fn from_disk() -> Self {
        let project = async {
            match std::env::current_dir() {
                Ok(dir) => read_values(&dir.join(".npmrc")).await,
                Err(_) => HashMap::new(),
            }
        };
        let user = async {
            match dirs::home_dir() {
                Some(dir) => read_values(&dir.join(".npmrc")).await,
                None => HashMap::new(),
            }
        };
        let (project, user) = tokio::join!(project, user);
        Self { project, user }
    }

    /// `registry=` from the project `.npmrc`, if any.
    pub fn project_registry(&self) -> Option<String> {
        registry_value(&self.project)
    }

    /// `registry=` from the user `~/.npmrc`, if any.
    pub fn user_registry(&self) -> Option<String> {
        registry_value(&self.user)
    }

    /// The `//host[/path]/:_authToken` credential for `registry`, matched with
    /// npm's rules: config sources are merged (project overrides user for the
    /// same key), then the most specific nerf-dart path wins. So a more
    /// specific user-level token beats a less specific project-level one, while
    /// project wins on an exact key tie.
    pub fn auth_token_for(&self, registry: &str) -> Option<String> {
        auth_token_from(&[&self.project, &self.user], registry).filter(|token| !token.is_empty())
    }
}

fn registry_value(values: &HashMap<String, String>) -> Option<String> {
    values.get("registry").filter(|v| !v.is_empty()).cloned()
}

async fn read_values(path: &Path) -> HashMap<String, String> {
    match crate::fs::read_to_string(path).await {
        Ok(content) => parse(&content),
        // A missing file is the norm (not every project/home has an .npmrc). A
        // present-but-unreadable one (permissions, etc.) is worth surfacing, but
        // not fatal: `.npmrc` is an optional compat layer, so a bad file must
        // not break commands that don't depend on it. Warn and contribute none.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
        Err(e) => {
            tracing::warn!("ignoring unreadable npmrc at {}: {e}", path.display());
            HashMap::new()
        }
    }
}

/// Parse ini-style `.npmrc` content into a key → value map.
///
/// Follows npm's config format: `#`/`;` comment lines, `key=value` pairs with
/// surrounding whitespace trimmed, optional quotes around the value, and
/// `${VAR}` environment interpolation. Bare keys (ini shorthand for `true`)
/// carry nothing we consume and are skipped, as is any entry referencing an
/// unset environment variable (npm errors there; we log and treat it as
/// absent so an optional token line can't break unrelated commands).
fn parse(content: &str) -> HashMap<String, String> {
    let mut values = HashMap::new();
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = strip_quotes(value.trim());
        match interpolate(value, |var| std::env::var(var).ok()) {
            Some(value) => {
                values.insert(key.to_string(), value);
            }
            None => {
                tracing::warn!("ignoring npmrc key {key:?}: value references an unset env var");
            }
        }
    }
    values
}

/// Strip one pair of matching surrounding quotes, npm-ini style.
fn strip_quotes(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2
        && (bytes[0] == b'"' || bytes[0] == b'\'')
        && bytes[bytes.len() - 1] == bytes[0]
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

/// Expand `${VAR}` references via `lookup`. Returns `None` when a referenced
/// variable is unset (the caller drops the entry). An unterminated `${` is
/// kept literally, matching npm's leniency for non-reference `$` uses.
fn interpolate(value: &str, lookup: impl Fn(&str) -> Option<String>) -> Option<String> {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[start..]);
            return Some(out);
        };
        out.push_str(&lookup(&after[..end])?);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// Look up `<nerf-dart>:_authToken` for a registry URL, walking the path up
/// exactly like npm's `regKeyFromURI`: starting from `//host/path/`, strip one
/// trailing slash or path segment per step until only `//host` remains, and
/// return the first match. This makes `//npm.corp/api/:_authToken` cover a
/// registry at `https://npm.corp/api/npm/repo`, while a more specific
/// `//npm.corp/api/npm/repo/:_authToken` wins over it.
///
/// `maps` are consulted in precedence order (project before user) at each path
/// level, so a longer path always wins over a shorter one regardless of source,
/// and an exact-key tie goes to the earlier (higher-precedence) map — matching
/// npm's merge-then-most-specific-path semantics without allocating a merged map.
fn auth_token_from(maps: &[&HashMap<String, String>], registry: &str) -> Option<String> {
    // parse_registry (not raw Url::parse) so scheme-less registry values like
    // `localhost:4873` match, consistent with the rest of the auth stack.
    let url = parse_registry(registry)?;
    let host = url.host_str()?;
    let mut reg_key = match url.port() {
        Some(port) => format!("//{host}:{port}{}", url.path()),
        None => format!("//{host}{}", url.path()),
    };
    if !reg_key.ends_with('/') {
        reg_key.push('/');
    }
    while reg_key.len() > 2 {
        let key = format!("{reg_key}:_authToken");
        if let Some(token) = maps.iter().find_map(|values| values.get(&key)) {
            return Some(token.clone());
        }
        if reg_key.ends_with('/') {
            reg_key.pop();
        } else {
            let last_slash = reg_key.rfind('/').unwrap_or(1);
            reg_key.truncate(last_slash + 1);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn npmrc(project: &str, user: &str) -> Npmrc {
        Npmrc {
            project: parse(project),
            user: parse(user),
        }
    }

    #[test]
    fn parse_reads_key_values_and_skips_comments() {
        let values = parse(
            "# a comment\n\
             ; another comment\n\
             \n\
             registry=https://registry.npmmirror.com\n\
             legacy-peer-deps\n\
             always-auth = true \n",
        );
        assert_eq!(
            values.get("registry").map(String::as_str),
            Some("https://registry.npmmirror.com")
        );
        // Bare key (ini `true` shorthand) is skipped, not misparsed.
        assert!(!values.contains_key("legacy-peer-deps"));
        assert_eq!(values.get("always-auth").map(String::as_str), Some("true"));
    }

    #[test]
    fn parse_strips_quotes() {
        let values = parse("registry=\"https://npm.corp/repo\"\ntoken='abc'\n");
        assert_eq!(
            values.get("registry").map(String::as_str),
            Some("https://npm.corp/repo")
        );
        assert_eq!(values.get("token").map(String::as_str), Some("abc"));
    }

    #[test]
    fn interpolate_expands_env_references() {
        let lookup = |var: &str| match var {
            "TOKEN" => Some("secret".to_string()),
            "HOST" => Some("npm.corp".to_string()),
            _ => None,
        };
        assert_eq!(interpolate("${TOKEN}", lookup).as_deref(), Some("secret"));
        assert_eq!(
            interpolate("https://${HOST}/x-${TOKEN}", lookup).as_deref(),
            Some("https://npm.corp/x-secret")
        );
        // Unset variable → entry dropped.
        assert_eq!(interpolate("${MISSING}", lookup), None);
        // Unterminated ${ is kept literally.
        assert_eq!(interpolate("a${b", lookup).as_deref(), Some("a${b"));
        // No references → unchanged.
        assert_eq!(interpolate("plain", lookup).as_deref(), Some("plain"));
    }

    #[test]
    fn registry_project_wins_over_user() {
        let rc = npmrc(
            "registry=https://project.example.com\n",
            "registry=https://user.example.com\n",
        );
        assert_eq!(
            rc.project_registry().as_deref(),
            Some("https://project.example.com")
        );
        assert_eq!(
            rc.user_registry().as_deref(),
            Some("https://user.example.com")
        );

        let rc = npmrc("", "registry=https://user.example.com\n");
        assert_eq!(rc.project_registry(), None);
        assert_eq!(
            rc.user_registry().as_deref(),
            Some("https://user.example.com")
        );
    }

    #[test]
    fn empty_registry_value_is_ignored() {
        let rc = npmrc("registry=\n", "");
        assert_eq!(rc.project_registry(), None);
    }

    #[test]
    fn auth_token_exact_host_match() {
        let rc = npmrc("//registry.npmjs.org/:_authToken=npm-token\n", "");
        assert_eq!(
            rc.auth_token_for("https://registry.npmjs.org").as_deref(),
            Some("npm-token")
        );
        // Different host → no token.
        assert_eq!(rc.auth_token_for("https://registry.npmmirror.com"), None);
    }

    #[test]
    fn auth_token_matches_host_with_port() {
        let rc = npmrc("//127.0.0.1:8080/:_authToken=local-token\n", "");
        assert_eq!(
            rc.auth_token_for("http://127.0.0.1:8080").as_deref(),
            Some("local-token")
        );
        assert_eq!(rc.auth_token_for("http://127.0.0.1:9090"), None);
    }

    #[test]
    fn auth_token_matches_scheme_less_registry() {
        // A registry configured without a scheme (e.g. `localhost:4873`) still
        // resolves its credential, like the rest of the auth stack.
        let rc = npmrc("//localhost:4873/:_authToken=bare-token\n", "");
        assert_eq!(
            rc.auth_token_for("localhost:4873").as_deref(),
            Some("bare-token")
        );
    }

    #[test]
    fn auth_token_walks_up_sub_paths() {
        // A host-level token covers a registry served under a sub-path…
        let rc = npmrc("//npm.corp/:_authToken=host-token\n", "");
        assert_eq!(
            rc.auth_token_for("https://npm.corp/api/npm/repo")
                .as_deref(),
            Some("host-token")
        );

        // …and a more specific path-level token wins over the host-level one.
        let rc = npmrc(
            "//npm.corp/:_authToken=host-token\n\
             //npm.corp/api/npm/repo/:_authToken=repo-token\n",
            "",
        );
        assert_eq!(
            rc.auth_token_for("https://npm.corp/api/npm/repo")
                .as_deref(),
            Some("repo-token")
        );
        // Key without the trailing slash matches too (npm checks both forms).
        let rc = npmrc("//npm.corp/api/npm/repo:_authToken=noslash-token\n", "");
        assert_eq!(
            rc.auth_token_for("https://npm.corp/api/npm/repo")
                .as_deref(),
            Some("noslash-token")
        );
    }

    #[test]
    fn auth_token_project_wins_over_user() {
        let rc = npmrc(
            "//registry.npmjs.org/:_authToken=project-token\n",
            "//registry.npmjs.org/:_authToken=user-token\n",
        );
        assert_eq!(
            rc.auth_token_for("https://registry.npmjs.org").as_deref(),
            Some("project-token")
        );
    }

    #[test]
    fn auth_token_more_specific_user_path_beats_project_host() {
        // npm merges all config, then matches the most specific path: a
        // user-level path-scoped token must win over a project-level host-scoped
        // token, since path specificity outranks file precedence.
        let rc = npmrc(
            "//npm.corp/:_authToken=project-host-token\n",
            "//npm.corp/api/npm/repo/:_authToken=user-repo-token\n",
        );
        assert_eq!(
            rc.auth_token_for("https://npm.corp/api/npm/repo")
                .as_deref(),
            Some("user-repo-token")
        );
        // But at the same path level, project still wins the tie.
        let rc = npmrc(
            "//npm.corp/:_authToken=project-host-token\n",
            "//npm.corp/:_authToken=user-host-token\n",
        );
        assert_eq!(
            rc.auth_token_for("https://npm.corp/api").as_deref(),
            Some("project-host-token")
        );
    }

    #[test]
    fn auth_token_ignores_empty_and_invalid() {
        let rc = npmrc("//registry.npmjs.org/:_authToken=\n", "");
        assert_eq!(rc.auth_token_for("https://registry.npmjs.org"), None);
        // Non-URL registry input never panics.
        assert_eq!(rc.auth_token_for("not a url"), None);
    }

    #[test]
    fn auth_token_matches_registry_with_explicit_default_port() {
        // The url crate strips scheme-default ports, so an explicit `:443` still
        // resolves against a standard `//host/:_authToken` key (no port).
        let rc = npmrc("//registry.npmjs.org/:_authToken=npm-token\n", "");
        assert_eq!(
            rc.auth_token_for("https://registry.npmjs.org:443")
                .as_deref(),
            Some("npm-token")
        );
    }
}
