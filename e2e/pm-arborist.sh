#!/bin/bash
#
# Arborist-derived e2e install tests for utoo PM
# Ported from npm/cli workspaces/arborist/test/fixtures (~163 fixtures)
#
# Usage:
#   ./e2e/pm-arborist.sh              # run all tests
#   ./e2e/pm-arborist.sh peer         # run only tests matching "peer"
#   ./e2e/pm-arborist.sh --list       # list all test cases
#   ./e2e/pm-arborist.sh --all        # run including known-skip tests
#

set -o pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
DIM='\033[2m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ARBORIST_DIR="$SCRIPT_DIR/pm/arborist"
PASS=0
FAIL=0
SKIP=0
FILTER="${1:-}"
RUN_ALL=false
[ "$FILTER" = "--all" ] && RUN_ALL=true && FILTER=""

pass() {
  PASS=$((PASS + 1))
  echo -e "  ${GREEN}PASS${NC} $1"
}

fail() {
  FAIL=$((FAIL + 1))
  echo -e "  ${RED}FAIL${NC} $1"
}

skip() {
  SKIP=$((SKIP + 1))
  echo -e "  ${DIM}SKIP${NC} $1"
}

clean_fixture() {
  local dir="$1"
  rm -rf "$dir/node_modules" "$dir/package-lock.json"
  find "$dir" -mindepth 2 -name "node_modules" -type d -exec rm -rf {} + 2>/dev/null || true
}

# Lightweight check: generated lockfile must be valid JSON when present.
# On failure, prints the fixture name via fail() and returns 1.
assert_valid_package_lock() {
  local name="$1"
  if [ ! -f "package-lock.json" ]; then
    return 0
  fi
  if ! node -e "JSON.parse(require('fs').readFileSync('package-lock.json','utf8'))" 2>/dev/null; then
    fail "$name (package-lock.json is not valid JSON)"
    return 1
  fi
  return 0
}

# ----------------------------------------------------------------
# Known-skip lists: features utoo PM does not yet support.
# See e2e/pm/arborist/README.md for details.
# Remove entries as features are implemented.
# ----------------------------------------------------------------

# [SKIP:file-target-missing] fixture declares `file:` deps that cannot be
# satisfied from our ported copy. Most of the originally-missing `target/`
# link dirs are now ported back; the remaining entries are kept for issues
# unrelated to simple data absence:
#   * link-dep-cycle — a→b→a file: cycle; needs cycle-safe resolution.
#   * link-dep-lifecycle-scripts — runs `prepare`/`postinstall` in a file: dep
#     and also requires lockfile-driven install-script semantics.
#   * external-link-dep — self-references `file:./node_modules/abbrev`, which
#     only exists after install (chicken-and-egg).
#   * yarn-stuff — references `file:abbrev-1.1.1.tgz` and
#     `file:./abbrev-link-target` which never existed even upstream.
SKIP_FILE_TARGET_MISSING=(
  link-dep-cycle
  link-dep-lifecycle-scripts
  external-link-dep
  yarn-stuff
)

# `SKIP_BUNDLED_FILE` has been opened up — utoo handles bundled file: deps.

# [SKIP:file-semantic] limitations of utoo's current `file:` resolver that
# surface on real fixtures (verified via CI):
#  * link-meta-deps / link-meta-deps-empty — transitive `file:` deps inside a
#    registry-published package; utoo cannot recover the origin dir since the
#    parent's tarball came from the registry, not disk.
#  * link-dep-has-dep-with-optional-dep — spec "./a" is parsed as GitHub
#    shorthand rather than a file path (pre-existing spec-parser behavior).
#  * audit-mkdirp — inner file: target has a `package.json` without a name.
SKIP_FILE_SEMANTIC=(
  link-meta-deps link-meta-deps-empty
  link-dep-has-dep-with-optional-dep
  audit-mkdirp
)

# [SKIP:optional-transitive] optional dep subtree with missing transitive deps
# should be silently skipped; utoo still hard-fails the whole install.
SKIP_OPTIONAL_TRANSITIVE=(
  optional-dep-tgz-missing optional-metadep-missing optional-metadep-enotarget
)

# [SKIP:peer-strict] utoo does not emit ERESOLVE for unsatisfiable peer deps.
SKIP_PEER_STRICT=(
  testing-peer-deps-unresolvable
)

# [SKIP:platform-reject] utoo does not check os/cpu/libc fields (EBADPLATFORM).
SKIP_PLATFORM=(
  platform-specification
)

# [SKIP:workspace-duplicate] utoo does not detect duplicate workspace package names.
SKIP_WORKSPACE_DUP=(
  workspaces-duplicate
)

