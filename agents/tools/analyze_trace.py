#!/usr/bin/env python3
"""
Utoopack Performance Analysis Script
Based on Utoopack Performance Analysis Agent Protocol

Improvements over v1:
- Self-time (exclusive) calculation via proper span tree reconstruction
- Build-phase timeline analysis (resolve → parse → analyze → chunk → codegen → emit)
- Category matching aligned with real Turbopack span names
- Critical path detection (longest sequential dependency chain)
- Comparison mode for regression detection (two trace files)
- TURBOPACK_TASK_STATISTICS integration
- Flame graph collapsed-stack output for flamegraph.pl

Key design decisions:
- Thread work is calculated using merged (non-overlapping) intervals per thread.
- Tasks < 10us are excluded from meaningful analysis (tracing noise).
- 2µs overhead is subtracted from each span duration.
- Self-time = span.dur - sum(child.dur for child in direct_children), clamped to 0.
- Streaming JSON parsing (ijson) is preferred for large traces (> 500MB).
"""

import json
import sys
import os
import math
import argparse
from collections import defaultdict
from datetime import datetime

TRACING_OVERHEAD_US = 2.0  # Approximate overhead per span from instrumentation
NOISE_THRESHOLD_US = 10.0  # Spans below this are tracing noise

# ── Build phases: maps span keywords to logical build phases ──
# Ordered by typical execution sequence
BUILD_PHASES = [
    ("Resolve", ["resolving", "internal resolving", "resolve", "read directory"]),
    ("Parse", ["parse ecmascript", "parse css", "read file"]),
    ("Analyze", [
        "analyze ecmascript module", "process module", "module",
        "compute async module info", "compute binding usage info",
    ]),
    ("Chunk", [
        "chunking", "compute async chunks", "make production chunks",
        "collect mergeable modules", "compute merged modules",
    ]),
    ("Codegen", [
        "code generation", "precompute code generation",
        "generate source map",
    ]),
    ("Emit", ["apply effects", "write file"]),
]

# ── Diagnostic tier categories matching real Turbopack span names ──
TIER_CATEGORIES = {
    "P0: Scheduling & Resolution": [
        "resolving", "internal resolving", "module",
        "process module", "compute async module info",
    ],
    "P1: I/O & Heavy Tasks": [
        "read file", "read directory", "stat",
        "write file", "apply effects",
    ],
    "P2: Architecture (Locks/Memory)": [
        "invalidate", "state value changed",
        "parking_lot", "scheduler",
    ],
    "P3: Asset Pipeline": [
        "parse ecmascript", "parse css",
        "analyze ecmascript module",
        "code generation", "precompute code generation",
        "generate source map", "chunking",
        "compute async chunks", "make production chunks",
        "collect mergeable modules", "compute merged modules",
        "swc", "minify", "transform",
        "compute binding usage info",
    ],
    "P4: Bridge/Interop": ["napi", "wasm", "bridge"],
}


def calc_percentile(data, p):
    if not data:
        return 0.0
    data.sort()
    idx = (len(data) - 1) * p / 100.0
    lower = math.floor(idx)
    upper = math.ceil(idx)
    weight = idx - lower
    if lower == upper:
        return data[int(lower)]
    return data[int(lower)] * (1 - weight) + data[int(upper)] * weight


def _load_events(trace_path):
    size_gb = os.path.getsize(trace_path) / (1024 ** 3)
    if size_gb > 0.5:
        try:
            import ijson
            print(f"   Using streaming parser (ijson) for {size_gb:.1f}GB file...")
            with open(trace_path, "rb") as f:
                first_char = b""
                while True:
                    ch = f.read(1)
                    if not ch:
                        break
                    if ch.strip():
                        first_char = ch
                        break
            prefix = "item" if first_char == b"[" else "traceEvents.item"
            events = []
            with open(trace_path, "rb") as f:
                for obj in ijson.items(f, prefix):
                    events.append(obj)
            if events:
                return events
        except ImportError:
            pass
        except Exception:
            pass
    with open(trace_path, "r") as f:
        data = json.load(f)
    return data if isinstance(data, list) else data.get("traceEvents", [])


