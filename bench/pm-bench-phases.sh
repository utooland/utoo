#!/bin/bash
# Phase-isolated cold-install bench: resolve vs cold-install vs warm-link.
# utoo vs bun only (they're the PMs with a true --lockfile-only mode).
set -eo pipefail

# --- config ---
PROJECT=${PROJECT:-ant-design}
REGISTRY=${REGISTRY:-https://registry.npmjs.org}
RUNS=${BENCH_RUNS:-3}
# Cheap phases (p1 resolve / p4 warm link) finish in seconds; more runs there
# buys statistical power exactly where it's affordable.
RUNS_CHEAP=${BENCH_RUNS_CHEAP:-$RUNS}
# Ablation variant: `utoo-alt` runs the SAME utoo binary with the extra env
# from UTOO_ALT_ENV (e.g. "UTOO_TARBALL_HTTP=h2" or
# "UTOO_CLONE_CONCURRENCY=8") and its own cache dir. Benching utoo vs
# utoo-alt on one runner isolates a single knob from network/runner weather
# — the way to validate protocol/concurrency choices instead of trusting
# cross-run deltas. Auto-appended to PM_LIST when UTOO_ALT_ENV is set.
if [ -n "${UTOO_ALT_ENV:-}" ] && [[ ",${PM_LIST:-}," != *",utoo-alt,"* ]]; then
  PM_LIST="${PM_LIST:+$PM_LIST,}utoo-alt"
fi
IFS=',' read -ra PACKAGE_MANAGERS <<< "${PM_LIST:-utoo,bun}"

BENCH_DIR=${BENCH_DIR:-/tmp/pm-bench}
RESULTS_DIR=${RESULTS_DIR:-/tmp/pm-bench-results}
mkdir -p "$BENCH_DIR" "$RESULTS_DIR"

# Explicit bench-scoped cache dirs so rm -rf is unambiguous and we don't
# depend on what each PM considers the default (which varies by HOME, OS
# image, and previously-set user config).
UTOO_CACHE="${UTOO_CACHE:-/tmp/utoo-bench-cache}"
UTOO_NPM_CACHE="${UTOO_NPM_CACHE:-/tmp/utoo-npm-bench-cache}"
UTOO_NEXT_CACHE="${UTOO_NEXT_CACHE:-/tmp/utoo-next-bench-cache}"
UTOO_ALT_CACHE="${UTOO_ALT_CACHE:-/tmp/utoo-alt-bench-cache}"
BUN_CACHE="${BUN_CACHE:-/tmp/bun-bench-cache}"
export BUN_INSTALL_CACHE_DIR="$BUN_CACHE"

# Drop optional baselines from the PM list when their binary is not wired
# up — UTOO_NPM_BIN is set by CI's "Install utoo@npm" step, UTOO_NEXT_BIN
# by the optional "Build next branch utoo" step. Local runs without them
# just skip the comparison instead of erroring.
FILTERED=()
for pm in "${PACKAGE_MANAGERS[@]}"; do
  case "$pm" in
    utoo-npm)
      if [ -z "${UTOO_NPM_BIN:-}" ]; then
        echo "skip utoo-npm: UTOO_NPM_BIN not set" >&2
        continue
      fi
      ;;
    utoo-next)
      if [ -z "${UTOO_NEXT_BIN:-}" ]; then
        echo "skip utoo-next: UTOO_NEXT_BIN not set" >&2
        continue
      fi
      ;;
    utoo-alt)
      if [ -z "${UTOO_ALT_ENV:-}" ]; then
        echo "skip utoo-alt: UTOO_ALT_ENV not set" >&2
        continue
      fi
      ;;
  esac
  FILTERED+=("$pm")
done
PACKAGE_MANAGERS=("${FILTERED[@]}")

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
RED='\033[0;31m'
NC='\033[0m'

banner() { echo -e "${YELLOW}=== $* ===${NC}"; }

# Retry a command a few times with a short pause — registry/network hiccups
# during the (untimed) seed installs were the dominant CI flake mode, and a
# single blip used to abort the whole bench via `set -e`.
retry() {
  local attempts=$1 i
  shift
  for ((i = 1; i <= attempts; i++)); do
    "$@" && return 0
    echo -e "  ${RED}attempt $i/$attempts failed:${NC} $*" >&2
    [ "$i" -lt "$attempts" ] && sleep 5
  done
  return 1
}

