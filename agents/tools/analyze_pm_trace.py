#!/usr/bin/env python3
"""
utoo-pm Performance Analysis Script
Based on utoo-pm Performance Analysis Agent Protocol

Usage:
  Single trace:  python3 analyze_pm_trace.py <trace_file> <output_report> [project_name]
  Summary mode:  python3 analyze_pm_trace.py --summary <trace_dir> <output_report>

Analyzes Chrome Trace data from utoo-pm to identify performance bottlenecks
in Network, File I/O, and Decompression operations.
"""

import json
import sys
import os
import glob
from collections import defaultdict
from datetime import datetime


def analyze_trace(trace_path, output_path=None, override_project_name=None):
    """Analyze Chrome Trace and generate PM performance report.
    Returns metrics dict if output_path is None."""

    print(f"Loading trace: {trace_path}")
    with open(trace_path, 'r') as f:
        data = json.load(f)

    events = data if isinstance(data, list) else data.get("traceEvents", [])
    print(f"Analyzing {len(events):,} events...")

    # Try to infer project name
    project_name = override_project_name or os.environ.get("TRACE_PROJECT") or "Unknown Project"

    # Initialize metrics
    min_ts, max_ts = float('inf'), float('-inf')
    threads = set()
    thread_work = 0.0

    # Task statistics (only meaningful tasks >= 10us)
    meaningful_tasks = defaultdict(lambda: {'count': 0, 'duration': 0.0, 'max': 0.0})
    noise_count = 0

    # Duration buckets
    buckets = {
        '<10us': 0, '10us-100us': 0, '100us-1ms': 0,
        '1ms-10ms': 0, '10ms-100ms': 0, '>100ms': 0
    }

    # Stack for B/E events
    stacks = defaultdict(list)

    # PM-specific metrics
    download_stats = {'count': 0, 'duration': 0.0, 'bytes': 0}
    clone_stats = {'count': 0, 'duration': 0.0, 'clonefile': 0, 'ficlone': 0, 'copy': 0}
    extract_stats = {'count': 0, 'duration': 0.0, 'files': 0}

    def categorize_duration(dur):
        if dur < 10:
            return '<10us'
        elif dur < 100:
            return '10us-100us'
        elif dur < 1000:
            return '100us-1ms'
        elif dur < 10000:
            return '1ms-10ms'
        elif dur < 100000:
            return '10ms-100ms'
        else:
            return '>100ms'

    def process_event(name, dur):
        nonlocal thread_work, noise_count
        thread_work += dur
        bucket = categorize_duration(dur)
        buckets[bucket] += 1

        if dur < 10:
            noise_count += 1
        else:
            meaningful_tasks[name]['count'] += 1
            meaningful_tasks[name]['duration'] += dur
            meaningful_tasks[name]['max'] = max(meaningful_tasks[name]['max'], dur)

            # PM-specific tracking
            name_lower = name.lower()
            if 'download' in name_lower:
                download_stats['count'] += 1
                download_stats['duration'] += dur
            elif 'clone' in name_lower or 'ficlone' in name_lower:
                clone_stats['count'] += 1
                clone_stats['duration'] += dur
                if 'clonefile' in name_lower:
                    clone_stats['clonefile'] += 1
                elif 'ficlone' in name_lower:
                    clone_stats['ficlone'] += 1
                elif 'copy' in name_lower:
                    clone_stats['copy'] += 1
            elif 'tar_extract' in name_lower or 'unpack' in name_lower:
                extract_stats['count'] += 1
                extract_stats['duration'] += dur

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
            process_event(name, dur)

        elif ph == 'B':  # Begin event
            stacks[tid].append((ts, name))

        elif ph == 'E':  # End event
            if stacks[tid]:
                start_ts, start_name = stacks[tid].pop()
                dur = ts - start_ts
                process_event(start_name, dur)

    # Calculate metrics
    if max_ts <= min_ts:
        print("Warning: No valid timestamps found in trace")
        wall_time_ms = 0
        parallelism = 0
        utilization = 0
    else:
        wall_time_ms = (max_ts - min_ts) / 1000.0
        parallelism = thread_work / (max_ts - min_ts) if (max_ts - min_ts) > 0 else 0

    thread_work_ms = thread_work / 1000.0
    num_threads = len(threads) if threads else 1
    utilization = (parallelism / num_threads) * 100 if num_threads > 0 else 0

    total_tasks = noise_count + sum(t['count'] for t in meaningful_tasks.values())
    meaningful_count = sum(t['count'] for t in meaningful_tasks.values())

    # Sort tasks
    by_duration = sorted(meaningful_tasks.items(), key=lambda x: x[1]['duration'], reverse=True)

    # PM-specific categories
    categories = {
        'P0: Network': ['download', 'http_request', 'ping_registry', 'select_registry'],
        'P1: File I/O': ['clone_package', 'clone_dir', 'clonefile', 'ficlone', 'linux_clone',
                        'windows_copy', 'file_write', 'create_dir'],
        'P2: Decompression': ['unpack_stream', 'tar_extract', 'file_write_batch'],
        'P3: Concurrency': ['install_packages', 'semaphore']
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

    # Pre-calculate derived metrics
    max_thread_work = max(thread_work, 1)

    # Network stats
    network_time_ms = download_stats['duration'] / 1000.0
    network_pct = (download_stats['duration'] * 100) / max_thread_work

    # I/O stats
    io_time_ms = clone_stats['duration'] / 1000.0
    io_pct = (clone_stats['duration'] * 100) / max_thread_work

    # Decompression stats
    decomp_time_ms = extract_stats['duration'] / 1000.0
    decomp_pct = (extract_stats['duration'] * 100) / max_thread_work

    # Build metrics dict
    metrics = {
        'project_name': project_name,
        'wall_time_ms': wall_time_ms,
        'thread_work_ms': thread_work_ms,
        'parallelism': parallelism,
        'utilization': utilization,
        'total_tasks': total_tasks,
        'meaningful_count': meaningful_count,
        'network_time_ms': network_time_ms,
        'network_pct': network_pct,
        'download_count': download_stats['count'],
        'io_time_ms': io_time_ms,
        'io_pct': io_pct,
        'clone_count': clone_stats['count'],
        'clone_stats': clone_stats,
        'decomp_time_ms': decomp_time_ms,
        'decomp_pct': decomp_pct,
        'extract_count': extract_stats['count'],
        'cat_stats': dict(cat_stats),
        'by_duration': by_duration[:15],
        'buckets': buckets,
        'num_threads': num_threads,
        'event_count': len(events),
    }

    if output_path is None:
        return metrics

    # Generate report
    generate_report(metrics, trace_path, output_path)
    return metrics


def generate_report(metrics, trace_path, output_path):
    """Generate markdown report from metrics."""
    report_id = f"utoopm_performance_report_{datetime.now().strftime('%Y%m%d_%H%M%S')}"
    trace_file = os.path.basename(trace_path)
    trace_size_mb = os.path.getsize(trace_path) / (1024 ** 2)

    # Utilization status
    utilization = metrics['utilization']
    if utilization < 60:
        util_status = 'Suboptimal'
    elif utilization > 80:
        util_status = 'Good'
    else:
        util_status = 'Average'

    # Workload Distribution Table Rows
    workload_rows = ""
    cat_stats = metrics['cat_stats']
    max_thread_work = max(metrics['thread_work_ms'] * 1000, 1)
    for cat_name in ['P0: Network', 'P1: File I/O', 'P2: Decompression', 'P3: Concurrency', 'Other']:
        stats = cat_stats.get(cat_name, {'count': 0, 'duration': 0.0})
        dur_ms = stats['duration'] / 1000.0
        pct = (stats['duration'] * 100) / max_thread_work
        workload_rows += f"| {cat_name} | {stats['count']:,} | {dur_ms:,.1f} | {pct:.1f}% |\n"

    total_tasks_safe = max(metrics['total_tasks'], 1)

    report = f"""# utoo-pm Performance Report

**Report ID**: `{report_id}`
**Generated**: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}
**Trace File**: `{trace_file}` ({trace_size_mb:.1f}MB, {metrics['event_count']/1e3:.1f}K events)
**Test Project**: `{metrics['project_name']}`

---

## Executive Summary

| Metric | Value | Assessment |
|--------|-------|------------|
| Total Wall Time | **{metrics['wall_time_ms']:,.1f} ms** | Baseline |
| Total Thread Work | **{metrics['thread_work_ms']:,.1f} ms** | ~{metrics['parallelism']:.1f}x parallelism |
| Thread Utilization | **{utilization:.1f}%** | {util_status} |
| Total Spans | **{metrics['total_tasks']:,}** | Total count |
| Meaningful Tasks (>= 10us) | **{metrics['meaningful_count']:,}** | ({metrics['meaningful_count']*100/total_tasks_safe:.1f}% of total) |

### Workload Distribution by Tier

| Category | Tasks | Total Time (ms) | % of Work |
|----------|-------|-----------------|-----------|
{workload_rows}
---

## P0: Network Analysis

| Metric | Value |
|--------|-------|
| Download Operations | {metrics['download_count']:,} |
| Total Network Time | {metrics['network_time_ms']:,.1f} ms |
| Network % of Work | {metrics['network_pct']:.1f}% |

**Assessment**: {'Network-bound - consider faster registry or more concurrent downloads' if metrics['network_pct'] > 50 else 'Network overhead is acceptable'}

---

## P1: File I/O Analysis

| Metric | Value |
|--------|-------|
| Clone Operations | {metrics['clone_count']:,} |
| Total I/O Time | {metrics['io_time_ms']:,.1f} ms |
| I/O % of Work | {metrics['io_pct']:.1f}% |

**Clone Strategy Distribution**:
- clonefile (macOS CoW): {metrics['clone_stats']['clonefile']}
- FICLONE (Linux CoW): {metrics['clone_stats']['ficlone']}
- Regular copy (fallback): {metrics['clone_stats']['copy']}

**Assessment**: {'I/O-bound - check filesystem CoW support' if metrics['io_pct'] > 30 else 'I/O performance is acceptable'}

---

## P2: Decompression Analysis

| Metric | Value |
|--------|-------|
| Extract Operations | {metrics['extract_count']:,} |
| Total Decompress Time | {metrics['decomp_time_ms']:,.1f} ms |
| Decompress % of Work | {metrics['decomp_pct']:.1f}% |

**Assessment**: {'Decompression-bound - consider pipeline tuning' if metrics['decomp_pct'] > 30 else 'Decompression performance is acceptable'}

---

## Top 15 Tasks by Duration

| Total (ms) | Count | Avg (us) | Max (ms) | Task Name |
|------------|-------|----------|----------|-----------|
"""

    for name, stats in metrics['by_duration']:
        avg_us = stats['duration'] / stats['count'] if stats['count'] > 0 else 0
        total_ms = stats['duration'] / 1000.0
        max_ms = stats['max'] / 1000.0
        report += f"| {total_ms:,.1f} | {stats['count']:,} | {avg_us:,.0f} | {max_ms:,.1f} | `{name}` |\n"

    buckets = metrics['buckets']
    report += f"""
---

## Diagnostic Signals

| Signal | Status | Finding |
|--------|--------|---------|
| Thread Utilization (P0) | {'**Low**' if utilization < 60 else 'Good'} | {utilization:.1f}% utilization |
| Network Dominance (P0) | {'**High**' if metrics['network_pct'] > 50 else 'Normal'} | {metrics['network_pct']:.1f}% of work |
| I/O Bottleneck (P1) | {'**Detected**' if metrics['io_pct'] > 30 else 'Minimal'} | {metrics['io_pct']:.1f}% of work |
| Decompress Overhead (P2) | {'**High**' if metrics['decomp_pct'] > 30 else 'Normal'} | {metrics['decomp_pct']:.1f}% of work |
| Heavy Tasks (>100ms) | {'**Review**' if buckets['>100ms'] > 5 else 'Minimal'} | {buckets['>100ms']} tasks |

---

## Recommendations

"""

    if utilization < 70:
        report += f"1. **[P0]** Low thread utilization ({utilization:.1f}%) - investigate concurrency bottlenecks\n"
    if metrics['network_pct'] > 50:
        report += f"2. **[P0]** Network dominates ({metrics['network_pct']:.1f}%) - consider faster registry or parallel downloads\n"
    if metrics['io_pct'] > 30:
        report += f"3. **[P1]** I/O overhead ({metrics['io_pct']:.1f}%) - verify CoW cloning is working\n"
    if metrics['decomp_pct'] > 30:
        report += f"4. **[P2]** Decompression overhead ({metrics['decomp_pct']:.1f}%) - tune streaming pipeline\n"
    if buckets['>100ms'] > 5:
        report += f"5. **[P1]** {buckets['>100ms']} heavy tasks (>100ms) detected - consider splitting\n"

    if utilization >= 70 and metrics['network_pct'] <= 50 and metrics['io_pct'] <= 30:
        report += "Performance is within acceptable parameters. No immediate action required.\n"

    report += f"""
---

*Report generated by utoo-pm Performance Analysis Agent*
*Protocol: utoopm-performance-agent.md*
"""

    print(f"Writing report to {output_path}")
    with open(output_path, 'w') as f:
        f.write(report)

    print(f"Report generated successfully!")
    print(f"\nQuick Summary:")
    print(f"   Wall Time: {metrics['wall_time_ms']:,.1f} ms")
    print(f"   Parallelism: {metrics['parallelism']:.1f}x")
    print(f"   Thread Util: {utilization:.1f}%")
    print(f"   Network: {metrics['network_pct']:.1f}%")
    print(f"   I/O: {metrics['io_pct']:.1f}%")
    print(f"   Decompress: {metrics['decomp_pct']:.1f}%")


def generate_summary_report(trace_dir, output_path):
    """Generate a combined summary report from multiple trace files."""
    trace_files = glob.glob(os.path.join(trace_dir, "*.json"))

    if not trace_files:
        print(f"No trace files found in {trace_dir}")
        with open(output_path, 'w') as f:
            f.write("# utoo-pm Performance Summary\n\nNo trace files found.\n")
        return

    print(f"Found {len(trace_files)} trace files")

    # Analyze all traces
    all_metrics = []
    for trace_file in sorted(trace_files):
        try:
            # Extract info from filename: {project}_{registry}_{type}_{os}.json
            basename = os.path.basename(trace_file).replace('.json', '')
            metrics = analyze_trace(trace_file, output_path=None, override_project_name=basename)
            metrics['trace_file'] = basename
            all_metrics.append(metrics)
        except Exception as e:
            print(f"Error analyzing {trace_file}: {e}")

    if not all_metrics:
        with open(output_path, 'w') as f:
            f.write("# utoo-pm Performance Summary\n\nNo valid trace files analyzed.\n")
        return

    # Generate summary report
    report = f"""# utoo-pm Performance Summary

**Generated**: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}
**Trace Count**: {len(all_metrics)}

---

## Benchmark Results Comparison

| Scenario | Wall Time | Network | I/O | Decompress | Parallelism |
|----------|-----------|---------|-----|------------|-------------|
"""

    for m in all_metrics:
        report += f"| `{m['trace_file']}` | {m['wall_time_ms']:,.0f}ms | {m['network_pct']:.1f}% | {m['io_pct']:.1f}% | {m['decomp_pct']:.1f}% | {m['parallelism']:.1f}x |\n"

    # Group by project for comparison
    projects = defaultdict(list)
    for m in all_metrics:
        # Parse project name from trace_file
        parts = m['trace_file'].split('_')
        if len(parts) >= 2:
            project = parts[0]
            if parts[1] in ['design', 'design-x']:
                project = f"{parts[0]}-{parts[1]}"
            projects[project].append(m)

    report += "\n---\n\n## By Project\n\n"

    for project, metrics_list in sorted(projects.items()):
        report += f"### {project}\n\n"
        report += "| Type | Registry | Wall Time | Network % | I/O % |\n"
        report += "|------|----------|-----------|-----------|-------|\n"

        for m in metrics_list:
            trace_name = m['trace_file']
            # Extract registry and type
            parts = trace_name.split('_')
            registry = "unknown"
            install_type = "unknown"
            for i, p in enumerate(parts):
                if p in ['npmjs', 'npmmirror']:
                    registry = p
                if p in ['cold', 'warm', 'hot']:
                    install_type = p

            report += f"| {install_type} | {registry} | {m['wall_time_ms']:,.0f}ms | {m['network_pct']:.1f}% | {m['io_pct']:.1f}% |\n"

        report += "\n"

    # Registry comparison
    report += "---\n\n## Registry Comparison\n\n"

    registries = defaultdict(list)
    for m in all_metrics:
        trace_name = m['trace_file']
        if 'npmjs' in trace_name:
            registries['npmjs'].append(m)
        elif 'npmmirror' in trace_name:
            registries['npmmirror'].append(m)

    if len(registries) >= 2:
        report += "| Registry | Avg Wall Time | Avg Network % | Scenarios |\n"
        report += "|----------|---------------|---------------|----------|\n"

        for reg, metrics_list in sorted(registries.items()):
            avg_wall = sum(m['wall_time_ms'] for m in metrics_list) / len(metrics_list)
            avg_net = sum(m['network_pct'] for m in metrics_list) / len(metrics_list)
            report += f"| {reg} | {avg_wall:,.0f}ms | {avg_net:.1f}% | {len(metrics_list)} |\n"
    else:
        report += "Only one registry tested.\n"

    # Cold vs Warm comparison
    report += "\n---\n\n## Cold vs Warm Install\n\n"

    install_types = defaultdict(list)
    for m in all_metrics:
        if '_cold_' in m['trace_file']:
            install_types['cold'].append(m)
        elif '_warm_' in m['trace_file']:
            install_types['warm'].append(m)

    if len(install_types) >= 2:
        report += "| Type | Avg Wall Time | Avg Network % | Avg I/O % | Scenarios |\n"
        report += "|------|---------------|---------------|-----------|----------|\n"

        for itype in ['cold', 'warm']:
            if itype in install_types:
                metrics_list = install_types[itype]
                avg_wall = sum(m['wall_time_ms'] for m in metrics_list) / len(metrics_list)
                avg_net = sum(m['network_pct'] for m in metrics_list) / len(metrics_list)
                avg_io = sum(m['io_pct'] for m in metrics_list) / len(metrics_list)
                report += f"| {itype} | {avg_wall:,.0f}ms | {avg_net:.1f}% | {avg_io:.1f}% | {len(metrics_list)} |\n"

        # Calculate speedup
        if 'cold' in install_types and 'warm' in install_types:
            cold_avg = sum(m['wall_time_ms'] for m in install_types['cold']) / len(install_types['cold'])
            warm_avg = sum(m['wall_time_ms'] for m in install_types['warm']) / len(install_types['warm'])
            if warm_avg > 0:
                speedup = cold_avg / warm_avg
                report += f"\n**Cache Speedup**: {speedup:.1f}x faster with warm cache\n"
    else:
        report += "Need both cold and warm install data for comparison.\n"

    report += f"""
---

*Summary generated by utoo-pm Performance Analysis Agent*
"""

    print(f"Writing summary to {output_path}")
    with open(output_path, 'w') as f:
        f.write(report)

    print("Summary generated successfully!")


if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Usage:")
        print("  Single trace:  python3 analyze_pm_trace.py <trace_file> <output_report> [project_name]")
        print("  Summary mode:  python3 analyze_pm_trace.py --summary <trace_dir> <output_report>")
        sys.exit(1)

    if sys.argv[1] == "--summary":
        trace_dir = sys.argv[2]
        report_file = sys.argv[3]
        generate_summary_report(trace_dir, report_file)
    else:
        trace_file = sys.argv[1]
        report_file = sys.argv[2]
        override_project_name = sys.argv[3] if len(sys.argv) > 3 else None

        if not os.path.exists(trace_file):
            print(f"Error: Trace file not found: {trace_file}")
            sys.exit(1)

        analyze_trace(trace_file, report_file, override_project_name)
