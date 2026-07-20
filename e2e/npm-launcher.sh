#!/bin/sh
# Offline E2E for the immutable npm launcher distribution.
#
# Stages the exact launcher templates below a fake `utoo` package and a nested
# optional platform package. The native artifact is a small executable stub.
# This verifies module-based artifact resolution, argument/stdio/exit-code
# forwarding, `utx` delegation, actionable missing-artifact errors, and the
# absence of install-time or first-run mutation.

set -u

case "$(uname -s 2>/dev/null || echo unknown)" in
    MINGW*|MSYS*|CYGWIN*)
        printf 'skipping POSIX npm launcher e2e on Windows\n'
        exit 0
        ;;
esac

if ! command -v node >/dev/null 2>&1; then
    printf 'node is required for the npm launcher e2e\n' >&2
    exit 1
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TEMPLATES="$REPO_ROOT/vendor/templates"
SANDBOX=$(mktemp -d)
trap 'rm -rf "$SANDBOX"' EXIT

case "$(uname -s)" in
    Darwin) OS=darwin ;;
    Linux) OS=linux ;;
    *)
        printf 'unsupported OS for this test\n' >&2
        exit 1
        ;;
esac
case "$(uname -m)" in
    x86_64|amd64) ARCH=x64 ;;
    aarch64|arm64) ARCH=arm64 ;;
    *)
        printf 'unsupported architecture for this test\n' >&2
        exit 1
        ;;
esac

PKG_DIR="$SANDBOX/node_modules/utoo"
PLATFORM_NAME="utoo-$OS-$ARCH"
PLATFORM_DIR="$PKG_DIR/node_modules/@utoo/$PLATFORM_NAME"
mkdir -p "$PKG_DIR/bin" "$PLATFORM_DIR/bin"
PKG_DIR_REAL=$(cd "$PKG_DIR" && pwd -P)

cp "$TEMPLATES/launcher.utoo.js.template" "$PKG_DIR/bin/launcher.js"
cp "$TEMPLATES/utoo.utoo.js.template" "$PKG_DIR/bin/utoo.js"
cp "$TEMPLATES/utx.utoo.js.template" "$PKG_DIR/bin/utx.js"
chmod +x "$PKG_DIR/bin/utoo.js" "$PKG_DIR/bin/utx.js"

cat > "$PKG_DIR/package.json" <<'JSON'
{
  "name": "utoo",
  "version": "9.9.9-e2e"
}
JSON
cat > "$PLATFORM_DIR/package.json" <<JSON
{
  "name": "@utoo/$PLATFORM_NAME",
  "version": "9.9.9-e2e",
  "os": ["$OS"],
  "cpu": ["$ARCH"]
}
JSON
cat > "$PLATFORM_DIR/bin/utoo" <<'STUB'
#!/bin/sh
if [ "${1:-}" = "exit-23" ]; then
    exit 23
fi
if [ "${1:-}" = "wait-signal" ]; then
    printf 'READY\n'
    while :; do sleep 1; done
fi
printf 'ROOT=%s\n' "${UTOO_MANAGED_PACKAGE_ROOT:-}"
printf 'ARGC=%s\n' "$#"
i=1
for arg in "$@"; do
    printf 'ARGV[%s]=%s\n' "$i" "$arg"
    i=$((i + 1))
done
STUB
chmod +x "$PLATFORM_DIR/bin/utoo"

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

printf '\n== platform resolution and immutable execution ==\n'
before=$(find "$PKG_DIR" -type f -exec cksum {} \; | sort | cksum)
out=$(node "$PKG_DIR/bin/utoo.js" hello "world arg" "你好" 2>&1)
rc=$?
after=$(find "$PKG_DIR" -type f -exec cksum {} \; | sort | cksum)

[ "$rc" = "0" ] && ok "launcher exits zero" || ko "launcher exits zero" "rc=$rc"
assert_contains "managed package root propagated" "ROOT=$PKG_DIR_REAL" "$out"
assert_contains "argument count preserved" "ARGC=3" "$out"
assert_contains "space-containing argument preserved" "ARGV[2]=world arg" "$out"
assert_contains "Unicode argument preserved" "ARGV[3]=你好" "$out"
[ "$before" = "$after" ] && ok "launcher does not mutate installed files" \
    || ko "launcher does not mutate installed files" "package checksum changed"

