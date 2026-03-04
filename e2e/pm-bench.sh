#!/bin/bash
set -e

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

# Parse PM list into array
IFS=',' read -ra PACKAGE_MANAGERS <<< "$PM_LIST"

# Check for required commands
REQUIRED_CMDS=(git node hyperfine)
for pm in "${PACKAGE_MANAGERS[@]}"; do
  REQUIRED_CMDS+=("$pm")
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

if [ "$REGISTRY_MODE" = "both" ] || [ "$REGISTRY_MODE" = "npmjs" ]; then
  REGISTRIES+=("https://registry.npmjs.org")
fi
if [ "$REGISTRY_MODE" = "both" ] || [ "$REGISTRY_MODE" = "npmmirror" ]; then
  REGISTRIES+=("https://registry.npmmirror.com")
fi

RESULTS_DIR="/tmp/pm-bench-results"
BENCH_DIR="/tmp/pm-bench"
mkdir -p "$RESULTS_DIR"

echo -e "${YELLOW}========================================${NC}"
echo -e "${YELLOW}  PM Benchmark Starting...${NC}"
echo -e "${YELLOW}========================================${NC}"
echo -e "Registry mode: ${CYAN}$REGISTRY_MODE${NC}"
echo -e "Projects: ${CYAN}${PROJECTS[*]}${NC}"
echo -e "Package managers: ${CYAN}${PACKAGE_MANAGERS[*]}${NC}"
echo ""

# Create pnpm workspace config from package.json
setup_pnpm_workspace() {
  local project_dir=$1
  cd "$project_dir"

  # Skip if pnpm-workspace.yaml already exists
  if [ -f "pnpm-workspace.yaml" ]; then
    return
  fi

  # Read workspaces from package.json and generate pnpm-workspace.yaml
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

  # Setup pnpm workspace configs
  for pm in "${PACKAGE_MANAGERS[@]}"; do
    if [ "$pm" = "pnpm" ]; then
      echo -e "${YELLOW}Setting up pnpm workspace configs...${NC}"
      setup_pnpm_workspace "$BENCH_DIR/ant-design"
      setup_pnpm_workspace "$BENCH_DIR/ant-design-x"
      break
    fi
  done

  echo -e "${GREEN}Projects ready${NC}"
  echo ""
}

# Clean lock files and node_modules using git clean
clean_local() {
  git clean -dfx
}

# Cache path configuration
UTOO_CACHE_DIR="${UTOO_CACHE_DIR:-$HOME/.cache/nm}"
PNPM_STORE_DIR="${PNPM_STORE_DIR:-$(pnpm store path 2>/dev/null || echo "$HOME/.pnpm-store")}"
BUN_INSTALL_DIR="${BUN_INSTALL_DIR:-$HOME/.bun/install}"

# Clean global cache for each package manager (including manifest and tgz)
clean_pm_cache() {
  local pm=$1
  case $pm in
    utoo)
      rm -rf "$UTOO_CACHE_DIR"
      ;;
    yarn)
      yarn cache clean 2>/dev/null || rm -rf ~/.yarn/cache "$(yarn cache dir 2>/dev/null)"
      ;;
    pnpm)
      pnpm store prune 2>/dev/null || rm -rf "$PNPM_STORE_DIR"
      ;;
    bun)
      rm -rf "$BUN_INSTALL_DIR"
      bun pm cache rm 2>/dev/null || true
      ;;
  esac
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
    yarn)
      echo "yarn install --ignore-scripts --registry $registry"
      ;;
    pnpm)
      echo "npm_config_package_manager_strict=false pnpm install --ignore-scripts --registry $registry"
      ;;
    bun)
      if [ "$cold" = "true" ]; then
        echo "bun install --ignore-scripts --registry $registry --no-cache"
      else
        echo "bun install --ignore-scripts --registry $registry"
      fi
      ;;
  esac
}

