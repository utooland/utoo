#!/bin/bash
set -eo pipefail

# pm-bench-set.sh — utoo-vs-pnpm install benchmark over a curated set of
# real, popular JS/TS projects (spiritual cousin of pnpm's
# benchmarks-of-javascript-package-managers, but using live repos at HEAD).
#
# Unlike pm-bench.sh (hard-wired to the ant-design family), the project set
# here is data-driven (PROJECTS below / BENCH_ONLY filter) and the focus is a
# clean head-to-head against pnpm: both PMs install the SAME dependency tree
# resolved fresh from package.json, so the number is a relative-perf signal,
# not an absolute one.
#
# Methodology
#   * --ignore-scripts for every PM (lifecycle/binary-download work is a
#     functional-correctness concern, covered by e2e/, not a perf concern).
#   * Lockfiles are stripped before each install by default
#     (BENCH_KEEP_LOCKFILES=1 to keep them) so utoo and pnpm both do a full
#     resolve from manifests — otherwise pnpm replays pnpm-lock.yaml while
#     utoo (which reads package-lock.json) full-resolves, which is not a fair
#     comparison.
#   * cold  = global cache purged  → resolve + network + link
#   * warm  = global cache primed  → resolve + link (no network)
#   * Each run is wrapped in /usr/bin/time for RSS / IO / ctx-switch metrics.
#   * A project that fails to install under some PM is logged and skipped, not
#     fatal — the set keeps going.
#
# Usage: pm-bench-set.sh [registry_mode] [pm_list]
#   $1: npmjs | npmmirror | both   (default: npmjs)
#   $2: comma-separated PM list    (default: utoo,pnpm)
#
# Env knobs:
#   BENCH_COLD_RUNS       cold runs per PM         (default 1; network-bound)
#   BENCH_WARM_RUNS       warm runs per PM         (default 3)
#   BENCH_KEEP_LOCKFILES  1 → keep native lockfiles (default 0 = strip)
#   BENCH_ONLY            comma list of project names to restrict the set
#   BENCH_OUT_DIR         where summary.md is written (default /tmp/pm-bench-set-output)
#   UTOO_NEXT_BIN         path to a second utoo binary (origin/next baseline)

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'

REGISTRY_MODE=${1:-npmjs}
PM_LIST=${2:-utoo,pnpm,bun}

BENCH_COLD_RUNS=${BENCH_COLD_RUNS:-1}
BENCH_WARM_RUNS=${BENCH_WARM_RUNS:-3}
BENCH_KEEP_LOCKFILES=${BENCH_KEEP_LOCKFILES:-0}
BENCH_OUT_DIR=${BENCH_OUT_DIR:-/tmp/pm-bench-set-output}
export BENCH_KEEP_LOCKFILES

# --------------------------------------------------------------------------
# Curated project set: "name|git-url|branch|utoo_from".
#   * empty branch     → clone the repo default branch
#   * utoo_from=pnpm   → utoo installs via `--from pnpm`, migrating
#                        pnpm-workspace.yaml → workspaces (and catalog → .utoo.toml)
#                        before installing. Needed for repos (mui, vuejs/core)
#                        that declare workspace members ONLY in pnpm-workspace.yaml
#                        — utoo doesn't read that file by default, so without the
#                        migration it would install a smaller tree than pnpm.
#                        These rows carry a `*` in the summary: utoo's measured
#                        time includes the (cheap, local) migration step.
# Diverse on purpose. Two tiers:
#   * package.json-workspaces repos (no utoo_from) — utoo / pnpm / bun ALL read
#     `workspaces` natively, so it's a clean 3-way comparison.
#   * pnpm-only repos (utoo_from=pnpm) — members live only in pnpm-workspace.yaml
#     (+ catalog:). pnpm reads them natively; utoo needs `--from pnpm`; bun/yarn
#     can't read them at all and are auto-skipped for these rows (see
#     validate_and_prime) to avoid a misleading root-only install.
# --------------------------------------------------------------------------
PROJECTS=(
  # --- 3-way comparable (package.json workspaces / single package) ---
  "ant-design|https://github.com/ant-design/ant-design.git||"
  "excalidraw|https://github.com/excalidraw/excalidraw.git||"
  # --- pnpm-only (utoo via --from pnpm vs pnpm; bun/yarn skipped) ---
  "material-ui|https://github.com/mui/material-ui.git||pnpm"
  "vue-core|https://github.com/vuejs/core.git||pnpm"
)
# Deliberately NOT in the set: babel / strapi / jest. All three are yarn@4
# repos (no pnpm-workspace.yaml) whose deps use yarn-side `catalog:` and/or
# `patch:` (.yarn/patches/...) protocols. utoo resolves neither, pnpm refuses
# the yarn-pinned project, and bun also chokes on `catalog:` — so every PM
# fails and the row carries no cross-PM signal. Tracked as a utoo capability
# gap, not something this perf harness should surface.

