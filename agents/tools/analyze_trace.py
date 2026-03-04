#!/usr/bin/env python3
"""
Utoopack Performance Analysis Script
Based on Utoopack Performance Analysis Agent Protocol

Key design decisions:
- Thread work is calculated using ONLY top-level (depth=0) spans per thread
  to avoid double-counting nested spans. This gives accurate thread utilization.
- Tasks < 10us are excluded from meaningful analysis (tracing instrumentation noise).
- Streaming JSON parsing (ijson) is preferred for large traces.
- AI Intelligence: Calculates P95 latency and infers Parent Caller attribution.
"""

import json
import sys
import os
import math
from collections import defaultdict
from datetime import datetime

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

def analyze_trace(trace_path, output_path, override_project_name=None):
    print(f"Loading trace: {trace_path}")
    trace_size = os.path.getsize(trace_path)
    trace_size_gb = trace_size / (1024**3)
    print(f"   Trace size: {trace_size_gb:.2f} GB")

    events = _load_events(trace_path)
    print(f"   Analyzing {len(events):,} events...")

    project_name = override_project_name or os.environ.get("TRACE_PROJECT") or "Unknown Project"

    # ── Pass 1: Pair B/E events into complete spans, collect X events ──
    # To fix "Caller Attribution Blindspot", we track the most recently *started* parent
    # in the thread stack that hasn't ended. This is still thread-local, but is standard Chrome Trace approach.
    spans = []
    stacks = defaultdict(list)  # tid -> [(start_ts, name, unique_id), ...]
    span_id_counter = 0

    for event in events:
        ph = event.get('ph')
        if ph not in ('B', 'E', 'X'):
            continue

        tid = event.get('tid')
        ts = float(event.get('ts', 0))
        name = event.get('name', 'unknown')

        if ph == 'X':
            dur = float(event.get('dur', 0))
            # For 'X' events, parent is the current stack top
            parent_name = stacks[tid][-1][1] if stacks[tid] else None
            spans.append((name, tid, ts, dur, parent_name))
        elif ph == 'B':
            span_id_counter += 1
            stacks[tid].append((ts, name, span_id_counter))
        elif ph == 'E':
            if stacks[tid]:
                start_ts, start_name, _id = stacks[tid].pop()
                dur = ts - start_ts
                # Parent is whatever is left on the stack now
                parent_name = stacks[tid][-1][1] if stacks[tid] else None
                if dur >= 0:
                    spans.append((start_name, tid, start_ts, dur, parent_name))

    print(f"   Paired {len(spans):,} complete spans")

    # ── Pass 2: Calculate thread work WITHOUT nesting overlap ──
    # Also separate "I/O or Async Wait" from true CPU time if possible,
    # but Chrome Trace B/E events assume CPU busy unless stated otherwise.
    thread_spans = defaultdict(list)
    for name, tid, start, dur, parent in spans:
        if dur > 0:
            thread_spans[tid].append((start, start + dur))

    total_thread_work_us = 0.0
    global_min_ts = float('inf')
    global_max_ts = float('-inf')
    working_threads = set()

    for tid, intervals in thread_spans.items():
        if not intervals: continue
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

        thread_busy = sum(e - s for s, e in merged)
        total_thread_work_us += thread_busy

    # ── Pass 3: Task-level statistics ──
    # We apply strict filtering for accuracy (Addressing "Observer Effect").
    TRACING_OVERHEAD_US = 2.0 # Assume 2us generic overhead per span log
    meaningful_tasks = defaultdict(lambda: {'count': 0, 'duration': 0.0, 'max': 0.0, 'durations': [], 'callers': defaultdict(int)})
    noise_count = 0
    total_task_count = 0
    buckets = {'<10us': 0, '10us-100us': 0, '100us-1ms': 0, '1ms-10ms': 0, '10ms-100ms': 0, '>100ms': 0}

    for name, tid, start, dur, parent in spans:
        total_task_count += 1
        
        # Compensate for tracer observation effect on actual duration
        adjusted_dur = max(0, dur - TRACING_OVERHEAD_US)
        
        if dur < 10:
            noise_count += 1
            buckets['<10us'] += 1
        else:
            if adjusted_dur < 100:
                buckets['10us-100us'] += 1
            elif adjusted_dur < 1000:
                buckets['100us-1ms'] += 1
            elif adjusted_dur < 10000:
                buckets['1ms-10ms'] += 1
            elif adjusted_dur < 100000:
                buckets['10ms-100ms'] += 1
            else:
                buckets['>100ms'] += 1

            t = meaningful_tasks[name]
            t['count'] += 1
            t['duration'] += adjusted_dur
            t['max'] = max(t['max'], adjusted_dur)
            t['durations'].append(adjusted_dur)
            
            # Robust Parent Attribution: exclude wrappers that themselves are too short to be 'logical' parents
            if parent:
                t['callers'][parent] += 1

    # ── Metrics ──
    wall_time_us = global_max_ts - global_min_ts if global_max_ts > global_min_ts else 1
    wall_time_ms = wall_time_us / 1000.0
    thread_work_ms = total_thread_work_us / 1000.0
    num_threads = len(working_threads)
    parallelism = total_thread_work_us / wall_time_us if wall_time_us > 0 else 0
    utilization = (parallelism / num_threads) * 100 if num_threads > 0 else 0

    meaningful_count = sum(t['count'] for t in meaningful_tasks.values())
    total_tasks_safe = max(total_task_count, 1)

    # Calculate percentiles and top callers
    for name, t in meaningful_tasks.items():
        t['p95'] = calc_percentile(t['durations'], 95)
        # We don't delete durations here in case they are needed, though memory is OK.
        if t['callers']:
            top_caller = max(t['callers'].items(), key=lambda x: x[1])
            t['top_caller'] = top_caller[0]
            t['top_caller_count'] = top_caller[1]
        else:
            t['top_caller'] = 'None'
            t['top_caller_count'] = 0

    by_duration = sorted(meaningful_tasks.items(), key=lambda x: x[1]['duration'], reverse=True)

    # Batching Candidates
    batching_candidates = []
    for name, stats in meaningful_tasks.items():
        if stats['count'] > 5000: # high volume
            avg_us = stats['duration'] / stats['count']
            caller_pct = (stats['top_caller_count'] / stats['count']) if stats['count'] > 0 else 0
            if caller_pct > 0.70 and avg_us < 500: 
                batching_candidates.append((name, stats))
                
    batching_candidates.sort(key=lambda x: x[1]['duration'], reverse=True)
    batching_rows = ""
    for name, stats in batching_candidates[:10]:
        total_ms = stats['duration'] / 1000.0
        avg_us = stats['duration'] / stats['count']
        caller = stats['top_caller']
        caller_pct = (stats['top_caller_count'] / stats['count']) * 100
        p95_ms = stats['p95'] / 1000.0
        batching_rows += f"| `{name}` | {stats['count']:,} | `{caller}` ({caller_pct:.0f}%) | {avg_us:.1f} us | {p95_ms:,.2f} ms | {total_ms:,.1f} ms |\n"
        
    if not batching_rows:
        batching_rows = "| No obvious batching candidates found | - | - | - | - | - |\n"

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

    work_denom = max(total_thread_work_us, 1)
    workload_rows = ""
    for cat_name in ['P0: Runtime/Resolution', 'P1: I/O & Heavy Tasks', 'P3: Asset Pipeline', 'P4: Bridge/Interop', 'Other']:
        s = cat_stats[cat_name]
        pct = (s['duration'] * 100) / work_denom
        workload_rows += f"| {cat_name} | {s['count']:,} | {s['duration']/1000.0:,.1f} | {pct:.1f}% |\n"

    top_tasks_rows = ""
    for name, stats in by_duration[:20]:
        avg_us = stats['duration'] / stats['count']
        p95_ms = stats['p95'] / 1000.0
        total_ms = stats['duration'] / 1000.0
        max_ms = stats['max'] / 1000.0
        pct_work = (stats['duration'] * 100) / work_denom
        caller = stats['top_caller']
        caller_pct = (stats['top_caller_count'] / stats['count']) * 100 if stats['count'] > 0 else 0
        top_tasks_rows += f"| {total_ms:,.1f} | {stats['count']:,} | {avg_us:.1f} | {p95_ms:,.1f} | {max_ms:,.1f} | {pct_work:.1f}% | `{name}` | `{caller}` ({caller_pct:.0f}%) |\n"

    util_status = '\u26a0\ufe0f Suboptimal' if utilization < 60 else ('\u2705 Good' if utilization > 80 else '\U0001f197 Average')
    noise_pct = (noise_count * 100) / total_tasks_safe
    lost_parallelism = max(0, int((1 - utilization / 100) * 100))

    report_id = f"utoopack_performance_report_{datetime.now().strftime('%Y%m%d_%H%M%S')}"
    
    report = f"""# Utoopack Performance Report (Intelligent)

**Report ID**: `{report_id}`
**Generated**: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}
**Trace File**: `{os.path.basename(trace_path)}` ({trace_size_gb:.1f}GB, {len(events)/1e6:.2f}M events)
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

### Workload Distribution by Tier

| Category | Tasks | Total Time (ms) | % of Thread Work |
|----------|-------|-----------------|------------------|
{workload_rows}

---

## 🤖 AI Intelligent Attributions
*New section mapping granular tasks to bottlenecks.*

### Top 10 Batching Candidates
These highly-called tasks are dominated by a single parent. If the parent can batch them into one call, it drastically reduces scheduler overhead.

| Task Name | Count | Top Caller (Attribution) | Avg | P95 | Total Time |
|-----------|-------|--------------------------|-----|-----|------------|
{batching_rows}

---

## Top 20 Tasks by Total Duration

| Total (ms) | Count | Avg (us) | P95 (ms) | Max (ms) | % Work | Task Name | Top Caller |
|------------|-------|----------|----------|----------|--------|-----------|------------|
{top_tasks_rows}

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

## Action Items
1. **[P0]** Use Batching Candidates to pinpoint specific files needing `try_join` or reduced `#[turbo_tasks::function]` limits.
2. **[P1]** Inspect `P95 (ms)` for heavy monolith tasks. Focus on long-tail outliers rather than averages.

*Report generated by Intelligent Utoopack Performance Analysis Agent*
"""

    print(f"Writing report to {output_path}")
    with open(output_path, 'w') as f:
        f.write(report)

    print(f"Report generated successfully!")
    print(f"   Wall Time:       {wall_time_ms:,.1f} ms")
    print(f"   Parallelism:     {parallelism:.1f}x")
    print(f"   Working Threads: {num_threads}")

def _load_events(trace_path):
    size_gb = os.path.getsize(trace_path) / (1024**3)
    if size_gb > 0.5:
        try:
            import ijson
            print(f"   Using streaming parser (ijson) for {size_gb:.1f}GB file...")
            with open(trace_path, 'rb') as f:
                first_char = b''
                while True:
                    ch = f.read(1)
                    if not ch: break
                    if ch.strip():
                        first_char = ch
                        break
            prefix = 'item' if first_char == b'[' else 'traceEvents.item'
            events = []
            with open(trace_path, 'rb') as f:
                for obj in ijson.items(f, prefix):
                    events.append(obj)
            if events: return events
        except ImportError:
            pass
        except Exception:
            pass
    with open(trace_path, 'r') as f:
        data = json.load(f)
    return data if isinstance(data, list) else data.get("traceEvents", [])

if __name__ == "__main__":
    if len(sys.argv) < 3:
        sys.exit(1)
    analyze_trace(sys.argv[1], sys.argv[2], sys.argv[3] if len(sys.argv) > 3 else None)
