#!/bin/bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# 参数: both | npmjs | npmmirror
REGISTRY_MODE=${1:-both}

# 配置
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

# 克隆项目
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

  echo -e "${GREEN}Projects ready${NC}"
  echo ""
}

# 清理 lock 文件和 node_modules（使用 git clean）
clean_local() {
  git clean -dfx
}

# 清理各包管理器的全局缓存
clean_pm_cache() {
  local pm=$1
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
      rm -rf ~/.bun/install/cache
      ;;
  esac
}

# 完整清理（首次安装前调用）
clean_all() {
  local pm=$1
  clean_local
  clean_pm_cache "$pm"
}

# 运行单次 benchmark
run_benchmark() {
  local project=$1
  local pm=$2
  local registry=$3
  local install_type=$4  # cold | warm

  cd "$BENCH_DIR/$project"

  if [ "$install_type" = "cold" ]; then
    # 首次安装：清理本地文件 + 该 pm 的全局缓存
    clean_all "$pm"
  else
    # 二次安装：只清理本地文件，保留缓存
    clean_local
  fi

  local cmd=""
  case $pm in
    utoo) cmd="utoo install --ignore-scripts --registry=$registry" ;;
    yarn) cmd="yarn install --ignore-scripts --registry $registry" ;;
    pnpm) cmd="pnpm install --ignore-scripts --registry $registry" ;;
    bun)  cmd="bun install --ignore-scripts --registry $registry" ;;
  esac

  local start=$(date +%s.%N)
  eval "$cmd" >/dev/null 2>&1 || true
  local end=$(date +%s.%N)
  local duration=$(echo "$end - $start" | bc)

  # 输出结果
  echo "$project,$pm,$registry,$install_type,$duration" >> "$RESULTS_DIR/results.csv"

  local registry_short=$(echo "$registry" | sed 's|https://||' | cut -d'.' -f2)
  echo -e "  ${CYAN}$pm${NC} @ $registry_short ($install_type): ${GREEN}${duration}s${NC}"
}

# 打印结果表格
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

# 主流程
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
        # 首次安装 (cold)
        run_benchmark "$project" "$pm" "$registry" "cold"
        # 二次安装 (warm)
        run_benchmark "$project" "$pm" "$registry" "warm"
      done
      echo ""
    done
  done

  print_results
}

main
