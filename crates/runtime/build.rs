fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();

    match target_os.as_str() {
        "macos" => {
            let path = format!("{manifest_dir}/napi_symbols_macos.def");
            println!("cargo:rustc-link-arg-bin=utoo-runtime=-Wl,-exported_symbols_list,{path}");
        }
        "linux" => {
            let path = format!("{manifest_dir}/napi_symbols_linux.def");
            println!("cargo:rustc-link-arg-bin=utoo-runtime=-Wl,--export-dynamic-symbol-list={path}");
        }
        _ => {}
    }
}
