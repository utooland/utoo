#!/bin/bash
# Phase-isolated cold-install bench: resolve vs cold-install vs warm-link.
# utoo vs bun only (they're the PMs with a true --lockfile-only mode).
set -eo pipefail

# --- config ---
PROJECT=${PROJECT:-ant-design}
REGISTRY=${REGISTRY:-https://registry.npmjs.org}
RUNS=${BENCH_RUNS:-3}
IFS=',' read -ra PACKAGE_MANAGERS <<< "${PM_LIST:-utoo,bun}"

BENCH_DIR=${BENCH_DIR:-/tmp/pm-bench}
RESULTS_DIR=${RESULTS_DIR:-/tmp/pm-bench-results}
mkdir -p "$BENCH_DIR" "$RESULTS_DIR"

# Explicit bench-scoped cache dirs so rm -rf is unambiguous and we don't
# depend on what each PM considers the default (which varies by HOME, OS
# image, and previously-set user config).
UTOO_CACHE="${UTOO_CACHE:-/tmp/utoo-bench-cache}"
BUN_CACHE="${BUN_CACHE:-/tmp/bun-bench-cache}"
export BUN_INSTALL_CACHE_DIR="$BUN_CACHE"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
RED='\033[0;31m'
NC='\033[0m'

banner() { echo -e "${YELLOW}=== $* ===${NC}"; }

# --- metrics wrapper ---
# Wraps each benchmark iteration in /usr/bin/time and appends a one-line
# JSON record with RSS / context switches / page faults / IO counts. One
# file per (phase, pm); hyperfine calls it once per run.
METRICS_WRAPPER="$RESULTS_DIR/metrics_wrapper.sh"
cat > "$METRICS_WRAPPER" <<'METRICS_EOF'
#!/bin/bash
METRICS_FILE="$1"; shift
TIME_TMP=$(mktemp)

if [[ "$(uname)" == "Darwin" ]]; then
  /usr/bin/time -l "$@" 2>"$TIME_TMP"
  EXIT_CODE=$?
  RSS=$(awk '/maximum resident set size/ {print $1}' "$TIME_TMP")
  PAGE_FAULTS=$(awk '/page faults/ {print $1}' "$TIME_TMP")
  VOL_CTX=$(awk '/ voluntary context switches/ {print $1}' "$TIME_TMP")
  INVOL_CTX=$(awk '/involuntary context switches/ {print $1}' "$TIME_TMP")
  IO_IN=0
  IO_OUT=0
else
  /usr/bin/time -v "$@" 2>"$TIME_TMP"
  EXIT_CODE=$?
  RSS_KB=$(awk '/Maximum resident set size/ {print $NF}' "$TIME_TMP")
  RSS=$(( ${RSS_KB:-0} * 1024 ))
  PAGE_FAULTS=$(awk '/Major \(requiring I\/O\) page faults/ {print $NF}' "$TIME_TMP")
  VOL_CTX=$(awk '/Voluntary context switches/ {print $NF}' "$TIME_TMP")
  INVOL_CTX=$(awk '/Involuntary context switches/ {print $NF}' "$TIME_TMP")
  IO_IN=$(awk '/File system inputs/ {print $NF}' "$TIME_TMP")
  IO_OUT=$(awk '/File system outputs/ {print $NF}' "$TIME_TMP")
fi

echo "{\"rss\":${RSS:-0},\"page_faults\":${PAGE_FAULTS:-0},\"vol_ctx\":${VOL_CTX:-0},\"invol_ctx\":${INVOL_CTX:-0},\"io_in\":${IO_IN:-0},\"io_out\":${IO_OUT:-0}}" >> "$METRICS_FILE"
rm -f "$TIME_TMP"
exit $EXIT_CODE
METRICS_EOF
chmod +x "$METRICS_WRAPPER"

# --- project ---
banner "Preparing $PROJECT"
cd "$BENCH_DIR"
if [ ! -d "$PROJECT" ]; then
  git clone --depth=1 "https://github.com/ant-design/${PROJECT}.git" "$PROJECT"
fi
PROJECT_DIR="$BENCH_DIR/$PROJECT"

# --- prepare script builder ---
# Emit a self-contained reset script that hyperfine's --prepare can re-exec.
# The script always prints what it deleted so the CI log proves prepare ran.
write_prepare() {
  local path=$1 phase=$2 pm=$3
  local cache
  case "$pm" in utoo) cache=$UTOO_CACHE ;; bun) cache=$BUN_CACHE ;; esac

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
    p1_*)
      # Phase 1: cold resolve — wipe lockfiles AND caches so nothing can be reused.
      cat >> "$path" <<EOF