# --- metrics wrapper ---
# Keeps the hot path fast: only /usr/bin/time + counter snapshots inside the
# measured window. Disk-footprint measurement (du on node_modules / cache)
# happens once per phase AFTER hyperfine exits — see capture_footprint in
# run_phase — so 1-2s of `du` traversal doesn't bleed into wall-clock.
METRICS_WRAPPER="$RESULTS_DIR/metrics_wrapper.sh"
cat > "$METRICS_WRAPPER" <<'METRICS_EOF'
#!/bin/bash
METRICS_FILE="$1"; shift
TIME_TMP=$(mktemp)

# --- snapshot network BEFORE the command (cheap: one read of /proc/net/dev) ---
snap_net_rx=0; snap_net_tx=0
if [ -r /proc/net/dev ]; then
  read snap_net_rx snap_net_tx < <(awk '/:/ && $1 !~ /^lo:/ {rx += $2; tx += $10} END {print rx+0, tx+0}' /proc/net/dev)
fi

if [[ "$(uname)" == "Darwin" ]]; then
  /usr/bin/time -l "$@" 2>"$TIME_TMP"
  EXIT_CODE=$?
  RSS=$(awk '/maximum resident set size/ {print $1}' "$TIME_TMP")
  PG_MAJOR=$(awk '/page faults/ {print $1}' "$TIME_TMP")
  PG_MINOR=$(awk '/page reclaims/ {print $1}' "$TIME_TMP")
  VOL_CTX=$(awk '/ voluntary context switches/ {print $1}' "$TIME_TMP")
  INVOL_CTX=$(awk '/involuntary context switches/ {print $1}' "$TIME_TMP")
  USER_S=$(awk '/real/ && /user/ && /sys/ {print $3}' "$TIME_TMP")
  SYS_S=$(awk '/real/ && /user/ && /sys/ {print $5}' "$TIME_TMP")
else
  /usr/bin/time -v "$@" 2>"$TIME_TMP"
  EXIT_CODE=$?
  RSS_KB=$(awk '/Maximum resident set size/ {print $NF}' "$TIME_TMP")
  RSS=$(( ${RSS_KB:-0} * 1024 ))
  PG_MAJOR=$(awk '/Major \(requiring I\/O\) page faults/ {print $NF}' "$TIME_TMP")
  PG_MINOR=$(awk '/Minor \(reclaiming a frame\) page faults/ {print $NF}' "$TIME_TMP")
  VOL_CTX=$(awk '/Voluntary context switches/ {print $NF}' "$TIME_TMP")
  INVOL_CTX=$(awk '/Involuntary context switches/ {print $NF}' "$TIME_TMP")
  USER_S=$(awk -F': ' '/User time \(seconds\)/ {print $2}' "$TIME_TMP")
  SYS_S=$(awk -F': ' '/System time \(seconds\)/  {print $2}' "$TIME_TMP")
fi

# --- snapshot network AFTER, compute delta ---
net_rx=0; net_tx=0
if [ -r /proc/net/dev ]; then
  read cur_net_rx cur_net_tx < <(awk '/:/ && $1 !~ /^lo:/ {rx += $2; tx += $10} END {print rx+0, tx+0}' /proc/net/dev)
  net_rx=$(( cur_net_rx - snap_net_rx ))
  net_tx=$(( cur_net_tx - snap_net_tx ))
fi

printf '{"rss":%d,"user_s":%s,"sys_s":%s,"page_major":%d,"page_minor":%d,"vol_ctx":%d,"invol_ctx":%d,"net_rx":%d,"net_tx":%d}\n' \
  "${RSS:-0}" "${USER_S:-0}" "${SYS_S:-0}" "${PG_MAJOR:-0}" "${PG_MINOR:-0}" \
  "${VOL_CTX:-0}" "${INVOL_CTX:-0}" \
  "${net_rx:-0}" "${net_tx:-0}" >> "$METRICS_FILE"
rm -f "$TIME_TMP"
exit $EXIT_CODE
METRICS_EOF
chmod +x "$METRICS_WRAPPER"