# BENCH_ONLY filter (comma-separated project names)
if [[ -n "$BENCH_ONLY" ]]; then
  IFS=',' read -ra ONLY <<< "$BENCH_ONLY"
  FILTERED=()
  for entry in "${PROJECTS[@]}"; do
    name="${entry%%|*}"
    for keep in "${ONLY[@]}"; do
      [[ "$name" == "$keep" ]] && FILTERED+=("$entry")
    done
  done
  PROJECTS=("${FILTERED[@]}")
fi

# Projects whose utoo install goes through `--from pnpm` (4th manifest field).
UTOO_FROM_PNPM_PROJECTS=""
for entry in "${PROJECTS[@]}"; do
  IFS='|' read -r name _ _ f4 <<< "$entry"
  [[ -n "$f4" ]] && UTOO_FROM_PNPM_PROJECTS+="$name "
done
export UTOO_FROM_PNPM_PROJECTS

IFS=',' read -ra PACKAGE_MANAGERS <<< "$PM_LIST"

# Insert utoo-next before utoo when its binary is available
if [[ -n "$UTOO_NEXT_BIN" && -x "$UTOO_NEXT_BIN" ]]; then
  NEW_PMS=()
  for pm in "${PACKAGE_MANAGERS[@]}"; do
    [[ "$pm" == "utoo" ]] && NEW_PMS+=("utoo-next")
    NEW_PMS+=("$pm")
  done
  PACKAGE_MANAGERS=("${NEW_PMS[@]}")
fi

# Required commands
REQUIRED_CMDS=(git node hyperfine /usr/bin/time)
for pm in "${PACKAGE_MANAGERS[@]}"; do
  [[ "$pm" == "utoo-next" ]] && continue
  REQUIRED_CMDS+=("$pm")
done
for cmd in "${REQUIRED_CMDS[@]}"; do
  if ! command -v "$cmd" &> /dev/null; then
    echo "Error: required command '$cmd' not found." >&2
    exit 1
  fi
done

REGISTRIES=()
[[ "$REGISTRY_MODE" == "both" || "$REGISTRY_MODE" == "npmjs" ]] && REGISTRIES+=("https://registry.npmjs.org")
[[ "$REGISTRY_MODE" == "both" || "$REGISTRY_MODE" == "npmmirror" ]] && REGISTRIES+=("https://registry.npmmirror.com")

RESULTS_DIR="/tmp/pm-bench-set-results"
LOG_DIR="$RESULTS_DIR/logs"
BENCH_DIR="/tmp/pm-bench-set"
mkdir -p "$RESULTS_DIR" "$LOG_DIR" "$BENCH_DIR" "$BENCH_OUT_DIR"

UTOO_CACHE_DIR="${UTOO_CACHE_DIR:-$HOME/.cache/nm}"
PNPM_STORE_DIR="${PNPM_STORE_DIR:-$(pnpm store path 2>/dev/null || echo "$HOME/.pnpm-store")}"
BUN_INSTALL_DIR="${BUN_INSTALL_DIR:-$HOME/.bun/install}"

echo -e "${YELLOW}========================================${NC}"
echo -e "${YELLOW}  PM Benchmark Set (utoo vs pnpm)${NC}"
echo -e "${YELLOW}========================================${NC}"
echo -e "Registry mode:    ${CYAN}$REGISTRY_MODE${NC}"
echo -e "Package managers: ${CYAN}${PACKAGE_MANAGERS[*]}${NC}"
echo -e "Projects:         ${CYAN}$(printf '%s ' "${PROJECTS[@]%%|*}")${NC}"
[[ -n "$UTOO_FROM_PNPM_PROJECTS" ]] && echo -e "utoo --from pnpm: ${CYAN}${UTOO_FROM_PNPM_PROJECTS}${NC}"
echo -e "Strip lockfiles:  ${CYAN}$([[ "$BENCH_KEEP_LOCKFILES" == 1 ]] && echo no || echo yes)${NC}"
echo -e "Cold/Warm runs:   ${CYAN}$BENCH_COLD_RUNS / $BENCH_WARM_RUNS${NC}"
echo ""