# Build the prepare command for warm install (clean local + setup)
get_warm_prepare() {
  local project_dir=$1
  local pm=$2

  local prepare="cd $project_dir && git clean -dfx"
  if [ "$pm" = "pnpm" ]; then
    # Inline pnpm workspace setup since git clean deletes it
    prepare="$prepare && cd $project_dir && node -e \"
      const pkg = require('./package.json');
      const ws = pkg.workspaces || [];
      const p = Array.isArray(ws) ? ws : (ws.packages || []);
      if (p.length > 0) {
        let y = 'packages:\\n';
        p.forEach(x => y += '  - \\\"' + x + '\\\"\\n');
        require('fs').writeFileSync('pnpm-workspace.yaml', y);
      }
    \""
  fi
  echo "$prepare"
}

# Run cold install benchmarks (1 run each, sequential per PM — network-bound)
run_cold_benchmarks() {
  local project=$1
  local project_dir="$BENCH_DIR/$project"
  local registry=$2
  local reg_short=$3

  echo -e "  ${YELLOW}Cold installs (1 run each):${NC}"

  for pm in "${PACKAGE_MANAGERS[@]}"; do
    local install_cmd
    install_cmd=$(get_install_cmd "$pm" "$registry" "true")
    local json_file="$RESULTS_DIR/${project}_${reg_short}_cold_${pm}.json"

    # Build prepare: clean local + clean PM cache + setup
    local prepare="cd $project_dir && git clean -dfx"
    # Clean PM cache
    case $pm in
      utoo) prepare="$prepare && rm -rf ${UTOO_CACHE_DIR}" ;;
      pnpm) prepare="$prepare && (pnpm store prune 2>/dev/null || rm -rf ${PNPM_STORE_DIR})" ;;
      bun)  prepare="$prepare && rm -rf ${BUN_INSTALL_DIR} && (bun pm cache rm 2>/dev/null || true)" ;;
      yarn) prepare="$prepare && (yarn cache clean 2>/dev/null || true)" ;;
    esac
    # Recreate pnpm workspace if needed
    if [ "$pm" = "pnpm" ]; then
      prepare="$prepare && cd $project_dir && node -e \"
        const pkg = require('./package.json');
        const ws = pkg.workspaces || [];
        const p = Array.isArray(ws) ? ws : (ws.packages || []);
        if (p.length > 0) {
          let y = 'packages:\\n';
          p.forEach(x => y += '  - \\\"' + x + '\\\"\\n');
          require('fs').writeFileSync('pnpm-workspace.yaml', y);
        }
      \""
    fi

    echo -e "    ${CYAN}$pm${NC}..."
    hyperfine \
      --runs 1 \
      --prepare "$prepare" \
      --export-json "$json_file" \
      -n "$pm" \
      "cd $project_dir && $install_cmd" \
      2>&1 | tail -1 || echo -e "    ${RED}$pm cold install failed${NC}"
  done
}

# Run warm install benchmarks (head-to-head, 3 runs with 1 warmup)
run_warm_benchmarks() {
  local project=$1
  local project_dir="$BENCH_DIR/$project"
  local registry=$2
  local reg_short=$3

  echo -e "  ${YELLOW}Warm installs (3 runs, 1 warmup):${NC}"

  # Pre-populate caches: run one cold install per PM (with cache, no --no-cache)
  for pm in "${PACKAGE_MANAGERS[@]}"; do
    local install_cmd
    install_cmd=$(get_install_cmd "$pm" "$registry" "false")
    cd "$project_dir"
    clean_local
    if [ "$pm" = "pnpm" ]; then
      setup_pnpm_workspace "$project_dir"
    fi
    echo -e "    ${CYAN}Pre-populating $pm cache...${NC}"
    eval "$install_cmd" >/dev/null 2>&1 || echo -e "    ${RED}$pm cache pre-populate failed${NC}"
  done

  # Build hyperfine command args
  local json_file="$RESULTS_DIR/${project}_${reg_short}_warm.json"
  local hyperfine_args=(
    --warmup 1
    --runs 3
    --export-json "$json_file"
  )

  for pm in "${PACKAGE_MANAGERS[@]}"; do
    local install_cmd
    install_cmd=$(get_install_cmd "$pm" "$registry" "false")
    local prepare
    prepare=$(get_warm_prepare "$project_dir" "$pm")

    hyperfine_args+=(-n "$pm" --prepare "$prepare" "cd $project_dir && $install_cmd")
  done

  hyperfine "${hyperfine_args[@]}" 2>&1 || echo -e "  ${RED}Warm benchmark failed${NC}"
}