# Post-phase footprint helper: records final on-disk size of the paths each
# phase should have touched. Called ONCE after hyperfine finishes, so the
# (slow) du traversal is not part of any timed window.
du_bytes() {
  for p in "$@"; do
    if [ -e "$p" ]; then
      if [[ "$(uname)" == "Darwin" ]]; then
        du -sk "$p" 2>/dev/null | awk '{print $1 * 1024}'
      else
        du -sb "$p" 2>/dev/null | awk '{print $1}'
      fi
    else
      echo 0
    fi
  done | awk '{s += $1} END {print s+0}'
}

capture_footprint() {
  local phase=$1 pm=$2 out=$3
  local cache
  case "$pm" in
    utoo)      cache=$UTOO_CACHE ;;
    utoo-npm)  cache=$UTOO_NPM_CACHE ;;
    utoo-next) cache=$UTOO_NEXT_CACHE ;;
    utoo-alt)  cache=$UTOO_ALT_CACHE ;;
    bun)       cache=$BUN_CACHE ;;
  esac
  printf '{"cache":%d,"node_modules":%d,"lockfile":%d}\n' \
    "$(du_bytes "$cache")" \
    "$(du_bytes "$PROJECT_DIR/node_modules")" \
    "$(du_bytes "$PROJECT_DIR/package-lock.json" "$PROJECT_DIR/bun.lock")" \
    > "$out"
}

# --- project ---
banner "Preparing $PROJECT"
cd "$BENCH_DIR"
if [ ! -d "$PROJECT" ]; then
  retry 3 git clone --depth=1 "https://github.com/ant-design/${PROJECT}.git" "$PROJECT"
fi
PROJECT_DIR="$BENCH_DIR/$PROJECT"

# --- prepare script builder ---
# Emit a self-contained reset script that hyperfine's --prepare can re-exec.
# The script always prints what it deleted so the CI log proves prepare ran.
write_prepare() {
  local path=$1 phase=$2 pm=$3
  local cache
  case "$pm" in
    utoo)      cache=$UTOO_CACHE ;;
    utoo-npm)  cache=$UTOO_NPM_CACHE ;;
    utoo-next) cache=$UTOO_NEXT_CACHE ;;
    utoo-alt)  cache=$UTOO_ALT_CACHE ;;
    bun)       cache=$BUN_CACHE ;;
  esac

  cat > "$path" <<EOF
#!/bin/bash
set -e
cd "$PROJECT_DIR"

# Always: drop node_modules (top-level + any workspace pkg trees).
rm -rf node_modules
find . -maxdepth 4 -type d -path '*/packages/*/node_modules' -exec rm -rf {} + 2>/dev/null || true

EOF

  # Use explicit bench-scoped cache paths and rm -rf them directly — previously
  # `utoo clean` / `bun pm cache rm` didn't actually empty the cache on the CI
  # runner, leading to cache-hit runs masquerading as cold installs.
  case "$phase" in
    p0_*)
      # Phase 0: full cold install — nothing reused. Lockfile + all caches wiped.
      cat >> "$path" <<EOF
rm -f package-lock.json bun.lock yarn.lock pnpm-lock.yaml
rm -rf "$UTOO_CACHE" "$UTOO_NPM_CACHE" "$UTOO_NEXT_CACHE" "$UTOO_ALT_CACHE" "$BUN_CACHE"
echo "[prep] phase 0 $pm: full cold (lockfile + caches + node_modules wiped)"
EOF
      ;;
    p1_*)
      # Phase 1: cold resolve — wipe lockfiles AND caches so nothing can be reused.
      cat >> "$path" <<EOF
rm -f package-lock.json bun.lock yarn.lock pnpm-lock.yaml
rm -rf "$UTOO_CACHE" "$UTOO_NPM_CACHE" "$UTOO_NEXT_CACHE" "$UTOO_ALT_CACHE" "$BUN_CACHE"
echo "[prep] phase 1 $pm: cleaned lockfiles + caches + node_modules"
EOF
      ;;
    p3_*)
      # Phase 3: cold install — keep THIS pm's lockfile, wipe THIS pm's cache
      # (the $cache resolved above). Delete the other pm's lockfile so bun
      # doesn't try to migrate utoo's package-lock.json (and vice versa).
      case "$pm" in
        bun) cat >> "$path" <<EOF
