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
    p1)
      # Phase 1: cold resolve — wipe lockfiles AND caches so nothing can be reused.
      cat >> "$path" <<EOF
rm -f package-lock.json bun.lock yarn.lock pnpm-lock.yaml
rm -rf "$UTOO_CACHE" "$BUN_CACHE"
echo "[prep] phase 1 $pm: cleaned lockfiles + caches + node_modules"
EOF
      ;;
    p3)
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
    p4)
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
    p3:utoo|p4:utoo)
      if [ ! -f package-lock.json ]; then
        echo -e "  ${CYAN}seed: running \`utoo deps\` to generate package-lock.json${NC}"
        utoo deps --registry="$REGISTRY" --cache-dir="$UTOO_CACHE" > "$RESULTS_DIR/seed_${phase}_${pm}.log" 2>&1
      fi
      ;;
    p3:bun|p4:bun)
      if [ ! -f bun.lock ]; then
        echo -e "  ${CYAN}seed: running \`bun install --lockfile-only\` to generate bun.lock${NC}"
        rm -f package-lock.json
        bun install --lockfile-only --registry="$REGISTRY" > "$RESULTS_DIR/seed_${phase}_${pm}.log" 2>&1
      fi
      ;;
  esac
  # Phase 4 also needs a pre-warmed cache.
  if [ "$phase" = "p4" ]; then
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
  local prep_script="$RESULTS_DIR/prep_${phase}_${pm}.sh"

  seed_for_phase "$phase" "$pm"
  write_prepare "$prep_script" "$phase" "$pm"

  echo -e "  ${CYAN}$pm${NC} · $phase · cmd: $cmd"
  if ! hyperfine \
    --runs "$RUNS" \
    --prepare "bash $prep_script" \
    --export-json "$json" \
    --show-output \
    -n "${pm}-${phase}" \
    "bash -c 'cd $PROJECT_DIR && $cmd'"; then
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
  const byPhase = Object.fromEntries(order.map(p => [p, []]));
  for (const f of fs.readdirSync(dir).filter(x => x.endsWith('.json'))) {
    let data;
    try { data = JSON.parse(fs.readFileSync(path.join(dir, f), 'utf8')); }
    catch (_) { continue; }
    for (const r of data.results) {
      const phase = order.find(p => r.command.includes(p) || f.includes(p));
      if (!phase) continue;
      byPhase[phase].push({ name: r.command, mean: r.mean, stddev: r.stddev, min: r.min, max: r.max });
    }
  }
  const pad = (s, n) => String(s).padEnd(n);
  const padR = (s, n) => String(s).padStart(n);
  for (const phase of order) {
    if (byPhase[phase].length === 0) continue;
    console.log('\n## ' + phase);
    console.log(pad('PM', 26) + ' ' + padR('mean', 9) + '   ' + padR('stddev', 7) + '   ' + padR('min', 7) + '   ' + padR('max', 7));
    for (const r of byPhase[phase]) {
      console.log(
        pad(r.name, 26) + ' ' +
        padR(r.mean.toFixed(2) + 's', 9) + '   ' +
        padR(r.stddev.toFixed(2) + 's', 7) + '   ' +
        padR(r.min.toFixed(2) + 's', 7) + '   ' +
        padR(r.max.toFixed(2) + 's', 7)
      );
    }
  }
"

echo -e "${GREEN}Done. Raw results in $RESULTS_DIR${NC}"