printf '\n== utx delegation ==\n'
out=$(node "$PKG_DIR/bin/utx.js" create-demo "two words" 2>&1)
assert_contains "utx prepends x" "ARGV[1]=x" "$out"
assert_contains "utx preserves user args" "ARGV[3]=two words" "$out"

printf '\n== exit status ==\n'
node "$PKG_DIR/bin/utoo.js" exit-23 >/dev/null 2>&1
rc=$?
[ "$rc" = "23" ] && ok "native exit status preserved" \
    || ko "native exit status preserved" "rc=$rc"

printf '\n== termination signal ==\n'
signal_out="$SANDBOX/signal.out"
node "$PKG_DIR/bin/utoo.js" wait-signal >"$signal_out" 2>&1 &
launcher_pid=$!
ready=0
i=0
while [ "$i" -lt 50 ]; do
    if grep -q READY "$signal_out" 2>/dev/null; then
        ready=1
        break
    fi
    sleep 0.1
    i=$((i + 1))
done
if [ "$ready" = "1" ]; then
    kill -TERM "$launcher_pid"
    wait "$launcher_pid" 2>/dev/null
    rc=$?
    [ "$rc" = "143" ] && ok "SIGTERM forwarded with shell exit semantics" \
        || ko "SIGTERM forwarded with shell exit semantics" "rc=$rc"
else
    kill -KILL "$launcher_pid" 2>/dev/null || true
    wait "$launcher_pid" 2>/dev/null
    ko "native process became ready" "READY marker not observed"
fi

printf '\n== target table ==\n'
node - "$PKG_DIR/bin/launcher.js" "$SANDBOX" <<'NODE'
const fs = require("fs");
const path = require("path");
const launcher = require(process.argv[2]);
const sandbox = process.argv[3];

const windows = launcher.resolveTarget("win32", "arm64");
if (windows.packageName !== "@utoo/utoo-win32-x64") process.exit(11);
if (windows.executable !== "bin/utoo.exe") process.exit(12);

const fakeRoot = path.join(sandbox, "windows-platform");
fs.mkdirSync(path.join(fakeRoot, "bin"), { recursive: true });
fs.writeFileSync(path.join(fakeRoot, "package.json"), "{}");
fs.writeFileSync(path.join(fakeRoot, "bin", "utoo.exe"), "PE fixture");
const binary = launcher.findBinary(
  windows,
  "win32",
  "arm64",
  () => path.join(fakeRoot, "package.json"),
);
if (binary !== path.join(fakeRoot, "bin", "utoo.exe")) process.exit(13);

try {
  launcher.resolveTarget("freebsd", "x64");
  process.exit(14);
} catch (error) {
  if (!error.message.includes("unsupported platform: freebsd-x64")) process.exit(15);
}
NODE
rc=$?
[ "$rc" = "0" ] && ok "Windows ARM64 fallback and unsupported targets" \
    || ko "target table" "node fixture exited $rc"

printf '\n== spawn failure ==\n'
out=$(node - "$PKG_DIR/bin/launcher.js" "$PLATFORM_DIR/package.json" <<'NODE' 2>&1
const { EventEmitter } = require("events");
const launcher = require(process.argv[2]);
const packageJson = process.argv[3];

function failedSpawn() {
  const child = new EventEmitter();
  child.killed = false;
  child.kill = () => {};
  process.nextTick(() => child.emit("error", new Error("EACCES fixture")));
  return child;
}

launcher.run([], {
  resolvePackage: () => packageJson,
  spawnImpl: failedSpawn,
}).then(
  () => process.exit(21),
  (error) => {
    console.error(error.message);
    console.error(error.cause?.message || "");
  },
);
NODE
)
rc=$?
[ "$rc" = "0" ] && ok "spawn failure rejects cleanly" \
    || ko "spawn failure rejects cleanly" "rc=$rc"