# --------------------------------------------------------------------------
# prepare.sh — restore pristine tree, wipe node_modules, optionally strip
# lockfiles, purge global cache on --cold, regenerate pnpm-workspace.yaml.
# Usage: bash prepare.sh <project_dir> <pm> [--cold]
# --------------------------------------------------------------------------
PREPARE_SCRIPT="$RESULTS_DIR/prepare.sh"
cat > "$PREPARE_SCRIPT" << 'PREPARE_EOF'
#!/bin/bash
set -e
PROJECT_DIR="$1"; PM="$2"; COLD="$3"
UTOO_CACHE_DIR="${UTOO_CACHE_DIR:-$HOME/.cache/nm}"
PNPM_STORE_DIR="${PNPM_STORE_DIR:-$(pnpm store path 2>/dev/null || echo "$HOME/.pnpm-store")}"
BUN_INSTALL_DIR="${BUN_INSTALL_DIR:-$HOME/.bun/install}"

cd "$PROJECT_DIR"
# Reset tracked files (lockfiles/manifests a prior install rewrote) to pristine,
# then drop untracked output (node_modules, generated locks).
git checkout -- . 2>/dev/null || true
git clean -dfx -q

if [ "$BENCH_KEEP_LOCKFILES" != "1" ]; then
  rm -f package-lock.json npm-shrinkwrap.json pnpm-lock.yaml yarn.lock bun.lockb bun.lock
fi

if [ "$COLD" = "--cold" ]; then
  case "$PM" in
    utoo|utoo-next) rm -rf "$UTOO_CACHE_DIR" ;;
    pnpm) pnpm store prune 2>/dev/null || rm -rf "$PNPM_STORE_DIR" ;;
    yarn) yarn cache clean 2>/dev/null || rm -rf ~/.yarn/cache "$(yarn cache dir 2>/dev/null)" ;;
    bun)  rm -rf "$BUN_INSTALL_DIR"; bun pm cache rm 2>/dev/null || true ;;
  esac
fi

# pnpm needs a pnpm-workspace.yaml; synthesize one from package.json
# "workspaces" only when the repo doesn't already ship its own (preserving
# catalog definitions in repos like vuejs/core). `linkWorkspacePackages: true`
# makes pnpm link sibling workspace packages by bare version — pnpm v10 defaults
# this off (links only via the `workspace:` protocol), but yarn/npm/utoo link by
# version, so yarn-workspace repos (e.g. excalidraw) reference siblings as plain
# `@scope/pkg@x.y.z`. Without this, pnpm hits the registry for an unpublished
# in-repo version and fails — an artifact of cross-PM install, not a real diff.
if [ "$PM" = "pnpm" ] && [ ! -f "pnpm-workspace.yaml" ] && grep -q '"workspaces"' package.json 2>/dev/null; then
  node -e "
    const pkg = require('./package.json');
    const ws = pkg.workspaces || [];
    const p = Array.isArray(ws) ? ws : (ws.packages || []);
    if (p.length) {
      let y = 'linkWorkspacePackages: true\npackages:\n';
      p.forEach(x => y += '  - \"' + x + '\"\n');
      require('fs').writeFileSync('pnpm-workspace.yaml', y);
    }
  "
fi
PREPARE_EOF
chmod +x "$PREPARE_SCRIPT"