rm -f package-lock.json yarn.lock pnpm-lock.yaml
rm -rf "$BUN_CACHE"
echo "[prep] phase 3 bun: kept bun.lock, wiped $BUN_CACHE"
EOF
          ;;
        *) cat >> "$path" <<EOF
rm -f bun.lock yarn.lock pnpm-lock.yaml
rm -rf "$cache"
echo "[prep] phase 3 $pm: kept package-lock.json, wiped $cache"
EOF
          ;;
      esac
      ;;
    p4_*)
      # Phase 4: warm link — keep lockfile AND cache, only drop node_modules.
      case "$pm" in
        bun) cat >> "$path" <<EOF
rm -f package-lock.json yarn.lock pnpm-lock.yaml
echo "[prep] phase 4 bun: kept bun.lock + cache"
EOF
          ;;
        *) cat >> "$path" <<EOF
rm -f bun.lock yarn.lock pnpm-lock.yaml
echo "[prep] phase 4 $pm: kept package-lock.json + cache"
EOF
          ;;
      esac
      ;;
  esac

  chmod +x "$path"
}

# Seed PM-specific state before a phase runs (lockfile + cache where needed).
# This runs ONCE before hyperfine starts, not per-iteration. Returns non-zero
# on terminal seed failure so run_phase can skip just that (phase, pm) cell
# instead of aborting the whole bench.
#
# Lockfile provenance: all utoo variants install from one package-lock.json.
# Generate it with the *baseline* binary (utoo-next) when wired up, so a PR
# that changes resolution/lockfile shape can never perturb the input the
# baseline is measured against — p3/p4 compare install speed on equal input.
seed_lockfile_cmd() {
  local pm=$1
  if [ -n "${UTOO_NEXT_BIN:-}" ]; then
    echo "$UTOO_NEXT_BIN deps --registry=$REGISTRY --cache-dir=$UTOO_NEXT_CACHE"
    return
  fi
  case "$pm" in
    utoo)      echo "utoo deps --registry=$REGISTRY --cache-dir=$UTOO_CACHE" ;;
    utoo-npm)  echo "$UTOO_NPM_BIN deps --registry=$REGISTRY --cache-dir=$UTOO_NPM_CACHE" ;;
    utoo-next) echo "$UTOO_NEXT_BIN deps --registry=$REGISTRY --cache-dir=$UTOO_NEXT_CACHE" ;;
    utoo-alt)  echo "env $UTOO_ALT_ENV utoo deps --registry=$REGISTRY --cache-dir=$UTOO_ALT_CACHE" ;;
  esac
}

seed_for_phase() {
  local phase=$1 pm=$2
  local seed_log="$RESULTS_DIR/seed_${phase}_${pm}.log"
  cd "$PROJECT_DIR"
  case "$phase:$pm" in
    p3_*:utoo|p4_*:utoo|p3_*:utoo-npm|p4_*:utoo-npm|p3_*:utoo-next|p4_*:utoo-next|p3_*:utoo-alt|p4_*:utoo-alt)
      if [ ! -f package-lock.json ]; then
        echo -e "  ${CYAN}seed: generating package-lock.json (baseline-pinned when available)${NC}"
        retry 3 bash -c "$(seed_lockfile_cmd "$pm") >> '$seed_log' 2>&1" || return 1
      fi
      ;;
    p3_*:bun|p4_*:bun)
      if [ ! -f bun.lock ]; then
        echo -e "  ${CYAN}seed: running \`bun install --lockfile-only\` to generate bun.lock${NC}"
        rm -f package-lock.json
        retry 3 bash -c "bun install --lockfile-only --registry='$REGISTRY' >> '$seed_log' 2>&1" || return 1
      fi
      ;;
  esac
  # Phase 4 also needs a pre-warmed cache.
  if [[ "$phase" == p4_* ]]; then
    local cache
    case "$pm" in
      utoo)      cache=$UTOO_CACHE ;;
      utoo-npm)  cache=$UTOO_NPM_CACHE ;;
      utoo-next) cache=$UTOO_NEXT_CACHE ;;
      utoo-alt)  cache=$UTOO_ALT_CACHE ;;
      bun)       cache=$BUN_CACHE ;;
    esac
    if [ ! -d "$cache" ] || [ -z "$(ls -A "$cache" 2>/dev/null)" ]; then
      echo -e "  ${CYAN}seed: warming $pm cache via full install${NC}"
      rm -rf node_modules
      retry 3 bash -c "$(install_cmd "$pm") >> '$RESULTS_DIR/seed_warmup_${pm}.log' 2>&1" || return 1
      rm -rf node_modules
    fi
  fi
}