rm -f package-lock.json bun.lock yarn.lock pnpm-lock.yaml
rm -rf "$UTOO_CACHE" "$BUN_CACHE"
echo "[prep] phase 1 $pm: cleaned lockfiles + caches + node_modules"
EOF
      ;;
    p3_*)
      # Phase 3: cold install — keep THIS pm's lockfile, wipe THIS pm's cache.
      # Delete the other pm's lockfile so bun doesn't try to migrate utoo's
      # package-lock.json (and vice versa).
      case "$pm" in
        utoo) cat >> "$path" <<EOF
rm -f bun.lock yarn.lock pnpm-lock.yaml
rm -rf "$UTOO_CACHE"
echo "[prep] phase 3 utoo: kept package-lock.json, wiped $UTOO_CACHE"
EOF
          ;;
        bun) cat >> "$path" <<EOF
rm -f package-lock.json yarn.lock pnpm-lock.yaml
rm -rf "$BUN_CACHE"
echo "[prep] phase 3 bun: kept bun.lock, wiped $BUN_CACHE"
EOF
          ;;
      esac
      ;;
    p4_*)
      # Phase 4: warm link — keep lockfile AND cache, only drop node_modules.
      case "$pm" in
        utoo) cat >> "$path" <<EOF
rm -f bun.lock yarn.lock pnpm-lock.yaml
echo "[prep] phase 4 utoo: kept package-lock.json + cache"
EOF
          ;;
        bun) cat >> "$path" <<EOF
rm -f package-lock.json yarn.lock pnpm-lock.yaml
echo "[prep] phase 4 bun: kept bun.lock + cache"
EOF
          ;;
      esac
      ;;
  esac

  chmod +x "$path"
}

# Seed PM-specific state before a phase runs (lockfile + cache where needed).
# This runs ONCE before hyperfine starts, not per-iteration.
seed_for_phase() {
  local phase=$1 pm=$2
  cd "$PROJECT_DIR"
  case "$phase:$pm" in
    p3_*:utoo|p4_*:utoo)
      if [ ! -f package-lock.json ]; then
        echo -e "  ${CYAN}seed: running \`utoo deps\` to generate package-lock.json${NC}"
        utoo deps --registry="$REGISTRY" --cache-dir="$UTOO_CACHE" > "$RESULTS_DIR/seed_${phase}_${pm}.log" 2>&1
      fi
      ;;
    p3_*:bun|p4_*:bun)
      if [ ! -f bun.lock ]; then
        echo -e "  ${CYAN}seed: running \`bun install --lockfile-only\` to generate bun.lock${NC}"
        rm -f package-lock.json
        bun install --lockfile-only --registry="$REGISTRY" > "$RESULTS_DIR/seed_${phase}_${pm}.log" 2>&1
      fi
      ;;
  esac
  # Phase 4 also needs a pre-warmed cache.
  if [[ "$phase" == p4_* ]]; then
    local cache
    case "$pm" in utoo) cache=$UTOO_CACHE ;; bun) cache=$BUN_CACHE ;; esac
    if [ ! -d "$cache" ] || [ -z "$(ls -A "$cache" 2>/dev/null)" ]; then
      echo -e "  ${CYAN}seed: warming $pm cache via full install${NC}"
      rm -rf node_modules
      case "$pm" in
        utoo) utoo install --ignore-scripts --registry="$REGISTRY" --cache-dir="$UTOO_CACHE" > "$RESULTS_DIR/seed_warmup_${pm}.log" 2>&1 ;;
        bun)  bun  install --ignore-scripts --registry="$REGISTRY" > "$RESULTS_DIR/seed_warmup_${pm}.log" 2>&1 ;;
      esac
      rm -rf node_modules
    fi
  fi
}

install_cmd() {
  case "$1" in
    utoo) echo "utoo install --ignore-scripts --registry=$REGISTRY --cache-dir=$UTOO_CACHE" ;;
    bun)  echo "bun install --ignore-scripts --registry=$REGISTRY" ;;
  esac
}

resolve_cmd() {
  case "$1" in
    utoo) echo "utoo deps --registry=$REGISTRY --cache-dir=$UTOO_CACHE" ;;
    bun)  echo "bun install --lockfile-only --registry=$REGISTRY" ;;
  esac
}