# Print summary table from hyperfine JSON results
print_results() {
  echo ""
  echo -e "${YELLOW}========================================${NC}"
  echo -e "${YELLOW}  Benchmark Results${NC}"
  echo -e "${YELLOW}========================================${NC}"
  echo ""

  node -e "
    const fs = require('fs');
    const path = require('path');
    const dir = '$RESULTS_DIR';
    const files = fs.readdirSync(dir).filter(f => f.endsWith('.json'));

    const rows = [];

    for (const file of files) {
      const data = JSON.parse(fs.readFileSync(path.join(dir, file), 'utf8'));
      // Parse filename: {project}_{registry}_{type}[_{pm}].json
      const base = file.replace('.json', '');
      const parts = base.split('_');

      // Handle project names with hyphens (e.g., ant-design, ant-design-x)
      // Format: {project}_{registry}_{type}[_{pm}]
      // Registry is always the second-to-last or third-to-last segment
      // Type is 'cold' or 'warm'
      let project, registry, type, pmFromFile;

      // Find 'cold' or 'warm' in parts to determine split point
      const typeIdx = parts.findIndex(p => p === 'cold' || p === 'warm');
      if (typeIdx === -1) continue;

      project = parts.slice(0, typeIdx - 1).join('-');
      registry = parts[typeIdx - 1];
      type = parts[typeIdx];
      pmFromFile = parts.slice(typeIdx + 1).join('-') || null;

      for (const result of data.results) {
        const pm = result.command;
        const mean = result.mean;
        const stddev = result.stddev;
        rows.push({ project, registry, type, pm, mean, stddev });
      }
    }

    if (rows.length === 0) {
      console.log('No results found.');
      process.exit(0);
    }

    // Collect all PMs
    const pms = [...new Set(rows.map(r => r.pm))];

    // Header
    const pmHeaders = pms.map(p => p.padStart(12)).join(' |');
    const header = '| ' + 'Project'.padEnd(14) + ' | ' + 'Registry'.padEnd(10) + ' | ' + 'Type'.padEnd(6) + ' |' + pmHeaders + ' |';
    const sep = '|' + '-'.repeat(16) + '|' + '-'.repeat(12) + '|' + '-'.repeat(8) + '|' + pms.map(() => '-'.repeat(13)).join('|') + '|';

    console.log(header);
    console.log(sep);

    // Group by project + registry + type
    const groups = {};
    for (const r of rows) {
      const key = r.project + '|' + r.registry + '|' + r.type;
      if (!groups[key]) groups[key] = {};
      const fmt = r.stddev > 0
        ? r.mean.toFixed(2) + 's +/- ' + r.stddev.toFixed(2)
        : r.mean.toFixed(2) + 's';
      groups[key][r.pm] = fmt;
    }

    for (const [key, pmResults] of Object.entries(groups)) {
      const [project, registry, type] = key.split('|');
      const pmCells = pms.map(p => (pmResults[p] || '-').padStart(12)).join(' |');
      console.log('| ' + project.padEnd(14) + ' | ' + registry.padEnd(10) + ' | ' + type.padEnd(6) + ' |' + pmCells + ' |');
    }

    console.log('');
    console.log('Raw JSON results saved to: $RESULTS_DIR/');
  "

  echo ""
  echo -e "${GREEN}Benchmark completed!${NC}"
}

# Main flow
main() {
  # Clean previous results
  rm -f "$RESULTS_DIR"/*.json

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

  print_results
}

main
