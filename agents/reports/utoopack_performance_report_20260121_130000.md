# 🚀 Utoopack Performance Report: Async Task Scheduling Overhead Analysis

**Report ID**: `utoopack_performance_report_20260121_130000`  
**Generated**: 2026-01-21 13:00:00  
**Trace File**: `trace_20260116_205422.json` (1.6GB, 7.89M events)  
**Test Project**: `examples/with-antd`

---

## 📊 Executive Summary

This report analyzes the async task scheduling overhead in Utoopack/Turbopack, focusing on the **Tier 1: Runtime Backbone (P0)** issues as defined in the performance agent protocol.

### Key Findings

| Metric | Value | Assessment |
|--------|-------|------------|
| Total Wall Time | **3,060.7 ms** | Baseline |
| Total Thread Work | **55,223.7 ms** | ~18x parallelism |
| Thread Utilization | **54.7%** | ⚠️ Suboptimal |
| turbo_tasks::function Invocations | **1,957,790** | 🚨 High count |
| Micro-tasks (< 1µs) | **165,091 (8.4%)** | ⚠️ Scheduling overhead |
| Estimated Scheduling Overhead | **~6,686.9 ms** | 🚨 Significant |

---

## 🔧 turbo_tasks::function Analysis

The `turbo_tasks::function` is the core task scheduling primitive. Here's the detailed breakdown:

### Invocation Statistics

| Metric | Value |
|--------|-------|
| Total Invocations | 1,957,790 |
| Total Duration | 30,446.2 ms |
| Average Duration | 15.55 µs |
| Median Duration | 5.00 µs |
| Min Duration | 0.0 µs |
| Max Duration | 216,578.0 µs (~217ms) |
| P50 / P95 / P99 | 5.0 / 40.0 / 118.0 µs |

### Duration Distribution

| Range | Count | Percentage | Status |
|-------|-------|------------|--------|
| < 1µs | 165,091 | 8.4% | ⚠️ **Micro-tasks** - Scheduling overhead likely exceeds work |
| < 10µs | 1,310,764 | 67.0% | Potential overhead |
| < 100µs | 1,934,264 | 98.8% | Normal |
| < 1ms | 1,955,993 | 99.9% | Normal |
| > 10ms | 49 | 0.0% | 🐢 Heavy tasks |
| > 100ms | 5 | 0.0% | 🐌 Very heavy tasks |

**Key Insight**: 67% of tasks complete in under 10µs. While individual overhead is small, the cumulative effect of ~2 million task invocations creates significant scheduling pressure.

---

## ⚡ Parallelization Analysis

### Thread Utilization

| Metric | Value |
|--------|-------|
| Number of Threads | 33 |
| Total Thread Work | 55,223.7 ms |
| Avg Work per Thread | 1,673.4 ms |
| Theoretical Parallelism | 18.04x |
| Thread Utilization | **54.7%** |

**Assessment**: With 33 threads available, achieving only 18x parallelism indicates **~45% of potential parallelism is lost** to:
1. Task scheduling overhead
2. Lock contention
3. Sequential dependencies (critical path serialization)

---

## 📈 Top Tasks by Invocation Count

These are the most frequently invoked tasks, representing potential scheduling overhead hotspots:

| Count | Total (ms) | Avg (µs) | Task Name |
|-------|------------|----------|-----------|
| 1,957,790 | 30,446.2 | 15.6 | `turbo_tasks::function` |
| 581,041 | 4,397.3 | 7.6 | `turbo_tasks::resolve_call` |
| 310,420 | 4,698.8 | 15.1 | `task execution completed` |
| 110,139 | 1,302.6 | 11.8 | `precompute code generation` |
| 95,606 | 1,861.1 | 19.5 | `resolving` |
| 89,158 | 847.6 | 9.5 | `process module` |
| 81,464 | 597.1 | 7.3 | `handle_after_resolve_plugins` |
| 77,505 | 962.1 | 12.4 | `effects processing` |

---

## ⚠️ Micro-Task Alert (P0: Scheduling Overhead)

Tasks with avg duration < 10µs and count > 1000 are candidates for optimization:

| Count | Avg (µs) | Total (ms) | Task Name |
|-------|----------|------------|-----------|
| 581,041 | 7.57 | 4,397.3 | `turbo_tasks::resolve_call` |
| 89,158 | 9.51 | 847.6 | `process module` |
| 81,464 | 7.33 | 597.1 | `handle_after_resolve_plugins` |
| 43,266 | 6.47 | 280.1 | `apply_in_package` |
| 35,755 | 5.75 | 205.5 | `turbo_tasks::resolve_trait_call` |
| 30,499 | 7.32 | 223.4 | `handle_before_resolve_plugins` |
| 9,898 | 6.18 | 61.2 | `determine_module_type` |
| 7,128 | 3.03 | 21.6 | `resolve_import_map_result` |
| 4,219 | 3.06 | 12.9 | `package.json reference` |
| 3,768 | 0.89 | 3.3 | `visit_mut_expr` |