def _load_task_statistics(stats_path):
    """Load TURBOPACK_TASK_STATISTICS JSON file if available."""
    if not stats_path or not os.path.exists(stats_path):
        return None
    with open(stats_path, "r") as f:
        return json.load(f)


# ──────────────────────────────────────────────────────────────────────
# Span tree reconstruction — enables self-time & critical path
# ──────────────────────────────────────────────────────────────────────

class Span:
    __slots__ = ("name", "tid", "start", "dur", "parent_idx", "children_idx", "self_time")

    def __init__(self, name, tid, start, dur, parent_idx):
        self.name = name
        self.tid = tid
        self.start = start
        self.dur = dur
        self.parent_idx = parent_idx
        self.children_idx = []
        self.self_time = dur  # adjusted after tree is built


def build_span_tree(events):
    """
    Build a span tree from Chrome Trace B/E/X events.
    Returns list of Span objects with parent-child relationships and self-time.
    """
    spans = []
    # tid -> [(start_ts, name, span_index)]
    stacks = defaultdict(list)

    for event in events:
        ph = event.get("ph")
        if ph not in ("B", "E", "X"):
            continue

        tid = event.get("tid")
        ts = float(event.get("ts", 0))
        name = event.get("name", "unknown")

        if ph == "X":
            dur = float(event.get("dur", 0))
            parent_idx = stacks[tid][-1][2] if stacks[tid] else -1
            idx = len(spans)
            span = Span(name, tid, ts, dur, parent_idx)
            spans.append(span)
            if parent_idx >= 0:
                spans[parent_idx].children_idx.append(idx)
        elif ph == "B":
            parent_idx = stacks[tid][-1][2] if stacks[tid] else -1
            idx = len(spans)
            span = Span(name, tid, ts, 0.0, parent_idx)
            spans.append(span)
            if parent_idx >= 0:
                spans[parent_idx].children_idx.append(idx)
            stacks[tid].append((ts, name, idx))
        elif ph == "E":
            if stacks[tid]:
                start_ts, start_name, span_idx = stacks[tid].pop()
                dur = ts - start_ts
                if dur >= 0:
                    spans[span_idx].dur = dur

    # Compute self-time: dur - sum(direct children durations)
    for span in spans:
        if span.children_idx:
            children_total = sum(spans[ci].dur for ci in span.children_idx)
            span.self_time = max(0.0, span.dur - children_total)
        else:
            span.self_time = span.dur

    return spans


# ──────────────────────────────────────────────────────────────────────
# Build-phase timeline
# ──────────────────────────────────────────────────────────────────────

def classify_phase(name):
    """Return the build phase for a span name, or 'Other'."""
    lower = name.lower()
    for phase_name, keywords in BUILD_PHASES:
        for kw in keywords:
            if kw in lower:
                return phase_name
    return "Other"


def compute_phase_timeline(spans):
    """
    For each build phase, compute:
    - total inclusive duration, total self-time, span count
    - wall-clock range [first_start, last_end]
    """
    phases = {}
    for phase_name, _ in BUILD_PHASES:
        phases[phase_name] = {
            "count": 0, "inclusive": 0.0, "self_time": 0.0,
            "min_ts": float("inf"), "max_ts": float("-inf"),
        }
    phases["Other"] = {
        "count": 0, "inclusive": 0.0, "self_time": 0.0,
        "min_ts": float("inf"), "max_ts": float("-inf"),
    }

    for span in spans:
        if span.dur < NOISE_THRESHOLD_US:
            continue
        phase = classify_phase(span.name)
        p = phases[phase]
        adjusted = max(0, span.dur - TRACING_OVERHEAD_US)
        adjusted_self = max(0, span.self_time - TRACING_OVERHEAD_US)
        p["count"] += 1
        p["inclusive"] += adjusted
        p["self_time"] += adjusted_self
        p["min_ts"] = min(p["min_ts"], span.start)
        p["max_ts"] = max(p["max_ts"], span.start + span.dur)

    return phases


