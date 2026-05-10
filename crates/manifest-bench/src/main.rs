//! Standalone HTTP-only manifest fetch benchmark.
//!
//! Isolates the network behaviour of `reqwest + rustls + tokio` from
//! ruborist's resolver pipeline (BFS, dedup, parse, lockfile, project
//! cache). Reads a list of package names, builds manifest URLs, fires
//! parallel `GET` requests, records `(start, end)` per request, and
//! reports the same diag shape as ruborist's `Preload HTTP diag` line.
//!
//! Two input modes:
//! - `--names-file <path>` — newline-separated package names
//! - `--lockfile <path>` — a npm-style package-lock.json; we extract
//!   the `packages.*` (v3) or `dependencies.*` (v2) keys
//!
//! Two registry modes:
//! - `<registry>/<name>` — full manifest endpoint (default, npmjs)
//! - `<registry>/<name>/latest` — single-version endpoint
//!   (gated behind `--single-version`)
//!
//! Each request reads the body to completion (we only measure I/O, no
//! parse). Output: same fields as preload's HTTP diag for direct
//! comparison.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use futures::stream::{FuturesUnordered, StreamExt};

#[derive(Parser, Debug)]
#[command(
    name = "manifest-bench",
    about = "HTTP-only manifest fetch bench (no parse, no resolver)"
)]
struct Args {
    /// Registry base URL.
    #[arg(long, default_value = "https://registry.npmjs.org")]
    registry: String,

    /// File of newline-separated package names. Mutually exclusive with `--lockfile`.
    #[arg(long, conflicts_with = "lockfile")]
    names_file: Option<PathBuf>,

    /// `package-lock.json` file. Reads top-level `packages.*.name` keys.
    #[arg(long)]
    lockfile: Option<PathBuf>,

    /// Maximum concurrent in-flight requests.
    #[arg(long, default_value_t = 128)]
    concurrency: usize,

    /// Number of times to repeat the whole sweep (each iteration is a
    /// fresh `reqwest::Client`, so connection pool / TLS handshake
    /// costs are paid each time, matching `hyperfine` cold-start).
    #[arg(long, default_value_t = 1)]
    reps: usize,

    /// Use the single-version endpoint `/<name>/latest` instead of the
    /// full-manifest endpoint `/<name>`. Smaller bodies, more requests
    /// served per byte.
    #[arg(long)]
    single_version: bool,

    /// Override `Accept` header. Default mimics ruborist's preload
    /// (`application/vnd.npm.install-v1+json` — abbreviated metadata).
    #[arg(long)]
    accept: Option<String>,

    /// Override `User-Agent`. Default uses reqwest's default. Try
    /// `Bun/1.x.x` to test whether Cloudflare differentiates by UA.
    #[arg(long)]
    user_agent: Option<String>,

    /// Force HTTP/1.1 (no H2 negotiation). Default lets ALPN decide.
    #[arg(long)]
    http1_only: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let names = load_names(&args)?;
    if names.is_empty() {
        return Err(anyhow!("no package names found in input"));
    }

    println!(
        "manifest-bench: registry={} concurrency={} reps={} names={} h1_only={} single_version={} accept={} ua={}",
        args.registry,
        args.concurrency,
        args.reps,
        names.len(),
        args.http1_only,
        args.single_version,
        args.accept.as_deref().unwrap_or("<default>"),
        args.user_agent.as_deref().unwrap_or("<reqwest default>"),
    );

    for rep in 1..=args.reps {
        run_once(&args, &names, rep).await?;
    }

    Ok(())
}

fn load_names(args: &Args) -> Result<Vec<String>> {
    if let Some(path) = &args.names_file {
        let raw = std::fs::read_to_string(path).with_context(|| format!("read {path:?}"))?;
        return Ok(raw
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.starts_with('#'))
            .map(str::to_string)
            .collect());
    }

    if let Some(path) = &args.lockfile {
        let raw = std::fs::read_to_string(path).with_context(|| format!("read {path:?}"))?;
        return extract_lockfile_names(&raw);
    }

    Err(anyhow!("provide --names-file or --lockfile"))
}

/// Pull unique package names from an npm v3 lockfile (`packages.*`)
/// or an older v2 lockfile (`dependencies.*`).
fn extract_lockfile_names(raw: &str) -> Result<Vec<String>> {
    use std::collections::BTreeSet;

    let v: serde_json::Value = serde_json::from_str(raw).context("parse lockfile JSON")?;
    let mut names: BTreeSet<String> = BTreeSet::new();

    if let Some(packages) = v.get("packages").and_then(|p| p.as_object()) {
        for key in packages.keys() {
            if key.is_empty() {
                continue;
            }
            // npm v3 packages key like "node_modules/foo" or
            // "node_modules/@scope/bar/node_modules/baz" — take the
            // last path segment (or @scope/name pair).
            let last = last_module_name(key);
            if !last.is_empty() {
                names.insert(last);
            }
        }
    } else if let Some(deps) = v.get("dependencies").and_then(|d| d.as_object()) {
        for key in deps.keys() {
            names.insert(key.clone());
        }
    }

    Ok(names.into_iter().collect())
}

fn last_module_name(key: &str) -> String {
    let parts: Vec<&str> = key.split("node_modules/").collect();
    let tail = parts.last().copied().unwrap_or("");
    tail.to_string()
}

