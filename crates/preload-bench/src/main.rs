//! Self-contained streaming preload bench with transitive walking.
//!
//! Same HTTP setup as `manifest-bench` (own `reqwest::Client` built
//! per rep with `aws-lc-rs` TLS, `pool_max_idle_per_host(256)`, no
//! proxy, default DNS, no retry). The only delta vs `manifest-bench`
//! is that this bench discovers names by walking transitive deps
//! from a `package.json` root, instead of consuming a flat name
//! list.
//!
//! Why a separate crate: ruborist's manifest-fetch path goes through
//! several service layers (custom DNS resolver, retry, cache,
//! single-flight gates, event receivers). Each layer might add
//! overhead. This bench bypasses all of them — same shape as
//! manifest-bench, just with a streaming `FuturesUnordered` that
//! refills from a pending queue extended by parsed transitive deps.
//!
//! Reports both the standalone preload wall and a per-rep eff_parallel
//! number so we can compare directly against manifest-bench's
//! `phase_wall` + `avg_conc` for the same workload.
//!
//! Output (one line per rep, matching manifest-bench shape):
//!   [rep N] preload_wall=Xms n=Y bytes=Z avg_conc=N.N parse_sum=Wms 200=A 4xx=B err=C

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use futures::stream::{FuturesUnordered, StreamExt};
use serde::Deserialize;

#[derive(Parser, Debug)]
#[command(
    name = "preload-bench",
    about = "Streaming preload bench with transitive walking (self-contained)"
)]
struct Args {
    /// Registry base URL.
    #[arg(long, default_value = "https://registry.npmjs.org")]
    registry: String,

    /// Path to a `package.json` to walk from. Reads `dependencies` +
    /// `devDependencies` + `optionalDependencies` as the initial seed.
    #[arg(long)]
    package_json: PathBuf,

    /// Maximum concurrent in-flight requests.
    #[arg(long, default_value_t = 96)]
    concurrency: usize,

    /// Number of times to repeat the whole walk (fresh client per rep).
    #[arg(long, default_value_t = 4)]
    reps: usize,

    /// Force HTTP/1.1.
    #[arg(long, default_value_t = true)]
    http1_only: bool,

    /// Override `User-Agent`.
    #[arg(long)]
    user_agent: Option<String>,

    /// Include `peerDependencies` when walking transitives. Off by
    /// default (matches utoo's default).
    #[arg(long)]
    include_peer: bool,
}

#[derive(Deserialize)]
struct PackageJson {
    #[serde(default)]
    dependencies: HashMap<String, String>,
    #[serde(default, rename = "devDependencies")]
    dev_dependencies: HashMap<String, String>,
    #[serde(default, rename = "optionalDependencies")]
    optional_dependencies: HashMap<String, String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let raw = std::fs::read_to_string(&args.package_json)
        .with_context(|| format!("read {:?}", args.package_json))?;
    let pkg: PackageJson = serde_json::from_str(&raw).context("parse package.json")?;
    let initial: Vec<(String, String)> = pkg
        .dependencies
        .into_iter()
        .chain(pkg.dev_dependencies)
        .chain(pkg.optional_dependencies)
        .filter(|(_, spec)| is_registry_spec(spec))
        .collect();

    println!(
        "preload-bench: registry={} concurrency={} reps={} initial={} h1_only={} ua={} include_peer={}",
        args.registry,
        args.concurrency,
        args.reps,
        initial.len(),
        args.http1_only,
        args.user_agent.as_deref().unwrap_or("<reqwest default>"),
        args.include_peer,
    );

    for rep in 1..=args.reps {
        run_once(&args, &initial, rep).await?;
    }

    Ok(())
}

/// Quick registry-spec check (a `^...` / `~...` / `latest` / etc).
/// Excludes `file:`, `link:`, `workspace:`, `git+`, `https://`, and
/// `<user>/<repo>` shorthand. Same intent as ruborist's
/// `SpecStr::is_registry_spec` but inlined to keep this crate
/// dependency-free.
fn is_registry_spec(spec: &str) -> bool {
    if spec.is_empty() {
        return true; // bare entries default to "*"
    }
    let lower = spec.to_ascii_lowercase();
    if lower.starts_with("file:")
        || lower.starts_with("link:")
        || lower.starts_with("workspace:")
        || lower.starts_with("portal:")
        || lower.starts_with("git+")
        || lower.starts_with("git://")
        || lower.starts_with("github:")
        || lower.starts_with("https://")
        || lower.starts_with("http://")
    {
        return false;
    }
    // `<user>/<repo>` shorthand — exactly one '/' and no '@' prefix on
    // first segment (rules out scoped names like `@scope/pkg`).
    if let Some((head, tail)) = spec.split_once('/')
        && !head.starts_with('@')
        && !tail.is_empty()
        && !tail.contains('/')
    {
        return false;
    }
    true
}

