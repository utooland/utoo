#!/bin/bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Argument: both | npmjs | npmmirror
REGISTRY_MODE=${1:-both}

# Configuration
PROJECTS=("ant-design" "ant-design-x")
PACKAGE_MANAGERS=("utoo" "yarn" "pnpm" "bun")
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
    echo -e "Creating pnpm-workspace.yaml for $(basename $project_dir)..."
    # Parse workspaces from package.json using node
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
  echo -e "${YELLOW}Setting up pnpm workspace configs...${NC}"
  setup_pnpm_workspace "$BENCH_DIR/ant-design"
  setup_pnpm_workspace "$BENCH_DIR/ant-design-x"

  echo -e "${GREEN}Projects ready${NC}"
  echo ""
}

# Clean lock files and node_modules using git clean
clean_local() {
  git clean -dfx
}

# Clean global cache for each package manager (including manifest and tgz)
clean_pm_cache() {
  local pm=$1
  echo -e "    ${YELLOW}Cleaning $pm global cache...${NC}"
  case $pm in
    utoo)
      rm -rf ~/.cache/nm
      ;;
    yarn)
      yarn cache clean 2>/dev/null || rm -rf ~/.yarn/cache $(yarn cache dir 2>/dev/null)
      ;;
    pnpm)
      pnpm store prune 2>/dev/null || rm -rf ~/.pnpm-store $(pnpm store path 2>/dev/null)
      ;;
    bun)
      # bun cache is under ~/.bun/install, clean the entire install directory
      rm -rf ~/.bun/install
      bun pm cache rm 2>/dev/null || true
      ;;
  esac
}

# Clean only manifest cache, keep tgz package cache
clean_manifest_cache() {
  local pm=$1
  echo -e "    ${YELLOW}Cleaning $pm manifest cache only...${NC}"
  case $pm in
    utoo)
      # utoo manifest cache location (to be confirmed)
      rm -rf ~/.cache/nm/manifests 2>/dev/null || true
      ;;
    yarn)
      # yarn v1 manifest cache
      rm -rf ~/.yarn/cache/.npm-*.json 2>/dev/null || true
      ;;
    pnpm)
      # pnpm metadata cache
      rm -rf ~/.pnpm-store/v3/metadata 2>/dev/null || true
      rm -rf $(pnpm store path 2>/dev/null)/v3/metadata 2>/dev/null || true
      ;;
    bun)
      # bun manifest cache are .npm files
      rm -f ~/.bun/install/cache/*.npm 2>/dev/null || true
      ;;
  esac
}

# Full cleanup (called before cold install)
clean_all() {
  local pm=$1
  clean_local
  clean_pm_cache "$pm"
}

# Run single benchmark
run_benchmark() {
  local project=$1
  local pm=$2
  local registry=$3
  local install_type=$4  # cold | warm | hot

  cd "$BENCH_DIR/$project"

  # hot type: no cleanup here (manifest cache cleaned by caller)
  if [ "$install_type" = "cold" ]; then
    # Cold install: clean local files + all global cache
    clean_all "$pm"
  elif [ "$install_type" = "warm" ] || [ "$install_type" = "hot" ]; then
    # warm/hot: only clean local files
    clean_local
  fi

  # pnpm needs workspace config recreated (git clean deletes it)
  if [ "$pm" = "pnpm" ]; then
    setup_pnpm_workspace "$BENCH_DIR/$project"
  fi

  local cmd=""
  case $pm in
    utoo) cmd="utoo install --ignore-scripts --registry=$registry" ;;
    yarn) cmd="yarn install --ignore-scripts --registry $registry" ;;
    pnpm) cmd="npm_config_package_manager_strict=false pnpm install --ignore-scripts --registry $registry" ;;
    bun)  cmd="bun install --ignore-scripts --registry $registry" ;;
  esac

  local start=$(date +%s.%N)
  eval "$cmd" >/dev/null 2>&1 || true
  local end=$(date +%s.%N)
  local duration=$(echo "$end - $start" | bc)

  # Output result
  echo "$project,$pm,$registry,$install_type,$duration" >> "$RESULTS_DIR/results.csv"

  local registry_short=$(echo "$registry" | sed 's|https://||' | cut -d'.' -f2)
  echo -e "  ${CYAN}$pm${NC} @ $registry_short ($install_type): ${GREEN}${duration}s${NC}"
}

# Print results table
print_results() {
  echo ""
  echo -e "${YELLOW}========================================${NC}"
  echo -e "${YELLOW}  Benchmark Results${NC}"
  echo -e "${YELLOW}========================================${NC}"
  echo ""

  if command -v column &> /dev/null; then
    cat "$RESULTS_DIR/results.csv" | column -t -s,
  else
    cat "$RESULTS_DIR/results.csv"
  fi

  echo ""
  echo -e "${GREEN}Benchmark completed!${NC}"
}

# Main flow
main() {
  echo "project,pm,registry,type,duration" > "$RESULTS_DIR/results.csv"

  clone_projects

  for project in "${PROJECTS[@]}"; do
    for registry in "${REGISTRIES[@]}"; do
      local registry_short=$(echo "$registry" | sed 's|https://||')
      echo -e "${YELLOW}----------------------------------------${NC}"
      echo -e "${YELLOW}Project: ${CYAN}$project${NC}"
      echo -e "${YELLOW}Registry: ${CYAN}$registry_short${NC}"
      echo -e "${YELLOW}----------------------------------------${NC}"

      for pm in "${PACKAGE_MANAGERS[@]}"; do
        # Cold install - no cache at all
        run_benchmark "$project" "$pm" "$registry" "cold"
        # Warm install - with all cache
        run_benchmark "$project" "$pm" "$registry" "warm"

        # bun specific test: delete manifest cache and run again
        if [ "$pm" = "bun" ]; then
          echo -e "    ${YELLOW}Cleaning bun manifest cache only...${NC}"
          rm -f ~/.bun/install/cache/*.npm 2>/dev/null || true
          run_benchmark "$project" "$pm" "$registry" "hot"
        fi
      done
      echo ""
    done
  done

  print_results
}

main
