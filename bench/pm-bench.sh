#!/bin/bash
set -eo pipefail

# Usage: pm-bench.sh [registry_mode] [pm_list]
#   $1: both | npmjs | npmmirror (default: both)
#   $2: comma-separated PM list (default: "utoo,bun")

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Arguments
REGISTRY_MODE=${1:-both}
PM_LIST=${2:-utoo,bun}
PNPM_BIN=${PNPM_BIN:-pnpm}
PNPM12_BIN=${PNPM12_BIN:-pnpm12}

# Run counts (configurable via env, default 3)
BENCH_COLD_RUNS=${BENCH_COLD_RUNS:-3}
BENCH_WARM_RUNS=${BENCH_WARM_RUNS:-3}

# Parse PM list into array
IFS=',' read -ra PACKAGE_MANAGERS <<< "$PM_LIST"

# Insert utoo-npm and/or utoo-next before utoo when their binaries are available
if [[ -n "$UTOO_NPM_BIN" && -x "$UTOO_NPM_BIN" ]] || [[ -n "$UTOO_NEXT_BIN" && -x "$UTOO_NEXT_BIN" ]]; then
  NEW_PMS=()
  for pm in "${PACKAGE_MANAGERS[@]}"; do
    if [[ "$pm" == "utoo" ]]; then
      [[ -n "$UTOO_NPM_BIN" && -x "$UTOO_NPM_BIN" ]] && NEW_PMS+=("utoo-npm")
      [[ -n "$UTOO_NEXT_BIN" && -x "$UTOO_NEXT_BIN" ]] && NEW_PMS+=("utoo-next")
    fi
    NEW_PMS+=("$pm")
  done
  PACKAGE_MANAGERS=("${NEW_PMS[@]}")
fi

# Check for required commands (skip utoo-npm/utoo-next — they're paths, not in $PATH)
REQUIRED_CMDS=(git node hyperfine /usr/bin/time)
for pm in "${PACKAGE_MANAGERS[@]}"; do
  case "$pm" in
    utoo-npm|utoo-next) ;;
    pnpm) REQUIRED_CMDS+=("$PNPM_BIN") ;;
    pnpm12) REQUIRED_CMDS+=("$PNPM12_BIN") ;;
    *) REQUIRED_CMDS+=("$pm") ;;
  esac
done

for cmd in "${REQUIRED_CMDS[@]}"; do
  if ! command -v "$cmd" &> /dev/null; then
    echo "Error: Required command '$cmd' not found. Please install it." >&2
    exit 1
  fi
done

# Configuration
PROJECTS=("ant-design" "ant-design-x")
REGISTRIES=()
OS="$(uname)"

if [ "$REGISTRY_MODE" = "both" ] || [ "$REGISTRY_MODE" = "npmjs" ]; then
  REGISTRIES+=("https://registry.npmjs.org")
fi
if [ "$REGISTRY_MODE" = "both" ] || [ "$REGISTRY_MODE" = "npmmirror" ]; then
  REGISTRIES+=("https://registry.npmmirror.com")
fi

RESULTS_DIR="/tmp/pm-bench-results"
LOG_DIR="$RESULTS_DIR/logs"
BENCH_DIR="/tmp/pm-bench"
mkdir -p "$RESULTS_DIR" "$LOG_DIR"

echo -e "${YELLOW}========================================${NC}"
echo -e "${YELLOW}  PM Benchmark Starting...${NC}"
echo -e "${YELLOW}========================================${NC}"
echo -e "Registry mode: ${CYAN}$REGISTRY_MODE${NC}"
echo -e "Projects: ${CYAN}${PROJECTS[*]}${NC}"
echo -e "Package managers: ${CYAN}${PACKAGE_MANAGERS[*]}${NC}"
echo -e "Platform: ${CYAN}$OS${NC}"
echo -e "Cold runs: ${CYAN}$BENCH_COLD_RUNS${NC}, Warm runs: ${CYAN}$BENCH_WARM_RUNS${NC}"
echo ""

