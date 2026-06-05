---
name: utoo-upstream-sync
description: Use when syncing upstream vercel/next.js changes into this repository's next.js submodule, especially changes under the upstream turbopack/ subtree, resolving rebase conflicts, adapting local Rust crates to upstream API changes, fixing utoo-web wasm builds, updating snapshots, or committing submodule and local follow-up fixes.
---

# Utoo Upstream Sync

This skill helps sync upstream `vercel/next.js` changes into this repository's `next.js` submodule. Keep the skill in this repository, not inside the `next.js` submodule, because submodule changes are committed to the `utooland/next.js` fork and should stay as close to upstream as possible.

In this workflow, `next.js` is the submodule directory and may also be used locally as a remote name; `turbopack/` is a subtree inside upstream `vercel/next.js`, not a separate upstream repository.

## Repository Model

- Current repository: the working tree where this skill is installed
- Submodule directory: `next.js`
- Submodule repository: `utooland/next.js`
- Submodule integration branch: `utoo`
- Upstream repository: `vercel/next.js`
- Upstream branch: `canary`
- Sync operation: rebase `utooland/next.js` branch `utoo` onto upstream `vercel/next.js` branch `canary`
- Submodule commits must be made inside `next.js`.
- Keep submodule changes minimal and directly related to syncing or adapting upstream `vercel/next.js`.
- Current repository commits should record the submodule pointer separately from local adapter fixes when practical.
- Do not commit local generated files unless explicitly requested, especially:
  - `packages/utoo-web/src/utoo/index.d.ts`
  - `report/`

## Default Workflow

1. Inspect both worktrees before changing anything:

   ```bash
   git status --short --branch
   git -C next.js status --short --branch
   ```

2. In the `next.js` submodule, identify the merge base between the `utoo` branch and upstream `vercel/next.js` `canary`.

   The exact remote names can differ by maintainer. Determine them from `git remote -v` instead of assuming `origin` or `upstream`.

   ```bash
   git -C next.js remote -v
   git -C next.js merge-base <utooland-next-remote>/utoo <vercel-next-remote>/canary
   ```

3. List upstream commits after the merge base and filter for Turbopack/TurboTasks related changes.

   Start with keywords such as `turbo`, `turbopack`, and `task`, but do not rely on keywords alone. Read commit titles and, for suspicious commits, inspect the diff or commit message to decide whether it affects `turbopack/`, `turbo-tasks`, shared Rust APIs, runtime output, wasm, or local utoo patches.

   ```bash
   git -C next.js log --oneline <merge-base>..<vercel-next-remote>/canary
   git -C next.js log --oneline <merge-base>..<vercel-next-remote>/canary | rg -i "(turbo|task)"
   ```

4. Mark risky upstream commits before rebasing. The goal is to choose rebase breakpoints where each stop is likely caused by one upstream commit or one coherent module/API change.

   Risk signals:
   - touches high-risk areas listed below
   - changes public Rust APIs used by local `crates/pack-*`
   - changes generated runtime output or snapshot-sensitive behavior
   - changes Cargo dependencies, toolchains, wasm, or CI assumptions
   - overlaps known utoo patches in `next.js`
   - appears to implement behavior that an existing utoo patch already carries

5. Before rebasing, print the Turbopack-related upstream history and mark the selected breakpoints.

   The user does not need to approve every breakpoint, but they should be able to see the plan. Include enough context to show why each breakpoint was selected.

   Example format:

   ```text
   Turbopack-related upstream commits after <merge-base>:
   [breakpoint] 0070514701 Turbopack: make available_modules an OperationVc ...
                1b77dba691 Turbopack: Fix unsound IntoIterator for ReadRef<T>
   [breakpoint] 5ee7005200 Turbopack: use module graph for NFT
                ...
   ```