# [SKIP:dep-cycle-oom] infinite dep cycle causes OOM/timeout — CI runner kills
# the whole job with SIGTERM once utoo goes into a recursive fetch loop.
SKIP_DEP_CYCLE=(
  pathological-dep-nesting-cycle
)

# [SKIP:mock-registry] packages only exist in npm's @npmcli/mock-registry
SKIP_REGISTRY_ONLY=(
  audit-linked-package
  testing-missing-tgz
)

# [SKIP:misc] various fixture-specific issues
# workspaces-conflicting-dev-deps: ajv@5.11.2 was unpublished from registry
# ancient-lockfile-invalid: depends on @isaacs/this-does-not-exist-at-all
SKIP_MISC=(
  workspaces-conflicting-dev-deps
  ancient-lockfile-invalid
)

# Build combined skip string (newline-separated "name|reason" pairs)
SKIP_LIST=""
_add_skip() {
  local reason="$1"; shift
  for n in "$@"; do
    SKIP_LIST="${SKIP_LIST}${n}|${reason}"$'\n'
  done
}
_add_skip "file: target missing from fixture port" "${SKIP_FILE_TARGET_MISSING[@]}"
_add_skip "file: resolver limitation"              "${SKIP_FILE_SEMANTIC[@]}"
_add_skip "optional transitive"  "${SKIP_OPTIONAL_TRANSITIVE[@]}"
_add_skip "strict peer deps"    "${SKIP_PEER_STRICT[@]}"
_add_skip "platform reject"     "${SKIP_PLATFORM[@]}"
_add_skip "workspace duplicate"  "${SKIP_WORKSPACE_DUP[@]}"
_add_skip "dep cycle OOM"       "${SKIP_DEP_CYCLE[@]}"
_add_skip "mock registry only"  "${SKIP_REGISTRY_ONLY[@]}"
_add_skip "misc"                "${SKIP_MISC[@]}"

is_skipped() {
  local name="$1"
  if [ "$RUN_ALL" = true ]; then
    return 1
  fi
  echo "$SKIP_LIST" | grep -q "^${name}|"
}

skip_reason() {
  echo "$SKIP_LIST" | grep "^${1}|" | head -1 | cut -d'|' -f2
}

# ----------------------------------------------------------------
# Test runners
# ----------------------------------------------------------------

test_install_success() {
  local name="$1"
  local dir="$ARBORIST_DIR/$name"
  local flags="${2:-}"

  if [ ! -f "$dir/package.json" ]; then
    skip "$name (no package.json)"
    return
  fi

  if is_skipped "$name"; then
    skip "$name ($(skip_reason "$name"))"
    return
  fi

  cd "$dir"
  clean_fixture .

  if utoo install $flags 2>&1; then
    if [ -d "node_modules" ] || [ -f "package-lock.json" ]; then
      assert_valid_package_lock "$name" || return
      pass "$name"
    else
      local dep_count
      dep_count=$(node -e "const p=JSON.parse(require('fs').readFileSync('package.json','utf8')); console.log(Object.keys(p.dependencies||{}).length + Object.keys(p.devDependencies||{}).length + Object.keys(p.optionalDependencies||{}).length)" 2>/dev/null || echo "0")
      if [ "$dep_count" = "0" ]; then
        assert_valid_package_lock "$name" || return
        pass "$name (zero deps)"
      else
        fail "$name (no node_modules created)"
      fi
    fi
  else
    fail "$name (install exited non-zero)"
  fi
}

test_install_failure() {
  local name="$1"
  local reason="$2"
  local dir="$ARBORIST_DIR/$name"

  if [ ! -f "$dir/package.json" ]; then
    skip "$name (no package.json)"
    return
  fi

  if is_skipped "$name"; then
    skip "$name ($(skip_reason "$name"))"
    return
  fi

  cd "$dir"
  clean_fixture .

  if utoo install 2>&1; then
    fail "$name (should have failed: $reason)"
  else
    pass "$name (correctly failed: $reason)"
  fi
}

test_install_optional_graceful() {
  local name="$1"
  local desc="$2"
  local dir="$ARBORIST_DIR/$name"

  if [ ! -f "$dir/package.json" ]; then
    skip "$name (no package.json)"
    return
  fi

  if is_skipped "$name"; then
    skip "$name ($(skip_reason "$name"))"
    return
  fi

  cd "$dir"
  clean_fixture .

  if utoo install 2>&1; then
    assert_valid_package_lock "$name" || return
    pass "$name ($desc)"
  else
    fail "$name (should succeed despite optional dep issue: $desc)"
  fi
}

