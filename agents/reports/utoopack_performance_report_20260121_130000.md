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
| turbo_tasks::function Invocations | **1,957,790** | Total count |
| Meaningful Tasks (≥ 10µs) | **647,026 (33%)** | ✅ Analyzed |
| Tracing Noise (< 10µs) | **1,310,764 (67%)** | ⚠️ Excluded from analysis |

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
| < 10µs | 1,310,764 | 67.0% | ⚠️ **Tracing noise** - Excluded from analysis (instrumentation overhead) |
| 10µs - 100µs | 623,500 | 31.8% | ✅ Normal micro-tasks |
| 100µs - 1ms | 21,729 | 1.1% | ✅ Normal |
| 1ms - 10ms | 1,748 | 0.09% | ✅ Normal |
| > 10ms | 49 | 0.0% | 🐢 Heavy tasks |
| > 100ms | 5 | 0.0% | 🐌 Very heavy tasks |

**Key Insight**: 67% of tasks complete in under 10µs, but these are **excluded from analysis** as they are likely dominated by tracing instrumentation overhead rather than actual work. The remaining **647,026 tasks (≥ 10µs)** represent meaningful work for optimization analysis.

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

These are the most frequently invoked tasks. Tasks with avg < 10µs are marked as potential tracing noise:

| Count | Total (ms) | Avg (µs) | Task Name | Status |
|-------|------------|----------|-----------|--------|
| 1,957,790 | 30,446.2 | 15.6 | `turbo_tasks::function` | ✅ Meaningful |
| 581,041 | 4,397.3 | 7.6 | `turbo_tasks::resolve_call` | ⚠️ Tracing noise |
| 310,420 | 4,698.8 | 15.1 | `task execution completed` | ✅ Meaningful |
| 110,139 | 1,302.6 | 11.8 | `precompute code generation` | ✅ Meaningful |
| 95,606 | 1,861.1 | 19.5 | `resolving` | ✅ Meaningful |
| 89,158 | 847.6 | 9.5 | `process module` | ⚠️ Tracing noise |
| 81,464 | 597.1 | 7.3 | `handle_after_resolve_plugins` | ⚠️ Tracing noise |
| 77,505 | 962.1 | 12.4 | `effects processing` | ✅ Meaningful |

---

## ⚠️ Task Analysis (P0: Scheduling Overhead)

> **Note**: Tasks with avg duration < 10µs are excluded from this analysis, as they are likely dominated by **tracing instrumentation overhead** rather than actual scheduling cost.

Tasks with avg duration ≥ 10µs and count > 1000 are candidates for optimization:

| Count | Avg (µs) | Total (ms) | Task Name |
|-------|----------|------------|------------|
| 310,420 | 15.1 | 4,698.8 | `task execution completed` |
| 110,139 | 11.8 | 1,302.6 | `precompute code generation` |
| 95,606 | 19.5 | 1,861.1 | `resolving` |
| 77,505 | 12.4 | 962.1 | `effects processing` |
| 44,396 | 19.1 | 847.2 | `analyze ecmascript module` |
| 29,112 | 18.3 | 533.1 | `parse ecmascript` |
| 8,555 | 22.7 | 194.2 | `read file` |

**Meaningful Task Analysis**: Focusing on tasks ≥ 10µs reveals the actual work distribution without tracing noise.

---

## 💡 Recommendations

> **⚠️ Methodology Note**: Tasks with avg duration < 10µs are excluded from optimization recommendations, as their measured duration is likely dominated by tracing instrumentation overhead (~5-10µs per span).

### 1. 🚨 Critical: Thread Utilization Improvement (P0)

**Problem**: Only 54.7% thread utilization with 33 threads available.

**Impact**: ~45% of potential parallelism is lost, adding ~1.4 seconds to wall time.

**Recommendations**:
1. **Profile lock contention**: Use `parking_lot` profiling to identify contested mutexes
2. **Reduce critical path depth**: Convert sequential `await` chains to `try_join` where possible
3. **Investigate scheduler gaps**: Look for empty timeline bands indicating threads waiting

### 2. ⚠️ High Priority: `resolving` Task Optimization (P0)

**Problem**: `resolving` has 95,606 invocations averaging 19.5µs each (total: 1,861ms).

**Recommendations**:
1. **Resolution caching**: Cache resolution results at package/directory level
2. **Batch resolution requests**: Group multiple resolve operations into single tasks
3. **Fast-path for common patterns**: Bypass task scheduling for trivial relative imports

### 3. ⚠️ Medium Priority: Code Generation Pipeline (P1)