# Cache path configuration
UTOO_CACHE_DIR="${UTOO_CACHE_DIR:-$HOME/.cache/nm}"
PNPM_STORE_DIR="${PNPM_STORE_DIR:-/tmp/pnpm-bench-store}"
PNPM12_STORE_DIR="${PNPM12_STORE_DIR:-/tmp/pnpm12-bench-store}"
BUN_INSTALL_DIR="${BUN_INSTALL_DIR:-$HOME/.bun/install}"
# aube: store = tarball CAS ($XDG_DATA_HOME/aube/store/v1/files),
#       cache = packument/manifest + global-links ($XDG_CACHE_HOME/aube)
AUBE_STORE_DIR="${AUBE_STORE_DIR:-$HOME/.local/share/aube}"
AUBE_CACHE_DIR="${AUBE_CACHE_DIR:-$HOME/.cache/aube}"

# ---------------------------------------------------------------------------
# Generate helper scripts (called by hyperfine in subprocesses)
# ---------------------------------------------------------------------------

# 1. prepare.sh — clean local + optionally purge PM global cache
#    Usage: bash prepare.sh <project_dir> <pm> [--cold]
PREPARE_SCRIPT="$RESULTS_DIR/prepare.sh"
cat > "$PREPARE_SCRIPT" << 'PREPARE_EOF'
#!/bin/bash
set -e
PROJECT_DIR="$1"; PM="$2"; COLD="$3"
UTOO_CACHE_DIR="${UTOO_CACHE_DIR:-$HOME/.cache/nm}"
PNPM_BIN="${PNPM_BIN:-pnpm}"
PNPM12_BIN="${PNPM12_BIN:-pnpm12}"
PNPM_STORE_DIR="${PNPM_STORE_DIR:-/tmp/pnpm-bench-store}"
PNPM12_STORE_DIR="${PNPM12_STORE_DIR:-/tmp/pnpm12-bench-store}"
BUN_INSTALL_DIR="${BUN_INSTALL_DIR:-$HOME/.bun/install}"
AUBE_STORE_DIR="${AUBE_STORE_DIR:-$HOME/.local/share/aube}"
AUBE_CACHE_DIR="${AUBE_CACHE_DIR:-$HOME/.cache/aube}"

cd "$PROJECT_DIR" && git clean -dfx

if [ "$COLD" = "--cold" ]; then
  case "$PM" in
    utoo|utoo-npm|utoo-next) rm -rf "$UTOO_CACHE_DIR" ;;
    yarn) yarn cache clean 2>/dev/null || rm -rf ~/.yarn/cache "$(yarn cache dir 2>/dev/null)" ;;
    pnpm) rm -rf "$PNPM_STORE_DIR" ;;
    pnpm12) rm -rf "$PNPM12_STORE_DIR" ;;
    bun)  rm -rf "$BUN_INSTALL_DIR"; bun pm cache rm 2>/dev/null || true ;;
    aube) rm -rf "$AUBE_STORE_DIR" "$AUBE_CACHE_DIR" ;;
  esac
fi

if [ "$PM" = "pnpm" ] || [ "$PM" = "pnpm12" ] || [ "$PM" = "aube" ]; then
  cd "$PROJECT_DIR"
  if [ -f "package.json" ] && grep -q '"workspaces"' package.json; then
    node -e "
      const pkg = require('./package.json');
      const ws = pkg.workspaces || [];
      const p = Array.isArray(ws) ? ws : (ws.packages || []);
      if (p.length > 0) {
        let y = 'packages:\n';
        p.forEach(x => y += '  - \"' + x + '\"\n');
        require('fs').writeFileSync('pnpm-workspace.yaml', y);
      }
    "
  fi
fi
PREPARE_EOF
chmod +x "$PREPARE_SCRIPT"

# 2. metrics_wrapper.sh — run command via /usr/bin/time, append resource metrics to JSONL
#    Usage: bash metrics_wrapper.sh <metrics_jsonl> <cmd_script>
METRICS_WRAPPER="$RESULTS_DIR/metrics_wrapper.sh"
cat > "$METRICS_WRAPPER" << 'METRICS_EOF'
#!/bin/bash
METRICS_FILE="$1"
CMD_SCRIPT="$2"
TIME_TMP=$(mktemp)