#[derive(Debug)]
struct ReqResult {
    start: Instant,
    end: Instant,
    bytes: usize,
    status: u16,
}

async fn run_once(args: &Args, names: &[String], rep: usize) -> Result<()> {
    // Build a fresh client per rep — matches hyperfine's cold-start
    // assumption that each iteration pays the TLS handshake cost.
    let client = build_client(args)?;
    let registry = Arc::new(args.registry.trim_end_matches('/').to_string());
    let accept = Arc::new(
        args.accept
            .clone()
            .unwrap_or_else(|| "application/vnd.npm.install-v1+json".to_string()),
    );

    let single_version = args.single_version;
    let concurrency = args.concurrency;

    let phase_start = Instant::now();
    let mut futs = FuturesUnordered::new();
    let mut idx = 0usize;
    let mut results: Vec<ReqResult> = Vec::with_capacity(names.len());

    while idx < names.len() && futs.len() < concurrency {
        spawn_one(
            &client,
            &registry,
            &names[idx],
            &accept,
            single_version,
            &mut futs,
        );
        idx += 1;
    }

    while let Some(res) = futs.next().await {
        results.push(res);
        if idx < names.len() {
            spawn_one(
                &client,
                &registry,
                &names[idx],
                &accept,
                single_version,
                &mut futs,
            );
            idx += 1;
        }
    }
    let phase_wall_ms = phase_start.elapsed().as_millis();

    report(rep, &results, phase_wall_ms);
    Ok(())
}

type Fut = std::pin::Pin<Box<dyn std::future::Future<Output = ReqResult> + Send>>;

fn spawn_one(
    client: &reqwest::Client,
    registry: &Arc<String>,
    name: &str,
    accept: &Arc<String>,
    single_version: bool,
    futs: &mut FuturesUnordered<Fut>,
) {
    let url = if single_version {
        format!("{registry}/{name}/latest")
    } else {
        format!("{registry}/{name}")
    };
    let client = client.clone();
    let accept = Arc::clone(accept);
    futs.push(Box::pin(async move {
        let start = Instant::now();
        let req = client.get(&url).header("accept", accept.as_str()).send();
        let (bytes, status) = match req.await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let body = resp.bytes().await.map(|b| b.len()).unwrap_or(0);
                (body, status)
            }
            Err(_) => (0, 0),
        };
        let end = Instant::now();
        ReqResult {
            start,
            end,
            bytes,
            status,
        }
    }));
}

fn build_client(args: &Args) -> Result<reqwest::Client> {
    // Install aws-lc-rs as the default crypto provider (idempotent —
    // first call wins). Matches ruborist's `service::http` setup.
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

fn report(rep: usize, results: &[ReqResult], wall_ms: u128) {
    if results.is_empty() {
        eprintln!("[rep {rep}] no results");
        return;
    }

    let mut spans: Vec<(Instant, Instant)> = results.iter().map(|r| (r.start, r.end)).collect();
    spans.sort_by_key(|(s, _)| *s);

    let first_start = spans.first().unwrap().0;
    let last_end = spans.iter().map(|(_, e)| *e).max().unwrap();
    let win_wall = last_end.duration_since(first_start).as_millis();

    let mut per_us: Vec<u128> = spans
        .iter()
        .map(|(s, e)| e.duration_since(*s).as_micros())
        .collect();
    per_us.sort_unstable();
    let n = per_us.len();
    let pct = |p: usize| per_us[(n * p).div_ceil(100).saturating_sub(1)];
    let sum: u128 = per_us.iter().sum();
    let p50 = per_us[n / 2];

    let mut busy_us: u128 = 0;
    let (mut cur_s, mut cur_e) = spans[0];
    for &(s, e) in &spans[1..] {
        if s <= cur_e {
            if e > cur_e {
                cur_e = e;
            }
        } else {
            busy_us += cur_e.duration_since(cur_s).as_micros();
            cur_s = s;
            cur_e = e;
        }
    }
    busy_us += cur_e.duration_since(cur_s).as_micros();

    let bytes_total: usize = results.iter().map(|r| r.bytes).sum();
    let ok = results.iter().filter(|r| r.status == 200).count();
    let err = results.iter().filter(|r| r.status == 0).count();
    let four_xx = results
        .iter()
        .filter(|r| (400..500).contains(&r.status))
        .count();
    let five_xx = results
        .iter()
        .filter(|r| (500..600).contains(&r.status))
        .count();

    let avg_conc = if busy_us > 0 {
        sum as f64 / busy_us as f64
    } else {
        0.0
    };

    println!(
        "[rep {rep}] n={} phase_wall={}ms win_wall={}ms busy={}ms ({:.0}%) sum={}ms avg_conc={:.1} p50={}ms p95={}ms p99={}ms max={}ms bytes={} 200={} 4xx={} 5xx={} err={}",
        n,
        wall_ms,
        win_wall,
        busy_us / 1000,
        if win_wall > 0 {
            100.0 * (busy_us as f64 / 1000.0) / win_wall as f64
        } else {
            0.0
        },
        sum / 1000,
        avg_conc,
        p50 / 1000,
        pct(95) / 1000,
        pct(99) / 1000,
        per_us.last().unwrap() / 1000,
        bytes_total,
        ok,
        four_xx,
        five_xx,
        err,
    );
}
