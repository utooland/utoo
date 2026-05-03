#!/bin/sh
# E2E test for vendor/templates/placeholder.utoo.sh.template self-heal flow.
#
# Stages a fake utoo package (placeholder as bin/utoo + package.json), a
# fake registry directory holding a tarball with a stub "real" binary, and
# runs the placeholder with UTOO_REGISTRY=file://... so the existing env
# override path serves the test offline (no network, works on every CI host).
#
# Verifies the contract a user actually depends on:
#   1. Happy path: placeholder downloads, atomic-renames onto bin/utoo,
#      exec's the real binary, args pass through with boundaries intact.
#   2. Idempotency: a second invocation skips bootstrap and runs the real
#      binary directly (proves the placeholder was actually replaced).
#   3. npm-style symlink shim: invoking through a relative symlink (what
#      npm creates for global bins) resolves $0 correctly so the
#      package.json walk-up finds the right root.
#   4. Failure recovery: registry unreachable → exit non-zero, placeholder
#      stays in place untouched, next invocation with a working registry
#      succeeds. The "permanent dead end" failure mode this PR fixes.
#
# Skipped on Windows: the placeholder is a POSIX sh script that in
# production is bypassed by a Windows .cmd shim; OS detection for the
# Windows code paths is covered by postinstall-os-detect.sh.

set -u

case "$(uname -s 2>/dev/null || echo unknown)" in
    MINGW*|MSYS*|CYGWIN*)
        printf 'skipping placeholder self-heal e2e on Windows\n'
        exit 0
        ;;
esac

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TEMPLATE="$REPO_ROOT/vendor/templates/placeholder.utoo.sh.template"
VERSION="9.9.9-test"

UNAME_S=$(uname -s)
case "$UNAME_S" in
    Darwin) OS=darwin ;;
    Linux) OS=linux ;;
    *)
        printf 'unsupported OS for this test: %s\n' "$UNAME_S" >&2
        exit 1
        ;;
esac
ARCH_RAW=$(uname -m)
case "$ARCH_RAW" in
    x86_64|amd64) ARCH=x64 ;;
    aarch64|arm64) ARCH=arm64 ;;
    *)
        printf 'unsupported arch for this test: %s\n' "$ARCH_RAW" >&2
        exit 1
        ;;
esac
PKG_NAME="utoo-${OS}-${ARCH}"

PASS=0
FAIL=0

ok() { printf '  PASS  %s\n' "$1"; PASS=$((PASS + 1)); }
ko() { printf '  FAIL  %s\n        %s\n' "$1" "$2"; FAIL=$((FAIL + 1)); }

assert_contains() {
    case "$3" in
        *"$2"*) ok "$1" ;;
        *) ko "$1" "expected substring: $2" ;;
    esac
}

assert_not_contains() {
    case "$3" in
        *"$2"*) ko "$1" "unexpected substring present: $2" ;;
        *) ok "$1" ;;
    esac
}

# Build a sandbox with: a fake utoo package (placeholder bin/utoo +
# package.json) and a registry directory that holds the stub tarball.
setup_sandbox() {
    SANDBOX=$(mktemp -d)
    PKG_DIR="$SANDBOX/utoo"
    REGISTRY_DIR="$SANDBOX/registry"

    mkdir -p "$PKG_DIR/bin"
    cat > "$PKG_DIR/package.json" <<JSON
{
  "name": "utoo",
  "version": "$VERSION"
}
JSON
    cp "$TEMPLATE" "$PKG_DIR/bin/utoo"
    chmod +x "$PKG_DIR/bin/utoo"
}

# Build a tarball at $REGISTRY_DIR/@utoo/<pkg>/-/<pkg>-<ver>.tgz containing
# package/bin/utoo as a stub that prints argc + each argv entry, so we can
# assert arg passthrough preserves boundaries (spaces inside args).
publish_stub() {
    STUB=$(mktemp -d)
    mkdir -p "$STUB/package/bin"
    cat > "$STUB/package/bin/utoo" <<'STUB_EOF'
#!/bin/sh
printf 'STUB argc=%d\n' "$#"
i=1
for a in "$@"; do
    printf 'STUB argv[%d]=%s\n' "$i" "$a"
    i=$((i + 1))
done
STUB_EOF
    chmod +x "$STUB/package/bin/utoo"

    REG_PATH="$REGISTRY_DIR/@utoo/$PKG_NAME/-"
    mkdir -p "$REG_PATH"
    (cd "$STUB" && tar -czf "$REG_PATH/${PKG_NAME}-${VERSION}.tgz" package)
    rm -rf "$STUB"
}

teardown_sandbox() {
    rm -rf "$SANDBOX"
}

# Run the placeholder with a controlled REGISTRY env. Wraps in a fresh
# TMPDIR so we can detect mktemp leaks (the trap-before-exec fix this PR
# adds is the contract being verified here).
# Capturing stdout via $(...) runs the function in a subshell, so any
# variables set inside don't reach the caller. Use sentinel files in the
# sandbox to surface tmp-leak count + exit code back to the test body.
run_placeholder() {
    bin=$1
    shift
    UTOO_TMPDIR=$(mktemp -d)
    TMPDIR="$UTOO_TMPDIR" UTOO_REGISTRY="file://$REGISTRY_DIR" \
        "$bin" "$@" 2>&1
    rc=$?
    find "$UTOO_TMPDIR" -mindepth 1 -maxdepth 1 -type d 2>/dev/null \
        | wc -l | tr -d ' ' > "$SANDBOX/leaked_tmp"
    printf '%s\n' "$rc" > "$SANDBOX/last_rc"
    rm -rf "$UTOO_TMPDIR"
}