# --------------------------------------------------------------------------
# metrics_wrapper.sh — run a command under /usr/bin/time, append a JSONL row
# of resource metrics. Usage: bash metrics_wrapper.sh <metrics_jsonl> <cmd_script>
# --------------------------------------------------------------------------
METRICS_WRAPPER="$RESULTS_DIR/metrics_wrapper.sh"
cat > "$METRICS_WRAPPER" << 'METRICS_EOF'
#!/bin/bash
# LC_ALL=C keeps /usr/bin/time's labels English so the greps below match
# regardless of the runner's locale.
export LC_ALL=C
METRICS_FILE="$1"; CMD_SCRIPT="$2"
TIME_TMP=$(mktemp)
if [[ "$(uname)" == "Darwin" ]]; then
  /usr/bin/time -l bash "$CMD_SCRIPT" 2>"$TIME_TMP"; EXIT_CODE=$?
  RSS=$(grep 'maximum resident set size' "$TIME_TMP" | awk '{print $1}')
  PAGE_FAULTS=$(grep 'page faults' "$TIME_TMP" | awk '{print $1}')
  VOL_CTX=$(grep 'voluntary context switches' "$TIME_TMP" | grep -v involuntary | awk '{print $1}')
  INVOL_CTX=$(grep 'involuntary context switches' "$TIME_TMP" | awk '{print $1}')
  IO_IN=$(grep 'block input operations' "$TIME_TMP" | awk '{print $1}')
  IO_OUT=$(grep 'block output operations' "$TIME_TMP" | awk '{print $1}')
else
  /usr/bin/time -v bash "$CMD_SCRIPT" 2>"$TIME_TMP"; EXIT_CODE=$?
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

# --------------------------------------------------------------------------
# Clone the set
# --------------------------------------------------------------------------
clone_projects() {
  cd "$BENCH_DIR"
  echo -e "${YELLOW}Cloning projects...${NC}"
  for entry in "${PROJECTS[@]}"; do
    local name url branch
    IFS='|' read -r name url branch _ <<< "$entry"
    if [ -d "$name/.git" ]; then
      echo -e "  $name already cloned, skipping"
      continue
    elif [ -d "$name" ]; then
      # A prior interrupted clone left a dir without .git; git clone refuses to
      # write into a non-empty dir, so clear it for a clean retry.
      echo -e "  ${YELLOW}removing partial clone dir $name${NC}"
      rm -rf "$name"
    fi
    echo -e "  Cloning ${CYAN}$name${NC}..."
    # Non-fatal: one unreachable repo shouldn't sink the whole set (set -e).
    if [ -n "$branch" ]; then
      git clone --branch "$branch" --depth=1 "$url" "$name" || echo -e "  ${RED}WARN: clone failed for $name — skipping${NC}"
    else
      git clone --depth=1 "$url" "$name" || echo -e "  ${RED}WARN: clone failed for $name — skipping${NC}"
    fi
  done
  echo -e "${GREEN}Projects ready${NC}\n"
}

# --------------------------------------------------------------------------
# Install command per PM
# --------------------------------------------------------------------------
get_install_cmd() {
  local pm=$1 registry=$2 cold=$3 utoo_from=$4
  local from_flag=""
  [[ -n "$utoo_from" ]] && from_flag=" --from $utoo_from"
  case $pm in
    utoo)      echo "utoo install --ignore-scripts$from_flag --registry=$registry" ;;
    utoo-next) echo "$UTOO_NEXT_BIN install --ignore-scripts$from_flag --registry=$registry" ;;
    # --config.package-manager-strict=false is the authoritative override (beats
    # both the env var and any .npmrc) so pnpm runs on a repo that pins a
    # different packageManager (e.g. excalidraw pins yarn) instead of erroring
    # "This project is configured to use yarn".
    pnpm)      echo "pnpm install --ignore-scripts --no-frozen-lockfile --config.package-manager-strict=false --registry $registry" ;;
    yarn)      echo "yarn install --ignore-scripts --registry $registry" ;;
    bun)
      if [ "$cold" = "true" ]; then echo "bun install --ignore-scripts --registry $registry --no-cache";
      else echo "bun install --ignore-scripts --registry $registry"; fi ;;
  esac
}

# --------------------------------------------------------------------------
# Disk-size helpers
# --------------------------------------------------------------------------
# Actual disk blocks (KB) of a dir; 0 if missing. Must never fail the script:
# under `set -eo pipefail`, a bare `du` on a not-yet-created cache dir exits
# non-zero and would abort. Guard existence + swallow du's exit.
dir_kb() {
  [ -d "$1" ] || { echo 0; return 0; }
  { du -sk "$1" 2>/dev/null || true; } | awk 'END{print $1+0}'
}

# Global cache/store dir for a PM (what --cold purges).
cache_dir_for() {
  case "$1" in
    utoo|utoo-next) echo "$UTOO_CACHE_DIR" ;;
    pnpm) echo "$PNPM_STORE_DIR" ;;
    bun)  echo "$BUN_INSTALL_DIR" ;;
    yarn) yarn cache dir 2>/dev/null || echo "$HOME/.yarn/cache" ;;
    *) echo "" ;;
  esac
}