assert_contains "spawn error names binary" "failed to start native binary:" "$out"
assert_contains "spawn error names package" "@utoo/$PLATFORM_NAME@9.9.9-e2e" "$out"
assert_contains "spawn error recommends reinstall" "without omitting optional dependencies" "$out"
assert_contains "spawn error retains cause" "EACCES fixture" "$out"

printf '\n== missing optional package ==\n'
mv "$PLATFORM_DIR" "$SANDBOX/platform-away"
out=$(node "$PKG_DIR/bin/utoo.js" --version 2>&1)
rc=$?
[ "$rc" != "0" ] && ok "missing package fails non-zero" \
    || ko "missing package fails non-zero" "rc=$rc"
assert_contains "error names expected package" "@utoo/$PLATFORM_NAME@9.9.9-e2e" "$out"
assert_contains "error recommends reinstall" "without omitting optional dependencies" "$out"
[ ! -e "$PLATFORM_DIR" ] && ok "missing package is not downloaded or recreated" \
    || ko "missing package is not downloaded or recreated" "unexpected $PLATFORM_DIR"

printf '\n== npm pack and install --ignore-scripts ==\n'
NPM_CASE="$SANDBOX/npm-case"
NPM_PLATFORM="$NPM_CASE/platform"
NPM_MAIN="$NPM_CASE/main"
NPM_PREFIX="$NPM_CASE/prefix"
mkdir -p "$NPM_PLATFORM/bin" "$NPM_MAIN/bin" "$NPM_PREFIX"
cp "$SANDBOX/platform-away/bin/utoo" "$NPM_PLATFORM/bin/utoo"
chmod +x "$NPM_PLATFORM/bin/utoo"
cat > "$NPM_PLATFORM/package.json" <<JSON
{
  "name": "@utoo/$PLATFORM_NAME",
  "version": "9.9.9-e2e",
  "os": ["$OS"],
  "cpu": ["$ARCH"],
  "preferUnplugged": true
}
JSON
platform_tgz=$(cd "$NPM_PLATFORM" && npm pack --silent)
platform_tgz="$NPM_PLATFORM/$platform_tgz"

cp "$TEMPLATES/launcher.utoo.js.template" "$NPM_MAIN/bin/launcher.js"
cp "$TEMPLATES/utoo.utoo.js.template" "$NPM_MAIN/bin/utoo.js"
cp "$TEMPLATES/utx.utoo.js.template" "$NPM_MAIN/bin/utx.js"
chmod +x "$NPM_MAIN/bin/utoo.js" "$NPM_MAIN/bin/utx.js"
cat > "$NPM_MAIN/package.json" <<JSON
{
  "name": "utoo",
  "version": "9.9.9-e2e",
  "bin": {
    "utoo": "bin/utoo.js",
    "ut": "bin/utoo.js",
    "utx": "bin/utx.js"
  },
  "optionalDependencies": {
    "@utoo/$PLATFORM_NAME": "file:$platform_tgz"
  }
}
JSON
main_tgz=$(cd "$NPM_MAIN" && npm pack --silent)
main_tgz="$NPM_MAIN/$main_tgz"
npm install --global "$main_tgz" --prefix "$NPM_PREFIX" --ignore-scripts \
    --no-audit --no-fund >/dev/null 2>&1
rc=$?
[ "$rc" = "0" ] && ok "npm installs without lifecycle scripts" \
    || ko "npm installs without lifecycle scripts" "rc=$rc"

npm_out=$("$NPM_PREFIX/bin/utoo" npm-shim 2>&1)
rc=$?
[ "$rc" = "0" ] && ok "npm-generated shim runs native artifact" \
    || ko "npm-generated shim runs native artifact" "rc=$rc"
assert_contains "npm shim preserves arguments" "ARGV[1]=npm-shim" "$npm_out"
if node -e 'const p=require(process.argv[1]); process.exit(p.scripts ? 1 : 0)' \
    "$NPM_PREFIX/lib/node_modules/utoo/package.json"; then
    ok "published main package has no lifecycle scripts"
else
    ko "published main package has no lifecycle scripts" "unexpected scripts field"
fi

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
