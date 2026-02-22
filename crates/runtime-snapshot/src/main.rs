use deno_core::JsRuntimeForSnapshot;

fn main() {
    eprintln!("Creating JsRuntimeForSnapshot...");
    let runtime = JsRuntimeForSnapshot::new(deno_core::RuntimeOptions {
        extensions: vec![utoo_runtime::runtime::create_ext()],
        ..Default::default()
    });

    eprintln!("Taking snapshot...");
    let snapshot = runtime.snapshot();

    let out_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../runtime/src/snapshot_data.bin");

    eprintln!("Writing {} bytes...", snapshot.len());
    std::fs::write(&out_path, &snapshot).unwrap();

    println!(
        "Snapshot written to {} ({:.1} KB)",
        out_path.display(),
        snapshot.len() as f64 / 1024.0
    );
}
