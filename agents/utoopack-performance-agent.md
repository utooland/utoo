---
name: utoopack-performance
description: >
  Utoopack/Turbopack performance analysis agent. Diagnoses bottlenecks
  using Chrome Trace data across a 5-tier priority matrix (scheduling,
  I/O, architecture, asset pipeline, runtime boundaries).
  Use when investigating build performance regressions or optimizing
  bundler throughput.
tools: Read, Grep, Glob, Bash
model: opus
maxTurns: 50
---

# Utoopack Performance Analysis Agent Protocol

Specialized diagnostic procedure for analyzing Utoopack and Turbopack performance. Designed for AI agents to identify and resolve bottlenecks by analyzing Chrome Trace data, task statistics, and Turbo engine internals.

---

## 🛠 Step 1: Data Acquisition & Environment Prep

### Building Utoopack

Build the NAPI bindings before profiling:
```bash
npm run build:local -w @utoo/pack
```

### Tracing Methods

Utoopack supports **four** tracing mechanisms. Choose based on your analysis needs:

| Method | Env Var | Output | Best For |
|--------|---------|--------|----------|
| Chrome Trace | `TRACING_CHROME=<path.json>` | Chrome Trace JSON | Timeline analysis in `chrome://tracing`, quantitative analysis with `analyze_trace.py` |
| Turbopack Trace | `TURBOPACK_TRACING=<preset>` | Binary `.turbopack/.trace-turbopack` | Live trace viewer, deep Turbo engine analysis |
| Trace Server | `TURBOPACK_TRACE_SERVER=1` (with `TURBOPACK_TRACING`) | Live server | Real-time visualization at [turbo-trace-viewer.vercel.app](https://turbo-trace-viewer.vercel.app/) |
| Task Statistics | `TURBOPACK_TASK_STATISTICS=<path.json>` | JSON | Task execution counts, cache hit rates |

#### Chrome Trace (primary for quantitative analysis)
```bash
mkdir -p .trace
TRACING_CHROME=$PWD/.trace/trace_$(date +%Y%m%d_%H%M%S).json \
  npm run build --prefix examples/with-antd
```

#### Turbopack Trace Presets
When using `TURBOPACK_TRACING`, specify a preset level:

| Preset | What it traces |
|--------|---------------|
| `overview` or `1` | High-level info from all crates |
| `pack` | + detailed trace from pack-core/pack-api/pack-napi |
| `turbopack` | + detailed trace from all turbopack crates + SWC minifier |
| `turbo-tasks` | + detailed trace from turbo-tasks engine internals |

```bash
TURBOPACK_TRACING=turbopack npm run build --prefix examples/with-antd
# Output: examples/with-antd/.turbopack/.trace-turbopack
```

#### Task Statistics (cache efficiency)
```bash
TURBOPACK_TASK_STATISTICS=$PWD/.trace/task_stats.json \
  npm run build --prefix examples/with-antd
```

#### Heap Profiling (dhat)
`DhatProfilerGuard` in `pack-napi/src/pack_api/project.rs` provides heap profiling via dhat. Enabled automatically when the `dhat` feature is active.

### Workspace Hygiene
- Place all intermediate files in `./.trace/` (ensure it's in `.gitignore`)
- Use `rg` (ripgrep) for all code searches — faster and respects `.gitignore`
- Never upload raw traces > 2GB; share filtered summaries

### ⚠️ Tracing Overhead Compensation
Chrome Trace instrumentation adds ~`2µs` per span. Tasks with total recorded duration **< 10µs** are tracing noise — exclude them. For valid tasks (≥ 10µs), subtract the 2µs base overhead before aggregating.

---

## 🔍 Step 2: Diagnostic Matrix (Tiers P0–P4)

> **Architecture context**: turbo-tasks scheduling is already efficient. Thread utilization (~50%) is constrained by the inherent `resolve → read → parse → analyze` dependency chain depth, not by scheduling, I/O, or lock contention. The tiers below are ordered by **actionability** — what an agent can realistically diagnose and improve.

### 🔴 P0: Regression Detection
*Focus: Compare against baseline to catch performance regressions early.*

Always start here. Use `--compare` mode in `analyze_trace.py` to diff against a known-good baseline.

- **A. Wall Time Regression**
  - **Signal**: Wall time increased >10% vs baseline.
  - **Action**: Check if new spans appeared in the Top 20 list. Look for increased invocation counts (new modules/dependencies added) or increased avg self-time (code regression).

- **B. Thread Utilization Drop**
  - **Signal**: Utilization dropped >5pp vs baseline.
  - **Action**: A new serialization point was introduced. Check Critical Path Analysis for longer chains. Look for new `await` calls in hot paths (e.g., `process module`, `resolving`).

- **C. New Heavy Spans**
  - **Signal**: A span not in the baseline's Top 20 now appears with significant self-time.
  - **Action**: Trace the span to its Rust source. It may be a new feature, a new plugin, or a newly-added dependency.

**Baseline reference** (examples/with-antd, ~2,170 modules):

| Metric | Value |
|--------|-------|
| Wall Time | ~2,100–2,170ms |
| Parallelism | ~6.3–6.5x |
| Thread Utilization | ~48–50% |
| Working Threads | 13 |

---

### 🟠 P1: Project-Level Optimization
*Focus: Levers that users and integrators can pull without modifying the engine.*

These are the **most impactful** optimizations available today.

- **D. Externals for Large Dependencies**
  - **Signal**: High `module` scheduling overhead (~1,924ms self-time for 82K invocations). Large packages like `antd`, `@ant-design/icons` contribute thousands of modules.
  - **Action**: Configure `externals` for heavy packages to skip resolution/parsing entirely. This directly reduces module count and the depth of dependency chains.

- **E. Barrel File Splitting**
  - **Signal**: Single `parse ecmascript` or `analyze ecmascript module` spans with P95/Max self-time >10ms.
  - **Action**: Break up barrel files (`index.ts` re-exporting hundreds of modules) into direct imports. Each barrel file creates a serialization bottleneck where one module depends on all its re-exports.

- **F. Tree Shaking Effectiveness**
  - **Signal**: High module count relative to expected usage. Many `module` spans for code paths never used.
  - **Action**: Ensure tree shaking is enabled. Check for side-effect annotations in `package.json`. Use `TURBOPACK_TASK_STATISTICS` to verify modules are being skipped on rebuild.

---

### 🟡 P2: Pipeline Hot Spots
*Focus: When a specific build phase has disproportionate self-time, investigate the specific files/modules causing it.*

Use the **Build Phase Timeline** section to identify which phase dominates, then drill into the Top 20 list.

- **G. Resolve Phase** (~694ms self-time, ~877ms wall)
  - **Key spans**: `resolving` (~258ms), `internal resolving` (~331ms), `read directory` (~105ms)
  - **Action**: Check for non-cached regex in resolution plugins. Verify `read directory` metadata is cached. Heavy `internal resolving` may indicate complex `package.json` exports conditions.

- **H. Parse Phase** (~1,265ms self-time, ~2,099ms wall)
  - **Key spans**: `read file` (~1,017ms, 91% from `process module`), `parse ecmascript` (~248ms)
  - **Note**: `read file` I/O is concurrency-limited by `read_semaphore` (default 64, tunable via `TURBO_ENGINE_READ_CONCURRENCY`). Pre-reading during resolution has minimal impact (~1.4pp) due to turbo-tasks' demand-driven scheduling.

- **I. Analyze Phase** (~4,284ms self-time, ~1,875ms wall)
  - **Key spans**: `analyze ecmascript module` (~809ms, 79% from `process module`), `compute async module info` (~1,149ms)
  - **Note**: Both are already well-parallelized. `analyze ecmascript module` is parallelized via turbo-tasks. `compute async module info` is a **graph-level** operation (not per-module), computing `is_self_async()` per node then propagating via reverse DFS.

- **J. Chunk & Codegen Phase** (~2,821ms self-time, ~743ms wall)
  - **Key spans**: `chunking` (~1,195ms), `code generation` (~502ms), `precompute code generation` (~552ms), `compute async chunks` (~468ms)
  - **Action**: High parallelism in this phase (wall << self-time). Look for outlier spans with high P95/Max.

---

### 🟢 P3: Engine Tuning
*Focus: turbo-tasks configuration and cache efficiency. Generally already well-optimized.*

- **K. Persistent Caching**
  - **Signal**: Rebuild times not improving despite persistent caching being enabled.
  - **Action**: Use `TURBOPACK_TASK_STATISTICS` to examine cache hit/miss rates. Low hit rates indicate excessive task invalidation.

- **L. Scheduling Overhead**
  - **Signal**: `module` self-time increases disproportionately vs module count.
  - **Current state**: ~23µs avg self-time per `module` invocation (82K invocations = ~1,924ms). This is inherent per-task scheduling cost in turbo-tasks.
  - **Action**: Only actionable if avg self-time increases significantly from baseline. Bypass `#[turbo_tasks::function]` only for extremely frequent, low-cost operations where aggregate self-time > 500ms.

- **M. Memory & Metadata Bloat**
  - **Signal**: Task execution time increases linearly over build duration.
  - **Action**: Investigate if task input/output types are too large. Use heap profiling (`dhat` feature in `pack-napi`).

---

### ⚪ P4: Known Architectural Constraints
*Focus: Document known limitations. These are NOT actionable without fundamental engine redesign.*

- **N. Dependency Chain Depth**
  - Thread utilization (~50%) is limited by the serial `resolve → read file → parse → analyze` chain per module. Each step requires the previous step's output. turbo-tasks schedules work as soon as dependencies are ready ("neither eager nor lazy"), but cannot parallelize steps within a single module's chain.
  - **Implication**: Doubling CPU cores will NOT double build speed. The effective lever is reducing module count (P1) or the chain depth per module.

- **O. Bridge Overhead**
  - NAPI bridge between Rust and JS contributes <0.1% of total work. NOT a bottleneck.

- **P. File I/O Floor**
  - `read file` (~1,017ms for ~2,170 files) represents physical disk access time. On first build, this is cold filesystem access. On rebuild, OS page cache and turbo-tasks memoization handle this. Pre-reading during resolution has been benchmarked at ~1.4pp utilization improvement — not worth the complexity.

---

## 🚀 Step 3: Actionable Diagnostic Workflow

1. **Generate Trace**:
   ```bash
   mkdir -p .trace
   TRACING_CHROME=$PWD/.trace/trace_$(date +%Y%m%d_%H%M%S).json \
     npm run build --prefix examples/with-antd
   ```

2. **Run Analysis Script**:
   ```bash
   python3 agents/tools/analyze_trace.py <trace_file> agents/reports/<report_name>.md \
     --project examples/with-antd
   ```
   
   Optional flags:
   ```bash
   --compare <baseline_trace.json>    # Regression comparison
   --task-stats <task_stats.json>     # Cache hit rate analysis
   --flamegraph <output.folded>       # For flamegraph.pl / speedscope
   ```

3. **Qualitative Timeline Scan**: Open the trace in `chrome://tracing` or `edge://tracing`. Look for "Wall" (parallel) vs "Staircase" (serial) patterns.

4. **Focus on Self-Time**: The report ranks tasks by **self-time** (exclusive), not inclusive time. This eliminates double-counting and reveals true CPU consumers. A task high in inclusive but low in self-time is just a container — its children are the real bottlenecks.

5. **Final Reporting**: Save the report to `./agents/reports/utoopack_performance_report_YYYYMMDD_HHMMSS.md`. Include tiered signals and recommended actions.

---

## 🔄 Step 4: Cache & Invalidation Deep-Dive

**Goal**: Ensure the Turbo incremental engine is working correctly.

- **Check**: Do `invalidate` events correlate with user-modified files?
- **Red Flag**: Feedback loops where a task execution triggers invalidation of its own input.
- **Logic**: Use `state value changed` events to track the ripple effect of updates.
- **Persistent Caching**: When `persistent_caching` is enabled, use `TURBOPACK_TASK_STATISTICS` to monitor cache hit rates. A hit rate below 60% suggests aggressive invalidation or insufficient caching granularity.

---

## 💡 Step 5: Optimization Playbook

1. **Task Bypassing**: Move from `#[turbo_tasks::function]` to plain Rust functions.
   - **⚠️ Caution**: Only apply if the *aggregate self-time* is proven substantial (> 500ms). Low self-time means scheduling overhead is negligible.
2. **Granularity Tuning**: Too-big tasks invalidate too often. Too-small tasks add scheduling pressure. Use the Batching Candidates table to find the sweet spot.
3. **Regex Caching**: Use `OnceLock` or `LazyLock` for regex in resolution plugins.
4. **I/O Batching**: Use `FileSystemPath::read_dir` to populate metadata caches before individual file stats.
5. **Critical Path Reduction**: The report's Critical Path Analysis shows the longest sequential chains. Target the deepest chains for parallelization with `try_join`.

## 📂 Resource Mapping
- **Tracing presets**: `crates/pack-core/src/tracing_presets.rs`
- **NAPI tracing setup**: `crates/pack-napi/src/pack_api/project.rs` (lines 242–355)
- **Analysis script**: `agents/tools/analyze_trace.py`
- **Reports**: `agents/reports/`
- **Benchmark projects**: `examples/with-antd` (standard), `examples/multi-entries-heavy` (stress test)