use utoo_pm::{InitializeOptions, commands, types};

/// Keep the embeddable surface wired to every business command dispatched by
/// the native CLI. Merely naming each function from an external-crate test
/// catches accidental visibility regressions without performing side effects.
#[test]
fn exposes_native_command_surface() {
    let _ = utoo_pm::initialize;
    let _ = InitializeOptions::default();
    let _ = utoo_pm::command();
    assert!(!utoo_pm::VERSION.is_empty());

    let _ = commands::clean::run;
    let _ = commands::install::run;
    let _ = commands::install::project;
    let _ = commands::install::current_project;
    let _ = commands::install::global;
    let _ = commands::uninstall::run;
    let _ = commands::rebuild::run;
    let _ = commands::deps::run;
    let _ = commands::update::run;
    let _ = commands::list::run;
    let _ = commands::execute::run;
    let _ = commands::run::run;
    let _ = commands::view::run;
    let _ = commands::link::run;
    let _ = commands::init::run;
    let _ = commands::pack::run;
    let _ = commands::publish::run;
    let _ = commands::ping::run;
    let _ = commands::login::run;
    let _ = commands::whoami::run;
    let _ = commands::logout::run;
    let _ = commands::config::run;
    let _ = commands::completions::generate;

    let _: Option<types::ScriptPolicy> = None;
    let _: Option<types::WorkspaceFilter> = None;
    let _: Option<types::ConfigCommands> = None;
}
