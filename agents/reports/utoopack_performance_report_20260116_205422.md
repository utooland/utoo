# 📊 Utoopack Performance Analysis Report

## 🛠 Build Context
- **Target**: `examples/with-antd`
- **Command**: `TRACING_CHROME=$PWD/.trace/trace_20260116_205422.json npm run build --prefix examples/with-antd`
- **Trace Date**: 2026-01-16 20:54:22
- **Trace File**: `.trace/trace_20260116_205422.json` (1.6GB)
- **Total Events**: 7,892,978

---

## 🔍 Tier 1: The Runtime Backbone (P0) - [CRITICAL]

### 💥 A. Task Explosion (High Priority)
| Metric | Count | Duration (ms) | Avg (ms) |
|:-------|------:|-------------:|--------:|
| `turbo_tasks::function` | **1,957,790** | 18,459.3 | 0.009 |
| `turbo_tasks::resolve_call` | **581,041** | 4,397.6 | 0.008 |
| `task execution completed` | 310,420 | 4,699.0 | 0.015 |

**Diagnosis**: The system is still in an "over-taskified" state. Nearly **2 million** turbo_tasks function calls, averaging only 9µs each, indicates that many micro-operations are being wrapped as tasks unnecessarily.

**Historical Comparison**:
| Date | turbo_tasks::function | resolve_call |
|:-----|----------------------:|-------------:|
| 2026-01-06 21:54 | 1,941,783 | 612,997 |
| 2026-01-06 22:32 | 1,966,398 | 589,058 |
| 2026-01-07 14:00 | 1,967,076 | 589,162 |
| **2026-01-16 20:54** | **1,957,790** | **581,041** |

**Trend**: resolve_call reduced by ~32K calls (-5.2%), showing slight improvement.

---

### 🗺 B. Resolution Hotspots
| Metric | Count | Duration (ms) |
|:-------|------:|-------------:|
| `resolving` | 95,606 | 1,861.2 |
| `internal resolving` | 67,645 | 921.5 |
| `handle_after_resolve_plugins` | 81,464 | 597.3 |
| `resolve_relative_request` | 43,108 | 576.0 |
| `resolve_module_request` | 26,094 | 360.4 |
| `handle_before_resolve_plugins` | 30,499 | 223.3 |

**Analysis**: The module resolution pipeline accounts for approximately **4.5 seconds** total, making it one of the primary hotspots.

---

## 🔍 Tier 2: Physical & Resource Barriers (P1)

### 📁 C. I/O Chokepoints
| Metric | Count | Duration (ms) | Avg (ms) |
|:-------|------:|-------------:|--------:|
| `read file` | 8,555 | 1,883.1 | 0.220 |
| `read directory` | 711 | 270.0 | 0.380 |
| `write file` | 65 | 107.1 | 1.647 |

**Diagnosis**: Total I/O time is ~2.26 seconds, which is reasonable. However, the high call count and total duration of `read file` suggests room for optimization.

---

## 🔍 Tier 4: The Asset Processing Pipeline (P3)

### ⚛️ G. Pipeline Performance
| Stage | Metric | Count | Duration (ms) |
|:------|:-------|------:|-------------:|
| G1. Parsing | `parse ecmascript` | 29,112 | 536.4 |
| G1. Parsing | `swc_parse` | 1,963 | 128.3 |
| G2. Analysis | `analyze ecmascript module` | 44,396 | 1,257.8 |
| G2. Analysis | `process parse result` | 14,941 | 582.1 |
| G2. Analysis | `analyze variable values` | 1,959 | 168.1 |
| G3. Transform | `transforms` | 3,926 | 56.3 |
| G3. Transform | `effects processing` | 77,505 | 962.0 |
| G4. CodeGen | `precompute code generation` | 110,139 | 1,302.6 |
| G4. CodeGen | `code generation` | 27,955 | 301.4 |
| G5. Output | `generate source map` | 1,920 | 117.2 |

**Key Findings**:
1. **`precompute code generation`**: 110,139 calls, 1.3s total - major overhead in code generation phase
2. **`analyze ecmascript module`**: 44,396 calls, 1.26s - significant static analysis overhead
3. **`effects processing`**: 77,505 calls, ~1s - frequent side-effect processing

---

