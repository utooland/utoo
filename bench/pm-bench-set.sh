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
PM_LIST=${2:-utoo,pnpm}

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
# Diverse on purpose: single-package React component lib, very-large pnpm dep
# tree, Vue monorepo (pnpm catalog), yarn-workspaces app.
# --------------------------------------------------------------------------
PROJECTS=(
  "ant-design|https://github.com/ant-design/ant-design.git||"
  "material-ui|https://github.com/mui/material-ui.git||pnpm"
  "vue-core|https://github.com/vuejs/core.git||pnpm"
  "excalidraw|https://github.com/excalidraw/excalidraw.git||"
)

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
  f4="$(echo "$entry" | cut -d'|' -f4)"
  [[ -n "$f4" ]] && UTOO_FROM_PNPM_PROJECTS+="${entry%%|*} "
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
# catalog definitions in repos like vuejs/core).
if [ "$PM" = "pnpm" ] && [ ! -f "pnpm-workspace.yaml" ] && grep -q '"workspaces"' package.json 2>/dev/null; then
  node -e "
    const pkg = require('./package.json');
    const ws = pkg.workspaces || [];
    const p = Array.isArray(ws) ? ws : (ws.packages || []);
    if (p.length) {
      let y = 'packages:\n';
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
    name="${entry%%|*}"; url="$(echo "$entry" | cut -d'|' -f2)"; branch="$(echo "$entry" | cut -d'|' -f3)"
    if [ -d "$name/.git" ]; then
      echo -e "  $name already cloned, skipping"
      continue
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
    pnpm)      echo "npm_config_package_manager_strict=false pnpm install --ignore-scripts --no-frozen-lockfile --registry $registry" ;;
    yarn)      echo "yarn install --ignore-scripts --registry $registry" ;;
    bun)
      if [ "$cold" = "true" ]; then echo "bun install --ignore-scripts --registry $registry --no-cache";
      else echo "bun install --ignore-scripts --registry $registry"; fi ;;
  esac
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
    local install_cmd; install_cmd=$(get_install_cmd "$pm" "$registry" "false" "$utoo_from")
    local cell_log="$LOG_DIR/${project}_${reg_short}_${pm}.log"
    bash "$PREPARE_SCRIPT" "$project_dir" "$pm" || true
    if ( cd "$project_dir" && eval "$install_cmd" ) > "$cell_log" 2>&1; then
      echo -e "    ${GREEN}✓ $pm${NC}"
      VALID_PMS+=("$pm")
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
    printf 'set -eo pipefail\ncd %s && %s\n' "$project_dir" "$install_cmd" > "$cmd_script"
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
    printf 'set -eo pipefail\ncd %s && %s\n' "$project_dir" "$install_cmd" > "$cmd_script"
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
      const project = parts.slice(0, typeIdx - 1).join("-");
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
      md += "\n_Both PMs install the same tree, `--ignore-scripts`, lockfiles stripped (fresh resolve). Cold = cache purged, warm = cache primed._\n";
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
    local project="${entry%%|*}"
    local utoo_from; utoo_from="$(echo "$entry" | cut -d'|' -f4)"
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
