#!/usr/bin/env bash
# Render the pm-bench-phases sticky PR comment from per-registry log files
# in /tmp/pm-bench-output/. Called by pm-e2e-bench.yml's bench-phases-* jobs.
#
# Output: /tmp/pm-bench-output/pr_comment.md
#
# Args:
#   $1  PLATFORM  e.g. linux, mac
#   $2  OS        e.g. ubuntu-latest, macos-latest
#
# Each phase block in the bench output looks like:
#
#   ## p0_full_cold
#   PM     wall     ±σ        user     sys       RSS    pgMinor
#   utoo     5.78s   0.12s     2.34s   1.23s     456M     12.3K
#   bun      4.32s   0.08s     1.98s   0.87s     234M      8.5K
#   PM     vCtx     iCtx        net RX   net TX     cache    node_mod    lock
#   utoo      45      12         234M     12K        450M     400M       3M
#   bun       38       8         220M     11K        220M     400M       2M
#
# We emit one markdown subsection per phase containing two tables (timing
# and resource), one row per PM. Sticky-PR-comment dedup is upstream of
# this script — we just produce the body.
set -eu

PLATFORM="$1"
OS="$2"
OUT=/tmp/pm-bench-output/pr_comment.md
SHA="${GITHUB_SHA:0:7}"
RUN_URL="${GITHUB_SERVER_URL:-https://github.com}/${GITHUB_REPOSITORY}/actions/runs/${GITHUB_RUN_ID}"

# Preferred path: the bench script exported raw per-cell JSON
# (results-<registry>/ dirs). The node renderer computes Δ% vs utoo-next
# with significance flags instead of scraping fixed-width log text. The awk
# path below stays as a fallback for runs that died before the export step.
if compgen -G "/tmp/pm-bench-output/results-*" > /dev/null; then
  exec node "$(dirname "$0")/render-bench-phases-comment.mjs" "$PLATFORM" "$OS"
fi

mkdir -p /tmp/pm-bench-output
{
  echo "## 📊 pm-bench-phases · \`$SHA\` · ${PLATFORM} (\`${OS}\`)"
  echo ""
  echo "[Workflow run]($RUN_URL) — ant-design"
  echo ""
  echo "_PMs: \`utoo\` (this branch) · \`utoo-npm\` (latest published) · \`bun\` (latest)_"
  echo ""
} > "$OUT"

# Convert one log file's fixed-width tables into markdown.
render_log() {
  local label="$1"
  local file="$2"
  if [ ! -f "$file" ]; then
    echo "_${label}: no output captured._"
    echo ""
    return
  fi

  echo "### ${label}"
  echo ""

  # Strip ANSI colour codes so awk sees clean text.
  local cleaned
  cleaned=$(mktemp)
  sed -E 's/\x1B\[[0-9;]*[a-zA-Z]//g' "$file" > "$cleaned"

  awk '
    function flush_table() {
      if (n_rows == 0) return
      # markdown header
      printf "| "
      for (i = 1; i <= n_cols; i++) printf "%s | ", header[i]
      printf "\n|"
      for (i = 1; i <= n_cols; i++) printf "---|"
      printf "\n"
      # data rows
      for (r = 1; r <= n_rows; r++) {
        printf "| "
        for (i = 1; i <= n_cols; i++) printf "%s | ", rows[r, i]
        printf "\n"
      }
      printf "\n"
      n_rows = 0
      n_cols = 0
    }

    /^## p[0-9]+_/ {
      flush_table()
      # phase heading (drop the leading `## `)
      sub(/^## /, "", $0)
      printf "#### %s\n\n", $0
      in_phase = 1
      next
    }

    in_phase && /^PM[ \t]+/ {
      # New table header — flush any preceding one.
      flush_table()
      n_cols = NF
      for (i = 1; i <= NF; i++) header[i] = $i
      next
    }

    # Data rows: indented or pm-name-leading lines while a header is open.
    in_phase && n_cols > 0 && NF == n_cols && $1 !~ /^(PM|##)$/ {
      n_rows++
      for (i = 1; i <= NF; i++) rows[n_rows, i] = $i
      next
    }

    # End-of-phase marker: another `##` heading or blank-then-banner.
    in_phase && /^##/ && !/^## p[0-9]+_/ {
      flush_table()
      in_phase = 0
      next
    }

    END { flush_table() }
  ' "$cleaned"

  rm -f "$cleaned"
}

render_log "npmjs.org" /tmp/pm-bench-output/bench-phases-npmjs.log >> "$OUT"
render_log "npmmirror.com" /tmp/pm-bench-output/bench-phases-npmmirror.log >> "$OUT"
