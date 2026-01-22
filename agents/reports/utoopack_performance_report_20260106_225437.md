# 📊 Utoopack Performance Analysis Report

## 🛠 Build Context
- **Target**: `examples/with-antd`
- **Command**: `up build`
- **Trace Date**: 2026-01-06 22:54:37
- **Trace File**: `.trace/trace_20260106_225437.json`

---

## 🔍 Tier 1: The Runtime Backbone (P0)
### 💥 A. Task Explosion
- **Metric**: `turbo_tasks::function`
- **Count**: 1,965,035 calls (derived from 3,930,070 B/E events).
- **Comparison**: Slight decrease from previous run (1,966,398 calls). The system overhead remains high but stable.

### 🗺 B. Resolution Hotspots
- **Metric**: `turbo_tasks::resolve_call`
- **Count**: **588,895 calls** (derived from 1,177,790 events).
- **Baseline**: 589,058 calls.
- **Delta**: **-163 calls (~0.0%)**.
- **Observation**: Resolution calls are effectively flat compared to the last run. No significant optimization or regression observed in this immediate iteration.
- **Specific Checks**:
    - `cached_parse_str_to_regex`: Not found (Count: 0).
    - `pre_process_externals_config`: Not found in summary.
    - `ExternalsPlugin::after_resolve`: Not found (Count: 0).

---

## 🔍 Tier 2: Physical & Resource Barriers (P1)
### 📁 C. I/O Chokepoints
- **Metric**: `read file`
- **Count**: 8,555 calls (derived from 17,110 events).
- **Baseline**: 8,556 calls.
- **Observation**: Identical IO profile.

---

## 🔍 Tier 4: The Asset Processing Pipeline (P3)
### ⚛️ G. Pipeline Performance
- **Metric**: `parse ecmascript`
- **Count**: 29,087 calls (derived from 58,174 events).
- **Baseline**: 29,128 calls.
- **Observation**: Slight improvement/variance.

- **Metric**: `precompute code generation`
- **Count**: 110,226 calls (derived from 220,452 events).
- **Baseline**: 109,969 calls.
- **Observation**: Stable.