# ──────────────────────────────────────────────────────────────────────
# Critical path detection
# ──────────────────────────────────────────────────────────────────────

def find_critical_paths(spans, top_n=5):
    """
    Find the longest sequential dependency chains by total duration.
    Walks root-to-leaf paths via the longest child at each level.
    Returns list of (total_dur_us, [span_name, ...]) tuples.
    """
    # Find root spans (no parent)
    roots = [i for i, s in enumerate(spans) if s.parent_idx < 0 and s.dur >= NOISE_THRESHOLD_US]

    paths = []
    for root_idx in roots:
        path = []
        total_dur = 0.0
        idx = root_idx
        depth = 0
        while idx >= 0 and depth < 200:
            s = spans[idx]
            path.append(s.name)
            total_dur += max(0, s.self_time - TRACING_OVERHEAD_US)
            # Follow the child with the longest duration
            if s.children_idx:
                best_child = max(s.children_idx, key=lambda ci: spans[ci].dur)
                if spans[best_child].dur >= NOISE_THRESHOLD_US:
                    idx = best_child
                else:
                    break
            else:
                break
            depth += 1
        if len(path) > 1:
            paths.append((total_dur, path))

    paths.sort(key=lambda x: x[0], reverse=True)
    return paths[:top_n]


# ──────────────────────────────────────────────────────────────────────
# Core analysis
# ──────────────────────────────────────────────────────────────────────

