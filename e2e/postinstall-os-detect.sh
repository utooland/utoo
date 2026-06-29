#!/bin/sh
# E2E test for vendor/templates/postinstall.sh.template OS detection.
#
# Runs the template under a sandboxed shell with `uname` and env vars
# (OSTYPE, PROCESSOR_ARCHITECTURE) mocked, then asserts the computed
# (OS, ARCH) tuple. Side-effecting commands (cp, mkdir, chmod, npm, rm)
# are stubbed so the script runs fast and offline.
#
# Regression target: GitHub `windows-latest` rolling from Server 2022
# (`MINGW64_NT-10.0-20348`) to Server 2025 (`MINGW64_NT-10.0-26100`)
# previously produced `@utoo/utoo-mingw64_nt-10.0-26100-x64` (404 on
# npm). All Windows kernel slugs must collapse to OS=win32.
#
# Note: the `utoo` package's postinstall is now Node
# (vendor/templates/postinstall.utoo.js.template), which reads
# process.platform / process.arch and so is immune to this whole class of
# uname-drift bug. Its self-heal + Windows prefix paths are covered by
# e2e/placeholder-self-heal.sh and e2e/utoo-pm.ps1.

set -u

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TEMPLATE_PM="$REPO_ROOT/vendor/templates/postinstall.sh.template"

PASS=0
FAIL=0

run_detection() {
    template=$1
    mock_uname_s=$2
    mock_uname_m=$3
    mock_ostype=$4
    mock_proc_arch=$5

    tmpl=$(sed 's|{{name}}|utoo|g' "$template")

    MOCK_UNAME_S="$mock_uname_s" \
    MOCK_UNAME_M="$mock_uname_m" \
    OSTYPE="$mock_ostype" \
    PROCESSOR_ARCHITECTURE="$mock_proc_arch" \
    sh 2>/dev/null <<SH_EOF
uname() {
    if [ "\$#" -eq 0 ]; then printf '%s\n' "\$MOCK_UNAME_S"; return 0; fi
    case "\$1" in
        -s) printf '%s\n' "\$MOCK_UNAME_S" ;;
        -m) printf '%s\n' "\$MOCK_UNAME_M" ;;
        *) printf '%s\n' "\$MOCK_UNAME_S" ;;
    esac
}
# Neutralize side effects so the install path can fail fast and the
# trap fires with OS/ARCH already populated.
mkdir() { :; }
cp() { :; }
chmod() { :; }
rm() { :; }
npm() { return 1; }
find_node_modules() { return 1; }
trap 'printf "RESULT OS=%s ARCH=%s\n" "\${OS:-}" "\${ARCH:-}"' EXIT

$tmpl
SH_EOF
}

assert_detection() {
    label=$1
    template=$2
    mock_uname_s=$3
    mock_uname_m=$4
    mock_ostype=$5
    mock_proc_arch=$6
    expected_os=$7
    expected_arch=$8

    out=$(run_detection "$template" "$mock_uname_s" "$mock_uname_m" "$mock_ostype" "$mock_proc_arch")
    actual=$(printf '%s\n' "$out" | grep '^RESULT ' | tail -1)
    expected="RESULT OS=$expected_os ARCH=$expected_arch"

    if [ "$actual" = "$expected" ]; then
        printf '  PASS  %s\n' "$label"
        PASS=$((PASS + 1))
    else
        printf '  FAIL  %s\n        expected: %s\n        actual:   %s\n' \
            "$label" "$expected" "$actual"
        FAIL=$((FAIL + 1))
    fi
}

for template in "$TEMPLATE_PM"; do
    printf '\n== %s ==\n' "$(basename "$template")"

    # Unix
    assert_detection "linux x86_64"  "$template" "Linux"  "x86_64"  "linux-gnu" "" "linux"  "x64"
    assert_detection "linux aarch64" "$template" "Linux"  "aarch64" "linux-gnu" "" "linux"  "arm64"
    assert_detection "darwin x86_64" "$template" "Darwin" "x86_64"  "darwin22"  "" "darwin" "x64"
    assert_detection "darwin arm64"  "$template" "Darwin" "arm64"   "darwin22"  "" "darwin" "arm64"

    # Windows: kernel-version drift must collapse to OS=win32.
    # OSTYPE is empty (PowerShell-spawned sh) so the legacy $OSTYPE
    # branch can no longer rescue us — uname-prefix matching must.
    assert_detection "win32 mingw64 server2022" "$template" \
        "MINGW64_NT-10.0-20348" "x86_64" "" "AMD64" "win32" "x64"
    assert_detection "win32 mingw64 server2025" "$template" \
        "MINGW64_NT-10.0-26100" "x86_64" "" "AMD64" "win32" "x64"
    assert_detection "win32 cygwin" "$template" \
        "CYGWIN_NT-10.0" "x86_64" "" "AMD64" "win32" "x64"
    assert_detection "win32 msys" "$template" \
        "MSYS_NT-10.0" "x86_64" "" "AMD64" "win32" "x64"
    assert_detection "win32 arm64" "$template" \
        "MINGW64_NT-10.0-26100" "x86_64" "" "ARM64" "win32" "arm64"

    # Legacy: OSTYPE=msys still works when uname is unhelpful.
    assert_detection "win32 OSTYPE=msys fallback" "$template" \
        "" "x86_64" "msys" "AMD64" "win32" "x64"

    # uname empty + non-Windows OSTYPE → "unknown" fallback rather
    # than a malformed `@utoo/utoo--x64` package name.
    assert_detection "unknown OS fallback" "$template" \
        "" "x86_64" "" "" "unknown" "x64"
done

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