if [[ "$(uname)" == "Darwin" ]]; then
  # macOS: /usr/bin/time -l — RSS in bytes
  /usr/bin/time -l bash "$CMD_SCRIPT" 2>"$TIME_TMP"
  EXIT_CODE=$?
  RSS=$(grep 'maximum resident set size' "$TIME_TMP" | awk '{print $1}')
  PAGE_FAULTS=$(grep 'page faults' "$TIME_TMP" | awk '{print $1}')
  VOL_CTX=$(grep 'voluntary context switches' "$TIME_TMP" | grep -v involuntary | awk '{print $1}')
  INVOL_CTX=$(grep 'involuntary context switches' "$TIME_TMP" | awk '{print $1}')
  IO_IN=$(grep 'block input operations' "$TIME_TMP" | awk '{print $1}')
  IO_OUT=$(grep 'block output operations' "$TIME_TMP" | awk '{print $1}')
else
  # Linux: /usr/bin/time -v — RSS in kbytes
  /usr/bin/time -v bash "$CMD_SCRIPT" 2>"$TIME_TMP"
  EXIT_CODE=$?
  RSS_KB=$(grep 'Maximum resident set size' "$TIME_TMP" | awk '{print $NF}')
  RSS=$(( ${RSS_KB:-0} * 1024 ))
  PAGE_FAULTS=$(grep 'Major (requiring I/O) page faults' "$TIME_TMP" | awk '{print $NF}')
  VOL_CTX=$(grep 'Voluntary context switches' "$TIME_TMP" | awk '{print $NF}')
  INVOL_CTX=$(grep 'Involuntary context switches' "$TIME_TMP" | awk '{print $NF}')
  IO_IN=$(grep 'File system inputs' "$TIME_TMP" | awk '{print $NF}')
  IO_OUT=$(grep 'File system outputs' "$TIME_TMP" | awk '{print $NF}')
fi

echo "{\"rss\":${RSS:-0},\"page_faults\":${PAGE_FAULTS:-0},\"vol_ctx\":${VOL_CTX:-0},\"invol_ctx\":${INVOL_CTX:-0},\"io_in\":${IO_IN:-0},\"io_out\":${IO_OUT:-0}}" >> "$METRICS_FILE"

rm -f "$TIME_TMP"
exit $EXIT_CODE
METRICS_EOF
chmod +x "$METRICS_WRAPPER"

# ---------------------------------------------------------------------------
# Helper functions
# ---------------------------------------------------------------------------

# Create pnpm workspace config from package.json
setup_pnpm_workspace() {
  local project_dir=$1
  cd "$project_dir"

  if [ -f "pnpm-workspace.yaml" ]; then
    return
  fi

  if [ -f "package.json" ] && grep -q '"workspaces"' package.json; then
    node -e "
      const pkg = require('./package.json');
      const workspaces = pkg.workspaces || [];
      const patterns = Array.isArray(workspaces) ? workspaces : (workspaces.packages || []);
      if (patterns.length > 0) {
        console.log('packages:');
        patterns.forEach(p => console.log('  - \"' + p + '\"'));
      }
    " > pnpm-workspace.yaml 2>/dev/null || true
  fi
}

# Clone test projects
clone_projects() {
  mkdir -p "$BENCH_DIR"
  cd "$BENCH_DIR"

  echo -e "${YELLOW}Cloning projects...${NC}"

  if [ ! -d "ant-design" ]; then
    echo -e "Cloning ant-design..."
    git clone --depth=1 https://github.com/ant-design/ant-design.git
  else
    echo -e "ant-design already exists, skipping clone"
  fi

  if [ ! -d "ant-design-x" ]; then
    echo -e "Cloning ant-design-x (next branch)..."
    git clone --branch next --depth=1 https://github.com/ant-design/x.git ant-design-x
  else
    echo -e "ant-design-x already exists, skipping clone"
  fi

  for pm in "${PACKAGE_MANAGERS[@]}"; do
    if [ "$pm" = "pnpm" ] || [ "$pm" = "pnpm12" ] || [ "$pm" = "aube" ]; then
      echo -e "${YELLOW}Setting up pnpm-workspace.yaml (shared by pnpm/pnpm12/aube)...${NC}"
      setup_pnpm_workspace "$BENCH_DIR/ant-design"
      setup_pnpm_workspace "$BENCH_DIR/ant-design-x"
      break
    fi
  done

  echo -e "${GREEN}Projects ready${NC}"
  echo ""
}

