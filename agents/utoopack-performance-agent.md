# 🤖 Utoopack Universal Performance Analysis Agent Protocol

This document defines the specialized diagnostic procedure for analyzing Utoopack and Turbopack performance. It is a universal protocol designed for AI agents to investigate bottlenecks within the Utoopack workspace.

## 🎯 Objective
Empower AI agents to identify and resolve performance bottlenecks in Utoopack/Turbopack by analyzing Chrome Trace data. Focus on high-level architectural patterns specific to the Turbo engine.

---

## 🛠 Step 1: Data Acquisition & Environment Prep
- **Build**: **First**, run `npm run build:local --workspace @utoo/pack` to compile utoopack.
- **Trace Generation**: Run Utoopack with `TRACING_CHROME=$PWD/.trace/trace_$(date +%Y%m%d_%H%M%S).json`.
  - *Example*: `TRACING_CHROME=$PWD/.trace/trace_$(date +%Y%m%d_%H%M%S).json npm run build --prefix examples/with-antd`
- **Intermediate Files**: All diagnostic scripts (Python/Node), filtered JSON fragments, and analytical results **MUST** be placed in the `./.trace/` directory.
- **Workspace Hygiene**: Ensure `./.trace/` is in `.gitignore`. Never upload too huge raw trace data (> 2000MB) directly; share filtered summaries or key findings.
- **Search Tooling**: You **MUST** use `ripgrep` (command: `rg`) for all code searches. It is significantly faster and respects `.gitignore`, which is crucial for large Rust/JS monorepos.

---

## 🔍 Step 2: Universal Diagnostic Matrix (Tiers P0-P4)

Follow this tiered hierarchy. Solve P0 before descending to P1, as high-level scheduler noise often masks lower-level logic bottlenecks.

### 🔴 Tier 1: The Runtime Backbone (Priority: P0)
*Focus: Task scheduling efficiency and resolution bottlenecks.*

- **💥 A. Critical Path Serialization (Latency vs. Throughput)**
  - **Signal**: "Staircase" patterns in the trace where tasks wait on each other sequentially (Waterfall), rather than efficient "Wall" patterns (Parallelism).
  - **Insight**: High task count is not inherently bad if tasks run in parallel. The bottleneck is usually the *depth* of the dependency graph, not the *width*.
  - **Action**: Identify long chains of `await` calls (e.g., `resolve` -> `fs` -> `plugin` -> `resolve`). Convert sequential loops to `try_join` or parallel iterators.

- **📉 B. Scheduling Overhead (Micro-Task Explosion)**
  - **Signal**: Millions of `turbo_tasks::function` spans with minuscule self-times (< 1µs), dominating the timeline visual noise.
  - **Expert Logic**: While high counts are acceptable, excessive wrapping of trivial logic (like `bool` checks) adds unnecessary scheduler pressure. 
  - **Action**: Bypass `#[turbo_tasks::function]` only for extremely frequent, low-cost operations (e.g., plugin matching conditions).

- **🗺 C. Resolution & Plugin Hotspots**
  - **Signal**: Slow `resolve`, `resolve_options`, or `plugin_condition` spans.
  - **Action**: Check for non-cached regex operations in resolution plugins or excessive file-system probing.

---

### 🟠 Tier 2: Physical & Resource Barriers (Priority: P1)
*Focus: Hardware utilization and I/O concurrency.*

- **📁 C. I/O Chokepoints (Disk Latency)**
  - **Signal**: Serialized `read file` or `stat` calls on a single thread while others are idle.
  - **Action**: Implement batching (e.g., `read_dir` metadata caching) or use concurrent `Vc` joins for file reads.

- **🧵 D. Parallelization Slums (Core Under-utilization)**
  - **Signal**: High total CPU time but narrow execution bands in the Chrome Trace timeline.
  - **Action**: Identify "Choke-points" where a single task waits for thousands of sub-tasks one-by-one instead of in batch.

