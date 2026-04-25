#!/usr/bin/env bash
# Run manifest-bench (standalone HTTP-only fetch tool) against a
# given REGISTRY for a given PROJECT. Used by `pm-bench-phases.yml`
# to capture HTTP stack ceiling numbers per registry / per platform.
#
# Required env: REGISTRY, PROJECT
# Generates the lockfile via `utoo deps` if it doesn't exist yet.
set -eu

PROJECT_DIR="/tmp/pm-bench/$PROJECT"
if [ ! -d "$PROJECT_DIR" ]; then
  mkdir -p /tmp/pm-bench
  git clone --depth 1 "https://github.com/ant-design/$PROJECT" "$PROJECT_DIR"
fi
cd "$PROJECT_DIR"
if [ ! -f package-lock.json ]; then
  echo "==> generating lockfile via utoo (registry=$REGISTRY)"
  utoo deps --registry "$REGISTRY" || true
fi
ls -la package-lock.json || { echo "no lockfile, skip"; exit 0; }

echo
echo "============================================================"
echo "manifest-bench: HTTP-only fetch (no parse, no resolver)"
echo "  registry=$REGISTRY"
echo "  Goal: isolate reqwest/rustls/tokio behaviour from"
echo "  ruborist's resolver pipeline. Same numbers (wall, busy,"
echo "  avg_conc, p50/p95/max) as ruborist's Preload HTTP diag."
echo "============================================================"

for CAP in 32 64 96 128 192 256; do
  echo
  echo "--- concurrency=$CAP, h1, full manifest, default UA ---"
  manifest-bench --lockfile package-lock.json --registry "$REGISTRY" \
    --concurrency "$CAP" --reps 2 --http1-only || true
done

echo
echo "--- concurrency=128, h2 negotiate, full manifest, default UA ---"
manifest-bench --lockfile package-lock.json --registry "$REGISTRY" \
  --concurrency 128 --reps 2 || true

echo
echo "--- concurrency=128, h1, single-version endpoint ---"
manifest-bench --lockfile package-lock.json --registry "$REGISTRY" \
  --concurrency 128 --reps 2 --http1-only --single-version || true

echo
echo "--- concurrency=128, h1, UA=Bun/1.2.21 ---"
manifest-bench --lockfile package-lock.json --registry "$REGISTRY" \
  --concurrency 128 --reps 2 --http1-only --user-agent "Bun/1.2.21" || true
