# Utoopack Performance Report

**Report ID**: `utoopack_performance_report_20260313_202108`
**Generated**: 2026-03-13 20:21:08
**Trace File**: `trace_20260313_201000.json` (0.6GB, 1.65M spans)
**Test Project**: `examples/with-antd`

---

## Executive Summary

| Metric | Value | Assessment |
|--------|-------|------------|
| Total Wall Time | **2,101.6 ms** | Baseline |
| Total Thread Work (de-duped) | **13,241.5 ms** | Non-overlapping busy time |
| Effective Parallelism | **6.3x** | thread_work / wall_time |
| Working Threads | **13** | Threads with actual spans |
| Thread Utilization | **48.5%** | ⚠️ Suboptimal |
| Total Spans | **1,650,705** | All B/E + X events |
| Meaningful Spans (>= 10us) | **217,778** | (13.2% of total) |
| Tracing Noise (< 10us) | **1,432,927** | (86.8% of total) |

---

## Build Phase Timeline

Shows when each build phase is active and how much CPU it consumes.
**Self-Time** is the time spent *exclusively* in that phase (excluding children).

| Phase | Spans | Inclusive (ms) | Self-Time (ms) | Wall Range (ms) |
|-------|-------|----------------|----------------|-----------------|
| Resolve | 30,353 | 1,112.6 | 746.7 | 902.5 |
| Parse | 6,851 | 2,366.1 | 1,369.6 | 2,037.0 |
| Analyze | 132,394 | 5,490.3 | 4,175.1 | 1,839.8 |
| Chunk | 17,285 | 976.7 | 894.9 | 641.1 |
| Codegen | 20,179 | 1,540.9 | 1,063.5 | 529.7 |
| Emit | 33 | 212.3 | 105.4 | 28.6 |
| Other | 10,683 | 766.0 | 706.5 | 2,101.6 |

---

## Workload Distribution by Diagnostic Tier

| Category | Spans | Inclusive (ms) | % Work | Self-Time (ms) | % Self |
|----------|-------|----------------|--------|----------------|--------|
| P0: Scheduling & Resolution | 161,386 | 6,449.8 | 48.7% | 4,768.7 | 36.0% |
| P1: I/O & Heavy Tasks | 2,928 | 1,415.8 | 10.7% | 1,308.9 | 9.9% |
| P2: Architecture (Locks/Memory) | 1 | 0.0 | 0.0% | 0.0 | 0.0% |
| P3: Asset Pipeline | 42,781 | 3,833.2 | 28.9% | 2,277.6 | 17.2% |
| P4: Bridge/Interop | 0 | 0.0 | 0.0% | 0.0 | 0.0% |
| Other | 10,682 | 766.0 | 5.8% | 706.5 | 5.3% |

---

## Top 20 Tasks by Self-Time

Self-time is the *exclusive* duration: time spent in the task itself, not in sub-tasks.
This is the most accurate indicator of where CPU cycles are actually spent.