- **🐘 E. Heavy Individual Monoliths (Latency Spikes)**
  - **Signal**: Single events (like `analyze ecmascript module`) exceeding 100ms.
  - **Action**: These are typically massive "Barrel Files" (thousands of exports) or generated code.
    - **Fix 1**: Isolate them into separate chunks/entries.
    - **Fix 2**: Use `ignore` or `externals` if they don't need analysis.
    - **Fix 3**: If it's your code, break it up.

---

### 🟡 Tier 3: Architecture & Engine Health (Priority: P2)
*Focus: Global state and concurrent safety.*

- **⏳ F. Scheduler Gaps (Lock Contention)**
  - **Signal**: Empty spaces in the timeline across all threads.
  - **Action**: Trace back to `parking_lot` mutexes or global counters that might be contested during high-concurrency phases.

- **🧠 F. Memory Gravity & Metadata Bloat**
  - **Signal**: Task execution time increases linearly or exponentially over the build duration.
  - **Action**: Investigate if task input/output types are too large (e.g., passing full ASTs instead of `Vc<Ast>`).

---

### 🟢 Tier 4: The Asset Processing Pipeline (Priority: P3)
*Focus: Heavy lifting transformation logic.*

- **⚛️ G. Pipeline Lifecycle (Combined Asset Logic)**
  - **G1. Parsing**: SWC lexing/parsing complexity.
  - **G2. Static Analysis**: Analyzer/Dependency tracking depth.
  - **G3. Transformation**: Visitor overhead in custom transforms.
  - **G4. Chunking**: Graph merging and module batching logic.
  - **G5. Finalization**: Minification and Code Generation.

---

### ⚪ Tier 5: Runtime Boundaries (Priority: P4)
*Focus: Cross-language serialization.*

- **🌉 H. Bridge & Serialization**
  - **Signal**: Large gaps in `napi` or `wasm` boundaries.
  - **Action**: Minimize "chatty" APIs between Rust and JS. Prefer multi-operation batches over individual property access.

---

## 🚀 Step 3: Actionable Diagnostic Workflow
1. **Quantitative Baseline**: Run a summary script (Python/Node) in `.trace/` to list Top 20 tasks by `count` and `sum(duration)`.
2. **Qualitative Timeline Scan**: Open the trace in `edge://tracing`. Look for "Wall-like" structures (Parallelism) vs "Staircase" structures (Serialism).
3. **Causal Attribution**: Identify the `Parent Span` of the top bottlenecks to understand *why* they were invoked.
4. **Final Reporting**: Summarize findings and save the report to `./agents/reports/utoopack_performance_report_YYYYMMDD_HHMMSS.md`. Include specific Tiered signals and recommended actions.

---

## 🔄 Step 4: Cache & Invalidation Deep-Dive
**Goal**: Ensure the "Turbo" incremental engine is actually working.
- **Check**: Do `invalidate` events correlate with user-modified files?
- **Red Flag**: "Feedback Loops" where a task execution triggers an invalidation of its own input.
- **Logic**: Use `state value changed` events to track the ripple effect of updates.

---

## 💡 Step 5: Optimization Playbook (Strategies)

1. **Task Bypassing**: Move from `#[turbo_tasks::function]` to plain Rust functions.
   - **⚠️ Caution**: This reduces the *count* of tasks but often has negligible impact on wall-clock time if the logic is lightweight (e.g., simple regex or boolean checks). Only apply if the *aggregate* overhead is proven to be substantial (>500ms).
2. **Granularity Tuning**: If a task is too big, it invalidates too often. If too small, scheduling overhead kills performance.
3. **Regex Caching**: Always use `OnceLock` or Turbo-cached regex providers for resolution plugins.
4. **I/O Batching**: Use `FileSystemPath::read_dir` to populate metadata caches before requesting individual file stats.

## 📂 Resource Mapping
- Treat trace data as a structured database for quantitative analysis.
- Prioritize optimizations based on the "Hotspots" and "Parent Chains" identified in Step 2.