## 🎯 TOP 10 Performance Bottlenecks Summary

| Rank | Operation | Call Count | Total Time (ms) | Issue Type |
|:---:|:-----|--------:|-----------:|:---------|
| 1 | `turbo_tasks::function` | 1,957,790 | 18,459 | P0 Task Explosion |
| 2 | `task execution completed` | 310,420 | 4,699 | P0 Scheduling Overhead |
| 3 | `turbo_tasks::resolve_call` | 581,041 | 4,398 | P0 Resolution Hotspot |
| 4 | `read file` | 8,555 | 1,883 | P1 I/O |
| 5 | `resolving` | 95,606 | 1,861 | P0 Resolution |
| 6 | `precompute code generation` | 110,139 | 1,303 | P3 CodeGen |
| 7 | `analyze ecmascript module` | 44,396 | 1,258 | P3 Module Analysis |
| 8 | `module` | 59,743 | 1,121 | P3 Module Processing |
| 9 | `effects processing` | 77,505 | 962 | P3 Side Effects |
| 10 | `internal resolving` | 67,645 | 922 | P0 Internal Resolution |

---

## ✅ Optimization Results

### Implemented Optimizations

The following optimizations were implemented in `turbopack-core/src/resolve/mod.rs`:

1. **Pre-fetch options once in `resolve_inline`** - Avoid repeated `.await?` calls on options
2. **Early return for empty plugin lists** - Skip plugin handling when no plugins are configured
3. **Pre-resolve plugin conditions** - Resolve all `after_resolve_condition()` calls once before the loop instead of per-path
4. **Consistent `package_json().resolve().await?`** - Ensure cache hits by using resolved Vc consistently
5. **Early return for empty `in_package` rules** - Skip unnecessary iteration in `apply_in_package`

### Performance Results (6 benchmark runs)

| Metric | Baseline | Optimized (avg) | Optimized (best) | Avg Improvement | Best Improvement |
|:-------|--------:|--------------:|-----------------:|----------------:|-----------------:|
| `turbo_tasks::function` | 18,459ms | 18,047ms | 16,784ms | **-2.2%** | **-9.1%** |
| `turbo_tasks::resolve_call` | 4,398ms | 4,240ms | 3,785ms | **-3.6%** | **-13.9%** |
| `read file` | 1,883ms | 1,062ms | 759ms | **-43.6%** | **-59.7%** |

### PR Links
- **Submodule PR**: https://github.com/utooland/next.js/pull/100

---

## 📈 Historical Data Comparison

```
Performance Trends (examples/with-antd):
┌─────────────────────────┬────────────┬────────────┬──────────┐
│ Metric                  │ 2026-01-06 │ 2026-01-16 │ Change   │
├─────────────────────────┼────────────┼────────────┼──────────┤
│ turbo_tasks::function   │ 1,941,783  │ 1,957,790  │ +0.8%    │
│ turbo_tasks::resolve_call│ 612,997   │ 581,041    │ -5.2% ✅ │
│ read file               │ 8,554      │ 8,555      │ +0.0%    │
│ parse ecmascript        │ 29,166     │ 29,112     │ -0.2%    │
└─────────────────────────┴────────────┴────────────┴──────────┘
```

---

## 💡 Future Optimization Recommendations

### P0 Level (Critical)
1. **Reduce Task Wrapping**: ~2M `turbo_tasks::function` calls averaging only 9µs suggests many micro-operations shouldn't be wrapped as tasks
2. **Resolution Cache Optimization**: 580K+ resolve_call needs further optimization:
   - Improve `ResolveOptions` cache hit rate
   - Merge duplicate resolution requests
   - Reduce `handle_before/after_resolve_plugins` call frequency

### P1 Level (Important)
3. **I/O Batching**: 8,555 `read file` calls could benefit from batch reading
4. **Parallelization**: `internal resolving` and `resolving` could benefit from better parallel strategies

### P3 Level (Enhancement)
5. **CodeGen Optimization**: `precompute code generation` with 110K calls could be merged or cached
6. **Module Analysis**: `analyze ecmascript module` with 44K calls may have redundant analysis

---

*Report generated by Utoopack Performance Analysis Agent on 2026-01-16*
*Updated with optimization results on 2026-01-16*
