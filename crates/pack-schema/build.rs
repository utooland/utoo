fn main() {
    // This is a workaround for Cargo workspace feature unification.
    // When building the entire workspace, `turbopack-node`'s `worker_pool` feature
    // is enabled (e.g., by `pack-napi`), which pulls in Node.js N-API symbols.
    // Since `pack-cli` is a standalone binary and not a Node.js addon, these symbols
    // are missing at link time. This flag tells the linker to resolve them at
    // runtime instead of failing the build.
    println!("cargo:rustc-link-arg=-Wl,-undefined,dynamic_lookup");
}
