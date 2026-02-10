#!/usr/bin/env python3
"""
Utoopack Performance Analysis Script
Based on Utoopack Performance Analysis Agent Protocol

Key design decisions:
- Thread work is calculated using ONLY top-level (depth=0) spans per thread
  to avoid double-counting nested spans. This gives accurate thread utilization.
- Tasks < 10us are excluded from meaningful analysis (tracing instrumentation noise).
- Streaming JSON parsing (ijson) is preferred for large traces, with json.load fallback.
- Thread count only includes threads with actual work events (B/E/X), not metadata-only threads.

Usage: python3 analyze_trace.py <trace_file> <output_report> [project_name]
"""

import json
import sys
import os
from collections import defaultdict
from datetime import datetime


def analyze_trace(trace_path, output_path, override_project_name=None):
    """Analyze Chrome Trace and generate performance report following the protocol"""

    print(f"Loading trace: {trace_path}")
    trace_size = os.path.getsize(trace_path)
    trace_size_gb = trace_size / (1024**3)
    print(f"   Trace size: {trace_size_gb:.2f} GB")

    # Try streaming parse for large files, fall back to json.load
    events = _load_events(trace_path)
    print(f"   Analyzing {len(events):,} events...")

    project_name = override_project_name or os.environ.get("TRACE_PROJECT") or "Unknown Project"

    # ── Pass 1: Pair B/E events into complete spans, collect X events ──
    # Each span: (name, tid, start_ts, duration)
    spans = []
    stacks = defaultdict(list)  # tid -> [(start_ts, name), ...]

    for event in events:
        ph = event.get('ph')
        if ph not in ('B', 'E', 'X'):
            continue

        tid = event.get('tid')
        ts = event.get('ts', 0)
        name = event.get('name', 'unknown')

        # Ensure numeric types are float (ijson may return Decimal)
        ts = float(ts) if ts else 0.0

        if ph == 'X':
            dur = float(event.get('dur', 0))
            spans.append((name, tid, ts, dur))
        elif ph == 'B':
            stacks[tid].append((ts, name))
        elif ph == 'E':
            if stacks[tid]:
                start_ts, start_name = stacks[tid].pop()
                dur = ts - start_ts
                if dur >= 0:
                    spans.append((start_name, tid, start_ts, dur))

    print(f"   Paired {len(spans):,} complete spans")

    # ── Pass 2: Calculate thread work WITHOUT nesting overlap ──
    # Group spans by thread, then compute non-overlapping coverage
    thread_spans = defaultdict(list)
    for name, tid, start, dur in spans:
        if dur > 0:
            thread_spans[tid].append((start, start + dur))

    # For each thread, merge overlapping intervals to get true busy time
    total_thread_work_us = 0.0
    global_min_ts = float('inf')
    global_max_ts = float('-inf')
    working_threads = set()

    for tid, intervals in thread_spans.items():
        if not intervals:
            continue
        working_threads.add(tid)

        # Update global time range
        for s, e in intervals:
            global_min_ts = min(global_min_ts, s)
            global_max_ts = max(global_max_ts, e)

        # Merge overlapping intervals
        intervals.sort()
        merged = [intervals[0]]
        for s, e in intervals[1:]:
            if s <= merged[-1][1]:
                merged[-1] = (merged[-1][0], max(merged[-1][1], e))
            else:
                merged.append((s, e))

        thread_busy = sum(e - s for s, e in merged)
        total_thread_work_us += thread_busy

    # ── Pass 3: Task-level statistics (per-span, including nested) ──
    # This is for identifying hotspots - nesting is fine here since we want
    # to know which *functions* are expensive, not thread occupancy.
    meaningful_tasks = defaultdict(lambda: {'count': 0, 'duration': 0.0, 'max': 0.0})
    noise_count = 0
    total_task_count = 0
    buckets = {'<10us': 0, '10us-100us': 0, '100us-1ms': 0, '1ms-10ms': 0, '10ms-100ms': 0, '>100ms': 0}

    for name, tid, start, dur in spans:
        total_task_count += 1
        if dur < 10:
            noise_count += 1
            buckets['<10us'] += 1
        else:
            if dur < 100:
                buckets['10us-100us'] += 1
            elif dur < 1000:
                buckets['100us-1ms'] += 1
            elif dur < 10000:
                buckets['1ms-10ms'] += 1
            elif dur < 100000:
                buckets['10ms-100ms'] += 1
            else:
                buckets['>100ms'] += 1

            meaningful_tasks[name]['count'] += 1
            meaningful_tasks[name]['duration'] += dur
            meaningful_tasks[name]['max'] = max(meaningful_tasks[name]['max'], dur)

    # ── Metrics ──
    wall_time_us = global_max_ts - global_min_ts if global_max_ts > global_min_ts else 1
    wall_time_ms = wall_time_us / 1000.0
    thread_work_ms = total_thread_work_us / 1000.0
    num_threads = len(working_threads)
    parallelism = total_thread_work_us / wall_time_us if wall_time_us > 0 else 0
    utilization = (parallelism / num_threads) * 100 if num_threads > 0 else 0

    meaningful_count = sum(t['count'] for t in meaningful_tasks.values())
    total_tasks_safe = max(total_task_count, 1)

    # Sort tasks
    by_duration = sorted(meaningful_tasks.items(), key=lambda x: x[1]['duration'], reverse=True)

    # Categorize for tiered analysis
    categories = {
        'P0: Runtime/Resolution': ['resolve', 'plugin', 'turbo_tasks'],
        'P1: I/O & Heavy Tasks': ['read', 'stat', 'fs', 'analyze ecmascript'],
        'P3: Asset Pipeline': ['parse', 'transform', 'analyze', 'chunk', 'minify', 'generate', 'swc'],
        'P4: Bridge/Interop': ['napi', 'wasm', 'bridge']
    }

    cat_stats = defaultdict(lambda: {'count': 0, 'duration': 0.0})
    for name, stats in meaningful_tasks.items():
        lower_name = name.lower()
        matched = False
        for cat, keywords in categories.items():
            if any(kw in lower_name for kw in keywords):
                cat_stats[cat]['count'] += stats['count']
                cat_stats[cat]['duration'] += stats['duration']
                matched = True
                break
        if not matched:
            cat_stats['Other']['count'] += stats['count']
            cat_stats['Other']['duration'] += stats['duration']

    # ── Generate Report ──
    report_id = f"utoopack_performance_report_{datetime.now().strftime('%Y%m%d_%H%M%S')}"
    trace_file = os.path.basename(trace_path)

    # Utilization status
    if utilization < 60:
        util_status = '\u26a0\ufe0f Suboptimal'
    elif utilization > 80:
        util_status = '\u2705 Good'
    else:
        util_status = '\U0001f197 Average'

    # Workload Distribution Table Rows
    # Use thread_work (non-overlapping) as denominator for percentage
    work_denom = max(total_thread_work_us, 1)
    workload_rows = ""
    for cat_name in ['P0: Runtime/Resolution', 'P1: I/O & Heavy Tasks', 'P3: Asset Pipeline', 'P4: Bridge/Interop', 'Other']:
        s = cat_stats[cat_name]
        dur_ms = s['duration'] / 1000.0
        # Note: these percentages can exceed 100% in aggregate because task durations
        # include nesting, while thread_work is de-duped. This is expected and useful
        # for understanding where time is *attributed* even if overlapping.
        pct = (s['duration'] * 100) / work_denom
        workload_rows += f"| {cat_name} | {s['count']:,} | {dur_ms:,.1f} | {pct:.1f}% |\n"

    # Top 20 tasks table
    top_tasks_rows = ""
    for name, stats in by_duration[:20]:
        avg_us = stats['duration'] / stats['count']
        total_ms = stats['duration'] / 1000.0
        max_ms = stats['max'] / 1000.0
        pct_work = (stats['duration'] * 100) / work_denom
        top_tasks_rows += f"| {total_ms:,.1f} | {stats['count']:,} | {avg_us:.1f} | {max_ms:,.1f} | {pct_work:.1f}% | `{name}` |\n"

    noise_pct = (noise_count * 100) / total_tasks_safe
    lost_parallelism = max(0, int((1 - utilization / 100) * 100))

    # P0 action
    if utilization < 70:
        p0_action = f"1. **[P0]** Profile lock contention to address {lost_parallelism}% lost parallelism"
    else:
        p0_action = "1. **[P0]** Investigate task scheduling gaps for incremental gains"

    # Diagnostic signals
    noise_signal = '\u26a0\ufe0f **Significant**' if noise_pct > 60 else '\u2705 Acceptable'
    util_signal = '\U0001f6a8 **Low**' if utilization < 60 else ('\U0001f197 Average' if utilization < 80 else '\u2705 Good')
    monolith_signal = '\u26a0\ufe0f **Detected**' if buckets['>100ms'] > 5 else '\u2705 Minimal'
    p1_io_status = '\u26a0\ufe0f Bound' if cat_stats['P1: I/O & Heavy Tasks']['duration'] / work_denom > 0.3 else '\u2705 Healthy'
    p0_sched_status = '\u26a0\ufe0f High' if cat_stats['P0: Runtime/Resolution']['duration'] / work_denom > 0.4 else '\u2705 Normal'
    p4_status = '\u26a0\ufe0f High' if cat_stats['P4: Bridge/Interop']['duration'] / work_denom > 0.1 else '\u2705 Low'
    p4_examine_signal = '\u26a0\ufe0f Examine' if cat_stats['P4: Bridge/Interop']['duration'] / work_denom > 0.1 else 'Low'
    throughput_assessment = '**reasonable** throughput' if utilization > 70 else '**significant loss** of potential parallelism'

    report = f"""# Utoopack Performance Report

**Report ID**: `{report_id}`
**Generated**: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}
**Trace File**: `{trace_file}` ({trace_size_gb:.1f}GB, {len(events)/1e6:.2f}M events)
**Test Project**: `{project_name}`

---

## Executive Summary

### Key Findings

| Metric | Value | Assessment |
|--------|-------|------------|
| Total Wall Time | **{wall_time_ms:,.1f} ms** | Baseline |
| Total Thread Work (de-duped) | **{thread_work_ms:,.1f} ms** | Non-overlapping busy time |
| Effective Parallelism | **{parallelism:.1f}x** | thread_work / wall_time |
| Working Threads | **{num_threads}** | Threads with actual spans |
| Thread Utilization | **{utilization:.1f}%** | {util_status} |
| Total Spans | **{total_task_count:,}** | All B/E + X events |
| Meaningful Spans (>= 10us) | **{meaningful_count:,}** | ({meaningful_count*100/total_tasks_safe:.1f}% of total) |
| Tracing Noise (< 10us) | **{noise_count:,}** | ({noise_pct:.1f}% of total) |

> **Note on Thread Work**: Thread work is computed by merging overlapping intervals
> per thread, eliminating double-counting from nested spans. This gives the true
> wall-clock busy time across all threads.

### Workload Distribution by Tier

| Category | Tasks | Total Time (ms) | % of Thread Work |
|----------|-------|-----------------|------------------|
{workload_rows}
> **Note**: Percentages may sum to >100% because task durations include nesting
> while thread work is de-duplicated. This is intentional for hotspot attribution.

---

## Parallelization Analysis

### Thread Utilization

| Metric | Value |
|--------|-------|
| Working Threads | {num_threads} |
| Total Thread Work (de-duped) | {thread_work_ms:,.1f} ms |
| Avg Work per Thread | {thread_work_ms/max(num_threads,1):,.1f} ms |
| Effective Parallelism | {parallelism:.2f}x |
| Thread Utilization | **{utilization:.1f}%** |

**Assessment**: With {num_threads} working threads, achieving {parallelism:.1f}x parallelism indicates {throughput_assessment}.

---

## Top 20 Tasks by Total Duration

| Total (ms) | Count | Avg (us) | Max (ms) | % Work | Task Name |
|------------|-------|----------|----------|--------|-----------|
{top_tasks_rows}
---

## Deep Dive by Tier

### Tier 1: Runtime & Resolution (P0)
*Focus: Task scheduling and dependency resolution.*

| Metric | Value | Status |
|--------|-------|--------|
| Total Scheduling Time | {cat_stats['P0: Runtime/Resolution']['duration']/1000:,.1f} ms | {p0_sched_status} |
| Resolution Hotspots | {len([n for n in meaningful_tasks if 'resolve' in n.lower()])} distinct task types | Check Top Tasks |

**Potential P0 Issues**:
- Thread utilization at {utilization:.1f}% {'suggests critical path serialization or lock contention.' if utilization < 70 else 'is reasonable but may have room for improvement.'}
- {noise_count:,} spans < 10us ({noise_pct:.1f}%) contribute to scheduler pressure.

### Tier 2: Physical & Resource Barriers (P1)
*Focus: Hardware utilization, I/O, and heavy monoliths.*

| Metric | Value | Status |
|--------|-------|--------|
| I/O Work (Estimated) | {cat_stats['P1: I/O & Heavy Tasks']['duration']/1000:,.1f} ms | {p1_io_status} |
| Large Tasks (> 100ms) | {buckets['>100ms']} | {'Critical' if buckets['>100ms'] > 5 else 'Minimal'} |

### Tier 3: Architecture & Asset Pipeline (P2-P3)
*Focus: Global state and transformation pipeline.*

| Metric | Value | Status |
|--------|-------|--------|
| Asset Processing (P3) | {cat_stats['P3: Asset Pipeline']['duration']/1000:,.1f} ms | {cat_stats['P3: Asset Pipeline']['duration']*100/work_denom:.1f}% of work |
| Bridge Overhead (P4) | {cat_stats['P4: Bridge/Interop']['duration']/1000:,.1f} ms | {p4_status} |

---

## Duration Distribution

| Range | Count | Percentage |
|-------|-------|------------|
| < 10us (noise) | {buckets['<10us']:,} | {buckets['<10us']*100/total_tasks_safe:.1f}% |
| 10us - 100us | {buckets['10us-100us']:,} | {buckets['10us-100us']*100/total_tasks_safe:.1f}% |
| 100us - 1ms | {buckets['100us-1ms']:,} | {buckets['100us-1ms']*100/total_tasks_safe:.1f}% |
| 1ms - 10ms | {buckets['1ms-10ms']:,} | {buckets['1ms-10ms']*100/total_tasks_safe:.1f}% |
| 10ms - 100ms | {buckets['10ms-100ms']:,} | {buckets['10ms-100ms']*100/total_tasks_safe:.1f}% |
| > 100ms | {buckets['>100ms']:,} | {buckets['>100ms']*100/total_tasks_safe:.1f}% |

---

## Diagnostic Signal Summary

| Signal | Status | Finding |
|--------|--------|---------|
| Tracing Noise (P0) | {noise_signal} | {noise_pct:.1f}% of spans < 10us |
| Thread Utilization (P0) | {util_signal} | {utilization:.1f}% utilization |
| Heavy Monoliths (P1) | {monolith_signal} | {buckets['>100ms']} tasks > 100ms |
| Asset Pipeline (P3) | Review | {cat_stats['P3: Asset Pipeline']['duration']/1000:,.1f} ms total |
| Bridge/Interop (P4) | {p4_examine_signal} | {cat_stats['P4: Bridge/Interop']['duration']/1000:,.1f} ms total |

---

## Action Items (P0-P4)

{p0_action}
2. **[P1]** Breakdown heavy monolith tasks (>100ms) to improve granularity
3. **[P1]** Review I/O patterns for potential batching opportunities
4. **[P3]** Optimize asset transformation pipeline hot-spots
5. **[P4]** Reduce "chatty" bridge operations if interop overhead is significant

---

*Report generated by Utoopack Performance Analysis Agent on {datetime.now().strftime('%Y-%m-%d')}*
*Following: Utoopack Performance Analysis Agent Protocol*
"""

    print(f"Writing report to {output_path}")
    with open(output_path, 'w') as f:
        f.write(report)

    print(f"Report generated successfully!")
    print(f"\nQuick Summary:")
    print(f"   Wall Time:       {wall_time_ms:,.1f} ms")
    print(f"   Thread Work:     {thread_work_ms:,.1f} ms (de-duped)")
    print(f"   Parallelism:     {parallelism:.1f}x")
    print(f"   Working Threads: {num_threads}")
    print(f"   Thread Util:     {utilization:.1f}%")
    print(f"   Meaningful Tasks:{meaningful_count:,}")