should_run() {
  local name="$1"
  if [ -z "$FILTER" ]; then
    return 0
  fi
  if [ "$FILTER" = "--list" ]; then
    return 1
  fi
  echo "$name" | grep -qi "$FILTER"
}

if [ "$FILTER" = "--list" ]; then
  echo "Available test fixtures:"
  for d in "$ARBORIST_DIR"/*/; do
    name="$(basename "$d")"
    [ ! -f "$d/package.json" ] && continue
    if is_skipped "$name"; then
      echo -e "  ${DIM}$name (skip: $(skip_reason "$name"))${NC}"
    else
      echo "  $name"
    fi
  done
  exit 0
fi

echo -e "${YELLOW}Arborist e2e tests${NC}"
echo -e "utoo: $(which utoo 2>/dev/null || echo 'not found')"
echo -e "filter: ${FILTER:-all}  run_all: $RUN_ALL"
echo ""

# ================================================================
# SECTION 1: Peer Dependencies
# ================================================================
echo -e "${CYAN}--- Peer Dependencies ---${NC}"

for name in \
  testing-peer-deps \
  testing-peer-deps-nested \
  testing-peer-deps-overlap \
  testing-peer-deps-bad-sw \
  peer-dep-cycle \
  peer-dep-cycle-with-sw \
  peer-dep-cycle-nested \
  peer-dep-cycle-nested-with-sw \
  peer-optional-installs \
  test-peer-looser-than-dev \
  sequental-peer-dep-tree-shuffling \
  carbonium \
; do
  should_run "$name" && test_install_success "$name"
done

# Unresolvable peer deps - should fail
for name in \
  testing-peer-deps-unresolvable \
; do
  should_run "$name" && test_install_failure "$name" "unresolvable peer deps"
done

# Peer dep conflict chain sub-fixtures
if should_run "testing-peer-dep-conflict-chain" && [ -d "$ARBORIST_DIR/testing-peer-dep-conflict-chain" ]; then
  for sub in "$ARBORIST_DIR/testing-peer-dep-conflict-chain"/*/; do
    subname="$(basename "$sub")"
    if [ -f "$sub/package.json" ]; then
      cd "$sub"
      clean_fixture .
      if utoo install 2>&1; then
        assert_valid_package_lock "testing-peer-dep-conflict-chain/$subname" || continue
        pass "testing-peer-dep-conflict-chain/$subname"
      else
        fail "testing-peer-dep-conflict-chain/$subname"
      fi
    fi
  done
fi

# ================================================================
# SECTION 2: Optional Dependencies
# ================================================================
echo -e "\n${CYAN}--- Optional Dependencies ---${NC}"

for name in \
  optional-ok \
  optional-engine-specification \
  optional-platform-specification \
; do
  should_run "$name" && test_install_optional_graceful "$name" "optional dep handled"
done

for name in \
  optional-dep-missing \
  optional-dep-enotarget \
  optional-dep-tgz-missing \
  optional-metadep-missing \
  optional-metadep-enotarget \
  optional-metadep-tgz-missing \
; do
  should_run "$name" && test_install_optional_graceful "$name" "missing optional skipped"
done

for name in \
  optional-dep-allinstall-fail \
  optional-dep-install-fail \
  optional-dep-postinstall-fail \
  optional-dep-preinstall-fail \
  optional-metadep-allinstall-fail \
  optional-metadep-install-fail \
  optional-metadep-postinstall-fail \
  optional-metadep-preinstall-fail \
; do
  should_run "$name" && test_install_optional_graceful "$name" "failing optional script skipped"
done

# ================================================================
# SECTION 3: Production Dependency Errors
# ================================================================
echo -e "\n${CYAN}--- Production Dependency Errors ---${NC}"

for name in \
  prod-dep-missing \
  prod-dep-enotarget \
  prod-dep-tgz-missing \
; do
  should_run "$name" && test_install_failure "$name" "missing/unavailable prod dep"
done

for name in \
  prod-dep-allinstall-fail \
  prod-dep-install-fail \
  prod-dep-postinstall-fail \
  prod-dep-preinstall-fail \
; do
  should_run "$name" && test_install_failure "$name" "prod dep script failure"
done

# ================================================================
# SECTION 4: Deduplication
# ================================================================
echo -e "\n${CYAN}--- Deduplication ---${NC}"

for name in \
  dedupe-tests \
  dedupe-tests-2 \
  dedupe-lockfile \
  dedupe-actual \
; do
  should_run "$name" && test_install_success "$name"
done

# ================================================================
# SECTION 5: Workspaces
# ================================================================
echo -e "\n${CYAN}--- Workspaces ---${NC}"

