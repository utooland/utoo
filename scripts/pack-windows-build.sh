#!/usr/bin/env bash
# Cross-compile the @utoo/pack x86_64 Windows N-API binary on Linux.

set -euo pipefail

TARGET="${TARGET:-x86_64-pc-windows-msvc}"
REPO_ROOT="${REPO_ROOT:-/build}"
OUTPUT="packages/pack/src/pack.win32-x64-msvc.node"

if [[ "$TARGET" != "x86_64-pc-windows-msvc" ]]; then
  echo "unsupported TARGET: $TARGET" >&2
  exit 2
fi
if [[ ! -d "$REPO_ROOT/.git" && ! -f "$REPO_ROOT/.git" ]]; then
  echo "REPO_ROOT is not a Git worktree: $REPO_ROOT" >&2
  exit 2
fi

cd "$REPO_ROOT"
git config --global --add safe.directory "$REPO_ROOT"

export CI="${CI:-true}"
export CARGO_INCREMENTAL=0
export CARGO_TERM_COLOR=always
export XWIN_ARCH=x86_64
export XWIN_VERSION=17
export XWIN_SDK_VERSION=10.0.26100
export XWIN_CRT_VERSION=14.44.17.14

# napi-rs detects a Linux-to-Windows build and invokes cargo-xwin. cargo-xwin
# resolves the repository's target rustflags and adds its SDK, CRT and lld-link
# arguments without losing tokio_unstable or +crt-static.
rm -f "$OUTPUT"

node --version
rustc --version
cargo-xwin --version
echo "Cross-compiling @utoo/pack for $TARGET"

npm run build:binding --workspace=@utoo/pack -- --target "$TARGET"

if [[ ! -s "$OUTPUT" ]]; then
  echo "expected N-API binary was not produced: $OUTPUT" >&2
  exit 1
fi

# Reject a wrong-architecture or malformed output before it reaches the npm
# artifact. The release workflow then loads it on a Windows runner.
PE_HEADER="$(llvm-readobj --file-header --coff-imports "$OUTPUT")"
echo "$PE_HEADER"
if ! grep -Eq 'Format: COFF-x86-64|Format:.*x86-64' <<<"$PE_HEADER"; then
  echo "unexpected PE/COFF format: $OUTPUT" >&2
  exit 1
fi
if ! grep -Eq 'Machine: IMAGE_FILE_MACHINE_AMD64 \(0x8664\)|Machine: AMD64' \
  <<<"$PE_HEADER"; then
  echo "unexpected PE/COFF machine: $OUTPUT" >&2
  exit 1
fi

echo "Built $OUTPUT"