def _load_events(trace_path):
    """Load trace events. Uses streaming for large files if ijson is available."""
    size_gb = os.path.getsize(trace_path) / (1024**3)

    # Try ijson for large files
    if size_gb > 0.5:
        try:
            import ijson
            print(f"   Using streaming parser (ijson) for {size_gb:.1f}GB file...")

            # Peek at first non-whitespace char to determine format
            with open(trace_path, 'rb') as f:
                first_char = b''
                while True:
                    ch = f.read(1)
                    if not ch:
                        break
                    if ch.strip():
                        first_char = ch
                        break

            # JSON array [...] uses 'item', object {"traceEvents":[...]} uses 'traceEvents.item'
            prefix = 'item' if first_char == b'[' else 'traceEvents.item'
            print(f"   Detected format: {'array' if first_char == b'[' else 'object'}, using prefix '{prefix}'")

            events = []
            with open(trace_path, 'rb') as f:
                for obj in ijson.items(f, prefix):
                    events.append(obj)
            if events:
                return events
            print(f"   ijson returned 0 events, falling back to json.load...")
        except ImportError:
            print(f"   ijson not available, falling back to json.load (may use significant memory)...")
        except Exception as e:
            print(f"   ijson error: {e}, falling back to json.load...")

    with open(trace_path, 'r') as f:
        data = json.load(f)
    return data if isinstance(data, list) else data.get("traceEvents", [])


if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Usage: python3 analyze_trace.py <trace_file> <output_report> [project_name]")
        sys.exit(1)

    trace_file = sys.argv[1]
    report_file = sys.argv[2]
    override_project_name = sys.argv[3] if len(sys.argv) > 3 else None

    if not os.path.exists(trace_file):
        print(f"Error: Trace file not found: {trace_file}")
        sys.exit(1)

    analyze_trace(trace_file, report_file, override_project_name)