# Build the install command for a given PM and registry
get_install_cmd() {
  local pm=$1
  local registry=$2
  local cold=$3  # "true" for cold install

  case $pm in
    utoo)
      echo "utoo install --ignore-scripts --registry=$registry"
      ;;
    utoo-npm)
      echo "$UTOO_NPM_BIN install --ignore-scripts --registry=$registry"
      ;;
    utoo-next)
      echo "$UTOO_NEXT_BIN install --ignore-scripts --registry=$registry"
      ;;
    yarn)
      echo "yarn install --ignore-scripts --registry $registry"
      ;;
    pnpm)
      echo "env PNPM_CONFIG_PM_ON_FAIL=ignore $PNPM_BIN install --ignore-scripts --registry $registry --store-dir=$PNPM_STORE_DIR"
      ;;
    pnpm12)
      echo "env PNPM_CONFIG_PM_ON_FAIL=ignore $PNPM12_BIN install --ignore-scripts --registry $registry --store-dir=$PNPM12_STORE_DIR"
      ;;
    bun)
      if [ "$cold" = "true" ]; then
        echo "bun install --ignore-scripts --registry $registry --no-cache"
      else
        echo "bun install --ignore-scripts --registry $registry"
      fi
      ;;
    aube)
      echo "NPM_CONFIG_REGISTRY=$registry aube install --ignore-scripts"
      ;;
  esac
}

# ---------------------------------------------------------------------------
# Benchmark runners
# ---------------------------------------------------------------------------

# Cold install: 1 run per PM, sequential (network-bound, can't be averaged)
run_cold_benchmarks() {
  local project=$1
  local project_dir="$BENCH_DIR/$project"
  local registry=$2
  local reg_short=$3

  echo -e "  ${YELLOW}Cold installs ($BENCH_COLD_RUNS runs each):${NC}"

  for pm in "${PACKAGE_MANAGERS[@]}"; do
    local install_cmd
    install_cmd=$(get_install_cmd "$pm" "$registry" "true")
    local json_file="$RESULTS_DIR/${project}_${reg_short}_cold_${pm}.json"
    local metrics_file="$RESULTS_DIR/${project}_${reg_short}_cold_${pm}_metrics.jsonl"
    local cmd_script="$RESULTS_DIR/cmd_cold_${pm}.sh"

    printf 'set -eo pipefail\ncd %s && %s\n' "$project_dir" "$install_cmd" > "$cmd_script"
    > "$metrics_file"

    echo -e "    ${CYAN}$pm${NC}..."
    hyperfine \
      --runs "$BENCH_COLD_RUNS" \
      --prepare "bash $PREPARE_SCRIPT $project_dir $pm --cold" \
      --export-json "$json_file" \
      -n "$pm" \
      "bash $METRICS_WRAPPER $metrics_file $cmd_script" \
      2>&1 | tail -1 || echo -e "    ${RED}$pm cold install failed${NC}"
  done
}