def analyze_trace(trace_path, output_path, override_project_name=None,
                  task_stats_path=None, compare_path=None, flamegraph_path=None):
    print(f"Loading trace: {trace_path}")
    trace_size = os.path.getsize(trace_path)
    trace_size_gb = trace_size / (1024 ** 3)
    print(f"   Trace size: {trace_size_gb:.2f} GB")

    events = _load_events(trace_path)
    print(f"   Loaded {len(events):,} events, building span tree...")

    project_name = (
        override_project_name
        or os.environ.get("TRACE_PROJECT")
        or "Unknown Project"
    )

    # ── Build span tree ──
    spans = build_span_tree(events)
    del events  # free memory
    print(f"   Built {len(spans):,} spans with parent-child relationships")

    # ── Thread work (merged non-overlapping intervals) ──
    thread_intervals = defaultdict(list)
    for span in spans:
        if span.dur > 0:
            thread_intervals[span.tid].append((span.start, span.start + span.dur))

    total_thread_work_us = 0.0
    global_min_ts = float("inf")
    global_max_ts = float("-inf")
    working_threads = set()

    for tid, intervals in thread_intervals.items():
        if not intervals:
            continue
        working_threads.add(tid)
        for s, e in intervals:
            global_min_ts = min(global_min_ts, s)
            global_max_ts = max(global_max_ts, e)
        intervals.sort()
        merged = [intervals[0]]
        for s, e in intervals[1:]:
            if s <= merged[-1][1]:
                merged[-1] = (merged[-1][0], max(merged[-1][1], e))
            else:
                merged.append((s, e))
        total_thread_work_us += sum(e - s for s, e in merged)

    # ── Task-level statistics ──
    meaningful_tasks = defaultdict(lambda: {
        "count": 0,
        "inclusive_dur": 0.0,
        "self_dur": 0.0,
        "max_inclusive": 0.0,
        "max_self": 0.0,
        "inclusive_durations": [],
        "self_durations": [],
        "callers": defaultdict(int),
    })
    noise_count = 0
    total_task_count = 0
    buckets = {
        "<10us": 0, "10us-100us": 0, "100us-1ms": 0,
        "1ms-10ms": 0, "10ms-100ms": 0, ">100ms": 0,
    }

    for span in spans:
        total_task_count += 1
        dur = span.dur
        adjusted_dur = max(0, dur - TRACING_OVERHEAD_US)
        adjusted_self = max(0, span.self_time - TRACING_OVERHEAD_US)

        if dur < NOISE_THRESHOLD_US:
            noise_count += 1
            buckets["<10us"] += 1
        else:
            if adjusted_dur < 100:
                buckets["10us-100us"] += 1
            elif adjusted_dur < 1000:
                buckets["100us-1ms"] += 1
            elif adjusted_dur < 10000:
                buckets["1ms-10ms"] += 1
            elif adjusted_dur < 100000:
                buckets["10ms-100ms"] += 1
            else:
                buckets[">100ms"] += 1

            t = meaningful_tasks[span.name]
            t["count"] += 1
            t["inclusive_dur"] += adjusted_dur
            t["self_dur"] += adjusted_self
            t["max_inclusive"] = max(t["max_inclusive"], adjusted_dur)
            t["max_self"] = max(t["max_self"], adjusted_self)
            t["inclusive_durations"].append(adjusted_dur)
            t["self_durations"].append(adjusted_self)

            # Parent attribution via tree structure
            if span.parent_idx >= 0:
                parent_name = spans[span.parent_idx].name
                t["callers"][parent_name] += 1

    # ── Compute percentiles and top callers ──
    for name, t in meaningful_tasks.items():
        t["p95_inclusive"] = calc_percentile(t["inclusive_durations"], 95)
        t["p95_self"] = calc_percentile(t["self_durations"], 95)
        if t["callers"]:
            top_caller = max(t["callers"].items(), key=lambda x: x[1])
            t["top_caller"] = top_caller[0]
            t["top_caller_count"] = top_caller[1]
        else:
            t["top_caller"] = "None"
            t["top_caller_count"] = 0

    # ── Build phase timeline ──
    phase_stats = compute_phase_timeline(spans)

    # ── Critical paths ──
    critical_paths = find_critical_paths(spans)

    # ── Metrics ──
    wall_time_us = global_max_ts - global_min_ts if global_max_ts > global_min_ts else 1
    wall_time_ms = wall_time_us / 1000.0
    thread_work_ms = total_thread_work_us / 1000.0
    num_threads = len(working_threads)
    parallelism = total_thread_work_us / wall_time_us if wall_time_us > 0 else 0
    utilization = (parallelism / num_threads) * 100 if num_threads > 0 else 0
    meaningful_count = sum(t["count"] for t in meaningful_tasks.values())
    total_tasks_safe = max(total_task_count, 1)

    # ── Sort ──
    by_self_time = sorted(
        meaningful_tasks.items(), key=lambda x: x[1]["self_dur"], reverse=True
    )
    by_inclusive_time = sorted(
        meaningful_tasks.items(), key=lambda x: x[1]["inclusive_dur"], reverse=True
    )

    # ── Tier workload distribution ──
    work_denom = max(total_thread_work_us, 1)
    cat_stats = defaultdict(lambda: {"count": 0, "inclusive": 0.0, "self_time": 0.0})
    for name, stats in meaningful_tasks.items():
        lower_name = name.lower()
        matched = False
        for cat, keywords in TIER_CATEGORIES.items():
            if any(kw in lower_name for kw in keywords):
                cat_stats[cat]["count"] += stats["count"]
                cat_stats[cat]["inclusive"] += stats["inclusive_dur"]
                cat_stats[cat]["self_time"] += stats["self_dur"]
                matched = True
                break
        if not matched:
            cat_stats["Other"]["count"] += stats["count"]
            cat_stats["Other"]["inclusive"] += stats["inclusive_dur"]
            cat_stats["Other"]["self_time"] += stats["self_dur"]

    # ── Batching candidates (high-volume, dominated by single caller) ──
    batching_candidates = []
    for name, stats in meaningful_tasks.items():
        if stats["count"] > 5000:
            avg_us = stats["self_dur"] / stats["count"]
            caller_pct = stats["top_caller_count"] / stats["count"] if stats["count"] > 0 else 0
            if caller_pct > 0.70 and avg_us < 500:
                batching_candidates.append((name, stats))
    batching_candidates.sort(key=lambda x: x[1]["self_dur"], reverse=True)

    # ── Format report ──
    report = _format_report(
        trace_path, trace_size_gb, len(spans), project_name,
        wall_time_ms, thread_work_ms, parallelism, num_threads, utilization,
        total_task_count, meaningful_count, noise_count, total_tasks_safe,
        by_self_time, by_inclusive_time, cat_stats, work_denom,
        phase_stats, critical_paths, batching_candidates, buckets,
    )

    # ── Optional: comparison mode ──
    if compare_path and os.path.exists(compare_path):
        report += _generate_comparison(compare_path, wall_time_ms, meaningful_tasks)

    # ── Optional: task statistics integration ──
    task_stats = _load_task_statistics(task_stats_path)
    if task_stats:
        report += _format_task_stats_section(task_stats)

    print(f"Writing report to {output_path}")
    with open(output_path, "w") as f:
        f.write(report)

    # ── Optional: flame graph output ──
    if flamegraph_path:
        _write_flamegraph(spans, flamegraph_path)

    print(f"Report generated successfully!")
    print(f"   Wall Time:       {wall_time_ms:,.1f} ms")
    print(f"   Parallelism:     {parallelism:.1f}x")
    print(f"   Working Threads: {num_threads}")


