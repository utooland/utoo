# Builder image for the two Linux musl variants of @utoo/pack.
#
# Build from the repository root so rust-toolchain.toml is available:
#   docker build -f scripts/pack-native-builder.Dockerfile -t utoo-pack-musl-builder .
#
# The container itself is glibc-based. The two pinned rust-musl-cross stages
# provide target headers, crt objects, libc, and libgcc without making Cargo or
# Node run under musl.

FROM ghcr.io/rust-cross/rust-musl-cross:x86_64-musl@sha256:bcf6a66615f9d5bae659e38ab4311260e0488d1c34ad0ab9f9147f4cd5ef64ed AS musl_x86_64
FROM ghcr.io/rust-cross/rust-musl-cross:aarch64-musl@sha256:eab6a58ff66eaa33fa87fc31ed11403596719ca3f23aa51626fb993d77c1200b AS musl_aarch64

# The host glibc version does not affect the musl output. Pin the Jammy image
# used by the verified benchmark so a tag release cannot silently change it.
FROM ubuntu:22.04@sha256:3b06811b2afd352be909dd088a004166d665dc76d38b13eada33522a9d915c6f AS builder

ENV DEBIAN_FRONTEND=noninteractive

# clang is the C compiler and Rust linker driver. Rust's bundled rust-lld does
# the final link; llvm-ar/llvm-ranlib are used by native crate build scripts.
RUN apt-get update && apt-get install -y --no-install-recommends \
    binutils \
    build-essential \
    ca-certificates \
    clang \
    curl \
    git \
    libclang-dev \
    lld \
    llvm \
    pkg-config \
    xz-utils \
    && rm -rf /var/lib/apt/lists/*

# Node is a build tool only. The output ABI is selected by Cargo's --target.
ARG NODE_VERSION=22.14.0
RUN case "$(dpkg --print-architecture)" in \
      amd64) \
        NODE_ARCH=x64; \
        NODE_SHA256=69b09dba5c8dcb05c4e4273a4340db1005abeafe3927efda2bc5b249e80437ec \
        ;; \
      arm64) \
        NODE_ARCH=arm64; \
        NODE_SHA256=08bfbf538bad0e8cbb0269f0173cca28d705874a67a22f60b57d99dc99e30050 \
        ;; \
      *) echo "unsupported builder architecture: $(dpkg --print-architecture)" >&2; exit 1 ;; \
    esac && \
    curl -fsSLo /tmp/node.tar.xz \
      "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-${NODE_ARCH}.tar.xz" && \
    echo "${NODE_SHA256}  /tmp/node.tar.xz" | sha256sum --check --strict && \
    tar -xJf /tmp/node.tar.xz -C /usr/local --strip-components=1 \
      --exclude CHANGELOG.md --exclude README.md && \
    rm /tmp/node.tar.xz && \
    node --version && npm --version

# Keep each imported sysroot self-contained. The target-name symlinks match the
# paths expected by clang, while copying crt/libgcc next to musl libc lets the
# clang driver and rust-lld find everything needed for a shared object link.
COPY --from=musl_x86_64 /usr/local/musl /opt/x86_64-linux-musl-cross
COPY --from=musl_aarch64 /usr/local/musl /opt/aarch64-linux-musl-cross
RUN ln -s x86_64-unknown-linux-musl \
      /opt/x86_64-linux-musl-cross/x86_64-linux-musl && \
    ln -s aarch64-unknown-linux-musl \
      /opt/aarch64-linux-musl-cross/aarch64-linux-musl && \
    cp /opt/x86_64-linux-musl-cross/lib/gcc/x86_64-unknown-linux-musl/*/crt*.o \
       /opt/x86_64-linux-musl-cross/lib/gcc/x86_64-unknown-linux-musl/*/libgcc.a \
       /opt/x86_64-linux-musl-cross/x86_64-linux-musl/lib/ && \
    cp /opt/aarch64-linux-musl-cross/lib/gcc/aarch64-unknown-linux-musl/*/crt*.o \
       /opt/aarch64-linux-musl-cross/lib/gcc/aarch64-unknown-linux-musl/*/libgcc.a \
       /opt/aarch64-linux-musl-cross/aarch64-linux-musl/lib/

# Pin Rust to the repository toolchain and bake both target standard libraries
# into the image. A rust-toolchain.toml change invalidates this layer.
COPY rust-toolchain.toml /tmp/rust-toolchain.toml
RUN TOOLCHAIN=$(sed -n 's/^channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' \
      /tmp/rust-toolchain.toml) && \
    test -n "$TOOLCHAIN" && \
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
      sh -s -- -y --default-toolchain "$TOOLCHAIN" --profile minimal && \
    rm /tmp/rust-toolchain.toml

ENV PATH="/root/.cargo/bin:${PATH}"

RUN rustup target add \
      x86_64-unknown-linux-musl \
      aarch64-unknown-linux-musl

# cargo-rustflags resolves repository, target, and command-line Cargo flags
# before the workspace-pinned napi-rs CLI launches Cargo.
RUN cargo install --locked cargo-rustflags --version 0.4.0 && \
    cargo rustflags --help >/dev/null && \
    rustc --version

WORKDIR /build