#[derive(Debug, Default)]
struct RepStats {
    n: usize,
    bytes: usize,
    parse_sum_us: u128,
    busy_us: u128,
    sum_us: u128,
    ok_200: usize,
    err_4xx: usize,
    err_other: usize,
}

async fn run_once(args: &Args, initial: &[(String, String)], rep: usize) -> Result<()> {
    let client = build_client(args)?;
    let registry = Arc::new(args.registry.trim_end_matches('/').to_string());
    let concurrency = args.concurrency;
    let include_peer = args.include_peer;

    let phase_start = Instant::now();
    let mut stats = RepStats::default();

    // (name, spec) dedup — same shape as ruborist's seen_specs but
    // self-contained. We dedup the *spec* level because two specs on
    // the same name might resolve to different versions.
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut pending: VecDeque<(String, String)> = VecDeque::new();
    for (name, spec) in initial {
        if seen.insert((name.clone(), spec.clone())) {
            pending.push_back((name.clone(), spec.clone()));
        }
    }

    // Sibling-fetch dedup: when two specs for the same name are both
    // pending, only one fetch is issued; subsequent specs settle from
    // the cached body. Keyed by name. Maps name → cached parsed body
    // (`Arc<Vec<u8>>`) once the first fetch lands.
    let body_cache: Arc<std::sync::Mutex<HashMap<String, Arc<Vec<u8>>>>> =
        Arc::new(std::sync::Mutex::new(HashMap::new()));
    let mut in_flight_names: HashSet<String> = HashSet::new();
    let mut deferred_by_name: HashMap<String, Vec<String>> = HashMap::new();

    let mut futs: FuturesUnordered<Fut> = FuturesUnordered::new();

    loop {
        while futs.len() < concurrency {
            let Some((name, spec)) = pending.pop_front() else {
                break;
            };

            // If the body is already cached (sibling spec for an
            // already-fetched name), spawn a settle-only future.
            if let Some(raw) = body_cache.lock().unwrap().get(&name).cloned() {
                let n = name.clone();
                let s = spec.clone();
                let fut: Fut = Box::pin(settle_only(n, s, raw, include_peer));
                futs.push(fut);
                continue;
            }

            // First time seeing this name: fetch + settle. Stash any
            // sibling specs that arrive while in-flight.
            if !in_flight_names.insert(name.clone()) {
                deferred_by_name.entry(name).or_default().push(spec);
                continue;
            }

            spawn_fetch(
                &client,
                &registry,
                name,
                spec,
                Arc::clone(&body_cache),
                include_peer,
                &mut futs,
            );
        }

        if futs.is_empty() {
            break;
        }

        let Some(out) = futs.next().await else { break };
        stats.n += 1;
        stats.busy_us += out.busy_us;
        stats.sum_us += out.sum_us;
        stats.parse_sum_us += out.parse_us;
        stats.bytes += out.bytes;
        match out.status {
            200 => stats.ok_200 += 1,
            400..=499 => stats.err_4xx += 1,
            _ => stats.err_other += 1,
        }

        // Drain sibling specs for this name now that body is cached.
        if out.fetched
            && let Some(siblings) = deferred_by_name.remove(&out.name)
            && let Some(raw) = body_cache.lock().unwrap().get(&out.name).cloned()
        {
            for sibling_spec in siblings {
                let n = out.name.clone();
                let r = Arc::clone(&raw);
                let fut: Fut = Box::pin(settle_only(n, sibling_spec, r, include_peer));
                futs.push(fut);
            }
        }

        // Extend pending with new transitives, dedup by (name, spec).
        for (name, spec) in out.transitives {
            if seen.insert((name.clone(), spec.clone())) {
                pending.push_back((name, spec));
            }
        }
    }

    let phase_wall_ms = phase_start.elapsed().as_millis();
    let parse_sum_ms = stats.parse_sum_us / 1000;
    // avg_conc = sum_request_us / busy_window_us. busy_us isn't a true
    // merged-interval here (we don't track per-req start/end timestamps
    // for that), so use phase_wall as the denominator — slightly
    // pessimistic but consistent.
    let avg_conc = if phase_wall_ms > 0 {
        stats.sum_us as f64 / 1000.0 / phase_wall_ms as f64
    } else {
        0.0
    };

    println!(
        "[rep {rep}] preload_wall={phase_wall_ms}ms n={} bytes={} parse_sum={parse_sum_ms}ms avg_conc={avg_conc:.1} 200={} 4xx={} err={}",
        stats.n, stats.bytes, stats.ok_200, stats.err_4xx, stats.err_other,
    );
    Ok(())
}