def _format_report(
    trace_path, trace_size_gb, span_count, project_name,
    wall_time_ms, thread_work_ms, parallelism, num_threads, utilization,
    total_task_count, meaningful_count, noise_count, total_tasks_safe,
    by_self_time, by_inclusive_time, cat_stats, work_denom,
    phase_stats, critical_paths, batching_candidates, buckets,
):
    util_status = (
        "\u26a0\ufe0f Suboptimal" if utilization < 60
        else ("\u2705 Good" if utilization > 80 else "\U0001f197 Average")
    )
    noise_pct = (noise_count * 100) / total_tasks_safe

    report_id = f"utoopack_performance_report_{datetime.now().strftime('%Y%m%d_%H%M%S')}"

    # ── Tier workload rows ──
    tier_order = [
        "P0: Scheduling & Resolution", "P1: I/O & Heavy Tasks",
        "P2: Architecture (Locks/Memory)", "P3: Asset Pipeline",
        "P4: Bridge/Interop", "Other",
    ]
    workload_rows = ""
    for cat_name in tier_order:
        s = cat_stats[cat_name]
        inc_pct = (s["inclusive"] * 100) / work_denom
        self_pct = (s["self_time"] * 100) / work_denom
        workload_rows += (
            f"| {cat_name} | {s['count']:,} "
            f"| {s['inclusive'] / 1000.0:,.1f} | {inc_pct:.1f}% "
            f"| {s['self_time'] / 1000.0:,.1f} | {self_pct:.1f}% |\n"
        )

    # ── Build phase timeline rows ──
    phase_rows = ""
    phase_order = [p[0] for p in BUILD_PHASES] + ["Other"]
    for phase_name in phase_order:
        p = phase_stats.get(phase_name)
        if not p or p["count"] == 0:
            continue
        wall_range_ms = (p["max_ts"] - p["min_ts"]) / 1000.0 if p["max_ts"] > p["min_ts"] else 0
        phase_rows += (
            f"| {phase_name} | {p['count']:,} "
            f"| {p['inclusive'] / 1000.0:,.1f} "
            f"| {p['self_time'] / 1000.0:,.1f} "
            f"| {wall_range_ms:,.1f} |\n"
        )

    # ── Top 20 by self-time ──
    top_self_rows = ""
    for name, stats in by_self_time[:20]:
        avg_self_us = stats["self_dur"] / stats["count"]
        p95_self_ms = stats["p95_self"] / 1000.0
        total_self_ms = stats["self_dur"] / 1000.0
        total_inc_ms = stats["inclusive_dur"] / 1000.0
        max_self_ms = stats["max_self"] / 1000.0
        self_pct = (stats["self_dur"] * 100) / work_denom
        caller = stats["top_caller"]
        caller_pct = (stats["top_caller_count"] / stats["count"]) * 100 if stats["count"] > 0 else 0
        top_self_rows += (
            f"| {total_self_ms:,.1f} | {total_inc_ms:,.1f} | {stats['count']:,} "
            f"| {avg_self_us:.1f} | {p95_self_ms:,.1f} | {max_self_ms:,.1f} "
            f"| {self_pct:.1f}% | `{name}` | `{caller}` ({caller_pct:.0f}%) |\n"
        )

    # ── Critical path rows ──
    critical_path_rows = ""
    for i, (total_dur, path) in enumerate(critical_paths):
        # Truncate long paths for readability
        if len(path) > 8:
            display = " → ".join(path[:3]) + " → ... → " + " → ".join(path[-3:])
        else:
            display = " → ".join(path)
        critical_path_rows += f"| {i + 1} | {total_dur / 1000.0:,.1f} | {len(path)} | {display} |\n"
    if not critical_path_rows:
        critical_path_rows = "| - | - | - | No critical paths detected |\n"

    # ── Batching candidates ──
    batching_rows = ""
    for name, stats in batching_candidates[:10]:
        total_ms = stats["self_dur"] / 1000.0
        avg_us = stats["self_dur"] / stats["count"]
        caller = stats["top_caller"]
        caller_pct = (stats["top_caller_count"] / stats["count"]) * 100
        p95_ms = stats["p95_self"] / 1000.0
        batching_rows += (
            f"| `{name}` | {stats['count']:,} "
            f"| `{caller}` ({caller_pct:.0f}%) "
            f"| {avg_us:.1f} us | {p95_ms:,.2f} ms | {total_ms:,.1f} ms |\n"
        )
    if not batching_rows:
        batching_rows = "| No obvious batching candidates found | - | - | - | - | - |\n"

    # ── Duration distribution ──
    bucket_rows = ""
    for label in ["<10us", "10us-100us", "100us-1ms", "1ms-10ms", "10ms-100ms", ">100ms"]:
        bucket_rows += (
            f"| {label} | {buckets[label]:,} "
            f"| {buckets[label] * 100 / total_tasks_safe:.1f}% |\n"
        )

    return f"""# Utoopack Performance Report

**Report ID**: `{report_id}`
**Generated**: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}
**Trace File**: `{os.path.basename(trace_path)}` ({trace_size_gb:.1f}GB, {span_count / 1e6:.2f}M spans)
**Test Project**: `{project_name}`

---

## Executive Summary

| Metric | Value | Assessment |
|--------|-------|------------|
| Total Wall Time | **{wall_time_ms:,.1f} ms** | Baseline |
| Total Thread Work (de-duped) | **{thread_work_ms:,.1f} ms** | Non-overlapping busy time |
| Effective Parallelism | **{parallelism:.1f}x** | thread_work / wall_time |
| Working Threads | **{num_threads}** | Threads with actual spans |
| Thread Utilization | **{utilization:.1f}%** | {util_status} |
| Total Spans | **{total_task_count:,}** | All B/E + X events |
| Meaningful Spans (>= 10us) | **{meaningful_count:,}** | ({meaningful_count * 100 / total_tasks_safe:.1f}% of total) |
| Tracing Noise (< 10us) | **{noise_count:,}** | ({noise_pct:.1f}% of total) |

---

## Build Phase Timeline

Shows when each build phase is active and how much CPU it consumes.
**Self-Time** is the time spent *exclusively* in that phase (excluding children).

| Phase | Spans | Inclusive (ms) | Self-Time (ms) | Wall Range (ms) |
|-------|-------|----------------|----------------|-----------------|
{phase_rows}
---

## Workload Distribution by Diagnostic Tier

| Category | Spans | Inclusive (ms) | % Work | Self-Time (ms) | % Self |
|----------|-------|----------------|--------|----------------|--------|
{workload_rows}
---

## Top 20 Tasks by Self-Time

Self-time is the *exclusive* duration: time spent in the task itself, not in sub-tasks.
This is the most accurate indicator of where CPU cycles are actually spent.

| Self (ms) | Inclusive (ms) | Count | Avg Self (us) | P95 Self (ms) | Max Self (ms) | % Work | Task Name | Top Caller |
|-----------|----------------|-------|---------------|---------------|---------------|--------|-----------|------------|
{top_self_rows}
---

## Critical Path Analysis

The longest sequential dependency chains that determine wall-clock time.
Focus on reducing the depth of these chains to improve parallelism.

| Rank | Self-Time (ms) | Depth | Path |
|------|----------------|-------|------|
{critical_path_rows}
---

## Batching Candidates

High-volume tasks dominated by a single parent. If the parent can batch them,
it drastically reduces scheduler overhead.

| Task Name | Count | Top Caller (Attribution) | Avg Self | P95 Self | Total Self |
|-----------|-------|--------------------------|----------|----------|------------|
{batching_rows}
---

## Duration Distribution

| Range | Count | Percentage |
|-------|-------|------------|
{bucket_rows}
---

## Action Items
1. **[P0]** Focus on tasks with the highest **Self-Time** — these are where CPU cycles are *actually* spent.
2. **[P0]** Use Batching Candidates to identify callers that should use `try_join` or reduce `#[turbo_tasks::function]` granularity.
3. **[P1]** Check Build Phase Timeline for phases with disproportionate wall range vs. self-time (= serialization).
4. **[P1]** Inspect `P95 Self (ms)` for heavy monolith tasks. Focus on long-tail outliers, not averages.
5. **[P1]** Review Critical Paths — reducing the longest chain depth directly improves wall-clock time.
6. **[P2]** If Thread Utilization < 60%, investigate scheduling gaps (lock contention or deep dependency chains).

*Report generated by Utoopack Performance Analysis Agent*
"""