# Total node_modules footprint of a project: sum every node_modules tree (root
# + each workspace package), pruning nested ones so they aren't double-counted.
node_modules_kb() {
  { find "$1" -type d -name node_modules -prune 2>/dev/null || true; } \
    | while read -r d; do du -sk "$d" 2>/dev/null || true; done \
    | awk '{s+=$1} END {print s+0}'
}

# --------------------------------------------------------------------------
# Validate + prime: install each PM once with output CAPTURED. On failure the
# real error tail is printed (no more opaque "exit code 1" from hyperfine) and
# the PM is dropped from VALID_PMS, so cold/warm never benchmark a failing
# command — which would otherwise emit an empty --export-json and crash the
# summary. This install also primes the global cache for the warm pass.
# Sets globals: VALID_PMS (this project), appends to FAILED_CELLS.
# --------------------------------------------------------------------------
validate_and_prime() {
  local project=$1 registry=$2 reg_short=$3 utoo_from=$4
  local project_dir="$BENCH_DIR/$project"
  VALID_PMS=()
  echo -e "  ${YELLOW}Validate + prime (one install each):${NC}"
  for pm in "${PACKAGE_MANAGERS[@]}"; do
    # pnpm-only repos (utoo_from set) declare members only in pnpm-workspace.yaml
    # (+ catalog:). bun/yarn can't read that — they'd silently install a root-only
    # tree (misleading) or fail. utoo handles it via --from pnpm; pnpm natively.
    # Skip the others rather than report a bogus number.
    if [[ -n "$utoo_from" && ( "$pm" == "bun" || "$pm" == "yarn" ) ]]; then
      echo -e "    ${YELLOW}- skip $pm (pnpm-only workspace; can't read pnpm-workspace.yaml/catalog)${NC}"
      continue
    fi
    local install_cmd; install_cmd=$(get_install_cmd "$pm" "$registry" "false" "$utoo_from")
    local cell_log="$LOG_DIR/${project}_${reg_short}_${pm}.log"
    bash "$PREPARE_SCRIPT" "$project_dir" "$pm" || true
    # Cache size before this install — the delta is the project's footprint in
    # the PM's store (prior project's cold run purged it, so this is ~per-project).
    local cache_dir cache_before; cache_dir=$(cache_dir_for "$pm"); cache_before=$(dir_kb "$cache_dir")
    if ( cd "$project_dir" && eval "$install_cmd" ) > "$cell_log" 2>&1; then
      echo -e "    ${GREEN}✓ $pm${NC}"
      VALID_PMS+=("$pm")
      # Record node_modules footprint + cache delta for the resources table.
      node_modules_kb "$project_dir" > "$RESULTS_DIR/${project}_${reg_short}_nm_${pm}.txt"
      local cache_after; cache_after=$(dir_kb "$cache_dir")
      echo $(( cache_after - cache_before )) > "$RESULTS_DIR/${project}_${reg_short}_cache_${pm}.txt"
    else
      echo -e "    ${RED}✗ $pm install FAILED — cmd: ${NC}$install_cmd"
      echo -e "    ${RED}  real error tail:${NC}"
      tail -n 30 "$cell_log" | sed 's/^/        | /'
      FAILED_CELLS+=("$project ($pm)")
    fi
  done
}