install_cmd() {
  case "$1" in
    utoo)      echo "utoo install --ignore-scripts --registry=$REGISTRY --cache-dir=$UTOO_CACHE" ;;
    utoo-npm)  echo "$UTOO_NPM_BIN install --ignore-scripts --registry=$REGISTRY --cache-dir=$UTOO_NPM_CACHE" ;;
    utoo-next) echo "$UTOO_NEXT_BIN install --ignore-scripts --registry=$REGISTRY --cache-dir=$UTOO_NEXT_CACHE" ;;
    utoo-alt)  echo "env $UTOO_ALT_ENV utoo install --ignore-scripts --registry=$REGISTRY --cache-dir=$UTOO_ALT_CACHE" ;;
    bun)       echo "bun install --ignore-scripts --registry=$REGISTRY" ;;
  esac
}

resolve_cmd() {
  case "$1" in
    utoo)      echo "utoo deps --registry=$REGISTRY --cache-dir=$UTOO_CACHE" ;;
    utoo-npm)  echo "$UTOO_NPM_BIN deps --registry=$REGISTRY --cache-dir=$UTOO_NPM_CACHE" ;;
    utoo-next) echo "$UTOO_NEXT_BIN deps --registry=$REGISTRY --cache-dir=$UTOO_NEXT_CACHE" ;;
    utoo-alt)  echo "env $UTOO_ALT_ENV utoo deps --registry=$REGISTRY --cache-dir=$UTOO_ALT_CACHE" ;;
    bun)       echo "bun install --lockfile-only --registry=$REGISTRY" ;;
  esac
}

run_phase() {
  local phase=$1 pm=$2 cmd=$3
  local json="$RESULTS_DIR/${PROJECT}_${phase}_${pm}.json"
  local metrics="$RESULTS_DIR/${PROJECT}_${phase}_${pm}_metrics.jsonl"
  local prep_script="$RESULTS_DIR/prep_${phase}_${pm}.sh"
  local cmd_script="$RESULTS_DIR/cmd_${phase}_${pm}.sh"

  # Cheap phases get more runs (see RUNS_CHEAP).
  local runs=$RUNS
  case "$phase" in
    p1_* | p4_*) runs=$RUNS_CHEAP ;;
  esac

  if ! seed_for_phase "$phase" "$pm"; then
    echo -e "  ${RED}$pm $phase seed failed after retries — skipping this cell${NC}"
    printf '{"failed":"seed"}\n' > "$RESULTS_DIR/${PROJECT}_${phase}_${pm}_failed.json"
    return 0
  fi
  write_prepare "$prep_script" "$phase" "$pm"

  # Reset metrics file; wrap the bench command so /usr/bin/time can measure it.
  # The wrapper reads PROJECT_DIR / UTOO_CACHE / BUN_CACHE from env to scope du.
  : > "$metrics"
  printf 'set -eo pipefail\ncd %s\n%s\n' "$PROJECT_DIR" "$cmd" > "$cmd_script"

  echo -e "  ${CYAN}$pm${NC} · $phase"
  export PROJECT_DIR UTOO_CACHE BUN_CACHE
  # `--warmup 1`: untimed iteration before the timed runs so each PM enters
  # its measurement window with DNS resolver cached, TLS session ticket
  # primed, and the CDN edge POP populated. Without this, the first PM in
  # the per-phase loop pays the cold-network tax (~+1s on p0) which the
  # 3-run `--runs` mean cannot fully amortize, biasing the first PM
  # systematically slow vs PMs that follow it. `--prepare` resets the
  # filesystem state (lockfile / cache / node_modules) on every iteration
  # — warmup included — so warmup leaves no on-disk artifact behind.
  if ! hyperfine \
    --runs "$RUNS" \
    --warmup 1 \
    --prepare "bash $prep_script" \
    --export-json "$json" \
    --show-output \
    -n "${pm}-${phase}" \
    "bash $METRICS_WRAPPER $metrics bash $cmd_script"; then
    echo -e "  ${RED}$pm $phase failed${NC}"
  fi

  # After the timed runs: capture the state left on disk by the last run.
  # Done outside hyperfine so du's traversal doesn't pollute wall-clock.
  capture_footprint "$phase" "$pm" "$RESULTS_DIR/${PROJECT}_${phase}_${pm}_footprint.json"
}