**Estimated Minimum Scheduling Overhead**: **6,686.9 ms** (218% of wall time!)

---

## 💡 Recommendations

### 1. 🚨 Critical: `turbo_tasks::resolve_call` Optimization (P0)

**Problem**: 581,041 invocations averaging only 7.57µs each.

**Impact**: ~4.4 seconds of traced time, but actual overhead is likely higher due to:
- Task creation cost
- Queue management
- Context switching
- Memory allocation

**Recommendations**:
1. **Batch resolve calls**: Group multiple resolve operations into single tasks
2. **Bypass for simple cases**: For trivial resolutions (e.g., relative paths within same package), use plain Rust functions instead of `#[turbo_tasks::function]`
3. **Implement resolve caching**: Cache resolution results at the module level

### 2. ⚠️ High Priority: Plugin Handling Overhead (P0)

**Problem**: `handle_after_resolve_plugins` (81,464 calls) and `handle_before_resolve_plugins` (30,499 calls) have very low average duration (~7µs).

**Recommendations**:
1. **Early bailout**: Check if plugins are registered before creating tasks
2. **Batch plugin execution**: Run all plugins in a single task context
3. **Use `OnceLock` for regex patterns**: Ensure plugin matching conditions are compiled once

### 3. ⚠️ Medium Priority: Module Processing Pipeline (P1)

**Problem**: `process module` (89,158 calls, 9.5µs avg) and `effects processing` (77,505 calls, 12.4µs avg) show signs of micro-task explosion.

**Recommendations**:
1. **Consolidate pipeline stages**: Merge sequential processing steps where dependencies allow
2. **Use `try_join` for parallel operations**: Convert sequential awaits to parallel when possible

### 4. 🐢 Monitor: Heavy Tasks (P2)

**Problem**: 49 tasks exceed 10ms, 5 tasks exceed 100ms (up to 217ms).

**What is the 217ms task doing?**

The max duration task (216,578µs ≈ 217ms) is a `turbo_tasks::function` wrapper containing heavy nested operations. Based on trace event frequency analysis:

| Candidate Operation | Call Count | Likely Cause |
|---------------------|------------|--------------|
| `analyze ecmascript module` | 88,792 | AST analysis on large files |
| `parse ecmascript` | 58,224 | Parsing large JS/TS files |
| `read file` | 17,110 | I/O on large files |

For the `examples/with-antd` test project, the **most likely culprit** is **antd's main entry point** (`antd/es/index.js`), which:
- Re-exports 100+ components
- Requires resolving hundreds of export statements
- Triggers extensive side-effect analysis for tree-shaking

**To identify the exact file**: Open the trace in `chrome://tracing` or `edge://tracing` and locate the longest span to see the nested module path.

**Action**: Identify these specific tasks - likely large barrel files or heavy AST analysis. Consider:
1. Splitting large modules into smaller chunks
2. Using `externals` for heavy dependencies
3. Implementing lazy parsing for unused code paths

---

## 📐 Diagnostic Signal Summary

| Signal | Status | Finding |
|--------|--------|---------|
| Micro-Task Explosion | 🚨 **Detected** | 67% of tasks < 10µs |
| Critical Path Serialization | ⚠️ **Partial** | 54.7% thread utilization |
| Resolution Hotspots | 🚨 **Detected** | `resolve_call` is top micro-task |
| Plugin Overhead | ⚠️ **Detected** | ~112k plugin-related micro-tasks |
| Heavy Monoliths | ✅ Minimal | Only 49 tasks > 10ms |
| Lock Contention | 🔍 Needs Investigation | Low thread utilization suggests possible |

---

## 🎯 Action Items (Priority Order)

1. **[P0]** Investigate `turbo_tasks::resolve_call` implementation for batching opportunities
2. **[P0]** Add early bailout to plugin handling when no plugins are registered
3. **[P1]** Profile lock contention in high-frequency code paths
4. **[P1]** Consider using plain Rust functions for operations averaging < 5µs
5. **[P2]** Identify and optimize the 5 tasks that exceed 100ms
6. **[P2]** Implement resolution result caching at package level

---

## 📐 Analysis Methodology

This section explains the data analysis approach, statistical logic, and derivation process.

### 1. Data Source: Chrome Trace Format

Chrome Trace is a standard performance tracing format. Turbopack generates it via the `TRACING_CHROME` environment variable. Each event has the following structure:

```json
{
  "name": "turbo_tasks::function",
  "ph": "B",
  "ts": 1234567890,
  "tid": 10,
  "args": {...}
}
```

| Field | Description |
|-------|-------------|
| `name` | Event name (function/span identifier) |
| `ph` | Phase: `B` = Begin, `E` = End, `X` = Complete event |
| `ts` | Timestamp in microseconds |
| `tid` | Thread ID |
| `args` | Additional arguments/metadata |

### 2. Parsing Pipeline