**Problem**: `precompute code generation` (110,139 calls, 11.8µs avg) and `effects processing` (77,505 calls, 12.4µs avg) represent significant aggregate work.

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
| Tracing Noise | ⚠️ **Significant** | 67% of tasks < 10µs (excluded from analysis) |
| Critical Path Serialization | 🚨 **Detected** | 54.7% thread utilization - primary bottleneck |
| Resolution Work | ⚠️ **Moderate** | `resolving` 95K calls @ 19.5µs = 1.86s |
| Code Generation | ⚠️ **Moderate** | 110K calls @ 11.8µs = 1.30s |
| Heavy Monoliths | ✅ Minimal | Only 49 tasks > 10ms |
| Lock Contention | 🔍 **Likely** | Low thread utilization suggests contention |

---

## 🎯 Action Items (Priority Order)

1. **[P0]** Profile lock contention to explain 45% lost parallelism
2. **[P0]** Investigate `resolving` task (95K calls, 1.86s total) for batching/caching opportunities
3. **[P1]** Optimize `precompute code generation` pipeline (110K calls, 1.30s total)
4. **[P1]** Convert sequential `await` chains to `try_join` in hot paths
5. **[P2]** Identify and optimize the 5 tasks that exceed 100ms (likely antd barrel files)
6. **[P2]** Investigate scheduler gaps in timeline for thread starvation patterns

> **Note**: Previous recommendations targeting `resolve_call`, `process module`, and plugin handlers have been removed as their avg duration < 10µs falls within tracing instrumentation noise.

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

#### 3.3 Tracing Overhead Filtering

```python
# Tasks < 10µs are excluded from analysis (tracing instrumentation noise)
# Only analyze tasks with avg duration >= 10µs
if avg_duration >= 10 and count > 1000:
    meaningful_tasks.append((name, count, avg, total))
```

**Rationale**: Chrome Trace instrumentation itself introduces overhead (~5-10µs per span). Tasks with duration < 10µs are likely dominated by this instrumentation cost rather than actual work, leading to misleading conclusions about micro-task explosion.

#### 3.4 Parallelism Loss Estimation

```
Theoretical Max Throughput = 33 threads × 3,060.7ms = 101,003 ms
Actual Thread Work         = 55,223.7 ms
Lost Parallelism           = 101,003 - 55,223.7 = 45,779 ms (45.3%)
```

This parallelism loss is attributed to:
- Lock contention in shared data structures
- Critical path serialization (sequential dependencies)
- Scheduler overhead (task dispatch latency)

> **Note**: Previous estimates of "scheduling overhead" based on micro-task durations have been removed, as tasks < 10µs are dominated by tracing instrumentation noise and cannot reliably measure actual scheduling cost.

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

### 6. Hotspot Call Chain Inference (Meaningful Tasks Only)

Based on invocation frequency, focusing on tasks with avg ≥ 10µs:

```
turbo_tasks::function (1.96M calls, 15.6µs avg)
    │
    ├── resolving (95K calls, 19.5µs) ← Primary optimization target
    │       └── Module path resolution
    │
    ├── precompute code generation (110K calls, 11.8µs)
    │       └── Code generation preparation
    │
    ├── effects processing (77K calls, 12.4µs)
    │       └── Side-effect tracking for tree-shaking
    │
    └── task execution completed (310K calls, 15.1µs)
            └── Task lifecycle bookkeeping

⚠️ Excluded (< 10µs, likely tracing noise):
   - turbo_tasks::resolve_call (581K calls, 7.57µs)
   - process module (89K calls, 9.5µs)
   - handle_*_resolve_plugins (~112K calls, ~7µs)
```

### 7. Optimization Impact Estimation

Using thread utilization improvement as the primary optimization target:

| Scenario | Thread Utilization | Parallelism | Est. Wall Time |
|----------|-------------------|-------------|----------------|
| Current | 54.7% | 18.04x | 3,060.7 ms |
| Target 70% | 70% | 23.1x | ~2,390 ms |
| Target 85% | 85% | 28.1x | ~1,965 ms |

**Potential Savings**: Improving thread utilization from 54.7% to 70% could reduce wall time by **~670ms (22%)**.

This is the data-driven rationale for prioritizing lock contention and critical path analysis over micro-task optimization.

---

## 📁 Artifacts

- Analysis Script: [.trace/analyze_async_scheduling_streaming.py](../../.trace/analyze_async_scheduling_streaming.py)
- Raw Trace Data: `.trace/trace_20260116_205422.json`
- Detailed Analysis: `.trace/trace_20260116_205422_async_analysis.md`

---

*Report generated following the Utoopack Performance Analysis Agent Protocol*