# === PHASE 0: full cold install (clean slate + full install) ===
# Matches the end-to-end user scenario: no lockfile, no cache, no node_modules.
# Directly comparable to `bun install` / `utoo install` on a freshly cloned repo.
banner "Phase 0 · full cold install (lockfile + cache + node_modules all wiped)"
for pm in "${PACKAGE_MANAGERS[@]}"; do
  run_phase "p0_full_cold" "$pm" "$(install_cmd "$pm")"
done

# === PHASE 1: resolve only (clean slate) ===
banner "Phase 1 · resolve (lockfile only, cold cache)"
for pm in "${PACKAGE_MANAGERS[@]}"; do
  run_phase "p1_resolve" "$pm" "$(resolve_cmd "$pm")"
done

# === PHASE 3: cold install (lockfile exists, cache empty) ===
banner "Phase 3 · cold install (lockfile present, empty cache, empty node_modules)"
for pm in "${PACKAGE_MANAGERS[@]}"; do
  run_phase "p3_cold_install" "$pm" "$(install_cmd "$pm")"
done

# === PHASE 4: warm link (cache populated, lockfile exists) ===
banner "Phase 4 · warm link (lockfile present, populated cache, empty node_modules)"
for pm in "${PACKAGE_MANAGERS[@]}"; do
  run_phase "p4_warm_link" "$pm" "$(install_cmd "$pm")"
done

