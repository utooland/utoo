# Repository Guidelines

- Repo: https://github.com/utooland/utoo

## Project Overview

Utoo is a unified frontend toolchain with a Rust core and TypeScript bindings. It has two main products:
- **Package Manager (PM)** — the `utoo`/`ut` CLI, implemented in `crates/pm`
- **Bundler (Pack)** — `@utoo/pack`, built on Turbopack, with Rust core in `crates/pack-core` and Node.js bindings via NAPI-RS in `crates/pack-napi`

The `next.js/` directory is a **git submodule** (`utooland/next.js`) providing Turbopack crates. Many Rust crates depend on paths within it. Initialize with `git submodule update --init --recursive`.

## Project Structure & Module Organization

### Rust Crates (`crates/`)

| Crate | Purpose |
|-------|---------|
| `pm` | Package manager CLI (`utoo`/`ut`). Modules: `cmd/` (CLI commands), `service/` (business logic), `util/` (registry, cache, linker), `helper/` (workspace, deps) |
| `pack-core` | Bundler core built on Turbopack |
| `pack-api` | API layer for bundling operations (ProjectContainer, entrypoints) |
| `pack-napi` | NAPI-RS bindings exposing bundler to Node.js |
| `pack-schema` | Configuration schema definitions |
| `pack-tests` | Snapshot-based bundler integration tests |
| `pack-cli` | Rust CLI wrapper for the bundler |
| `ruborist` | Dependency resolution helper for PM |
| `utoo-wasm` | WebAssembly bindings for `@utoo/web` |

### TypeScript Packages (`packages/`)

| Package | Purpose |
|---------|---------|
| `@utoo/pack` | Main bundler library wrapping pack-napi, includes Webpack compatibility layer |
| `@utoo/pack-cli` | CLI for bundler (`up`/`utoopack` commands) |
| `@utoo/pack-shared` | Shared types and utilities |
| `@utoo/web` | Browser-compatible toolchain via WASM |

### Key Data Flow

- **Pack**: User config → `@utoo/pack` (TS) → `pack-napi` (NAPI bridge) → `pack-api` → `pack-core` (Turbopack) → bundled output
- **PM**: CLI args → `crates/pm/src/cmd/` (command parsing) → `crates/pm/src/service/` (business logic) → `crates/pm/src/util/` (registry, cache, linker)
- **Web**: `@utoo/web` is an independent TS project that uses `utoo-wasm` for browser-compatible toolchain. Demo workflow:
  1. `ut build:local --workspace @utoo/web`
  2. `ut start --workspace utooweb-demo`

## Build, Test, and Development Commands

- Install deps: `npm install` (or bootstrap via global `utoo`/`ut` or locally built `target/release` binary)
- Build all packages: `npm run build` (via Turbo)
- Build specific package: `npx turbo run build --filter=@utoo/pack`
- Watch mode: `npm run dev`
- Build Rust crates: `cargo build`
- Faster local release build (no LTO): `cargo build --profile release-local`

### Testing

- PM unit tests: `cargo test -p utoo-pm`
- Bundler snapshot tests: `cargo test -p pack-tests`
- Update snapshots: `UPDATE=1 cargo test -p pack-tests`
- PM end-to-end tests: `./e2e/utoo-pm.sh`
- All JS tests: `npm run test` (via Turbo)

Bundler snapshot tests live in `crates/pack-tests/tests/snapshot/`. Each test case has an `input/` dir and an `output/` dir with expected results. Set `UPDATE=1` to regenerate snapshots.

### Formatting & Linting

- Format JS/JSON: `npm run biome` (Biome, double quotes, 2-space indent)
- Format Rust: `cargo fmt`
- Format TOML: `tombi format` (or `npx tombi format`)
- Spellcheck: `typos`

The pre-push hook runs: `cargo fmt --check`, `tombi format --check`, `npx biome ci`, `typos`.

## Coding Style & Conventions

- **Commits**: Conventional Commits with scopes — `feat(pm):`, `feat(pack):`, `fix(wasm):`, etc.
- **Rust edition**: 2024, nightly toolchain (version pinned in `rust-toolchain.toml`)
- **JS formatting**: Biome with double quotes, 2-space indent
- **Monorepo**: npm workspaces (JS) + Cargo workspace (Rust), orchestrated by Turborepo
- **Platform binaries**: Pack builds per-platform NAPI binaries (darwin-arm64, linux-gnu, linux-musl, win32-msvc)

## Commit & Pull Request Guidelines

- Follow Conventional Commits: `feat(scope):`, `fix(scope):`, `docs:`, `refactor(scope):`, etc.
- Group related changes; avoid bundling unrelated refactors.
- Keep commit messages concise and action-oriented.

## Subagents

Agent definitions live in `agents/` and are symlinked to `.claude/agents/` for Claude Code discovery.

| Agent | Path | Purpose |
|-------|------|---------|
| **rust-code-guard** | `agents/rust-code-guard.md` | Rust idiom & style review agent. Enforces idiomatic patterns and project conventions based on real PR review cases. |
| **utoopack-performance** | `agents/utoopack-performance-agent.md` | Turbopack performance diagnostics via Chrome Trace analysis across a 5-tier priority matrix. |

## Post-Edit Verification

After modifying Rust code, **always** run these checks before considering the task done:

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings --no-deps
```

Fix any issues found. Do not leave clippy warnings or formatting drift for the user to clean up.

## Agent-Specific Notes

- When adding a new `AGENTS.md` anywhere in the repo, also add a `CLAUDE.md` symlink pointing to it (example: `ln -s AGENTS.md CLAUDE.md`).
- When answering questions, respond with high-confidence answers only: verify in code; do not guess.
- Never edit `node_modules`.
- Lint/format churn:
  - If staged+unstaged diffs are formatting-only, auto-resolve without asking.
  - If commit/push already requested, auto-stage and include formatting-only follow-ups in the same commit, no extra confirmation.
  - Only ask when changes are semantic (logic/data/behavior).