# Warm install: head-to-head, 3 runs + 1 warmup (global cache pre-populated)
run_warm_benchmarks() {
  local project=$1
  local project_dir="$BENCH_DIR/$project"
  local registry=$2
  local reg_short=$3

  echo -e "  ${YELLOW}Warm installs ($BENCH_WARM_RUNS runs, 1 warmup):${NC}"

  # Pre-populate global caches: one install per PM
  for pm in "${PACKAGE_MANAGERS[@]}"; do
    local install_cmd
    install_cmd=$(get_install_cmd "$pm" "$registry" "false")
    local prepop_log="$LOG_DIR/${project}_${reg_short}_prepopulate_${pm}.log"
    cd "$project_dir"
    git clean -dfx
    if [ "$pm" = "pnpm" ] || [ "$pm" = "pnpm12" ] || [ "$pm" = "aube" ]; then
      setup_pnpm_workspace "$project_dir"
    fi
    echo -e "    ${CYAN}Pre-populating $pm cache...${NC}"
    if ! eval "$install_cmd" > "$prepop_log" 2>&1; then
      echo -e "    ${RED}$pm cache pre-populate failed${NC}"
    fi
  done

  # Build hyperfine args
  local json_file="$RESULTS_DIR/${project}_${reg_short}_warm.json"
  local hyperfine_args=(
    --warmup 1
    --runs "$BENCH_WARM_RUNS"
    --export-json "$json_file"
  )

  for pm in "${PACKAGE_MANAGERS[@]}"; do
    local install_cmd
    install_cmd=$(get_install_cmd "$pm" "$registry" "false")
    local metrics_file="$RESULTS_DIR/${project}_${reg_short}_warm_${pm}_metrics.jsonl"
    local cmd_script="$RESULTS_DIR/cmd_warm_${pm}.sh"

    printf 'set -eo pipefail\ncd %s && %s\n' "$project_dir" "$install_cmd" > "$cmd_script"
    > "$metrics_file"

    hyperfine_args+=(-n "$pm" --prepare "bash $PREPARE_SCRIPT $project_dir $pm" "bash $METRICS_WRAPPER $metrics_file $cmd_script")
  done

  hyperfine "${hyperfine_args[@]}" 2>&1 || echo -e "  ${RED}Warm benchmark failed${NC}"
}

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------