def _generate_comparison(compare_path, current_wall_ms, current_tasks):
    """Generate a comparison section against a baseline trace."""
    try:
        events2 = _load_events(compare_path)
        spans2 = build_span_tree(events2)
        del events2

        # Baseline wall time
        g_min, g_max = float("inf"), float("-inf")
        for span in spans2:
            if span.dur > 0:
                g_min = min(g_min, span.start)
                g_max = max(g_max, span.start + span.dur)
        baseline_wall_ms = (g_max - g_min) / 1000.0 if g_max > g_min else 1

        # Baseline task self-times
        baseline_tasks = defaultdict(lambda: {"count": 0, "self_dur": 0.0})
        for span in spans2:
            if span.dur >= NOISE_THRESHOLD_US:
                adjusted_self = max(0, span.self_time - TRACING_OVERHEAD_US)
                baseline_tasks[span.name]["count"] += 1
                baseline_tasks[span.name]["self_dur"] += adjusted_self

        wall_diff = current_wall_ms - baseline_wall_ms
        wall_pct = (wall_diff / baseline_wall_ms) * 100 if baseline_wall_ms > 0 else 0

        # Find tasks with largest self-time regressions
        regressions = []
        for name, stats in current_tasks.items():
            baseline = baseline_tasks.get(name)
            if baseline and baseline["self_dur"] > 0:
                delta = stats["self_dur"] - baseline["self_dur"]
                delta_pct = (delta / baseline["self_dur"]) * 100
                if abs(delta) > 1000:  # only show > 1ms changes
                    regressions.append((name, stats["self_dur"], baseline["self_dur"], delta, delta_pct))
        regressions.sort(key=lambda x: abs(x[3]), reverse=True)

        rows = ""
        for name, curr, base, delta, pct in regressions[:15]:
            sign = "+" if delta > 0 else ""
            emoji = "\U0001f534" if pct > 20 else ("\U0001f7e1" if pct > 5 else "\U0001f7e2")
            rows += (
                f"| {emoji} `{name}` | {base / 1000:.1f} | {curr / 1000:.1f} "
                f"| {sign}{delta / 1000:.1f} | {sign}{pct:.1f}% |\n"
            )
        if not rows:
            rows = "| No significant regressions detected | - | - | - | - |\n"

        sign = "+" if wall_diff > 0 else ""
        return f"""
---

## \U0001f4ca Comparison vs Baseline

**Baseline Trace**: `{os.path.basename(compare_path)}`
**Wall Time Delta**: {sign}{wall_diff:,.1f} ms ({sign}{wall_pct:.1f}%)

| Task | Baseline Self (ms) | Current Self (ms) | Delta (ms) | Delta % |
|------|--------------------|-------------------|------------|---------|
{rows}
"""
    except Exception as e:
        return f"\n\n---\n\n## Comparison Error\n\nFailed to load comparison trace: {e}\n"


