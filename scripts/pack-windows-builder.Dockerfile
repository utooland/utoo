# Builder image for the x86_64 Windows MSVC variant of @utoo/pack.
#
# Build from the repository root so rust-toolchain.toml is available:
#   docker build -f scripts/pack-windows-builder.Dockerfile \
#     -t utoo-pack-windows-builder .
#
# Cargo and Node run natively on Linux. cargo-xwin supplies the Windows SDK
# and MSVC CRT while clang-cl and Rust's bundled lld-link produce the PE/COFF
# N-API binary.

# The host image does not affect the Windows ABI. Pin the image used by this
# experiment so a release cannot silently switch the build environment.
FROM ubuntu:22.04@sha256:3b06811b2afd352be909dd088a004166d665dc76d38b13eada33522a9d915c6f AS builder

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
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

# Node is a build tool only. Verify the official archive before installing it.
ARG NODE_VERSION=22.14.0
ARG NODE_SHA256=69b09dba5c8dcb05c4e4273a4340db1005abeafe3927efda2bc5b249e80437ec
RUN curl -fsSLo /tmp/node.tar.xz \
      "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-x64.tar.xz" && \
    echo "${NODE_SHA256}  /tmp/node.tar.xz" | sha256sum --check --strict && \
    tar -xJf /tmp/node.tar.xz -C /usr/local --strip-components=1 \
      --exclude CHANGELOG.md --exclude README.md && \
    rm /tmp/node.tar.xz && \
    node --version && npm --version

# Match the repository's pinned Rust nightly and bake the Windows stdlib into
# the image. A rust-toolchain.toml update invalidates this layer.
COPY rust-toolchain.toml /tmp/rust-toolchain.toml
RUN TOOLCHAIN=$(sed -n 's/^channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' \
      /tmp/rust-toolchain.toml) && \
    test -n "$TOOLCHAIN" && \
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
      sh -s -- -y --default-toolchain "$TOOLCHAIN" --profile minimal && \
    rm /tmp/rust-toolchain.toml

ENV PATH="/root/.cargo/bin:${PATH}"

RUN rustup target add x86_64-pc-windows-msvc

# Use the prebuilt cargo-xwin binary from the version evaluated by Next.js in
# vercel/next.js#92594. The checksum is published with that release.
ARG CARGO_XWIN_VERSION=0.21.5
ARG CARGO_XWIN_SHA256=42928296cfaaede33b85cc1b96c0db424070f32dfbd5c0a2abbd6d0d64f05334
RUN curl --retry 5 --retry-all-errors -fsSLo /tmp/cargo-xwin.tar.gz \
      "https://github.com/rust-cross/cargo-xwin/releases/download/v${CARGO_XWIN_VERSION}/cargo-xwin-v${CARGO_XWIN_VERSION}.x86_64-unknown-linux-musl.tar.gz" && \
    echo "${CARGO_XWIN_SHA256}  /tmp/cargo-xwin.tar.gz" | sha256sum --check --strict && \
    tar -xzf /tmp/cargo-xwin.tar.gz -C /root/.cargo/bin && \
    rm /tmp/cargo-xwin.tar.gz && \
    cargo-xwin --version

# Ubuntu's LLD is too old for the MSVC loadcfg object used by cargo-xwin.
# Rust's bundled rust-lld supports /guard:ehcont and selects the COFF flavor
# from the lld-link executable name. cargo-xwin expects these LLVM tool names.
RUN RUST_SYSROOT=$(rustc --print sysroot) && \
    RUST_HOST=$(rustc -vV | sed -n 's/^host: //p') && \
    ln -sf "$RUST_SYSROOT/lib/rustlib/$RUST_HOST/bin/rust-lld" \
      /usr/local/bin/lld-link && \
    ln -sf "$(command -v llvm-ar)" /usr/local/bin/llvm-lib && \
    ln -sf "$(command -v clang)" /usr/local/bin/clang-cl && \
    lld-link --version && clang-cl --version

# Restrict the downloaded SDK/CRT payload to the only target Utoo publishes.
# Pin the versions resolved from Microsoft's VS 17.14 release manifest so a
# later manifest update cannot silently select a different toolchain.
ENV XWIN_ARCH=x86_64 \
    XWIN_VERSION=17 \
    XWIN_SDK_VERSION=10.0.26100 \
    XWIN_CRT_VERSION=14.44.17.14
RUN cargo xwin cache xwin

WORKDIR /build