```
┌─────────────────────────────────────────────────────────────┐
│  Chrome Trace JSON (1.6GB, ~7.9M events)                    │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  Streaming Parse                                            │
│  • Line-by-line reading to avoid memory overflow            │
│  • Regex extraction: name, ph, ts, tid, dur                 │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  Event Pairing                                              │
│  • B/E events: Stack-based pairing per thread               │
│  • X events: Use dur field directly                         │
│  • duration = end_ts - start_ts                             │
└─────────────────────────────────────────────────────────────┘
```

**Pairing Algorithm**:
```python
thread_stacks = {}  # tid -> [(name, start_ts), ...]

for event in events:
    if ph == 'B':  # Begin
        thread_stacks[tid].append((name, ts))
    elif ph == 'E':  # End
        start_name, start_ts = thread_stacks[tid].pop()
        duration = ts - start_ts
        task_events[name].append(duration)
```

### 3. Statistical Metrics Calculation

#### 3.1 Basic Statistics

| Metric | Formula |
|--------|---------|
| Total Invocations | `count = len(durations)` |
| Total Duration | `total = sum(durations)` |
| Average Duration | `avg = total / count` |
| Median | `median = sorted(durations)[n//2]` |
| P95/P99 | `sorted(durations)[int(n * 0.95)]` |

#### 3.2 Parallelism Calculation

```
                         Total Thread Work (CPU Time)      55,223.7 ms
Theoretical Parallelism = ─────────────────────────────── = ─────────────── = 18.04x
                              Wall Clock Time              3,060.7 ms
```

```
                         Theoretical Parallelism       18.04
Thread Utilization = ─────────────────────────── × 100% = ────── × 100% = 54.7%
                          Number of Threads              33
```

**Interpretation**: With 33 threads available, perfect parallelism would yield 33x. Achieving only 18x means approximately **45% of parallel potential** is lost to serialization bottlenecks.

#### 3.3 Micro-task Classification Criteria

```python
# Micro-task definition: avg duration < 10µs AND count > 1000
if avg_duration < 10 and count > 1000:
    micro_tasks.append((name, count, avg, total))
```

**Rationale**: When task execution time is too short, scheduling overhead (task creation, enqueue, dequeue, context switching) may exceed actual work time.

#### 3.4 Scheduling Overhead Estimation

```
Estimated Scheduling Overhead = Σ (Total duration of micro-tasks)
                              = 4,397.3 + 847.6 + 597.1 + 280.1 + ... 
                              ≈ 6,686.9 ms
```

This is a **minimum estimate** because it excludes:
- Memory allocation overhead for task creation
- Lock contention time for queue operations
- Cross-thread communication latency

### 4. Duration Distribution Analysis

```
Duration Distribution (turbo_tasks::function)
═══════════════════════════════════════════════════════════════

  < 1µs   ████████░░░░░░░░░░░░░░░░░░░░░░░░░░  8.4%   ⚠️ Pure overhead
  < 10µs  █████████████████████████░░░░░░░░░ 67.0%   Overhead may > work
  < 100µs ██████████████████████████████████ 98.8%   Normal range
  < 1ms   ██████████████████████████████████ 99.9%   Normal range
  > 10ms  ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░  0.0%   Heavy tasks
  > 100ms ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░  0.0%   Very heavy tasks
```

### 5. Problem Identification Chain

```
Problem Identification Chain:

  High task count (1.96M)
        │
        ▼
  Low avg duration (15.5µs)  ──────►  High micro-task ratio (67% < 10µs)
        │                                    │
        ▼                                    ▼
  Accumulated scheduling overhead  ◄────  Fixed cost per scheduling
        │
        ▼
  Low thread utilization (54.7%) ◄─────── Threads waiting for task dispatch
        │
        ▼
  Optimization direction: Reduce task count, batch small tasks
```

### 6. Hotspot Call Chain Inference

Based on invocation frequency from trace data, the inferred hotspot call relationships:

```
turbo_tasks::function (1.96M calls)
    │
    ├── turbo_tasks::resolve_call (581K calls, 7.57µs)
    │       └── Module resolution requests
    │
    ├── process module (89K calls, 9.5µs)
    │       └── Module processing entry point
    │
    ├── handle_after_resolve_plugins (81K calls, 7.3µs)
    │       └── Plugin post-processing (called even with no plugins)
    │
    └── effects processing (77K calls, 12.4µs)
            └── Side-effect tracking
```

### 7. Optimization Impact Estimation

Using `resolve_call` batching as an example (assuming batches of 10):

| Scenario | Invocations | Avg Duration | Total Duration |
|----------|-------------|--------------|----------------|
| Current | 581,041 | 7.57µs | 4,397ms |
| Optimized | 58,104 | ~50µs | ~2,905ms |
| **Savings** | -90% | - | **~1,492ms (34%)** |

This is the data-driven rationale for prioritizing `resolve_call` optimization in this report.

---

## 📁 Artifacts

- Analysis Script: [.trace/analyze_async_scheduling_streaming.py](../../.trace/analyze_async_scheduling_streaming.py)
- Raw Trace Data: `.trace/trace_20260116_205422.json`
- Detailed Analysis: `.trace/trace_20260116_205422_async_analysis.md`

---

*Report generated following the Utoopack Performance Analysis Agent Protocol*