def _format_task_stats_section(task_stats):
    """Format TURBOPACK_TASK_STATISTICS data into a report section."""
    rows = ""
    if isinstance(task_stats, dict):
        sorted_stats = sorted(
            task_stats.items(),
            key=lambda x: x[1].get("executions", 0) if isinstance(x[1], dict) else 0,
            reverse=True,
        )
        for name, stats in sorted_stats[:20]:
            if isinstance(stats, dict):
                execs = stats.get("executions", 0)
                cache_hits = stats.get("cache_hits", 0)
                cache_misses = stats.get("cache_misses", 0)
                total_reqs = cache_hits + cache_misses
                hit_rate = (cache_hits * 100 / total_reqs) if total_reqs > 0 else 0
                rows += f"| `{name}` | {execs:,} | {cache_hits:,} | {cache_misses:,} | {hit_rate:.1f}% |\n"
    if not rows:
        return ""

    return f"""
---

## \U0001f4c8 Task Statistics (Turbo Engine)

Data from `TURBOPACK_TASK_STATISTICS`. Shows task execution counts and cache efficiency.

| Task | Executions | Cache Hits | Cache Misses | Hit Rate |
|------|------------|------------|--------------|----------|
{rows}
"""


def _write_flamegraph(spans, output_path):
    """
    Write collapsed-stack format for flamegraph.pl / speedscope.
    Each line: ancestor;parent;child self_time_us
    """
    lines = defaultdict(int)
    for span in spans:
        if span.dur < NOISE_THRESHOLD_US:
            continue
        stack = [span.name]
        idx = span.parent_idx
        depth = 0
        while idx >= 0 and depth < 100:
            stack.append(spans[idx].name)
            idx = spans[idx].parent_idx
            depth += 1
        stack.reverse()
        key = ";".join(stack)
        self_us = max(0, int(span.self_time - TRACING_OVERHEAD_US))
        if self_us > 0:
            lines[key] += self_us

    with open(output_path, "w") as f:
        for stack, value in sorted(lines.items(), key=lambda x: -x[1]):
            f.write(f"{stack} {value}\n")
    print(f"   Flame graph data written to {output_path}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(
        description="Utoopack Performance Trace Analyzer",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  # Basic analysis
  python3 analyze_trace.py trace.json report.md

  # With comparison against baseline
  python3 analyze_trace.py trace.json report.md --compare baseline_trace.json

  # With task statistics and flame graph
  python3 analyze_trace.py trace.json report.md --task-stats stats.json --flamegraph trace.folded

  # Specify project name
  python3 analyze_trace.py trace.json report.md --project examples/with-antd
        """,
    )
    parser.add_argument("trace_file", help="Path to Chrome Trace JSON file")
    parser.add_argument("output_file", help="Path to write the report markdown")
    parser.add_argument("--project", help="Project name for the report header")
    parser.add_argument("--compare", help="Path to baseline trace file for regression comparison")
    parser.add_argument("--task-stats", help="Path to TURBOPACK_TASK_STATISTICS JSON file")
    parser.add_argument("--flamegraph", help="Path to write collapsed-stack format for flamegraph.pl")

    # Support legacy positional-only invocation: analyze_trace.py <trace> <output> [project]
    if len(sys.argv) >= 3 and not sys.argv[1].startswith("-") and not sys.argv[2].startswith("-"):
        has_flags = any(a.startswith("-") for a in sys.argv[3:])
        if not has_flags and len(sys.argv) <= 4:
            trace = sys.argv[1]
            output = sys.argv[2]
            proj = sys.argv[3] if len(sys.argv) > 3 else None
            analyze_trace(trace, output, override_project_name=proj)
            sys.exit(0)

    args = parser.parse_args()
    analyze_trace(
        args.trace_file,
        args.output_file,
        override_project_name=args.project,
        task_stats_path=args.task_stats,
        compare_path=args.compare,
        flamegraph_path=args.flamegraph,
    )
