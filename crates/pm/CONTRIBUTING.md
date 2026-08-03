# Contributing to `utoo-pm`

A lightweight guide for new contributors to the `utoo` / `ut` package manager.
For project-wide setup (submodules, prerequisites, release flow) see the
root [CONTRIBUTING.md](../../CONTRIBUTING.md).

---

## Toolchain

[rust-toolchain.toml](../../rust-toolchain.toml) pins `nightly-2026-05-15` with
`rustfmt` + `clippy`. The correct toolchain is selected automatically — just run
`cargo`.

---

## Code Layout

The PM crate (`crates/pm`) follows a strict three-layer separation.
Each layer has a single responsibility, enforced by convention:

| Layer | Directory | Role |
|-------|-----------|------|
| CLI definitions | `src/cli.rs` | Clap types only — `Cli` struct, `Commands` enum, global flags. No business logic. |
| Command dispatch | `src/cmd/` | Thin handlers, one module per subcommand. Assemble args, delegate to `service`. |
| Business logic | `src/service/` | Orchestrates installs, publish, workspace ops. Calls `util` + `utoo_ruborist`. |
| Helpers | `src/helper/` | Cross-cutting: workspace topology, lockfile lifecycle, dep-graph, migration. |
| Utilities | `src/util/` | Leaf I/O and runtime adapters: http, cache, linker, registry, logger. |
| Binary entry | `src/main.rs` | Builds tokio runtime, parses `Cli`, routes subcommands to `cmd::*`. |
| Constants | `src/constants.rs` | Command names, aliases, about strings, app metadata. |

### Request flow

```
main.rs  ──parse Cli──►  cmd::<subcommand>  ──delegate──►  service::<logic>  ──uses──►  util:: / utoo_ruborist
                           (assemble args,       (install, publish,                 (http, cache,
                            map flags)            workspace, deps...)                linker, registry)
```

### What goes where

- **Adding a CLI flag?** Define it in `src/cli.rs` (or `src/cmd/<command>.rs`
  for command-specific args via `#[derive(Args)]`), then read it in the
  `cmd::*` handler and pass it to `service`.
- **Changing install behaviour?** Edit `src/service/install.rs` +
  `src/service/install_scheduler.rs`.
- **New registry/cache logic?** Add to `src/util/`, never to `service/`.
- **New subcommand?** Add a variant in `Commands` (`cli.rs`), a module in
  `src/cmd/`, a handler in `src/service/`, and constants in `src/constants.rs`.

Where to find common code:

| You want to find... | Look at... |
|---------------------|------------|
| Command parsing & subcommand variants | `src/cli.rs` — `Cli` struct, `Commands` enum |
| Where a subcommand is dispatched | `src/main.rs` — `match cli.command` block |
| A specific command's entry function | `src/cmd/<name>.rs` — e.g. `cmd::install::run`, `cmd::ping::ping` |
| Business logic behind a command | `src/service/<name>.rs` — e.g. `service::install::InstallService` |
| HTTP / registry / cache / linker | `src/util/` — `http.rs`, `registry.rs`, `cache.rs`, `linker.rs` |
| Lockfile & workspace topology | `src/helper/` — `lock.rs`, `workspace.rs`, `deps.rs` |
| Command names, aliases, help text | `src/constants.rs` (inline `cmd` submodule) |

---

## Development Commands

### Build

```bash
cargo build -p utoo-pm                          # debug build
cargo build -p utoo-pm --profile release-local  # fast release (no LTO)
```

The binary is `target/debug/utoo` (or `target/release-local/utoo` for the
fast-release profile). Symlink or alias it as `ut` for local testing.

### Test

```bash
cargo test -p utoo-pm              # unit tests (includes mockito HTTP mocks)
./e2e/utoo-pm.sh                   # end-to-end integration tests
```

### Lint & format (required)

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings --no-deps
```

These two checks must pass before pushing — `cargo fmt --check` runs in the
pre-push hook.

> **Tip:** Run `cargo fmt` first, then `cargo clippy`. If clippy reports
> warnings, fix them and re-run both — clippy auto-fixes can reformat code.

### Commit

Follow [Conventional Commits](https://www.conventionalcommits.org/) with the
`pm` scope: e.g. `fix(pm): respect --ignore-scripts in workspace install`.

---

## Quick-start: make a small change

1. **Locate the code.** Find the subcommand in `src/cmd/<name>.rs`, follow the
   call into `src/service/<name>.rs`.
2. **Make the change.** Keep the layering rule: `cmd/` assembles args,
   `service/` does the work.
3. **Verify:**

   ```bash
   cargo fmt
   cargo clippy --all-targets -- -D warnings --no-deps
   cargo test -p utoo-pm
   ```

4. **Commit** with `pm` scope and open a PR.