run_phase() {
  local phase=$1 pm=$2 cmd=$3
  local json="$RESULTS_DIR/${PROJECT}_${phase}_${pm}.json"
  local metrics="$RESULTS_DIR/${PROJECT}_${phase}_${pm}_metrics.jsonl"
  local prep_script="$RESULTS_DIR/prep_${phase}_${pm}.sh"
  local cmd_script="$RESULTS_DIR/cmd_${phase}_${pm}.sh"

  seed_for_phase "$phase" "$pm"
  write_prepare "$prep_script" "$phase" "$pm"

  # Reset metrics file; wrap the bench command so /usr/bin/time can measure it.
  : > "$metrics"
  printf 'set -eo pipefail\ncd %s\n%s\n' "$PROJECT_DIR" "$cmd" > "$cmd_script"

  echo -e "  ${CYAN}$pm${NC} · $phase"
  if ! hyperfine \
    --runs "$RUNS" \
    --prepare "bash $prep_script" \
    --export-json "$json" \
    --show-output \
    -n "${pm}-${phase}" \
    "bash $METRICS_WRAPPER $metrics bash $cmd_script"; then
    echo -e "  ${RED}$pm $phase failed${NC}"
  fi
}

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
  const order = ['p1_resolve', 'p3_cold_install', 'p4_warm_link'];
  const timing = {};    // phase -> pm -> {mean,stddev,min,max}
  const metrics = {};   // phase -> pm -> {rss,vol_ctx,invol_ctx,page_faults,io_in,io_out}

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

  for (const f of fs.readdirSync(dir).filter(x => x.endsWith('.json') && !x.endsWith('_metrics.jsonl'))) {
    const key = parseKey(f, '.json');
    if (!key) continue;
    let data; try { data = JSON.parse(fs.readFileSync(path.join(dir, f), 'utf8')); } catch (_) { continue; }
    const r = data.results[0];
    if (!r) continue;
    (timing[key.phase] ??= {})[key.pm] = { mean: r.mean, stddev: r.stddev, min: r.min, max: r.max };
  }

  for (const f of fs.readdirSync(dir).filter(x => x.endsWith('_metrics.jsonl'))) {
    const key = parseKey(f, '_metrics.jsonl');
    if (!key) continue;
    const lines = fs.readFileSync(path.join(dir, f), 'utf8').trim().split('\n').filter(Boolean);
    const rows = [];
    for (const l of lines) { try { rows.push(JSON.parse(l)); } catch (_) {} }
    if (rows.length === 0) continue;
    const avg = {};
    for (const k of ['rss','page_faults','vol_ctx','invol_ctx','io_in','io_out']) {
      avg[k] = Math.round(rows.reduce((s,e)=>s+(e[k]||0),0) / rows.length);
    }
    (metrics[key.phase] ??= {})[key.pm] = avg;
  }

  const pad = (s, n) => String(s).padEnd(n);
  const padR = (s, n) => String(s).padStart(n);
  const fmtB = b => b >= 1<<30 ? (b/(1<<30)).toFixed(1)+'G' : b >= 1<<20 ? Math.round(b/(1<<20))+'M' : b >= 1<<10 ? Math.round(b/(1<<10))+'K' : b+'B';

  for (const phase of order) {
    const tp = timing[phase] || {}, mp = metrics[phase] || {};
    const pms = [...new Set([...Object.keys(tp), ...Object.keys(mp)])];
    if (pms.length === 0) continue;
    console.log('\n## ' + phase);
    console.log(pad('PM', 8) + ' ' + padR('mean', 8) + ' ' + padR('stddev', 7) + '   ' + padR('RSS', 6) + '  ' + padR('vCtx', 8) + '  ' + padR('iCtx', 8) + '  ' + padR('pgFlt', 7) + '  ' + padR('ioIn', 8) + '  ' + padR('ioOut', 8));
    for (const pm of pms) {
      const t = tp[pm], m = mp[pm] || {};
      const tstr = t ? padR(t.mean.toFixed(2)+'s', 8) + ' ' + padR(t.stddev.toFixed(2)+'s', 7) : padR('-', 8) + ' ' + padR('-', 7);
      console.log(
        pad(pm, 8) + ' ' + tstr + '   ' +
        padR(m.rss ? fmtB(m.rss) : '-', 6) + '  ' +
        padR(m.vol_ctx ?? '-', 8) + '  ' +
        padR(m.invol_ctx ?? '-', 8) + '  ' +
        padR(m.page_faults ?? '-', 7) + '  ' +
        padR(m.io_in ?? '-', 8) + '  ' +
        padR(m.io_out ?? '-', 8)
      );
    }
  }
"

echo -e "${GREEN}Done. Raw results in $RESULTS_DIR${NC}"