for name in \
  workspaces-simple \
  workspaces-simple-virtual \
  workspaces-conflicting-versions \
  workspaces-conflicting-versions-virtual \
  workspaces-conflicting-dev-deps \
  workspaces-shared-deps \
  workspaces-shared-deps-virtual \
  workspaces-scoped-pkg \
  workspaces-prefer-linking \
  workspaces-prefer-linking-virtual \
  workspaces-version-unsatisfied \
  workspaces-version-unsatisfied-virtual \
  workspaces-top-level-link \
  workspaces-top-level-link-virtual \
  workspaces-transitive-deps \
  workspaces-transitive-deps-virtual \
  workspaces-peer-ranges \
  workspaces-with-overrides \
  workspaces-with-files-spec \
  workspaces-root-linked \
  workspaces-need-update \
  workspaces-not-root \
  workspaces-ignore-nm \
  workspaces-ignore-nm-virtual \
  workspaces-link-bin \
  workspaces-add-new-dep \
  workspaces-non-simplistic \
  workspace \
  workspace3 \
  workspace4 \
; do
  should_run "$name" && test_install_success "$name"
done

for name in \
  workspaces-duplicate \
; do
  should_run "$name" && test_install_failure "$name" "duplicate workspace names"
done

# ================================================================
# SECTION 6: Link Dependencies
# ================================================================
echo -e "\n${CYAN}--- Link Dependencies ---${NC}"

for name in \
  link-dep \
  link-dep-empty \
  link-dep-nested \
  link-dep-cycle \
  link-dep-has-dep-with-optional-dep \
  link-dep-lifecycle-scripts \
  link-dev-dep \
  link-meta-deps \
  link-meta-deps-empty \
  external-link-dep \
  cli-750 \
  cli-750-fresh \
  old-lock-with-link \
; do
  should_run "$name" && test_install_success "$name"
done

# ================================================================
# SECTION 7: Bundled Dependencies
# ================================================================
echo -e "\n${CYAN}--- Bundled Dependencies ---${NC}"

for name in \
  testing-bundledeps \
  testing-bundledeps-2 \
  testing-bundledeps-3 \
  testing-bundledeps-empty \
  testing-bundledeps-sw \
  testing-bundledeps-legacy-bundling \
  testing-rebuild-bundle \
  testing-rebuild-bundle-reified \
  two-bundled-deps \
  bundle-metadep-duplication \
  conflict-bundle-file-dep \
  root-bundler \
; do
  should_run "$name" && test_install_success "$name"
done

# ================================================================
# SECTION 8: Dev/Optional Flags
# ================================================================
echo -e "\n${CYAN}--- Dev/Optional Flags ---${NC}"

for name in \
  testing-dev-optional-flags \
  testing-dev-optional-flags-2 \
  prune-dev-dep-duplicate \
; do
  should_run "$name" && test_install_success "$name"
done

# ================================================================
# SECTION 9: Shrinkwrap & Lockfile Compat
# ================================================================
echo -e "\n${CYAN}--- Shrinkwrap & Lockfiles ---${NC}"

for name in \
  shrinkwrapped-dep-no-lock \
  shrinkwrapped-dep-no-lock-empty \
  shrinkwrapped-dep-with-lock \
  shrinkwrapped-dep-with-lock-empty \
  test-package-with-shrinkwrap \
  empty-with-shrinkwrap \
  old-package-lock \
  old-package-lock-with-bins \
  semver-installed-with-old-package-lock \
  ancient-lockfile-invalid \
; do
  should_run "$name" && test_install_success "$name"
done

# ================================================================
# SECTION 10: Yarn Lock Compat
# ================================================================
echo -e "\n${CYAN}--- Yarn Lock ---${NC}"

for name in \
  yarn-lock-mkdirp \
  yarn-lock-mkdirp-no-resolved \
  yarn-stuff \
  tap-with-yarn-lock \
; do
  should_run "$name" && test_install_success "$name"
done

# ================================================================
# SECTION 11: Bin Handling
# ================================================================
echo -e "\n${CYAN}--- Bin Handling ---${NC}"

for name in \
  testing-asymmetrical-bin-no-lock \
  testing-asymmetrical-bin-with-lock \
  dep-installed-without-bin-link \
; do
  should_run "$name" && test_install_success "$name"
done

# ================================================================
# SECTION 12: Engine & Platform
# ================================================================
echo -e "\n${CYAN}--- Engine & Platform ---${NC}"

for name in \
  engine-specification \
; do
  should_run "$name" && test_install_success "$name"
done

for name in \
  platform-specification \
