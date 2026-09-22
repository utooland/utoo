# Package manager benchmarks

`pm-bench-phases.sh` alternates package-manager order between rounds and
records command wall time, peak RSS and CPU time with `/usr/bin/time`.
Preparation and final disk-footprint collection are outside the timed window.
Use at least five rounds when comparing a change:

```sh
UTOO_NEXT_BIN=/absolute/path/to/baseline/utoo \
BENCH_RUNS=5 BENCH_RUNS_CHEAP=5 PM_LIST=utoo-next,utoo \
PROJECT=ant-design PHASES=p0,p1,p3,p4 bash bench/pm-bench-phases.sh
```

`utoo` on PATH is the candidate. Build both binaries with the same compiler,
target and profile. Record their commits, hashes, project revision and machine
details alongside the raw results. Run `PROJECT=x` for a large workspace.

| Phase | Lockfile | Client cache | Work measured |
| --- | --- | --- | --- |
| p0 | Removed | Empty | Resolve and install |
| p1 | Removed | Empty | Resolve and save lock |
| p3 | Shared lock for utoo variants | Empty | Install |
| p4 | Shared lock for utoo variants | Warm | Recreate node_modules |

The script deletes lockfiles, node_modules and its configured cache paths.
Use dedicated benchmark directories and an isolated HOME; do not point it at
a working project or a cache you want to preserve. Hooks are disabled.

## Frozen local registry

Public-registry timing includes WAN/CDN variation. `pm-local-registry.py`
provides an optional loopback snapshot for measuring client work with fixed
responses. It changes manifest tarball URLs to the local origin and preserves
tarball bytes and integrity. This measures local client behavior, not public
registry throughput.

Start the server in a separate terminal:

```sh
python3 bench/pm-local-registry.py \
  --cache /tmp/pm-bench-registry \
  --address-file /tmp/pm-bench-registry-address
```

Then run the existing benchmark with telemetry enabled:

```sh
bench_registry=$(cat /tmp/pm-bench-registry-address)
REGISTRY="$bench_registry" BENCH_REGISTRY_STATS="$bench_registry/_pm_bench" \
UTOO_NEXT_BIN=/absolute/path/to/baseline/utoo \
BENCH_RUNS=5 BENCH_RUNS_CHEAP=5 PM_LIST=utoo-next,utoo \
PROJECT=ant-design PHASES=p0,p1,p3,p4 bash bench/pm-bench-phases.sh
```

Untimed seeds and warmups populate the snapshot. Timed rounds freeze it and
reject missing upstream responses. The snapshot keeps its original port so
cached manifest URLs remain valid after a server restart. Use a new cache
directory if that port is no longer available.

Each `*_metrics.jsonl` sample has a corresponding
`*_metrics_network.jsonl` entry with HTTP body bytes, tarball bytes, request
counts and peak simultaneous requests. Require `frozen: true`, zero
`upstream_requests`, zero `offline_misses`, no active requests, and a successful
command for every accepted sample. HTTP peaks are not extraction or clone
concurrency; observe those scheduler stages separately when changing them.

Keep all samples, including failed runs, and compare paired rounds and baseline
variance before attributing a delta to the implementation. A warm cache test
must report zero tarball downloads. Record intentional correctness changes
that affect the dependency graph or work performed.