| Self (ms) | Inclusive (ms) | Count | Avg Self (us) | P95 Self (ms) | Max Self (ms) | % Work | Task Name | Top Caller |
|-----------|----------------|-------|---------------|---------------|---------------|--------|-----------|------------|
| 1,977.5 | 2,232.7 | 70,323 | 28.1 | 0.1 | 2.4 | 14.9% | `module` | `write all entrypoints to disk` (1%) |
| 1,095.4 | 1,095.4 | 2,170 | 504.8 | 1.6 | 5.5 | 8.3% | `read file` | `parse ecmascript` (91%) |
| 1,015.7 | 1,015.7 | 20,539 | 49.5 | 0.2 | 71.9 | 7.7% | `compute async module info` | `None` (0%) |
| 797.3 | 1,048.6 | 16,341 | 48.8 | 0.1 | 63.6 | 6.0% | `analyze ecmascript module` | `process module` (79%) |
| 672.2 | 725.9 | 10,098 | 66.6 | 0.0 | 159.0 | 5.1% | `write all entrypoints to disk` | `None` (0%) |
| 517.1 | 517.1 | 9,169 | 56.4 | 0.2 | 2.6 | 3.9% | `precompute code generation` | `code generation` (52%) |
| 487.9 | 553.7 | 7,827 | 62.3 | 0.2 | 24.7 | 3.7% | `chunking` | `write all entrypoints to disk` (0%) |
| 465.5 | 942.9 | 9,677 | 48.1 | 0.2 | 14.4 | 3.5% | `code generation` | `chunking` (4%) |
| 393.9 | 394.7 | 9,378 | 42.0 | 0.1 | 23.0 | 3.0% | `compute async chunks` | `None` (0%) |
| 342.8 | 552.8 | 13,346 | 25.7 | 0.1 | 1.7 | 2.6% | `internal resolving` | `resolving` (30%) |
| 318.2 | 1,110.8 | 23,953 | 13.3 | 0.1 | 1.0 | 2.4% | `process module` | `module` (8%) |
| 295.8 | 451.8 | 16,285 | 18.2 | 0.1 | 2.7 | 2.2% | `resolving` | `module` (18%) |
| 274.2 | 1,270.7 | 4,678 | 58.6 | 0.2 | 9.8 | 2.1% | `parse ecmascript` | `analyze ecmascript module` (46%) |
| 108.1 | 108.1 | 722 | 149.7 | 0.3 | 0.6 | 0.8% | `read directory` | `internal resolving` (100%) |
| 105.3 | 105.3 | 13 | 8098.3 | 21.3 | 23.5 | 0.8% | `write file` | `apply effects` (100%) |
| 80.9 | 80.9 | 1,333 | 60.7 | 0.2 | 5.6 | 0.6% | `generate source map` | `code generation` (97%) |
| 45.0 | 45.0 | 639 | 70.4 | 0.2 | 11.2 | 0.3% | `compute binding usage info` | `write all entrypoints to disk` (0%) |
| 16.4 | 16.4 | 584 | 28.1 | 0.0 | 7.0 | 0.1% | `collect mergeable modules` | `compute merged modules` (0%) |
| 13.1 | 28.3 | 80 | 163.4 | 0.4 | 7.4 | 0.1% | `make production chunks` | `chunking` (5%) |
| 8.7 | 9.5 | 311 | 28.0 | 0.1 | 0.4 | 0.1% | `async reference` | `write all entrypoints to disk` (1%) |

---

## Critical Path Analysis

The longest sequential dependency chains that determine wall-clock time.
Focus on reducing the depth of these chains to improve parallelism.

| Rank | Self-Time (ms) | Depth | Path |
|------|----------------|-------|------|
| 1 | 63.6 | 2 | process module → analyze ecmascript module |
| 2 | 23.5 | 2 | apply effects → write file |
| 3 | 21.5 | 2 | process module → analyze ecmascript module |
| 4 | 20.0 | 2 | code generation → generate source map |
| 5 | 19.8 | 2 | apply effects → write file |

---

## Batching Candidates

High-volume tasks dominated by a single parent. If the parent can batch them,
it drastically reduces scheduler overhead.

| Task Name | Count | Top Caller (Attribution) | Avg Self | P95 Self | Total Self |
|-----------|-------|--------------------------|----------|----------|------------|
| `analyze ecmascript module` | 16,341 | `process module` (79%) | 48.8 us | 0.15 ms | 797.3 ms |

---

## Duration Distribution

| Range | Count | Percentage |
|-------|-------|------------|
| <10us | 1,432,927 | 86.8% |
| 10us-100us | 195,084 | 11.8% |
| 100us-1ms | 21,846 | 1.3% |
| 1ms-10ms | 811 | 0.0% |
| 10ms-100ms | 35 | 0.0% |
| >100ms | 2 | 0.0% |

---

## Action Items
1. **[P0]** Focus on tasks with the highest **Self-Time** — these are where CPU cycles are *actually* spent.
2. **[P0]** Use Batching Candidates to identify callers that should use `try_join` or reduce `#[turbo_tasks::function]` granularity.
3. **[P1]** Check Build Phase Timeline for phases with disproportionate wall range vs. self-time (= serialization).
4. **[P1]** Inspect `P95 Self (ms)` for heavy monolith tasks. Focus on long-tail outliers, not averages.
5. **[P1]** Review Critical Paths — reducing the longest chain depth directly improves wall-clock time.
6. **[P2]** If Thread Utilization < 60%, investigate scheduling gaps (lock contention or deep dependency chains).

*Report generated by Utoopack Performance Analysis Agent*