; do
  should_run "$name" && test_install_failure "$name" "platform mismatch"
done

# ================================================================
# SECTION 13: Lifecycle Script Failures
# ================================================================
echo -e "\n${CYAN}--- Lifecycle Script Failures ---${NC}"

for name in \
  fail-install \
  fail-preinstall \
  fail-postinstall \
  fail-allinstall \
; do
  should_run "$name" && test_install_failure "$name" "lifecycle script failure"
done

for name in \
  rebuild-foreground-scripts \
  testing-rebuild-script-env-flags \
; do
  should_run "$name" && test_install_success "$name"
done

# ================================================================
# SECTION 14: Tarball & Git Dependencies
# ================================================================
echo -e "\n${CYAN}--- Tarball & Git Dependencies ---${NC}"

for name in \
  tarball-dependencies \
  minimist-git-dep \
  minimist-git-metadep \
; do
  should_run "$name" && test_install_success "$name"
done

# ================================================================
# SECTION 15: Update & Outdated
# ================================================================
echo -e "\n${CYAN}--- Update & Outdated ---${NC}"

for name in \
  once-outdated \
  flow-outdated \
  update-exact-version \
; do
  should_run "$name" && test_install_success "$name"
done

# ================================================================
# SECTION 16: Prune
# ================================================================
echo -e "\n${CYAN}--- Prune ---${NC}"

for name in \
  prune-lockfile \
  prune-lockfile-omit-dev \
  prune-lockfile-optional-peer \
  prune-actual \
  prune-actual-omit-dev \
  prune-dev-bins \
; do
  should_run "$name" && test_install_success "$name"
done

# ================================================================
# SECTION 17: Real-World Packages
# ================================================================
echo -e "\n${CYAN}--- Real-World Packages ---${NC}"

for name in \
  sax \
  yargs \
  simple \
  deprecated-dep \
  dep-missing-resolved \
  mkdirp-pinned \
  test-root-matches-metadep \
  pathological-dep-nesting-cycle \
; do
  should_run "$name" && test_install_success "$name"
done

# ================================================================
# SECTION 18: Large Integration
# ================================================================
echo -e "\n${CYAN}--- Large Integration ---${NC}"

for name in \
  tap-and-flow \
  tap-react15-collision \
  tap-react15-collision-legacy-sw \
; do
  should_run "$name" && test_install_success "$name"
done

# ================================================================
# SECTION 19: Package.json Edge Cases
# ================================================================
echo -e "\n${CYAN}--- Package.json Edge Cases ---${NC}"

for name in \
  package-json-shorthands \
  tab-indented-package-json \
  testing-missing-tgz \
; do
  should_run "$name" && test_install_success "$name"
done

for name in \
  bad \
; do
  should_run "$name" && test_install_failure "$name" "malformed package.json"
done

# ================================================================
# SECTION 20: Audit
# ================================================================
echo -e "\n${CYAN}--- Audit ---${NC}"

for name in \
  audit-all-severities \
  audit-dep-vuln-with-own-advisory \
  audit-fix-old-tap \
  audit-linked-package \
  audit-missing-packument \
  audit-mkdirp \
  audit-nyc-mkdirp \
  audit-omit \
  audit-one-vuln \
; do
  should_run "$name" && test_install_success "$name"
done

# ================================================================
# SECTION 21: Idempotent Reinstall
# ================================================================
echo -e "\n${CYAN}--- Idempotent Reinstall ---${NC}"

for name in \
  testing-peer-deps \
  dedupe-tests \
  workspaces-simple \
  sax \
  once-outdated \
; do
  if should_run "reinstall-$name"; then
    dir="$ARBORIST_DIR/$name"
    if is_skipped "$name"; then
      skip "reinstall-$name ($(skip_reason "$name"))"
    elif [ -d "$dir/node_modules" ]; then
      cd "$dir"
      if utoo install 2>&1; then
        assert_valid_package_lock "reinstall-$name" || continue
        pass "reinstall-$name"
      else
        fail "reinstall-$name (second install failed)"
      fi
    else
      skip "reinstall-$name (no prior install)"
    fi
  fi
done

# ================================================================
# Summary
# ================================================================
echo ""
echo "========================================"
TOTAL=$((PASS + FAIL))
echo -e "Results: ${GREEN}$PASS passed${NC}, ${RED}$FAIL failed${NC}, ${DIM}$SKIP skipped${NC}, $TOTAL run"
echo "========================================"

if [ "$FAIL" -gt 0 ]; then
  echo -e "${RED}Some tests failed!${NC}"
  exit 1
fi

echo -e "${GREEN}All arborist e2e tests passed!${NC}"
