//! Black-box CLI transcript tests for stable `utoo` command surface.
//!
//! Keep filesystem/network-heavy behavior in subprocess integration tests; the
//! `.trycmd` layer should stay focused on args, exit codes, and durable console
//! contracts.

#[test]
fn cli_snapshots() {
    let home = tempfile::tempdir().expect("temp home");
    trycmd::TestCases::new()
        .case("tests/cmd/*.trycmd")
        .env("CI", "true")
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "off")
        .env("HOME", home.path().display().to_string())
        .env("APPDATA", home.path().join("appdata").display().to_string())
        .env(
            "XDG_CONFIG_HOME",
            home.path().join("config").display().to_string(),
        )
        .env(
            "XDG_DATA_HOME",
            home.path().join("data").display().to_string(),
        );
}