#[derive(Debug)]
struct FetchOutcome {
    name: String,
    /// `(name, spec)` transitive deps unfolded by parsing the resolved
    /// version's `dependencies` / `optionalDependencies` (and
    /// optionally `peerDependencies`).
    transitives: Vec<(String, String)>,
    /// `true` if this future fetched the body (vs settle-only on a
    /// cached body); only fetchers populate `body_cache` and trigger
    /// sibling drain.
    fetched: bool,
    /// HTTP status code (200 / 4xx / 5xx / 0 on transport error).
    status: u16,
    /// Body byte count (0 on error).
    bytes: usize,
    /// Self-reported per-future busy_us — `end - start`. Approximate.
    busy_us: u128,
    /// Sum of all per-future durations summed by the main loop.
    sum_us: u128,
    /// Parse work done inside this future (for accounting).
    parse_us: u128,
}

type Fut = std::pin::Pin<Box<dyn std::future::Future<Output = FetchOutcome> + Send>>;

fn spawn_fetch(
    client: &reqwest::Client,
    registry: &Arc<String>,
    name: String,
    spec: String,
    body_cache: Arc<std::sync::Mutex<HashMap<String, Arc<Vec<u8>>>>>,
    include_peer: bool,
    futs: &mut FuturesUnordered<Fut>,
) {
    let url = format!("{}/{}", registry, name);
    let client = client.clone();
    let fut: Fut = Box::pin(async move {
        let start = Instant::now();
        let req = client
            .get(&url)
            .header("accept", "application/vnd.npm.install-v1+json")
            .send();
        let (raw_bytes, status) = match req.await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let body = resp.bytes().await.map(|b| b.to_vec()).unwrap_or_default();
                (body, status)
            }
            Err(_) => (Vec::new(), 0),
        };
        let bytes = raw_bytes.len();

        let (parse_us, transitives) = if status == 200 && !raw_bytes.is_empty() {
            let raw_arc = Arc::new(raw_bytes);
            body_cache
                .lock()
                .unwrap()
                .insert(name.clone(), Arc::clone(&raw_arc));
            // Move the Arc<Vec<u8>> into spawn_blocking; the parser
            // mutates a clone, so the cached copy is unaffected.
            let spec_for_parse = spec.clone();
            let parse_start = Instant::now();
            let result = tokio::task::spawn_blocking(move || {
                parse_and_extract(&raw_arc, &spec_for_parse, include_peer)
            })
            .await
            .ok()
            .flatten()
            .unwrap_or_default();
            (parse_start.elapsed().as_micros(), result)
        } else {
            (0, Vec::new())
        };

        let end = Instant::now();
        let busy_us = end.duration_since(start).as_micros();
        FetchOutcome {
            name,
            transitives,
            fetched: true,
            status,
            bytes,
            busy_us,
            sum_us: busy_us,
            parse_us,
        }
    });
    futs.push(fut);
}

async fn settle_only(
    name: String,
    spec: String,
    raw: Arc<Vec<u8>>,
    include_peer: bool,
) -> FetchOutcome {
    let start = Instant::now();
    let parse_start = start;
    let transitives = tokio::task::spawn_blocking(move || {
        parse_and_extract(&raw, &spec, include_peer).unwrap_or_default()
    })
    .await
    .unwrap_or_default();
    let parse_us = parse_start.elapsed().as_micros();
    let end = Instant::now();
    let busy_us = end.duration_since(start).as_micros();
    FetchOutcome {
        name,
        transitives,
        fetched: false,
        status: 200,
        bytes: 0,
        busy_us,
        sum_us: busy_us,
        parse_us,
    }
}

