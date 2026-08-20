#!/usr/bin/env bash
# Build one @utoo/pack Linux musl N-API binary inside pack-native-builder.
#
# Required:
#   TARGET=x86_64-unknown-linux-musl | aarch64-unknown-linux-musl
# Optional:
#   REPO_ROOT=/build

set -euo pipefail

TARGET="${TARGET:?TARGET must be x86_64-unknown-linux-musl or aarch64-unknown-linux-musl}"
REPO_ROOT="${REPO_ROOT:-/build}"

case "$TARGET" in
  x86_64-unknown-linux-musl)
    CLANG_TARGET="x86_64-linux-musl"
    CROSS_ROOT="/opt/x86_64-linux-musl-cross"
    OUTPUT="packages/pack/src/pack.linux-x64-musl.node"
    EXPECTED_MACHINE="Advanced Micro Devices X86-64"
    ;;
  aarch64-unknown-linux-musl)
    CLANG_TARGET="aarch64-linux-musl"
    CROSS_ROOT="/opt/aarch64-linux-musl-cross"
    OUTPUT="packages/pack/src/pack.linux-arm64-musl.node"
    EXPECTED_MACHINE="AArch64"
    ;;
  *)
    echo "unsupported TARGET: $TARGET" >&2
    exit 2
    ;;
esac

TARGET_SYSROOT="$CROSS_ROOT/$CLANG_TARGET"
if [[ ! -d "$REPO_ROOT/.git" && ! -f "$REPO_ROOT/.git" ]]; then
  echo "REPO_ROOT is not a Git worktree: $REPO_ROOT" >&2
  exit 2
fi
if [[ ! -d "$TARGET_SYSROOT" ]]; then
  echo "musl sysroot is missing: $TARGET_SYSROOT" >&2
  exit 2
fi

cd "$REPO_ROOT"
git config --global --add safe.directory "$REPO_ROOT"
export CI="${CI:-true}"
export CARGO_INCREMENTAL=0
export CARGO_TERM_COLOR=always

# Use clang as the linker driver so target/sysroot/gcc-toolchain paths resolve
# crt objects, musl libc, and libgcc. gnu-lld-cc selects Rust's bundled lld.
# Cargo target rustflags take precedence over [build].rustflags instead of
# merging with them, so this list intentionally includes all release flags.
CROSS_FLAGS=(
  --cfg
  tokio_unstable
  -Zshare-generics=y
  -Zthreads=8
  -Zunstable-options
  -Csymbol-mangling-version=v0
  -Clinker=clang
  -Clinker-flavor=gnu-lld-cc
  -Clink-arg=-Wl,--icf=all
  -Clink-arg="--target=$CLANG_TARGET"
  -Clink-arg="--sysroot=$TARGET_SYSROOT"
  -Clink-arg="--gcc-toolchain=$CROSS_ROOT"
  -C
  target-feature=-crt-static
)

CONFIG_FLAGS=""
for FLAG in "${CROSS_FLAGS[@]}"; do
  if [[ -n "$CONFIG_FLAGS" ]]; then
    CONFIG_FLAGS+=", "
  fi
  CONFIG_FLAGS+="\"$FLAG\""
done
CROSS_CONFIG="target.${TARGET}.rustflags=[$CONFIG_FLAGS]"

# Ask Cargo to resolve the effective target flags. napi-rs receives the result
# through RUSTFLAGS and may safely append its own N-API cdylib flags.
RUSTFLAGS="$(cargo rustflags --target "$TARGET" --config "$CROSS_CONFIG")"
export RUSTFLAGS

for REQUIRED_FLAG in \
  tokio_unstable \
  -Zshare-generics=y \
  -Zthreads=8 \
  -Csymbol-mangling-version=v0 \
  target-feature=-crt-static \
  -Clinker=clang \
  -Clinker-flavor=gnu-lld-cc
do
  if [[ " $RUSTFLAGS " != *" $REQUIRED_FLAG "* ]]; then
    echo "resolved RUSTFLAGS is missing: $REQUIRED_FLAG" >&2
    exit 2
  fi
done

# rustc's gcc-ld directory contains rust-lld but not the `ld` name clang looks
# up after rustc selects the gnu-lld-cc flavor.
RUST_SYSROOT="$(rustc --print sysroot)"
GCC_LD_DIR="$RUST_SYSROOT/lib/rustlib/$TARGET/bin/gcc-ld"
if [[ -d "$GCC_LD_DIR" && ! -e "$GCC_LD_DIR/ld" ]]; then
  ln -sf ../rust-lld "$GCC_LD_DIR/ld"
fi

# Native build scripts (mimalloc, ring, psm, and zstd) must emit target musl
# objects, not objects for the glibc builder host. cc-rs recognizes the
# lower-case, underscore-normalized target suffix used here.
TARGET_ENV="${TARGET//-/_}"
TARGET_ENV_UPPER="${TARGET_ENV^^}"
TARGET_CFLAGS="--target=$CLANG_TARGET --sysroot=$TARGET_SYSROOT --gcc-toolchain=$CROSS_ROOT"
export "CARGO_TARGET_${TARGET_ENV_UPPER}_LINKER=clang"
export "CC_${TARGET_ENV}=clang"
export "CXX_${TARGET_ENV}=clang++"
export "AR_${TARGET_ENV}=llvm-ar"
export "RANLIB_${TARGET_ENV}=llvm-ranlib"
export "CFLAGS_${TARGET_ENV}=$TARGET_CFLAGS"
export "CXXFLAGS_${TARGET_ENV}=$TARGET_CFLAGS"

node --version
rustc --version
echo "Building @utoo/pack for $TARGET"
echo "RUSTFLAGS=$RUSTFLAGS"

rustup target add "$TARGET"
npm run build:binding --workspace=@utoo/pack -- --target "$TARGET"

if [[ ! -f "$OUTPUT" ]]; then
  echo "expected N-API binary was not produced: $OUTPUT" >&2
  exit 1
fi

echo "Built $OUTPUT"
ACTUAL_CLASS="$(readelf -h "$OUTPUT" | awk -F: '/Class:/ { sub(/^[[:space:]]+/, "", $2); print $2 }')"
ACTUAL_MACHINE="$(readelf -h "$OUTPUT" | awk -F: '/Machine:/ { sub(/^[[:space:]]+/, "", $2); print $2 }')"
if [[ "$ACTUAL_CLASS" != "ELF64" || "$ACTUAL_MACHINE" != "$EXPECTED_MACHINE" ]]; then
  echo "unexpected ELF target: class=$ACTUAL_CLASS machine=$ACTUAL_MACHINE" >&2
  exit 1
fi
echo "ELF class: $ACTUAL_CLASS"
echo "ELF machine: $ACTUAL_MACHINE"
readelf -d "$OUTPUT" | grep NEEDED
if readelf -d "$OUTPUT" | grep -q 'Shared library: \[libc\.so\.6\]'; then
  echo "musl artifact unexpectedly links glibc: $OUTPUT" >&2
  exit 1
fi
