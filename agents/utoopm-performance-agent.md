# utoo-pm Performance Analysis Agent Protocol

This document defines the specialized diagnostic procedure for analyzing utoo-pm (package manager) performance. It is a universal protocol designed for AI agents to investigate bottlenecks within package installation workflows.

## Objective

Empower AI agents to identify and resolve performance bottlenecks in utoo-pm by analyzing Chrome Trace data. Focus on Network, File I/O, and Decompression operations specific to package management.

---

## Step 1: Data Acquisition & Environment Prep

- **Build**: Run `cargo build --release --bin utoo -p utoo-pm` to compile utoo-pm.
- **Trace Generation**: Run utoo with `TRACING_CHROME=$PWD/.trace/pm_trace_$(date +%Y%m%d_%H%M%S).json`.
  - *Example*: `TRACING_CHROME=$PWD/.trace/pm_trace.json utoo install --prefix examples/with-antd`
- **Intermediate Files**: All filtered JSON fragments and analytical results **MUST** be placed in the `./.trace/` directory. Diagnostic scripts are maintained in the `./agents/tools/` directory.
- **Workspace Hygiene**: Ensure `./.trace/` is in `.gitignore`. Never upload raw trace data (> 500MB) directly; share filtered summaries or key findings.
- **Search Tooling**: Use `ripgrep` (command: `rg`) for all code searches.

### Tracing Overhead Note

Chrome Trace instrumentation introduces overhead. Tasks with duration **< 10us** are likely dominated by tracing instrumentation cost rather than actual work. **Exclude these from statistical analysis** to avoid misleading conclusions.

---

## Step 2: Universal Diagnostic Matrix (Tiers P0-P3)

Follow this tiered hierarchy. Solve P0 before descending to P1, as network latency often dominates cold install time.

### Tier 1: Network Operations (Priority: P0)

*Focus: Download latency, throughput, retry frequency, registry selection.*

- **A. Download Latency (HTTP Round-Trip)**
  - **Signal**: Long `download` spans with significant time before data transfer begins.
  - **Insight**: DNS resolution, TLS handshake, and connection establishment can dominate for many small packages.
  - **Action**: Check if connection pooling is effective. Look for repeated connection setup to the same host.

- **B. Retry Frequency (Network Failures)**
  - **Signal**: Multiple `retry` events for the same URL.
  - **Insight**: High retry count indicates unstable network or overloaded registry.
  - **Action**: Check registry selection logic. Consider faster mirrors (npmmirror vs npmjs).

- **C. Registry Selection Latency**
  - **Signal**: Slow `ping_registry` spans during startup.
  - **Action**: Ensure registry ping is concurrent. Cache selected registry for subsequent installs.

- **D. Throughput Bottleneck**
  - **Signal**: Long download duration with low bytes/second.
  - **Action**: Check concurrent download limits. Current limit is 40 concurrent downloads (from semaphore).

### Tier 2: File I/O Operations (Priority: P1)

*Focus: Clone/copy performance, directory creation, permission setting.*

- **E. Clone Strategy Distribution**
  - **Signal**: Time distribution across `clonefile` (macOS), `ficlone` (Linux), `hardlink`, `copy_file`.
  - **Insight**: CoW cloning (clonefile/ficlone) should be near-instant. Fallback to copy indicates filesystem limitation.
  - **Platforms**:
    - macOS: Native `clonefile()` syscall for instant CoW cloning
    - Linux: FICLONE ioctl (reflink), `copy_file_range()` fallback
    - Windows: Regular async copy

- **F. Directory Creation Overhead**
  - **Signal**: Many `create_dir` spans with cumulative high duration.
  - **Action**: Check if directory creation is deduplicated (DashSet cache should prevent duplicates).

- **G. Permission Setting Overhead (Unix)**
  - **Signal**: Significant time in `set_permissions` spans.
  - **Action**: Batch permission setting or use async operations.

- **H. Hardlink vs Copy Decision**
  - **Signal**: Packages with install scripts using `copy_file` instead of `hardlink`.
  - **Insight**: Hardlinks are used for packages without install scripts. Packages with scripts require full copies.

### Tier 3: Decompression Pipeline (Priority: P2)

*Focus: Gzip decode time, tar extraction throughput, streaming pipeline efficiency.*

- **I. Gzip Decode Throughput**
  - **Signal**: Long `gzip_decode` spans relative to download size.
  - **Insight**: Gzip decoding is CPU-bound. Large packages may benefit from parallel decompression.
  - **Action**: Check if decompression is happening in a blocking context.