# --------------------------------------------------------------------------
# Warm benchmark: global cache already primed by validate_and_prime; only the
# local node_modules is wiped between runs. Benchmarks VALID_PMS only.
# --------------------------------------------------------------------------
run_warm_benchmarks() {
  local project=$1 registry=$2 reg_short=$3 utoo_from=$4
  local project_dir="$BENCH_DIR/$project"
  [ ${#VALID_PMS[@]} -eq 0 ] && { echo -e "  ${RED}No valid PMs — skipping warm.${NC}"; return; }
  echo -e "  ${YELLOW}Warm installs ($BENCH_WARM_RUNS runs, cache primed): ${VALID_PMS[*]}${NC}"
  local json_file="$RESULTS_DIR/${project}_${reg_short}_warm.json"
  local hyperfine_args=(--warmup 1 --runs "$BENCH_WARM_RUNS" --export-json "$json_file")
  for pm in "${VALID_PMS[@]}"; do
    local install_cmd; install_cmd=$(get_install_cmd "$pm" "$registry" "false" "$utoo_from")
    local metrics_file="$RESULTS_DIR/${project}_${reg_short}_warm_${pm}_metrics.jsonl"
    local cmd_script="$RESULTS_DIR/cmd_warm_${pm}.sh"
    printf 'set -eo pipefail\ncd "%s" && %s\n' "$project_dir" "$install_cmd" > "$cmd_script"
    > "$metrics_file"
    hyperfine_args+=(-n "$pm" --prepare "bash $PREPARE_SCRIPT $project_dir $pm" "bash $METRICS_WRAPPER $metrics_file $cmd_script")
  done
  hyperfine "${hyperfine_args[@]}" 2>&1 || echo -e "  ${RED}Warm benchmark error for $project${NC}"
}

# --------------------------------------------------------------------------
# Cold benchmark: cache purged each run (network-bound). Benchmarks VALID_PMS only.
# --------------------------------------------------------------------------
run_cold_benchmarks() {
  local project=$1 registry=$2 reg_short=$3 utoo_from=$4
  local project_dir="$BENCH_DIR/$project"
  [ ${#VALID_PMS[@]} -eq 0 ] && { echo -e "  ${RED}No valid PMs — skipping cold.${NC}"; return; }
  echo -e "  ${YELLOW}Cold installs ($BENCH_COLD_RUNS run(s) each, cache purged): ${VALID_PMS[*]}${NC}"
  for pm in "${VALID_PMS[@]}"; do
    local install_cmd; install_cmd=$(get_install_cmd "$pm" "$registry" "true" "$utoo_from")
    local json_file="$RESULTS_DIR/${project}_${reg_short}_cold_${pm}.json"
    local metrics_file="$RESULTS_DIR/${project}_${reg_short}_cold_${pm}_metrics.jsonl"
    local cmd_script="$RESULTS_DIR/cmd_cold_${pm}.sh"
    printf 'set -eo pipefail\ncd "%s" && %s\n' "$project_dir" "$install_cmd" > "$cmd_script"
    > "$metrics_file"
    echo -e "    ${CYAN}$pm${NC}..."
    hyperfine \
      --runs "$BENCH_COLD_RUNS" \
      --prepare "bash $PREPARE_SCRIPT $project_dir $pm --cold" \
      --export-json "$json_file" \
      -n "$pm" \
      "bash $METRICS_WRAPPER $metrics_file $cmd_script" \
      2>&1 | tail -1 || echo -e "    ${RED}$pm cold error for $project${NC}"
  done
}

# --------------------------------------------------------------------------
# Summary — timing table + speedup-vs-pnpm table, written to summary.md
# --------------------------------------------------------------------------
print_results() {
  echo ""
  echo -e "${YELLOW}========================================${NC}"
  echo -e "${YELLOW}  Results${NC}"
  echo -e "${YELLOW}========================================${NC}\n"

  RESULTS_DIR="$RESULTS_DIR" BENCH_OUT_DIR="$BENCH_OUT_DIR" node -e '
    const fs = require("fs"), path = require("path");
    const dir = process.env.RESULTS_DIR, outDir = process.env.BENCH_OUT_DIR;
    const fromPnpm = new Set((process.env.UTOO_FROM_PNPM_PROJECTS || "").trim().split(/\s+/).filter(Boolean));
    const star = p => fromPnpm.has(p) ? p + " *" : p;
    const jsonFiles = fs.readdirSync(dir).filter(f => f.endsWith(".json"));

    const rows = [];
    for (const file of jsonFiles) {
      let data;
      try {
        const raw = fs.readFileSync(path.join(dir, file), "utf8");
        if (!raw.trim()) continue;             // a failed cell can leave an empty --export-json
        data = JSON.parse(raw);
      } catch (e) { continue; }                // one bad file must never sink the whole summary
      const base = file.replace(".json", "");
      const parts = base.split("_");
      const typeIdx = parts.findIndex(p => p === "cold" || p === "warm");
      if (typeIdx === -1) continue;
      const project = parts.slice(0, typeIdx - 1).join("_");
      const registry = parts[typeIdx - 1];
      const type = parts[typeIdx];
      for (const r of (data.results || [])) rows.push({ project, registry, type, pm: r.command, mean: r.mean, stddev: r.stddev });
    }

    const failed = (process.env.FAILED_CELLS_STR || "").split("\n").map(s => s.trim()).filter(Boolean);

    const pms = [...new Set(rows.map(r => r.pm))];
    const fmt = r => r.stddev > 0.005 ? r.mean.toFixed(2)+"s ±"+r.stddev.toFixed(2) : r.mean.toFixed(2)+"s";
    const groups = {};
    for (const r of rows) {
      const key = [r.project, r.registry, r.type].join("|");
      (groups[key] ||= {})[r.pm] = r;
    }

    let md = "";
    if (rows.length) {
      // ---- Timing ----
      md += "## Timing — install wall-clock\n\n";
      md += "| Project | Registry | Type | " + pms.join(" | ") + " |\n";
      md += "|---|---|---|" + pms.map(() => "---").join("|") + "|\n";
      for (const [key, byPm] of Object.entries(groups)) {
        const [project, registry, type] = key.split("|");
        const cells = pms.map(p => byPm[p] ? fmt(byPm[p]) : "—").join(" | ");
        md += `| ${star(project)} | ${registry} | ${type} | ${cells} |\n`;
      }

      // ---- Speedup vs pnpm (only if pnpm present) ----
      if (pms.includes("pnpm")) {
        const challengers = pms.filter(p => p !== "pnpm");
        if (challengers.length) {
          md += "\n## Speedup vs pnpm\n\n";
          md += "| Project | Registry | Type | " + challengers.map(c => `${c} vs pnpm`).join(" | ") + " |\n";
          md += "|---|---|---|" + challengers.map(() => "---").join("|") + "|\n";
          for (const [key, byPm] of Object.entries(groups)) {
            const [project, registry, type] = key.split("|");
            const pnpm = byPm["pnpm"];
            const cells = challengers.map(c => {
              if (!pnpm || !byPm[c]) return "—";
              const ratio = pnpm.mean / byPm[c].mean;
              return ratio >= 1 ? `**${ratio.toFixed(2)}× faster**` : `${(1/ratio).toFixed(2)}× slower`;
            }).join(" | ");
            md += `| ${star(project)} | ${registry} | ${type} | ${cells} |\n`;
          }
        }
      }
      // ---- Resources: RSS + IO + ctx (warm avg) + node_modules + cache footprint ----
      const res = {};  // key: project|registry|pm
      const ensure = k => (res[k] ||= { rss:0, io_in:0, io_out:0, vol_ctx:0, invol_ctx:0, nm:null, cache:null });
      for (const file of fs.readdirSync(dir)) {
        if (file.endsWith("_metrics.jsonl")) {
          const parts = file.replace("_metrics.jsonl","").split("_");
          const ti = parts.findIndex(p => p === "cold" || p === "warm");
          if (ti === -1 || parts[ti] !== "warm") continue;       // warm runs only
          const project = parts.slice(0, ti-1).join("_"), registry = parts[ti-1], pm = parts.slice(ti+1).join("_");
          let lines;
          try { lines = fs.readFileSync(path.join(dir,file),"utf8").trim().split("\n").filter(Boolean).map(JSON.parse); }
          catch(e) { continue; }
          if (!lines.length) continue;
          const avg = key => Math.round(lines.reduce((s,e)=>s+(e[key]||0),0)/lines.length);
          const r = ensure([project,registry,pm].join("|"));
          r.rss=avg("rss"); r.io_in=avg("io_in"); r.io_out=avg("io_out"); r.vol_ctx=avg("vol_ctx"); r.invol_ctx=avg("invol_ctx");
        } else if (file.endsWith(".txt") && (file.includes("_nm_") || file.includes("_cache_"))) {
          const parts = file.replace(".txt","").split("_");
          const mi = parts.findIndex(p => p === "nm" || p === "cache");
          if (mi === -1) continue;
          const project = parts.slice(0, mi-1).join("_"), registry = parts[mi-1], pm = parts.slice(mi+1).join("_");
          let kb = 0; try { kb = parseInt(fs.readFileSync(path.join(dir,file),"utf8").trim(),10) || 0; } catch(e){}
          const r = ensure([project,registry,pm].join("|"));
          if (parts[mi] === "nm") r.nm = kb; else r.cache = kb;
        }
      }
      const resKeys = Object.keys(res).sort();
      if (resKeys.length) {
        const fmtKB = kb => kb == null ? "—" : kb >= 1048576 ? (kb/1048576).toFixed(1)+"G" : kb >= 1024 ? (kb/1024).toFixed(0)+"M" : kb+"K";
        const fmtB  = b  => b >= 1073741824 ? (b/1073741824).toFixed(1)+"G" : b >= 1048576 ? (b/1048576).toFixed(0)+"M" : Math.round(b/1024)+"K";
        md += "\n## Resources (warm-run avg)\n\n";
        md += "| Project | PM | RSS | node_modules | cache Δ | IO r/w | ctx v/i |\n|---|---|---|---|---|---|---|\n";
        for (const k of resKeys) {
          const [project, , pm] = k.split("|"); const r = res[k];
          md += `| ${star(project)} | ${pm} | ${fmtB(r.rss)} | ${fmtKB(r.nm)} | ${fmtKB(r.cache)} | ${r.io_in}/${r.io_out} | ${r.vol_ctx}/${r.invol_ctx} |\n`;
        }
      }
      md += "\n_Both PMs install the same tree, `--ignore-scripts`, lockfiles stripped (fresh resolve). Cold = cache purged, warm = cache primed. node_modules = sum of all node_modules trees (du, actual blocks); cache Δ = bytes the project added to each PM global store._\n";
      if (fromPnpm.size) {
        md += "_\\* utoo installed via `--from pnpm` (migrates pnpm-workspace.yaml → workspaces + catalog → .utoo.toml before installing); its time includes that local migration step. pnpm reads the workspace natively._\n";
      }
    } else {
      md += "## Timing\n\n_No timing results — every benchmarked install failed. See Failures below and the run log._\n";
    }

    // ---- Failures (surfaced, not swallowed) ----
    if (failed.length) {
      md += `\n## ⚠️ Failures (${failed.length})\n\n`;
      for (const f of failed) md += `- \`${f}\`\n`;
      md += "\n_The real error output for each failed cell is in the run-log artifact (search the project/pm name)._\n";
    }

    process.stdout.write(md);
    fs.writeFileSync(path.join(outDir, "summary.md"), md);
  ' 2>&1

  echo ""
  echo -e "${GREEN}Summary written to ${BENCH_OUT_DIR}/summary.md${NC}"
}

# --------------------------------------------------------------------------
main() {
  rm -f "$RESULTS_DIR"/*.json "$RESULTS_DIR"/*.jsonl 2>/dev/null || true
  rm -rf "$LOG_DIR" && mkdir -p "$LOG_DIR"

  clone_projects

  FAILED_CELLS=()
  for entry in "${PROJECTS[@]}"; do
    local project utoo_from
    IFS='|' read -r project _ _ utoo_from <<< "$entry"
    if [ ! -d "$BENCH_DIR/$project/.git" ]; then
      echo -e "${RED}Skipping $project (not cloned)${NC}\n"
      FAILED_CELLS+=("$project (clone)")
      continue
    fi
    for registry in "${REGISTRIES[@]}"; do
      local reg_short; reg_short=$(echo "$registry" | sed 's|https://registry\.||;s|\.org||;s|\.com||')
      echo -e "${YELLOW}----------------------------------------${NC}"
      echo -e "${YELLOW}Project: ${CYAN}$project${NC}  Registry: ${CYAN}$reg_short${NC}$([[ -n "$utoo_from" ]] && echo "  (utoo --from $utoo_from)")"
      echo -e "${YELLOW}----------------------------------------${NC}"
      # validate (surfaces real errors, builds VALID_PMS + primes cache)
      # → warm (consumes primed cache) → cold (purges per run).
      validate_and_prime "$project" "$registry" "$reg_short" "$utoo_from"
      run_warm_benchmarks "$project" "$registry" "$reg_short" "$utoo_from"
      run_cold_benchmarks "$project" "$registry" "$reg_short" "$utoo_from"
      echo ""
    done
  done

  # Newline-joined so cell labels (which contain spaces) survive into the printer.
  FAILED_CELLS_STR=""
  for c in "${FAILED_CELLS[@]}"; do FAILED_CELLS_STR+="$c"$'\n'; done
  export FAILED_CELLS_STR

  print_results
}

main
