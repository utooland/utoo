#!/bin/bash
# One-off network trace capture: runs a single cold p1_resolve per PM with
# tcpdump active, so we can inspect connection topology (IPs, concurrency,
# timing) on CI after local evidence was inconclusive.
#
# Outputs:
#   $PCAP_DIR/dns.txt           — A records for the registry host
#   $PCAP_DIR/{bun,utoo}.pcap   — tcpdump capture of the resolve phase
#   $PCAP_DIR/{bun,utoo}.log    — stdout/stderr of the run
set -eo pipefail

PROJECT=${PROJECT:-ant-design}
REGISTRY=${REGISTRY:-https://registry.npmjs.org}
BENCH_DIR=${BENCH_DIR:-/tmp/pm-bench}
PCAP_DIR=${PCAP_DIR:-/tmp/pm-bench-pcap}
UTOO_CACHE=/tmp/utoo-pcap-cache
BUN_CACHE=/tmp/bun-pcap-cache
export BUN_INSTALL_CACHE_DIR="$BUN_CACHE"

mkdir -p "$PCAP_DIR"

# Project must already be cloned by pm-bench-phases.sh (this script is meant
# to run after it, reusing the clone).
PROJECT_DIR="$BENCH_DIR/$PROJECT"
if [ ! -d "$PROJECT_DIR" ]; then
  echo "missing $PROJECT_DIR — run pm-bench-phases.sh first" >&2
  exit 1
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

capture_one() {
  local name=$1
  shift
  local pcap="$PCAP_DIR/$name.pcap"
  local log="$PCAP_DIR/$name.log"

  cd "$PROJECT_DIR"
  # Cold: nothing reused.
  rm -f package-lock.json bun.lock
  rm -rf "$UTOO_CACHE" "$BUN_CACHE" node_modules

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

capture_one utoo \
  utoo deps --registry="$REGISTRY" --cache-dir="$UTOO_CACHE"

capture_one bun \
  bun install --lockfile-only --registry="$REGISTRY"

echo "done. files:"
ls -lh "$PCAP_DIR"
