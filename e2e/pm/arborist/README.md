# Arborist E2E Test Fixtures

Test fixtures ported from [npm/cli `workspaces/arborist/test/fixtures/`](https://github.com/npm/cli/tree/latest/workspaces/arborist/test/fixtures).
163 fixtures covering dependency resolution, installation, and edge cases.

All `@isaacs/testing-*` and `@isaacs/dedupe-tests-*` packages are real packages
published to the npm registry (created by npm team for arborist testing).
No mock registry is needed for most fixtures.

## Usage

```bash
./e2e/pm-arborist.sh              # run all (skip known unsupported)
./e2e/pm-arborist.sh --all        # run everything including skipped
./e2e/pm-arborist.sh peer         # filter by keyword
./e2e/pm-arborist.sh --list       # list all fixtures with skip status
```

## Skip Categories (TODO)

Features utoo PM does not yet support. Tests are skipped in the runner and
tracked here. Remove entries from both this file and `pm-arborist.sh` as
each feature is implemented.

---

### `file:` protocol dependencies — remaining skips

The bulk of `file:`-protocol fixtures now run (utoo supports the spec, and the
missing `target/` link dirs were ported back from upstream). The remaining
skips in this category are issues unrelated to simple data absence:

| Fixture | Issue |
|---|---|
| `link-dep-cycle` | `a→b→a` file: cycle; needs cycle-safe resolution |
| `link-dep-lifecycle-scripts` | runs `prepare`/`postinstall` in a file: dep; needs lockfile-driven install-script semantics |
| `external-link-dep` | self-references `file:./node_modules/abbrev` — only exists after install (chicken-and-egg) |
| `yarn-stuff` | references `file:abbrev-1.1.1.tgz` and `file:./abbrev-link-target` which never existed even upstream |

---

### `file:` resolver semantic limitations

**Fixtures (4):** `link-meta-deps`, `link-meta-deps-empty`, `link-dep-has-dep-with-optional-dep`, `audit-mkdirp`

- `link-meta-deps` / `link-meta-deps-empty` — transitive `file:` dep inside a registry-published package; utoo cannot recover the origin dir after the parent's tarball came from the registry.
- `link-dep-has-dep-with-optional-dep` — spec `"./a"` is parsed as a GitHub shorthand rather than a file path (spec-parser behavior).
- `audit-mkdirp` — inner `file:` target has a `package.json` without a name.

---

### Optional transitive dep failure tolerance

**Fixtures (3):** `optional-dep-tgz-missing`, `optional-metadep-missing`, `optional-metadep-enotarget`

When an optional dependency has a transitive dep that is missing or unreachable, utoo hard-fails the entire install instead of silently skipping the optional subtree.

---

### Strict peer dep conflict detection (ERESOLVE)

**Fixtures (1):** `testing-peer-deps-unresolvable`

utoo does not validate whether peer-dep constraints are mutually satisfiable and does not emit an `ERESOLVE`-style error when they conflict.

---

### Platform mismatch rejection (EBADPLATFORM)

**Fixtures (1):** `platform-specification`

utoo does not check `os` / `cpu` / `libc` fields against the current platform for non-optional dependencies.

---

### Duplicate workspace name detection

**Fixtures (1):** `workspaces-duplicate`

utoo does not detect when multiple workspace packages declare the same `name` in their package.json (should error with `EDUPLICATEWORKSPACE`).

---

### Dependency cycle OOM

**Fixtures (1):** `pathological-dep-nesting-cycle`

`@isaacs/pathological-dep-nesting-a` creates a deep recursive `A→B→A→B` cycle. utoo enters a recursive fetch loop and CI kills the runner with SIGTERM (exit 143).

---

### Mock-registry-only packages

**Fixtures (2):**
`audit-linked-package`, `testing-missing-tgz`

**Details:**
- `audit-linked-package` — depends on `electron-test-app@1.0.0` which does not
  exist on the real npm registry (only in npm's mock `@npmcli/mock-registry`).
- `testing-missing-tgz` — has a `preinstall` script `"this never gets run"` which
  utoo tries to execute (it should be a zero-dep package with no install needed).

---

### Misc

**Fixtures (5):**

| Fixture | Error | Root cause |
|---------|-------|------------|
| `workspaces-conflicting-dev-deps` | `Failed to fetch ajv@5.11.2: Package not found` | ajv@5.11.2 was unpublished from registry |
| `yarn-stuff` | `Failed to fetch remote@https://...abbrev-1.1.1.tgz` | Has `"remote": "https://...tgz"` dep spec — utoo does not support URL-as-version |
| `rebuild-foreground-scripts` | file: dep in sub-package | Contains `file:` protocol dependency in nested package |
| `testing-rebuild-script-env-flags` | file: dep in sub-package | Contains `file:` protocol dependency in nested package |
| `audit-mkdirp` | `Failed to fetch mkdirp-unfixable@file:mkdirp-unfixable` | Has `file:` protocol dependency |

---

## Assertion Coverage (TODO)

Current tests only verify that `utoo install` exits with code 0 (or non-zero
for expected failures) and that `node_modules/` is created. This is
**smoke testing** — it catches crashes but cannot detect wrong versions,
incorrect tree structure, or broken peer dep resolution.

npm/cli's arborist tests use multi-layered assertions:

| Assertion Type | npm/cli | utoo (current) |
|---|---|---|
| Tree structure snapshot | `matchSnapshot(printTree(tree))` | not checked |
| Resolved version checks | `tree.children.get('once').version === '1.3.3'` | not checked |
| File system layout | verifies dirs exist/absent, bin symlinks | `node_modules/` exists only |
| Lock file content | snapshots `package-lock.json`, checks metadata flags | not checked |
| Error codes | `rejects(…, { code: 'ERESOLVE' })` | exit code non-zero only |
| Omitted dep absence | verifies omitted packages not on disk | not checked |
| Idempotence | `reify()` twice → same result | not checked |

### Planned improvements

**Phase 1 — version & structure assertions:**
- Add expected version checks for 15–20 key fixtures (peer-deps, dedup,
  workspace variants) using `node -e "require('./node_modules/pkg/package.json').version"`
- Verify expected packages exist (and unexpected ones don't) in `node_modules/`
- Validate generated `package-lock.json` is valid JSON (implemented in `e2e/pm-arborist.sh`)

**Phase 2 — tree snapshots & error validation:**
- Snapshot `utoo ls --json` output and compare against baselines
- Check specific error messages/codes for expected-failure fixtures
- Verify omitted deps are truly absent on disk for omit test cases

**Phase 3 — full parity:**
- Bin symlink verification
- Lock file content snapshots
- Idempotent reinstall validation (install twice, compare results)

---

## Test Sections

| Section | Description | Count |
|---------|-------------|-------|
| Peer Dependencies | Basic, nested, cyclic, conflict chain | ~16 + sub-fixtures |
| Optional Dependencies | Missing, enotarget, script failures | ~17 |
| Production Dep Errors | Expected failures for missing/bad deps | 7 |
| Deduplication | Version dedup strategies | 4 |
| Workspaces | Simple, conflicting, scoped, transitive, etc. | ~31 |
| Link Dependencies | file: protocol, nested, cyclic | 13 |
| Bundled Dependencies | bundleDependencies scenarios | 12 |
| Dev/Optional Flags | omit flags | 3 |
| Shrinkwrap & Lockfiles | shrinkwrap, lockfile v1/v2 | 10 |
| Yarn Lock | yarn.lock compat | 4 |
| Bin Handling | bin field linking | 3 |
| Engine & Platform | engine/os/cpu checks | 2 |
| Lifecycle Scripts | pre/post/install script failures | 6 |
| Tarball & Git | tgz and git dependencies | 3 |
| Update & Outdated | update scenarios | 3 |
| Prune | prune unused deps | 6 |
| Real-World Packages | sax, yargs, mkdirp, etc. | 8 |
| Large Integration | tap, react, flow | 3 |
| Package.json Edge Cases | shorthands, indentation, malformed | 4 |
| Audit | npm audit scenarios | 9 |
| Idempotent Reinstall | reinstall consistency | 5 |
