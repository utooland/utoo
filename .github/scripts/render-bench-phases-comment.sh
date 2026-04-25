#!/usr/bin/env bash
# Render the pm-bench-phases sticky PR comment from the per-registry log
# files in /tmp/pm-bench-output/. Called by pm-build.yml's bench-phases-*
# jobs. Output: /tmp/pm-bench-output/pr_comment.md
#
# Args:
#   $1  PLATFORM  e.g. linux, mac
#   $2  OS        e.g. ubuntu-latest, macos-latest
set -eu

PLATFORM="$1"
OS="$2"
OUT=/tmp/pm-bench-output/pr_comment.md
SHA="${GITHUB_SHA:0:7}"
RUN_URL="${GITHUB_SERVER_URL:-https://github.com}/${GITHUB_REPOSITORY}/actions/runs/${GITHUB_RUN_ID}"

mkdir -p /tmp/pm-bench-output
{
  echo "## 📊 pm-bench-phases · \`$SHA\` · ${PLATFORM} (\`${OS}\`)"
  echo ""
  echo "[Workflow run]($RUN_URL) — ant-design"
  echo ""
} > "$OUT"

# Extract `## p<N>_<name>` sections from a log under a labeled heading.
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
  local cleaned
  cleaned=$(mktemp)
  sed -E 's/\x1B\[[0-9;]*[a-zA-Z]//g' "$file" > "$cleaned"
  awk '
    /^## p[0-9]+_[a-z0-9_]+/ {
      if (in_section) print "```\n";
      in_section = 1;
      print "```";
      print $0;
      next;
    }
    /^## / && in_section {
      print "```\n";
      in_section = 0;
    }
    in_section { print }
    END { if (in_section) print "```" }
  ' "$cleaned"
  echo ""
  rm -f "$cleaned"
}

render_log "npmjs.org" /tmp/pm-bench-output/bench-phases-npmjs.log >> "$OUT"
render_log "npmmirror.com" /tmp/pm-bench-output/bench-phases-npmmirror.log >> "$OUT"