# === SUMMARY ===
banner "Summary"
RESULTS_DIR="$RESULTS_DIR" node -e "
  const fs = require('fs'), path = require('path');
  const dir = process.env.RESULTS_DIR;
  const order = ['p0_full_cold', 'p1_resolve', 'p3_cold_install', 'p4_warm_link'];
  const timing = {};    // phase -> pm -> {mean,stddev,min,max}
  const metrics = {};   // phase -> pm -> averaged resource fields

  const parseKey = (file, suffix) => {
    const base = file.replace(suffix, '');
    for (const p of order) {
      const idx = base.indexOf('_' + p + '_');
      if (idx === -1) continue;
      const pm = base.slice(idx + p.length + 2);
      return { phase: p, pm };
    }
    return null;
  };

  for (const f of fs.readdirSync(dir).filter(x => x.endsWith('.json') && !x.endsWith('_metrics.jsonl') && !x.endsWith('_footprint.json'))) {
    const key = parseKey(f, '.json');
    if (!key) continue;
    let data; try { data = JSON.parse(fs.readFileSync(path.join(dir, f), 'utf8')); } catch (_) { continue; }
    const r = data.results[0];
    if (!r) continue;
    (timing[key.phase] ??= {})[key.pm] = { mean: r.mean, stddev: r.stddev, min: r.min, max: r.max };
  }

  const metricKeys = ['rss','user_s','sys_s','page_major','page_minor','vol_ctx','invol_ctx','net_rx','net_tx'];
  for (const f of fs.readdirSync(dir).filter(x => x.endsWith('_metrics.jsonl'))) {
    const key = parseKey(f, '_metrics.jsonl');
    if (!key) continue;
    const rows = [];
    for (const l of fs.readFileSync(path.join(dir, f), 'utf8').trim().split('\n').filter(Boolean)) {
      try { rows.push(JSON.parse(l)); } catch (_) {}
    }
    if (rows.length === 0) continue;
    const avg = {};
    for (const k of metricKeys) {
      avg[k] = rows.reduce((s,e) => s + Number(e[k] || 0), 0) / rows.length;
    }
    (metrics[key.phase] ??= {})[key.pm] = avg;
  }

  // Footprint is a post-phase single sample (not averaged): final on-disk size
  // of the paths the phase should have touched.
  for (const f of fs.readdirSync(dir).filter(x => x.endsWith('_footprint.json'))) {
    const key = parseKey(f, '_footprint.json');
    if (!key) continue;
    let data; try { data = JSON.parse(fs.readFileSync(path.join(dir, f), 'utf8')); } catch (_) { continue; }
    const slot = (metrics[key.phase] ??= {})[key.pm] ??= {};
    slot.foot_cache = data.cache || 0;
    slot.foot_nm    = data.node_modules || 0;
    slot.foot_lock  = data.lockfile || 0;
  }

  const pad = (s, n) => String(s).padEnd(n);
  const padR = (s, n) => String(s).padStart(n);
  const fmtB = b => b >= 1<<30 ? (b/(1<<30)).toFixed(2)+'G' : b >= 1<<20 ? (b/(1<<20)).toFixed(0)+'M' : b >= 1<<10 ? (b/(1<<10)).toFixed(0)+'K' : Math.round(b)+'B';
  const fmtN = n => n >= 1e6 ? (n/1e6).toFixed(2)+'M' : n >= 1e3 ? (n/1e3).toFixed(1)+'K' : String(Math.round(n));

  for (const phase of order) {
    const tp = timing[phase] || {}, mp = metrics[phase] || {};
    const pms = [...new Set([...Object.keys(tp), ...Object.keys(mp)])];
    if (pms.length === 0) continue;

    console.log('\n## ' + phase);

    // Table A: wall + CPU + memory
    console.log(pad('PM', 6) + ' ' + padR('wall', 8) + ' ' + padR('±σ', 7) + '   ' + padR('user', 7) + ' ' + padR('sys', 7) + '   ' + padR('RSS', 6) + '   ' + padR('pgMinor', 8));
    for (const pm of pms) {
      const t = tp[pm] || {}, m = mp[pm] || {};
      console.log(
        pad(pm, 6) + ' ' +
        padR((t.mean ?? 0).toFixed(2)+'s', 8) + ' ' +
        padR((t.stddev ?? 0).toFixed(2)+'s', 7) + '   ' +
        padR((m.user_s || 0).toFixed(2)+'s', 7) + ' ' +
        padR((m.sys_s  || 0).toFixed(2)+'s', 7) + '   ' +
        padR(m.rss ? fmtB(m.rss) : '-', 6) + '   ' +
        padR(m.page_minor ? fmtN(m.page_minor) : '-', 8)
      );
    }

    // Table B: context switches + network + final on-disk footprint
    console.log(pad('PM', 6) + ' ' + padR('vCtx', 8) + ' ' + padR('iCtx', 8) + '   ' + padR('netRX', 8) + ' ' + padR('netTX', 8) + '   ' + padR('cache', 8) + ' ' + padR('node_mod', 9) + ' ' + padR('lock', 7));
    for (const pm of pms) {
      const m = mp[pm] || {};
      console.log(
        pad(pm, 6) + ' ' +
        padR(m.vol_ctx   ? fmtN(m.vol_ctx)   : '-', 8) + ' ' +
        padR(m.invol_ctx ? fmtN(m.invol_ctx) : '-', 8) + '   ' +
        padR(m.net_rx    ? fmtB(m.net_rx)    : '-', 8) + ' ' +
        padR(m.net_tx    ? fmtB(m.net_tx)    : '-', 8) + '   ' +
        padR(m.foot_cache ? fmtB(m.foot_cache) : '-', 8) + ' ' +
        padR(m.foot_nm    ? fmtB(m.foot_nm)    : '-', 9) + ' ' +
        padR(m.foot_lock  ? fmtB(m.foot_lock)  : '-', 7)
      );
    }
  }
"

# Export raw per-cell results (hyperfine JSON, metrics, footprint, failure
# markers) for the CI comment renderer — keyed by registry host so multi-leg
# runs (npmjs + npmmirror) don't clobber each other across state wipes.
if [ -n "${BENCH_OUT_DIR:-}" ]; then
  REG_LABEL=$(echo "$REGISTRY" | sed -E 's|^https?://||; s|/.*$||')
  EXPORT_DIR="$BENCH_OUT_DIR/results-$REG_LABEL"
  mkdir -p "$EXPORT_DIR"
  cp "$RESULTS_DIR/$PROJECT"_* "$EXPORT_DIR/" 2>/dev/null || true
  echo -e "${GREEN}Exported raw results to $EXPORT_DIR${NC}"
fi

echo -e "${GREEN}Done. Raw results in $RESULTS_DIR${NC}"