6. Rebase in stages using the selected breakpoints, not as one large jump.

   At each breakpoint:
   - resolve conflicts caused by that upstream commit/module change
   - run the smallest verification that covers the touched area
   - for conflicts, build failures, or snapshot failures, identify the causing upstream update, explain why it broke, propose a fix, and wait for user confirmation before editing
   - if an existing utoo patch is now implemented upstream, drop that patch commit only after explaining which patch is covered, which upstream commit covers it, and why the local patch is no longer needed
   - prefer fixes that are easy for a human to review as one feature/module adaptation

7. Keep utoo-specific behavior when it is intentional:
   - wasm guards
   - lightningcss fork usage
   - pack-core / pack-api public API behavior
   - utoo-web wasm build requirements
8. Prefer upstream semantics for shared Turbopack/TurboTasks APIs.
9. Make the smallest compatible local adapter change.
10. Verify the exact path that failed.
11. Commit only related files.

## User Confirmation Gate

Do not silently fix these cases:

- rebase conflicts
- compile failures after a rebase breakpoint
- snapshot test failures
- CI failures that imply code or snapshot changes
- dependency or lockfile conflicts

For these, first report:

- which upstream commit or upstream change likely caused it
- what changed upstream
- why local utoo code, patches, snapshots, or CI assumptions broke
- the proposed fix and why it is the right direction
- any alternatives or risk if the fix is not obvious

Only edit files after the user confirms the proposed direction.

## High-Risk Upstream Areas

Set rebase breakpoints or inspect carefully around commits touching:

- `turbo_tasks::Vc`, `ResolvedVc`, `ReadRef`, `OperationVc`
- resolve APIs: `ResolveResult`, `ModuleResolveResult`, `first_module`, `first_source`, `primary_modules`
- chunking APIs: `make_chunk_group`, `evaluated_chunk_group`, `ChunkGroupResult`, availability info
- `EcmascriptChunkItem`, `ChunkItem`, resolve plugins, `BeforeResolvePlugin`, `AfterResolvePlugin`
- `turbo-rcstr`, `rcstr!`, static atoms, inline atom size
- `turbo-tasks` strongly consistent reads and `#[turbo_tasks::function(root)]`
- file tracing / NFT
- runtime output snapshots
- wasm, worker threads, `wasm-bindgen`, `-Z build-std`

## Common Adaptation Patterns

### Vc methods moved to inner values

If an API now exists on `T` instead of `Vc<T>`, await first:

```rust
result.await?.first_module().await?
result.await?.primary_modules().await?
```

Do not keep old direct calls like:

```rust
result.first_module().await?
```

### `first_module()` now returns `Option<ResolvedVc<_>>`

Do not dereference the `Option`.

Use:

```rust
let Some(module) = result.await?.first_module().await? else {
    return Ok(None);
};
```

Avoid:

```rust
let Some(module) = &*result.await?.first_module().await? else { ... };
```

### ReadRef iteration

After upstream fixes to `ReadRef<T>` iteration, avoid direct owned iteration unless the type supports it.

Prefer:

```rust
for item in &read_ref {
    // ...
}
```

or explicitly access/copy owned data if needed.

### ResolvedVc vs Vc

Check function signatures before converting.

- Expected `Vc<T>`, found `ResolvedVc<T>`: often use `*resolved`.
- Expected `ResolvedVc<T>`, found `Vc<T>`: often use `.to_resolved().await?`.
- Do not mechanically add `*`; compare with upstream call sites first.

### Resolve API

Typical new patterns:

```rust
origin
    .resolve_asset(request, origin.resolve_options(), ty)
    .await?
    .await?
    .primary_modules()
    .await?
```

For one module:

```rust
let Some(module) = resolved.await?.first_module().await? else {
    bail!("unable to resolve ...");
};
```

### Chunking API

When `evaluated_chunk_group` / `evaluated_chunk_group_assets` gains extra chunks:

- Update trait impl and all callers together.
- For no extra assets, pass `OutputAssets::empty()` or the current upstream equivalent.
- If `make_chunk_group` now takes entries rather than `ChunkGroup`, pass `chunk_group.entries()`.
- Match `Vc<ModuleGraph>` vs `ResolvedVc<ModuleGraph>` exactly.

