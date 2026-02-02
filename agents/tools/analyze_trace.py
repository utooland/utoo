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

def analyze_trace(trace_path, output_path, override_project_name=None):
    """Analyze Chrome Trace and generate performance report following the protocol"""
    
    print(f"📊 Loading trace: {trace_path}")
    with open(trace_path, 'r') as f:
        data = json.load(f)
    
    events = data if isinstance(data, list) else data.get("traceEvents", [])
    print(f"✅ Analyzing {len(events):,} events...")

    # Try to infer project name
    project_name = override_project_name or os.environ.get("TRACE_PROJECT") or "Unknown Project"
    
    # Initialize metrics
    min_ts, max_ts = float('inf'), float('-inf')
    threads = set()
    thread_work = 0.0
    
    # Task statistics (only meaningful tasks ≥10µs)
    meaningful_tasks = defaultdict(lambda: {'count': 0, 'duration': 0.0})
    noise_count = 0
    
    # Duration buckets
    buckets = {'<10us': 0, '10us-100us': 0, '100us-1ms': 0, '1ms-10ms': 0, '10ms-100ms': 0, '>100ms': 0}
    
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
                elif dur < 100000: buckets['10ms-100ms'] += 1
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
                    elif dur < 100000: buckets['10ms-100ms'] += 1
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
    
    # Categorize tasks for Tiered analysis
    categories = {
        'P0: Runtime/Resolution': ['resolve', 'plugin', 'turbo_tasks'],
        'P1: I/O & Heavy Tasks': ['read', 'stat', 'fs', 'analyze ecmascript'],
        'P3: Asset Pipeline': ['parse', 'transform', 'analyze', 'chunk', 'minify', 'generate', 'swc'],
        'P4: Bridge/Interop': ['napi', 'wasm', 'bridge']
    }
    
    cat_stats = defaultdict(lambda: {'count': 0, 'duration': 0.0})
    for name, stats in meaningful_tasks.items():
        matched = False
        lower_name = name.lower()
        for cat, keywords in categories.items():
            if any(kw in lower_name for kw in keywords):
                cat_stats[cat]['count'] += stats['count']
                cat_stats[cat]['duration'] += stats['duration']
                matched = True
                break
        if not matched:
            cat_stats['Other']['count'] += stats['count']
            cat_stats['Other']['duration'] += stats['duration']

    # Generate report
    report_id = f"utoopack_performance_report_{datetime.now().strftime('%Y%m%d_%H%M%S')}"
    trace_file = os.path.basename(trace_path)
    trace_size_gb = os.path.getsize(trace_path) / (1024**3)
    
    # Pre-calculate derived metrics
    max_thread_work = max(thread_work, 1) # Avoid division by zero
    total_tasks_safe = max(total_tasks, 1)

    # Utilization status
    if utilization < 60:
        util_status = '⚠️ Suboptimal'
    elif utilization > 80:
        util_status = '✅ Good'
    else:
        util_status = '🆗 Average'

    # Workload Distribution Table Rows
    workload_rows = ""
    for cat_name in ['P0: Runtime/Resolution', 'P1: I/O & Heavy Tasks', 'P3: Asset Pipeline', 'P4: Bridge/Interop', 'Other']:
        stats = cat_stats[cat_name]
        dur_ms = stats['duration'] / 1000.0
        pct = (stats['duration'] * 100) / max_thread_work
        workload_rows += f"| {cat_name} | {stats['count']:,} | {dur_ms:,.1f} | {pct:.1f}% |\n"

    report = f"""# 🚀 Utoopack Performance Report: Async Task Scheduling Overhead Analysis

**Report ID**: `{report_id}`  
**Generated**: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}  
**Trace File**: `{trace_file}` ({trace_size_gb:.1f}GB, {len(events)/1e6:.2f}M events)  
**Test Project**: `{project_name}`

---

## 📊 Executive Summary

This report analyzes the performance of Utoopack/Turbopack, covering the full spectrum of the Performance Analysis Protocol (P0-P4).

### Key Findings

| Metric | Value | Assessment |
|--------|-------|------------|
| Total Wall Time | **{wall_time_ms:,.1f} ms** | Baseline |
| Total Thread Work | **{thread_work_ms:,.1f} ms** | ~{parallelism:.1f}x parallelism |
| Thread Utilization | **{utilization:.1f}%** | {util_status} |
| turbo_tasks::function Invocations | **{total_tasks:,}** | Total count |
| Meaningful Tasks (≥ 10µs) | **{meaningful_count:,}** | ({meaningful_count*100/total_tasks_safe:.1f}% of total) |
| Tracing Noise (< 10µs) | **{noise_count:,}** | ({noise_count*100/total_tasks_safe:.1f}% of total) |

### Workload Distribution by Tier

| Category | Tasks | Total Time (ms) | % of Work |
|----------|-------|-----------------|-----------|
{workload_rows}

---

## ⚡ Parallelization Analysis (P0-P2)

### Thread Utilization

| Metric | Value |
|--------|-------|
| Number of Threads | {num_threads} |
| Total Thread Work | {thread_work_ms:,.1f} ms |
| Avg Work per Thread | {thread_work_ms/num_threads:,.1f} ms |
| Theoretical Parallelism | {parallelism:.2f}x |
| Thread Utilization | **{utilization:.1f}%** |

**Assessment**: With {num_threads} threads available, achieving {parallelism:.1f}x parallelism indicates {'**reasonable** throughput' if utilization > 70 else '**significant loss** of potential parallelism'}.

---

## 📈 Top 20 Tasks (Global)

These are the most significant tasks by total duration:

| Total (ms) | Count | Avg (µs) | % Work | Task Name |
|------------|-------|----------|--------|-----------|
"""
    
    for name, stats in by_duration[:20]:
        avg_us = stats['duration'] / stats['count']
        total_ms = stats['duration'] / 1000.0
        pct_work = (stats['duration'] * 100) / max_thread_work
        report += f"| {total_ms:,.1f} | {stats['count']:,} | {avg_us:.1f} | {pct_work:.1f}% | `{name}` |\n"
    
    report += f"""
---

## 🔍 Deep Dive by Tier

### 🔴 Tier 1: Runtime & Resolution (P0)
*Focus: Task scheduling and dependency resolution.*

| Metric | Value | Status |
|--------|-------|--------|
| Total Scheduling Time | {cat_stats['P0: Runtime/Resolution']['duration']/1000:,.1f} ms | {'⚠️ High' if cat_stats['P0: Runtime/Resolution']['duration']/max_thread_work > 0.4 else '✅ Normal'} |
| Resolution Hotspots | {len([n for n in meaningful_tasks if 'resolve' in n.lower()])} tasks | 🔍 Check Top Tasks |

**Potential P0 Issues**: 
- Low thread utilization ({utilization:.1f}%) suggests critical path serialization or lock contention.
- {noise_count:,} tasks < 10µs ({noise_count*100/total_tasks_safe:.1f}%) contribute to scheduler pressure.

### 🟠 Tier 2: Physical & Resource Barriers (P1)
*Focus: Hardware utilization, I/O, and heavy monoliths.*

| Metric | Value | Status |
|--------|-------|--------|
| I/O Work (Estimated) | {cat_stats['P1: I/O & Heavy Tasks']['duration']/1000:,.1f} ms | {'⚠️ Bound' if cat_stats['P1: I/O & Heavy Tasks']['duration']/max_thread_work > 0.3 else '✅ Healthy'} |
| Large Tasks (> 100ms) | {buckets['>100ms']} | {'🚨 Critical' if buckets['>100ms'] > 5 else '✅ Minimal'} |

**Potential P1 Issues**:
- {buckets['>100ms']} tasks exceed 100ms. These "Heavy Monoliths" are prime candidates for splitting.

### 🟡 Tier 3: Architecture & Asset Pipeline (P2-P3)
*Focus: Global state and transformation pipeline.*

| Metric | Value | Status |
|--------|-------|--------|
| Asset Processing (P3) | {cat_stats['P3: Asset Pipeline']['duration']/1000:,.1f} ms | {cat_stats['P3: Asset Pipeline']['duration']*100/max_thread_work:.1f}% of work |
| Bridge Overhead (P4) | {cat_stats['P4: Bridge/Interop']['duration']/1000:,.1f} ms | {'⚠️ High' if cat_stats['P4: Bridge/Interop']['duration']/max_thread_work > 0.1 else '✅ Low'} |

---

## 💡 Recommendations (Prioritized P0-P2)

### 🚨 Critical: (P0) Improvement

**Problem**: {utilization:.1f}% thread utilization.
**Action**: 
1. Profile lock contention if utilization < 60%.
2. Convert sequential `await` chains to `try_join`.

### ⚠️ High Priority: (P1) Optimization

**Problem**: {buckets['>100ms']} heavy tasks detected.
**Action**:
1. Identify module-level bottlenecks (e.g., barrel files).
2. Optimize I/O batching for metadata.

### ⚠️ Medium Priority: (P3) Pipeline Efficiency

**Action**:
1. Review transformation logic for frequently changed assets.
2. Minimize cross-language serialization (P4) if overhead exceeds 10%.

---

## 📐 Diagnostic Signal Summary

| Signal | Status | Finding |
|--------|--------|---------|"""
    
    noise_pct = (noise_count * 100) / total_tasks_safe
    report += f"\n| Tracing Noise (P0) | {'⚠️ **Significant**' if noise_pct > 60 else '✅ Acceptable'} | {noise_pct:.1f}% of tasks < 10µs |"
    report += f"\n| Thread Utilization (P0) | {'🚨 **Low**' if utilization < 60 else '✅ Good'} | {utilization:.1f}% utilization |"
    report += f"\n| Heavy Monoliths (P1) | {'⚠️ **Detected**' if buckets['>100ms'] > 5 else '✅ Minimal'} | {buckets['>100ms']} tasks > 100ms |"
    report += f"\n| Asset Pipeline (P3) | 🔍 **Review** | {cat_stats['P3: Asset Pipeline']['duration']/1000:,.1f} ms total |"
    report += f"\n| Bridge/Interop (P4) | {'⚠️ **Examine**' if cat_stats['P4: Bridge/Interop']['duration']/max_thread_work > 0.1 else '✅ Low'} | {cat_stats['P4: Bridge/Interop']['duration']/1000:,.1f} ms total |"
    
    report += """

---

## 🎯 Action Items (Comprehensive P0-P4)

"""
    
    lost_parallelism = int((1 - utilization/100) * 100)
    if utilization < 70:
        report += f"1. **[P0]** Profile lock contention to address {lost_parallelism}% lost parallelism\n"
    else:
        report += "1. **[P0]** Investigate task scheduling gaps for incremental gains\n"
    
    report += """2. **[P1]** Breakdown heavy monolith tasks (>100ms) to improve granularity  
3. **[P1]** Review I/O patterns for potential batching opportunities  
4. **[P3]** Optimize asset transformation pipeline hot-spots  
5. **[P4]** Reduce "chatty" bridge operations if interop overhead is significant  

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
        print("Usage: python3 analyze_trace.py <trace_file> <output_report> [project_name]")
        sys.exit(1)
    
    trace_file = sys.argv[1]
    report_file = sys.argv[2]
    override_project_name = sys.argv[3] if len(sys.argv) > 3 else None
    
    if not os.path.exists(trace_file):
        print(f"Error: Trace file not found: {trace_file}")
        sys.exit(1)
    
    analyze_trace(trace_file, report_file, override_project_name)
