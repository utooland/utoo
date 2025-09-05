fn main() {
    // Check if the utoo-pack feature is enabled via environment variable
    if std::env::var("CARGO_FEATURE_UTOO_PACK").is_ok() {
        use turbo_tasks_build::generate_register;
        generate_register();
    }
}