### Diagnostics removal

If `turbopack_core::diagnostics` is gone:

- Do not add compatibility APIs unless the JS package consumes them.
- Prefer upstream `issue`/collectibles flow.
- If package consumers do not read diagnostics, remove diagnostics fields from API/test structs.

### RcStr and wasm

Upstream `rcstr!` static atom optimizations can break wasm or const eval.

Rules:

- wasm long strings must not materialize pointer-backed static atoms in const eval.
- native long-string expansion should keep static prehashed storage, but return `RcStr` by runtime wrapping:

  ```rust
  static RCSTR_STORAGE: StaticPrehashedString = make_const_prehashed_string("...");
  from_static(&RCSTR_STORAGE)
  ```

- Avoid:

  ```rust
  const RCSTR: RcStr = from_static(&RCSTR_STORAGE);
  ```

- `utoo-wasm` needs:

  ```toml
  turbo-rcstr = { workspace = true, features = ["atom_size_128"], optional = true }
  ```

- threaded wasm-bindgen builds need:

  ```toml
  "-Clink-arg=--export=__heap_base",
  ```

### Rust toolchain sync

Keep these aligned:

- current repository `rust-toolchain.toml`
- submodule `next.js/rust-toolchain.toml`
- workflow Rust `toolchain:` values
- explicit `rustup install/default nightly-...` commands

If CI installs one nightly but `rust-toolchain.toml` selects another, `-Z build-std` may fail because `rust-src` was installed for the wrong toolchain.

## Verification Matrix

Run the smallest command that covers the failure.

### General Rust

```bash
cargo check
cargo clippy -p pack-api -- -D warnings
cargo nextest r -p pack-tests
```

### Pack plugin feature

Required when `packages/pack`, `pack-napi`, SWC plugins, or plugin feature paths are touched:

```bash
cargo build --features plugin -p pack-napi --profile release-local
```

### utoo-web wasm

From `packages/utoo-web`:

```bash
ut build:local
```

This validates:

- wasm target
- `-Z build-std`
- `wasm-bindgen`
- worker bundle build
- generated wasm copy

### Snapshots

When snapshots fail:

1. Inspect the actual diff.
2. Identify the upstream commit/change that caused the output change.
3. Explain why JS/CSS/runtime output changed and whether it follows upstream semantics.
4. Propose either code adaptation or snapshot update, then wait for user confirmation.
5. Update only affected snapshots when the change is expected and confirmed.
6. Do not bulk-update snapshots without explaining why the output changed.

## Cargo.lock Conflicts

Do not blindly regenerate `Cargo.lock`.

First identify whether the conflicting side is:

- upstream dependency graph
- utoo fork dependency
- stale stash/local lockfile

If the user does not want the stash `Cargo.lock` during `stash pop`:

```bash
git checkout --ours Cargo.lock
git add Cargo.lock
```

## Commit Policy

### Submodule

Inside `next.js`, commit or amend actual submodule code changes.

Commit message should mention the upstream change that required the adaptation when known.

If a utoo patch commit becomes redundant because upstream now implements equivalent behavior, it can be dropped during the rebase. Before doing so, explicitly report:

- the local patch commit being dropped
- the upstream commit/change that supersedes it
- why the upstream implementation is equivalent or preferable
- any remaining difference, if one exists

### Current repository

Commit separately when possible:

- submodule pointer updates
- local Rust API adaptations
- CI/toolchain updates
- wasm configuration updates
- snapshot updates

Do not include unrelated local files or generated artifacts.

Before committing, inspect:

```bash
git status --short
git diff --cached --stat
git diff --submodule=log -- next.js
```

## Useful Commands

```bash
# Show submodule pointer change
git diff --submodule=log -- next.js

# Verify plugin feature path
cargo build --features plugin -p pack-napi --profile release-local

# Verify wasm package
cd packages/utoo-web
ut build:local
```
