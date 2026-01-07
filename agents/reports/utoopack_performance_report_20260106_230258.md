# 📊 Utoopack Performance Analysis Report

## 🛠 Build Context
- **Target**: `examples/with-antd`
- **Command**: `npm run build --prefix examples/with-antd`
- **Trace Date**: 2026-01-06 23:02:58
- **Trace File**: `.trace/trace_20260106_230258.json`

---

## 🔍 Tier 1: The Runtime Backbone (P0)
### 💥 A. Task Explosion
- **Metric**: `turbo_tasks::function`
- **Count**: **2,016,007 calls** (derived from 4,032,014 B/E events).
- **Baseline**: 1,965,035 calls (Report 20260106_225437).
- **Delta**: **+50,972 calls (+2.6%)**.
- **Observation**: **FAILURE**. The expected drop of ~590k tasks was not achieved. Instead, there is a slight increase in task count.

### 🗺 B. Resolution Hotspots
- **Metric**: `turbo_tasks::resolve_call`
- **Count**: **630,650 calls** (derived from 1,261,300 events).
- **Baseline**: 588,895 calls.
- **Delta**: **+41,755 calls (+7.1%)**.
- **Observation**: Resolution overhead has increased, contrary to expectations.

### 🔎 C. Specific Resolution Checks
- **Metric**: `resolve_internal`
- **Count**: **0** (Verified via grep).
- **Observation**: The span `resolve_internal` is absent, consistent with the intention to remove it, but this did not translate to overall task reduction.

---

## 🔍 Tier 2: Physical & Resource Barriers (P1)
### 📁 C. I/O Chokepoints
- **Metric**: `read file`
- **Count**: 8,555 calls (derived from 17,110 events).
- **Baseline**: 8,555 calls.
- **Observation**: Identity. No change in I/O profile.

---

## 🔍 Tier 4: The Asset Processing Pipeline (P3)
### ⚛️ G. Pipeline Performance
- **Metric**: `parse ecmascript`
- **Count**: 29,022 calls (derived from 58,044 events).
- **Baseline**: 29,087 calls.
- **Observation**: Minor variance (-65 calls).

- **Metric**: `precompute code generation`
- **Count**: 110,116 calls (derived from 220,232 events).
- **Baseline**: 110,226 calls.
- **Observation**: Stable.

---

## 🚨 Conclusion
The optimization attempt to remove `resolve_internal` appears to have successfully removed the specific span (count is 0), but **failed** to reduce the overall `turbo_tasks::function` count. In fact, both total tasks and resolution calls increased significantly (+50k and +41k respectively).

Investigation required:
1. Did the logic for `resolve_internal` just move to another function (e.g., `resolve_call`)?
2. Why did `resolve_call` increase by ~42k?
