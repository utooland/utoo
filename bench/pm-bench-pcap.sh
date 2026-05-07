#!/bin/bash
# Network trace capture for the install hot path: runs a single cold
# p1_resolve and a single cold p3_install per PM with tcpdump active,
# then post-processes each pcap with tshark to extract TCP stress
# metrics — directly comparing utoo (greedy install) vs utoo-next
# (baseline) on signals that prove or refute "install starves
# download" without needing TLS decryption.
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
#   <pm>-{resolve,install}.pcap      — raw tcpdump capture
#   <pm>-{resolve,install}.log       — /usr/bin/time -v output
#   <pm>-{resolve,install}.summary.json — per-capture tshark metrics
#   summary.json                     — aggregated metrics for all captures
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

# --- post-capture analysis: tshark metrics per pcap ---------------------
# Extract TCP-level stress signals to validate the "install greediness
# starves download" hypothesis. All of these are pre-TLS so we don't need
# session-key dumping:
#
#   zero_windows    — receive buffer full → server paused. Direct evidence
#                     that the app (utoo's tokio runtime) is not draining
#                     the socket fast enough.
#   retransmits     — server resent because ACK was late. Indirect evidence
#                     of receive-side stall.
#   duplicate_acks  — receiver re-sent ACK because it perceived a gap.
#   stream_gap_*    — inter-packet gap distribution per TCP stream. p99 /
#                     max measures the longest pause an active connection
#                     experienced — if utoo shows multi-hundred-ms gaps
#                     while utoo-next shows tens of ms, install is freezing
#                     the runtime mid-download.
analyze_pcap() {
  local name=$1
  local pcap="$PCAP_DIR/$name.pcap"
  local log="$PCAP_DIR/$name.log"
  local summary="$PCAP_DIR/$name.summary.json"

  if [ ! -f "$pcap" ]; then
    echo "  skip analyze: $pcap missing" >&2
    return
  fi

  echo "=== analyzing $name ==="

  local pcap_bytes
  pcap_bytes=$(wc -c < "$pcap")

  # /usr/bin/time -v writes "Elapsed (wall clock) time (h:mm:ss or m:ss): 0:19.05"
  local wall_str
  wall_str=$(grep -oE 'Elapsed \(wall clock\) time[^:]*: [0-9:.]+' "$log" 2>/dev/null \
    | awk -F': ' '{print $NF}')
  local wall_s
  wall_s=$(awk -v t="${wall_str:-0}" 'BEGIN{
    n = split(t, p, ":")
    if (n == 3)      printf "%.3f", p[1]*3600 + p[2]*60 + p[3]
    else if (n == 2) printf "%.3f", p[1]*60 + p[2]
    else             printf "%.3f", t+0
  }')

  # Single-pass extraction of analysis flags + per-packet stream/time.
  # Each emitted line is `tcp.stream,frame.time_relative,zwin,retx,fast_retx,dupack,ooo`.
  # Empty fields where the analysis flag does not apply.
  local stats_tmp
  stats_tmp=$(mktemp)
  sudo tshark -r "$pcap" -T fields \
    -e tcp.stream \
    -e frame.time_relative \
    -e tcp.analysis.zero_window \
    -e tcp.analysis.retransmission \
    -e tcp.analysis.fast_retransmission \
    -e tcp.analysis.duplicate_ack \
    -e tcp.analysis.out_of_order \
    -E separator=, -E quote=n -E header=n 2>/dev/null \
    > "$stats_tmp"

  local total_packets zero_windows retransmits fast_retx dup_acks out_of_order distinct_streams
  total_packets=$(wc -l < "$stats_tmp")
  zero_windows=$(awk -F, '$3 != "" {c++} END {print c+0}' "$stats_tmp")
  retransmits=$(awk -F, '$4 != "" {c++} END {print c+0}' "$stats_tmp")
  fast_retx=$(awk -F, '$5 != "" {c++} END {print c+0}' "$stats_tmp")
  dup_acks=$(awk -F, '$6 != "" {c++} END {print c+0}' "$stats_tmp")
  out_of_order=$(awk -F, '$7 != "" {c++} END {print c+0}' "$stats_tmp")
  distinct_streams=$(awk -F, 'NF>0 && $1!="" {print $1}' "$stats_tmp" | sort -nu | wc -l)

  # Per-stream inter-packet gaps in microseconds. Awk keeps prev_time
  # per stream id so the input doesn't need to be sorted.
  local gaps_tmp
  gaps_tmp=$(mktemp)
  awk -F, '
    $1 != "" && $2 != "" {
      if ($1 in prev_time) {
        delta_us = ($2 - prev_time[$1]) * 1e6
        if (delta_us > 0) print int(delta_us)
      }
      prev_time[$1] = $2
    }' "$stats_tmp" | sort -n > "$gaps_tmp"

  local gap_count gap_p50 gap_p99 gap_max
  gap_count=$(wc -l < "$gaps_tmp")
  gap_p50=0; gap_p99=0; gap_max=0
  if [ "$gap_count" -gt 0 ]; then
    local p50_idx=$(( gap_count / 2 ))
    local p99_idx=$(( gap_count * 99 / 100 ))
    [ "$p50_idx" -lt 1 ] && p50_idx=1
    [ "$p99_idx" -lt 1 ] && p99_idx=1
    gap_p50=$(sed -n "${p50_idx}p" "$gaps_tmp")
    gap_p99=$(sed -n "${p99_idx}p" "$gaps_tmp")
    gap_max=$(tail -1 "$gaps_tmp")
  fi

  cat > "$summary" <<EOF
{
  "name": "$name",
  "wall_time_str": "$wall_str",
  "wall_seconds": $wall_s,
  "pcap_bytes": $pcap_bytes,
  "packet_count": $total_packets,
  "distinct_streams": $distinct_streams,
  "zero_windows": $zero_windows,
  "retransmits": $retransmits,
  "fast_retransmits": $fast_retx,
  "duplicate_acks": $dup_acks,
  "out_of_order": $out_of_order,
  "stream_gap_count": $gap_count,
  "stream_gap_p50_us": $gap_p50,
  "stream_gap_p99_us": $gap_p99,
  "stream_gap_max_us": $gap_max
}
EOF

  rm -f "$stats_tmp" "$gaps_tmp"

  echo "  packets=$total_packets streams=$distinct_streams retx=$retransmits zwin=$zero_windows dup_ack=$dup_acks gap_p99=${gap_p99}us gap_max=${gap_max}us"
}

echo "=== analysis pass ==="
SUMMARIES=()
for pcap in "$PCAP_DIR"/*.pcap; do
  name=$(basename "$pcap" .pcap)
  analyze_pcap "$name"
  SUMMARIES+=("$PCAP_DIR/$name.summary.json")
done

# Aggregate per-capture summaries into a single top-level summary.json.
if command -v jq >/dev/null && [ "${#SUMMARIES[@]}" -gt 0 ]; then
  jq -s '{captures: .}' "${SUMMARIES[@]}" > "$PCAP_DIR/summary.json"
else
  echo "skip summary aggregation: jq not available or no summaries" >&2
fi

echo "done. files:"
ls -lh "$PCAP_DIR"
