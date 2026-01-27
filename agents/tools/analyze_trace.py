#!/usr/bin/env python3
"""
Utoopack Performance Analysis Script
Based on Utoopack Performance Analysis Agent Protocol

Usage: python3 analyze_trace.py <trace_file> <output_report>
"""

import json
import sys
import os
from collections import defaultdict
from datetime import datetime

def analyze_trace(trace_path, output_path):
    """Analyze Chrome Trace and generate performance report following the protocol"""
    
    print(f"📊 Loading trace: {trace_path}")
    with open(trace_path, 'r') as f:
        data = json.load(f)
    
    events = data if isinstance(data, list) else data.get("traceEvents", [])
    print(f"✅ Analyzing {len(events):,} events...")
    
    # Initialize metrics
    min_ts, max_ts = float('inf'), float('-inf')
    threads = set()
    thread_work = 0.0
    
    # Task statistics (only meaningful tasks ≥10µs)
    meaningful_tasks = defaultdict(lambda: {'count': 0, 'duration': 0.0})
    noise_count = 0
    
    # Duration buckets
    buckets = {'<10us': 0, '10us-100us': 0, '100us-1ms': 0, 'q-10ms': 0, '>10ms': 0, '>100ms': 0}
    
    # Stack for B/E events
    stacks = defaultdict(list)
    
    for event in events:
        ph = event.get('ph')
        tid = event.get('tid')
        ts = event.get('ts', 0)
        name = event.get('name', 'unknown')
        
        if ts > 0:
            min_ts = min(min_ts, ts)
            max_ts = max(max_ts, ts)
        
        threads.add(tid)
        
        if ph == 'X':  # Complete event
            dur = event.get('dur', 0)
            max_ts = max(max_ts, ts + dur)
            thread_work += dur
            
            if dur < 10:
                noise_count += 1
                buckets['<10us'] += 1
            else:
                if dur < 100: buckets['10us-100us'] += 1
                elif dur < 1000: buckets['100us-1ms'] += 1
                elif dur < 10000: buckets['1ms-10ms'] += 1
                elif dur < 100000: buckets['>10ms'] += 1
                else: buckets['>100ms'] += 1
                
                meaningful_tasks[name]['count'] += 1
                meaningful_tasks[name]['duration'] += dur
                
        elif ph == 'B':  # Begin event
            stacks[tid].append((ts, name))
            
        elif ph == 'E':  # End event
            if stacks[tid]:
                start_ts, start_name = stacks[tid].pop()
                dur = ts - start_ts
                thread_work += dur
                
                if dur < 10:
                    noise_count += 1
                    buckets['<10us'] += 1
                else:
                    if dur < 100: buckets['10us-100us'] += 1
                    elif dur < 1000: buckets['100us-1ms'] += 1
                    elif dur < 10000: buckets['1ms-10ms'] += 1
                    elif dur < 100000: buckets['>10ms'] += 1
                    else: buckets['>100ms'] += 1
                    
                    meaningful_tasks[start_name]['count'] += 1
                    meaningful_tasks[start_name]['duration'] += dur
    
    # Calculate metrics
    wall_time_ms = (max_ts - min_ts) / 1000.0
    thread_work_ms = thread_work / 1000.0
    num_threads = len(threads) if threads else 1
    parallelism = thread_work / (max_ts - min_ts) if (max_ts - min_ts) > 0 else 0
    utilization = (parallelism / num_threads) * 100 if num_threads > 0 else 0
    
    total_tasks = noise_count + sum(t['count'] for t in meaningful_tasks.values())
    meaningful_count = sum(t['count'] for t in meaningful_tasks.values())
    
    # Sort tasks
    by_count = sorted(meaningful_tasks.items(), key=lambda x: x[1]['count'], reverse=True)
    by_duration = sorted(meaningful_tasks.items(), key=lambda x: x[1]['duration'], reverse=True)
    
    # Generate report
    report_id = f"utoopack_performance_report_{datetime.now().strftime('%Y%m%d_%H%M%S')}"
    trace_file = os.path.basename(trace_path)
    trace_size_gb = os.path.getsize(trace_path) / (1024**3)
    
    report = f"""# 🚀 Utoopack Performance Report: Async Task Scheduling Overhead Analysis

**Report ID**: `{report_id}`  
**Generated**: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}  
**Trace File**: `{trace_file}` ({trace_size_gb:.1f}GB, {len(events)/1e6:.2f}M events)  
**Test Project**: `examples/with-antd`

---

## 📊 Executive Summary

This report analyzes the async task scheduling overhead in Utoopack/Turbopack, focusing on the **Tier 1: Runtime Backbone (P0)** issues as defined in the performance agent protocol.

### Key Findings

| Metric | Value | Assessment |
|--------|-------|------------|
| Total Wall Time | **{wall_time_ms:,.1f} ms** | Baseline |
| Total Thread Work | **{thread_work_ms:,.1f} ms** | ~{parallelism:.1f}x parallelism |
| Thread Utilization | **{utilization:.1f}%** | {'⚠️ Suboptimal' if utilization < 60 else '✅ Good'} |
| turbo_tasks::function Invocations | **{total_tasks:,}** | Total count |
| Meaningful Tasks (≥ 10µs) | **{meaningful_count:,} ({meaningful_count*100/max(total_tasks,1):.1f}%)** | ✅ Analyzed |
| Tracing Noise (< 10µs) | **{noise_count:,} ({noise_count*100/max(total_tasks,1):.1f}%)** | ⚠️ Excluded from analysis |

---

## 🔧 turbo_tasks::function Analysis

The `turbo_tasks::function` is the core task scheduling primitive. Here's the detailed breakdown:

### Duration Distribution

| Range | Count | Status |
|-------|-------|--------|
| < 10µs | {buckets['<10us']:,} | ⚠️ **Tracing noise** - Excluded from analysis (instrumentation overhead) |
| 10µs - 100µs | {buckets['10us-100us']:,} | ✅ Normal micro-tasks |
| 100µs - 1ms | {buckets['100us-1ms']:,} | ✅ Normal |
| 1ms - 10ms | {buckets['1ms-10ms']:,} | ✅ Normal |
| > 10ms | {buckets['>10ms']:,} | 🐢 Heavy tasks |
| > 100ms | {buckets['>100ms']:,} | 🐌 Very heavy tasks |

**Key Insight**: {noise_count*100/max(total_tasks,1):.1f}% of tasks complete in under 10µs, but these are **excluded from analysis** as they are likely dominated by tracing instrumentation overhead rather than actual work. The remaining **{meaningful_count:,} tasks (≥ 10µs)** represent meaningful work for optimization analysis.

---

## ⚡ Parallelization Analysis

### Thread Utilization

| Metric | Value |
|--------|-------|
| Number of Threads | {num_threads} |
| Total Thread Work | {thread_work_ms:,.1f} ms |
| Avg Work per Thread | {thread_work_ms/num_threads:,.1f} ms |
| Theoretical Parallelism | {parallelism:.2f}x |
| Thread Utilization | **{utilization:.1f}%** |

**Assessment**: With {num_threads} threads available, achieving {parallelism:.1f}x parallelism """
    
    if utilization < 70:
        lost_pct = int((1 - utilization/100) * 100)
        report += f"indicates **~{lost_pct}% of potential parallelism is lost** to:"
    else:
        report += "is reasonable. Potential areas:"
    
    report += """
1. Task scheduling overhead
2. Lock contention
3. Sequential dependencies (critical path serialization)

---

## 📈 Top Tasks by Invocation Count

These are the most frequently invoked tasks. Tasks with avg < 10µs are marked as potential tracing noise:

| Count | Total (ms) | Avg (µs) | Task Name | Status |
|-------|------------|----------|-----------|--------|
"""
    
    for name, stats in by_count[:15]:
        avg_us = stats['duration'] / stats['count']
        total_ms = stats['duration'] / 1000.0
        status = '✅ Meaningful' if avg_us >= 10 else '⚠️ Tracing noise'
        report += f"| {stats['count']:,} | {total_ms:,.1f} | {avg_us:.1f} | `{name}` | {status} |\n"
    
    report += """
---

## ⚠️ Task Analysis (P0: Scheduling Overhead)

> **Note**: Tasks with avg duration < 10µs are excluded from this analysis, as they are likely dominated by **tracing instrumentation overhead** rather than actual scheduling cost.

Tasks with avg duration ≥ 10µs and count > 1000 are candidates for optimization:

| Count | Avg (µs) | Total (ms) | Task Name |
|-------|----------|------------|-----------|
"""
    
    candidates = [(n, s) for n, s in meaningful_tasks.items() if s['count'] > 1000]
    candidates.sort(key=lambda x: x[1]['duration'], reverse=True)
    
    for name, stats in candidates[:15]:
        avg_us = stats['duration'] / stats['count']
        total_ms = stats['duration'] / 1000.0
        report += f"| {stats['count']:,} | {avg_us:.1f} | {total_ms:,.1f} | `{name}` |\n"
    
    report += f"""
**Meaningful Task Analysis**: Focusing on tasks ≥ 10µs reveals the actual work distribution without tracing noise.

---

## 💡 Recommendations

> **⚠️ Methodology Note**: Tasks with avg duration < 10µs are excluded from optimization recommendations, as their measured duration is likely dominated by tracing instrumentation overhead (~5-10µs per span).

### 1. 🚨 Critical: Thread Utilization Improvement (P0)

**Problem**: {utilization:.1f}% thread utilization with {num_threads} threads available.

**Impact**: ~{100 - utilization:.0f}% of potential parallelism is lost, adding significant overhead to wall time.

**Recommendations**:
1. **Profile lock contention**: Use `parking_lot` profiling to identify contested mutexes
2. **Reduce critical path depth**: Convert sequential `await` chains to `try_join` where possible
3. **Investigate scheduler gaps**: Look for empty timeline bands indicating threads waiting

### 2. ⚠️ High Priority: Resolution Pipeline Optimization (P0)

**Recommendations**:
1. **Resolution caching**: Cache resolution results at package/directory level
2. **Batch resolution requests**: Group multiple resolve operations into single tasks
3. **Fast-path for common patterns**: Bypass task scheduling for trivial patterns

### 3. ⚠️ Medium Priority: Code Generation Pipeline (P1)

**Recommendations**:
1. **Consolidate pipeline stages**: Merge sequential processing steps where dependencies allow
2. **Use `try_join` for parallel operations**: Convert sequential awaits to parallel when possible

---

## 📐 Diagnostic Signal Summary

| Signal | Status | Finding |
|--------|--------|---------|"""
    
    noise_pct = noise_count*100/max(total_tasks,1)
    report += f"\n| Tracing Noise | {'⚠️ **Significant**' if noise_pct > 60 else '✅ Acceptable'} | {noise_pct:.1f}% of tasks < 10µs (excluded from analysis) |"
    report += f"\n| Critical Path Serialization | {'🚨 **Detected**' if utilization < 60 else '✅ Good'} | {utilization:.1f}% thread utilization |"
    report += "\n| Resolution Work | 🔍 **Check** | Review `resolving` task metrics |"
    report += "\n| Code Generation | 🔍 **Check** | Review pipeline tasks |"
    report += f"\n| Heavy Monoliths | {'⚠️ **Detected**' if buckets['>100ms'] > 5 else '✅ Minimal'} | {buckets['>100ms']} tasks > 100ms |"
    report += f"\n| Lock Contention | {'🔍 **Likely**' if utilization < 60 else '✅ Unlikely'} | Low thread utilization suggests contention |"
    
    report += """

---

## 🎯 Action Items (Priority Order)

"""
    
    lost_parallelism = int((1 - utilization/100) * 100)
    if utilization < 70:
        report += f"1. **[P0]** Profile lock contention to explain {lost_parallelism}% lost parallelism\n"
    else:
        report += "1. **[P0]** Maintain current parallelism levels\n"
    
    report += """2. **[P0]** Investigate high-frequency tasks for batching/caching opportunities  
3. **[P1]** Optimize code generation pipeline
4. **[P1]** Convert sequential `await` chains to `try_join` in hot paths
5. **[P2]** Identify and optimize tasks exceeding 100ms

---

*Report generated by Utoopack Performance Analysis Agent on """ + datetime.now().strftime('%Y-%m-%d') + """*
*Following: Utoopack Performance Analysis Agent Protocol*
"""
    
    print(f"💾 Writing report to {output_path}")
    with open(output_path, 'w') as f:
        f.write(report)
    
    print(f"✅ Report generated successfully!")
    print(f"\n📊 Quick Summary:")
    print(f"   Wall Time: {wall_time_ms:,.1f} ms")
    print(f"   Parallelism: {parallelism:.1f}x")
    print(f"   Thread Util: {utilization:.1f}%")
    print(f"   Meaningful Tasks: {meaningful_count:,}")

if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Usage: python3 analyze_trace.py <trace_file> <output_report>")
        sys.exit(1)
    
    trace_file = sys.argv[1]
    report_file = sys.argv[2]
    
    if not os.path.exists(trace_file):
        print(f"Error: Trace file not found: {trace_file}")
        sys.exit(1)
    
    analyze_trace(trace_file, report_file)