- **J. Tar Extraction Efficiency**
  - **Signal**: Long `tar_extract` spans with many small files.
  - **Insight**: Tar extraction is I/O-bound. Many small files cause syscall overhead.
  - **Action**: Consider batching file writes (current batch: 100 files or 50MB).

- **K. Streaming Pipeline Efficiency**
  - **Signal**: Gaps between `gzip_decode` and `file_write_batch` spans.
  - **Insight**: Two-stage pipeline (extract -> write) should overlap.
  - **Action**: Check channel capacity (current: 500 entries). Increase if producer outpaces consumer.

### Tier 4: Concurrency & Batching (Priority: P3)

*Focus: Semaphore utilization, batching efficiency.*

- **L. Semaphore Wait Time**
  - **Signal**: Long waits for `semaphore_acquire` spans.
  - **Insight**: Indicates concurrency bottleneck.
  - **Config**:
    - Download semaphore: 40 concurrent downloads
    - File write semaphore: 16 concurrent writes
  - **Action**: Adjust semaphore limits based on system capabilities.

- **M. Batch Size Efficiency**
  - **Signal**: Many small batches processed sequentially.
  - **Current Config**: 100 files or 50MB per batch.
  - **Action**: Tune batch size based on workload characteristics.

---

## Step 3: Actionable Diagnostic Workflow

1. **Quantitative Baseline**: Run the summary script with the **`TRACE_PROJECT`** environment variable.
   - *Command*: `TRACE_PROJECT=examples/with-antd python3 agents/tools/analyze_pm_trace.py <trace_file> <output_report>`

2. **Qualitative Timeline Scan**: Open the trace in `chrome://tracing` or `edge://tracing`. Look for:
   - Network gaps (idle time between downloads)
   - I/O bottlenecks (long sequential file operations)
   - Pipeline stalls (gaps in streaming decode -> write)

3. **Causal Attribution**: Identify the parent span of top bottlenecks to understand *why* they were invoked.

4. **Final Reporting**: Summarize findings and save the report to `./agents/reports/utoopm_performance_report_YYYYMMDD_HHMMSS.md`. Include specific tiered signals and recommended actions.

---

## Step 4: Cache Analysis

**Goal**: Ensure warm installs are significantly faster than cold installs.

- **Manifest Cache**: Check if package metadata is cached (`~/.cache/nm/manifests`).
- **Tarball Cache**: Check if downloaded packages are cached (`~/.cache/nm/tarballs`).
- **Resolved Marker**: Each extracted package has `_resolved` marker to skip re-extraction.

**Red Flags**:
- Warm install time close to cold install (cache not effective)
- Re-downloading cached packages (cache invalidation issue)

---

## Step 5: Optimization Playbook

1. **Network Optimization**
   - Use concurrent registry pings for faster selection
   - Increase download concurrency if bandwidth allows
   - Consider HTTP/2 multiplexing for registry requests

2. **I/O Optimization**
   - Prefer CoW cloning (clonefile/ficlone) over copy
   - Use hardlinks for packages without install scripts
   - Batch directory creation to reduce syscall overhead

3. **Decompression Optimization**
   - Increase streaming buffer size for large packages
   - Consider parallel decompression for multi-core utilization
   - Tune batch size for optimal memory/throughput tradeoff

4. **Concurrency Tuning**
   - Profile semaphore wait times
   - Adjust limits based on system resources (CPU cores, disk IOPS, network bandwidth)

---

## Resource Mapping

| Operation | File | Key Span Names |
|-----------|------|----------------|
| Download | `util/downloader.rs` | `download`, `http_request` |
| Decompress | `util/downloader.rs` | `gzip_decode`, `tar_extract`, `unpack_stream` |
| Clone/Copy | `util/cloner.rs` | `clone_package`, `clonefile`, `ficlone`, `copy_file` |
| Registry | `util/registry.rs` | `ping_registry`, `select_registry` |
| Install | `service/install.rs` | `install_packages`, `resolve_package` |

---

## Key Metrics to Track

| Metric | Good | Warning | Critical |
|--------|------|---------|----------|
| Cold install time | < 30s | 30-60s | > 60s |
| Warm install time | < 5s | 5-15s | > 15s |
| Download throughput | > 10 MB/s | 5-10 MB/s | < 5 MB/s |
| Clone success rate | > 95% | 80-95% | < 80% |
| Retry rate | < 1% | 1-5% | > 5% |

---

*Protocol Version: 1.0*
*Last Updated: 2026-02*
