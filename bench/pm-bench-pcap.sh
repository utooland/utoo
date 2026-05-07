#!/bin/bash
# Network trace capture for the install hot path: runs a single cold
# p1_resolve and a single cold p3_install per PM with tcpdump active,
# so we can inspect connection topology (concurrency, timing, request
# bursts, RST/dup-acks) when phase-bench σ widens unexplainedly.
#
# PMs:
#   utoo       — the binary on $PATH (built by build-linux from the
#                dispatched ref)
#   utoo-next  — UTOO_NEXT_BIN, the bench baseline (skipped if unset)
#   bun        — latest bun
#
# Phases per PM:
#   resolve  → lockfile generation only (metadata fan-out)
#   install  → tarball download + extract with lock present and cache
#              cold (this is where p3_cold_install regressions live)
#
# Outputs in $PCAP_DIR:
#   dns.txt                          — A records for the registry host
#   <pm>-{resolve,install}.pcap      — tcpdump capture
#   <pm>-{resolve,install}.log       — /usr/bin/time -v output
set -eo pipefail

PROJECT=${PROJECT:-ant-design}
REGISTRY=${REGISTRY:-https://registry.npmjs.org}
BENCH_DIR=${BENCH_DIR:-/tmp/pm-bench}
PCAP_DIR=${PCAP_DIR:-/tmp/pm-bench-pcap}

# Per-PM cache dirs so a wipe between phases is unambiguous.
UTOO_CACHE=${UTOO_CACHE:-/tmp/utoo-pcap-cache}
UTOO_NEXT_CACHE=${UTOO_NEXT_CACHE:-/tmp/utoo-next-pcap-cache}
BUN_CACHE=${BUN_CACHE:-/tmp/bun-pcap-cache}

mkdir -p "$PCAP_DIR" "$BENCH_DIR"

# Self-clone the project if missing. Mirrors pm-bench-phases.sh so this
# script is runnable as a standalone CI job.
PROJECT_DIR="$BENCH_DIR/$PROJECT"
if [ ! -d "$PROJECT_DIR" ]; then
  echo "=== cloning $PROJECT ==="
  git clone --depth=1 "https://github.com/ant-design/${PROJECT}.git" "$PROJECT_DIR"
fi

# Extract hostname (strip scheme + any path) for DNS lookup.
HOST=$(echo "$REGISTRY" | sed -E 's#^https?://([^/]+).*#\1#')
echo "=== DNS records for $HOST ==="
{
  echo "# getent hosts"
  getent hosts "$HOST" || true
  echo
  echo "# dig +short A"
  dig +short A "$HOST" || true
  echo
  echo "# dig +short AAAA"
  dig +short AAAA "$HOST" || true
} | tee "$PCAP_DIR/dns.txt"

IFACE=$(ip route | awk '/default/ {print $5; exit}')
echo "capturing on interface: $IFACE"

# Run a single command under tcpdump, write pcap+log to $PCAP_DIR/<name>.*.
capture_one() {
  local name=$1
  shift
  local pcap="$PCAP_DIR/$name.pcap"
  local log="$PCAP_DIR/$name.log"

  echo "=== capturing $name ==="
  # tcpdump as root; ubuntu-latest runners allow sudo without password.
  sudo tcpdump -s 0 -i "$IFACE" -w "$pcap" 'tcp port 443' >/dev/null 2>&1 &
  local tcpdump_pid=$!
  # Let tcpdump bind before the workload starts.
  sleep 1

  /usr/bin/time -v "$@" >"$log" 2>&1 || echo "  run exited with $?"

  sudo kill "$tcpdump_pid" 2>/dev/null || true
  wait "$tcpdump_pid" 2>/dev/null || true

  # Make pcap readable by the later upload-artifact step (tcpdump writes
  # as root).
  sudo chmod 644 "$pcap"
  echo "  → $pcap ($(wc -c <"$pcap") bytes), log: $log"
}

# Capture both phases (resolve, install) for a single PM. Wipes lock +
# cache + node_modules before resolve; keeps the lock between resolve
# and install but wipes cache + node_modules so install is truly cold.
run_pm_phases() {
  local pm_name=$1   # display name (utoo, utoo-next, bun)
  local pm_bin=$2    # absolute path / `bun`
  local cache_dir=$3

  cd "$PROJECT_DIR"
  rm -f package-lock.json bun.lock
  rm -rf "$cache_dir" node_modules

  if [ "$pm_name" = "bun" ]; then
    BUN_INSTALL_CACHE_DIR="$cache_dir" \
      capture_one "${pm_name}-resolve" \
        "$pm_bin" install --lockfile-only --registry="$REGISTRY"
    rm -rf "$cache_dir" node_modules
    BUN_INSTALL_CACHE_DIR="$cache_dir" \
      capture_one "${pm_name}-install" \
        "$pm_bin" install --registry="$REGISTRY"
  else
    capture_one "${pm_name}-resolve" \
      "$pm_bin" deps --registry="$REGISTRY" --cache-dir="$cache_dir"
    rm -rf "$cache_dir" node_modules
    capture_one "${pm_name}-install" \
      "$pm_bin" install --registry="$REGISTRY" --cache-dir="$cache_dir"
  fi
}

run_pm_phases utoo "$(command -v utoo)" "$UTOO_CACHE"

if [ -n "${UTOO_NEXT_BIN:-}" ] && [ -x "$UTOO_NEXT_BIN" ]; then
  run_pm_phases utoo-next "$UTOO_NEXT_BIN" "$UTOO_NEXT_CACHE"
else
  echo "skip utoo-next: UTOO_NEXT_BIN not set or not executable"
fi

run_pm_phases bun "$(command -v bun)" "$BUN_CACHE"

echo "done. files:"
ls -lh "$PCAP_DIR"