print_results() {
  echo ""
  echo -e "${YELLOW}========================================${NC}"
  echo -e "${YELLOW}  Benchmark Results${NC}"
  echo -e "${YELLOW}========================================${NC}"
  echo ""

  node -e "
    const fs = require('fs');
    const path = require('path');
    const dir = process.env.RESULTS_DIR;
    const jsonFiles = fs.readdirSync(dir).filter(f => f.endsWith('.json'));
    const metricsFiles = fs.readdirSync(dir).filter(f => f.endsWith('_metrics.jsonl'));

    // --- Parse hyperfine timing ---
    const timeRows = [];
    for (const file of jsonFiles) {
      const data = JSON.parse(fs.readFileSync(path.join(dir, file), 'utf8'));
      const base = file.replace('.json', '');
      const parts = base.split('_');
      const typeIdx = parts.findIndex(p => p === 'cold' || p === 'warm');
      if (typeIdx === -1) continue;
      const project = parts.slice(0, typeIdx - 1).join('-');
      const registry = parts[typeIdx - 1];
      const type = parts[typeIdx];
      for (const result of data.results) {
        timeRows.push({
          project, registry, type,
          pm: result.command,
          mean: result.mean,
          stddev: result.stddev,
        });
      }
    }

    // --- Parse resource metrics ---
    const metricsMap = {};  // key: project|registry|type|pm -> averaged metrics
    for (const file of metricsFiles) {
      const base = file.replace('_metrics.jsonl', '');
      const parts = base.split('_');
      const typeIdx = parts.findIndex(p => p === 'cold' || p === 'warm');
      if (typeIdx === -1) continue;
      const project = parts.slice(0, typeIdx - 1).join('-');
      const registry = parts[typeIdx - 1];
      const type = parts[typeIdx];
      const pm = parts.slice(typeIdx + 1).join('-');

      const lines = fs.readFileSync(path.join(dir, file), 'utf8')
        .trim().split('\n').filter(Boolean);
      if (lines.length === 0) continue;

      const entries = lines.map(l => JSON.parse(l));
      const avg = {};
      for (const key of ['rss', 'page_faults', 'vol_ctx', 'invol_ctx', 'io_in', 'io_out']) {
        avg[key] = Math.round(entries.reduce((s, e) => s + (e[key] || 0), 0) / entries.length);
      }
      metricsMap[project + '|' + registry + '|' + type + '|' + pm] = avg;
    }

    if (timeRows.length === 0) {
      console.log('No results found.');
      process.exit(0);
    }

    const pms = [...new Set(timeRows.map(r => r.pm))];
    const fmtBytes = (b) => {
      if (b >= 1024 * 1024 * 1024) return (b / 1024 / 1024 / 1024).toFixed(1) + 'G';
      if (b >= 1024 * 1024) return (b / 1024 / 1024).toFixed(0) + 'M';
      if (b >= 1024) return (b / 1024).toFixed(0) + 'K';
      return b + 'B';
    };

    // ---- Timing table ----
    console.log('## Timing\n');
    const hdr = '| ' + 'Project'.padEnd(14) + ' | ' + 'Registry'.padEnd(12) + ' | ' + 'Type'.padEnd(6) + ' | '
      + pms.map(p => p.padStart(16)).join(' | ') + ' |';
    const sep = '|' + '-'.repeat(16) + '|' + '-'.repeat(14) + '|' + '-'.repeat(8) + '|'
      + pms.map(() => '-'.repeat(18)).join('|') + '|';
    console.log(hdr);
    console.log(sep);

    const timeGroups = {};
    for (const r of timeRows) {
      const key = r.project + '|' + r.registry + '|' + r.type;
      if (!timeGroups[key]) timeGroups[key] = {};
      const fmt = r.stddev > 0.005
        ? r.mean.toFixed(2) + 's +/- ' + r.stddev.toFixed(2)
        : r.mean.toFixed(2) + 's';
      timeGroups[key][r.pm] = fmt;
    }
    for (const [key, pmResults] of Object.entries(timeGroups)) {
      const [project, registry, type] = key.split('|');
      const cells = pms.map(p => (pmResults[p] || '-').padStart(16)).join(' | ');
      console.log('| ' + project.padEnd(14) + ' | ' + registry.padEnd(12) + ' | ' + type.padEnd(6) + ' | ' + cells + ' |');
    }

    // ---- Resource table ----
    const hasMetrics = Object.keys(metricsMap).length > 0;
    if (hasMetrics) {
      console.log('\n## Resources (avg across runs)\n');
      console.log('| ' + 'Project'.padEnd(14) + ' | ' + 'Registry'.padEnd(12) + ' | ' + 'Type'.padEnd(6)
        + ' | ' + 'PM'.padEnd(6)
        + ' | ' + 'RSS'.padStart(7)
        + ' | ' + 'IO r/w'.padStart(14)
        + ' | ' + 'Ctx v/i'.padStart(14)
        + ' | ' + 'PgFault'.padStart(8) + ' |');
      console.log('|' + '-'.repeat(16) + '|' + '-'.repeat(14) + '|' + '-'.repeat(8)
        + '|' + '-'.repeat(8)
        + '|' + '-'.repeat(9)
        + '|' + '-'.repeat(16)
        + '|' + '-'.repeat(16)
        + '|' + '-'.repeat(10) + '|');

      // Sort keys for consistent output
      const sortedKeys = Object.keys(metricsMap).sort();
      for (const key of sortedKeys) {
        const [project, registry, type, pm] = key.split('|');
        const m = metricsMap[key];
        console.log('| ' + project.padEnd(14)
          + ' | ' + registry.padEnd(12)
          + ' | ' + type.padEnd(6)
          + ' | ' + pm.padEnd(6)
          + ' | ' + fmtBytes(m.rss).padStart(7)
          + ' | ' + (m.io_in + '/' + m.io_out).padStart(14)
          + ' | ' + (m.vol_ctx + '/' + m.invol_ctx).padStart(14)
          + ' | ' + String(m.page_faults).padStart(8)
          + ' |');
      }
    }

    console.log('\nRaw results saved to: ' + dir + '/');
  " 2>&1

  echo ""
  echo -e "${GREEN}Benchmark completed!${NC}"
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

main() {
  rm -f "$RESULTS_DIR"/*.json "$RESULTS_DIR"/*.jsonl
  rm -rf "$LOG_DIR" && mkdir -p "$LOG_DIR"

  clone_projects

  for project in "${PROJECTS[@]}"; do
    for registry in "${REGISTRIES[@]}"; do
      local reg_short
      reg_short=$(echo "$registry" | sed 's|https://registry\.||;s|\.org||;s|\.com||')

      echo -e "${YELLOW}----------------------------------------${NC}"
      echo -e "${YELLOW}Project: ${CYAN}$project${NC}"
      echo -e "${YELLOW}Registry: ${CYAN}$reg_short${NC}"
      echo -e "${YELLOW}----------------------------------------${NC}"

      run_cold_benchmarks "$project" "$registry" "$reg_short"
      run_warm_benchmarks "$project" "$registry" "$reg_short"

      echo ""
    done
  done

  export RESULTS_DIR
  print_results
}

main