/// Parse a manifest body, resolve `spec` against the version list,
/// extract that version's transitive deps. Single
/// `simd_json::to_borrowed_value` pass for the whole body — same as
/// ruborist's combined-parse path, but inlined here so this crate
/// has no ruborist dependency.
fn parse_and_extract(
    raw: &Arc<Vec<u8>>,
    spec: &str,
    include_peer: bool,
) -> Option<Vec<(String, String)>> {
    use simd_json::prelude::{ValueAsObject, ValueObjectAccess};

    let mut buf = (**raw).clone();
    let parsed = simd_json::to_borrowed_value(&mut buf).ok()?;

    let dist_tags: HashMap<String, String> = parsed
        .get("dist-tags")
        .and_then(|v| HashMap::<String, String>::deserialize(v).ok())
        .unwrap_or_default();
    let versions_obj = parsed.get("versions").and_then(ValueAsObject::as_object)?;

    // Resolve spec. Three cases: dist-tag match, exact-version key, or
    // semver range (we approximate with "first version that satisfies"
    // — preload-bench is a measurement tool, not a real resolver, so
    // we tolerate slight selection differences vs ruborist for the
    // purpose of timing the network path).
    let resolved = if let Some(via_tag) = dist_tags.get(spec) {
        via_tag.clone()
    } else if versions_obj.contains_key(spec) {
        spec.to_string()
    } else if let Some(latest) = dist_tags.get("latest")
        && spec_satisfied_by(spec, latest)
    {
        latest.clone()
    } else {
        // Last-resort: pick the lexicographically-largest version. Not
        // semver-correct but bounded by the version set, and good
        // enough for timing.
        versions_obj.keys().max().map(|k| k.to_string())?
    };

    let version_obj = versions_obj.get(resolved.as_str())?;
    let mut out: Vec<(String, String)> = Vec::new();

    if let Some(deps) = version_obj.get("dependencies")
        && let Ok(map) = HashMap::<String, String>::deserialize(deps)
    {
        out.extend(map.into_iter().filter(|(_, s)| is_registry_spec(s)));
    }
    if include_peer
        && let Some(deps) = version_obj.get("peerDependencies")
        && let Ok(map) = HashMap::<String, String>::deserialize(deps)
    {
        out.extend(map.into_iter().filter(|(_, s)| is_registry_spec(s)));
    }
    if let Some(deps) = version_obj.get("optionalDependencies")
        && let Ok(map) = HashMap::<String, String>::deserialize(deps)
    {
        out.extend(map.into_iter().filter(|(_, s)| is_registry_spec(s)));
    }
    Some(out)
}

/// Crude semver-satisfies check: only handles `^X.Y.Z` and `~X.Y.Z`
/// against an exact target. Sufficient for "does latest satisfy spec"
/// in this measurement context — full semver is in the resolver, not
/// the bench.
fn spec_satisfied_by(spec: &str, target: &str) -> bool {
    let s = spec.trim();
    let body = s
        .strip_prefix('^')
        .or_else(|| s.strip_prefix('~'))
        .unwrap_or(s);
    target.starts_with(body) || target == body
}

fn build_client(args: &Args) -> Result<reqwest::Client> {
    // Install aws-lc-rs as the default crypto provider (idempotent —
    // first call wins). Same setup as manifest-bench.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let mut roots = rustls::RootCertStore::empty();
    let native = rustls_native_certs::load_native_certs();
    for cert in native.certs {
        let _ = roots.add(cert);
    }

    let tls_config = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| anyhow!("rustls protocol versions: {e}"))?
    .with_root_certificates(roots)
    .with_no_client_auth();

    let mut builder = reqwest::Client::builder()
        .use_preconfigured_tls(tls_config)
        .no_proxy()
        .pool_max_idle_per_host(256);
    if args.http1_only {
        builder = builder.http1_only();
    }
    if let Some(ua) = &args.user_agent {
        builder = builder.user_agent(ua);
    }
    builder.build().context("build reqwest client")
}