#------------------------------------------------------------------------------
# Test 1: happy path
#------------------------------------------------------------------------------
printf '\n== happy path ==\n'
setup_sandbox
publish_stub

out=$(run_placeholder "$PKG_DIR/bin/utoo" hello "world arg" final)
rc=$(cat "$SANDBOX/last_rc")
leaked_tmp=$(cat "$SANDBOX/leaked_tmp")

[ "$rc" = "0" ] && ok "exit 0" || ko "exit 0" "rc=$rc"
assert_contains "bootstrap log emitted"        "bootstrapping" "$out"
assert_contains "stub argc=3"                  "STUB argc=3" "$out"
assert_contains "stub argv[1]=hello"           "STUB argv[1]=hello" "$out"
assert_contains "stub argv[2]=world arg"       "STUB argv[2]=world arg" "$out"
assert_contains "stub argv[3]=final"           "STUB argv[3]=final" "$out"

# bin/utoo must no longer be the placeholder (header line is gone).
new_second_line=$(head -2 "$PKG_DIR/bin/utoo" | tail -1)
case "$new_second_line" in
    *bootstrap*) ko "bin/utoo replaced" "second line still references 'bootstrap'" ;;
    *) ok "bin/utoo replaced with real binary" ;;
esac

# tmp leak guard: trap-before-exec means no subdir survives in our scoped TMPDIR.
[ "$leaked_tmp" = "0" ] && ok "no tmp leak after exec" || ko "no tmp leak" "leaked $leaked_tmp dir(s)"

#------------------------------------------------------------------------------
# Test 2: idempotency — second invocation runs the real binary directly,
# no bootstrap message, no re-download.
#------------------------------------------------------------------------------
printf '\n== idempotency ==\n'
out2=$(run_placeholder "$PKG_DIR/bin/utoo" again)
rc2=$(cat "$SANDBOX/last_rc")

[ "$rc2" = "0" ] && ok "exit 0" || ko "exit 0" "rc=$rc2"
assert_contains     "stub argv[1]=again"        "STUB argv[1]=again" "$out2"
assert_not_contains "no re-bootstrap"           "bootstrapping" "$out2"
teardown_sandbox

#------------------------------------------------------------------------------
# Test 3: invocation through an npm-style relative symlink
#------------------------------------------------------------------------------
printf '\n== npm symlink shim ==\n'
setup_sandbox
publish_stub

SHIM_DIR="$SANDBOX/prefix-bin"
mkdir -p "$SHIM_DIR"
# npm uses relative symlinks ($prefix/bin/utoo -> ../lib/node_modules/utoo/bin/utoo);
# mirror that so the readlink + dirname loop in the placeholder is exercised.
(cd "$SHIM_DIR" && ln -s "../utoo/bin/utoo" utoo)

out3=$(run_placeholder "$SHIM_DIR/utoo" via-shim)
rc3=$(cat "$SANDBOX/last_rc")

[ "$rc3" = "0" ] && ok "exit 0" || ko "exit 0" "rc=$rc3"
assert_contains "shim: bootstrap reached"     "bootstrapping" "$out3"
assert_contains "shim: real binary args"      "STUB argv[1]=via-shim" "$out3"
teardown_sandbox

#------------------------------------------------------------------------------
# Test 4: failure recovery — broken registry leaves placeholder untouched,
# fixing the registry on the next invocation succeeds. Encodes the
# anti-regression: a failed bootstrap must not be a permanent dead end.
#------------------------------------------------------------------------------
printf '\n== failure recovery ==\n'
setup_sandbox
# Don't publish_stub yet — registry dir exists but is empty, so curl 404s.
mkdir -p "$REGISTRY_DIR"

placeholder_sha_before=$(cksum "$PKG_DIR/bin/utoo" | awk '{print $1}')
out4=$(run_placeholder "$PKG_DIR/bin/utoo" first-attempt)
rc4=$(cat "$SANDBOX/last_rc")
placeholder_sha_after=$(cksum "$PKG_DIR/bin/utoo" | awk '{print $1}')

[ "$rc4" != "0" ] && ok "first attempt fails non-zero" || ko "first attempt fails" "rc=$rc4 (expected non-zero)"
assert_contains "failure log emitted"            "failed to download" "$out4"
[ "$placeholder_sha_before" = "$placeholder_sha_after" ] \
    && ok "placeholder unchanged after failure" \
    || ko "placeholder unchanged" "checksum changed: $placeholder_sha_before -> $placeholder_sha_after"

# Now publish the stub and retry — must succeed without manual cleanup.
publish_stub
out4b=$(run_placeholder "$PKG_DIR/bin/utoo" second-attempt)
rc4b=$(cat "$SANDBOX/last_rc")

[ "$rc4b" = "0" ] && ok "second attempt succeeds" || ko "second attempt" "rc=$rc4b"
assert_contains "second attempt: real binary"  "STUB argv[1]=second-attempt" "$out4b"
teardown_sandbox

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
