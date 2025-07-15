use turbo_tasks_build::{generate_register, rerun_if_glob};

fn main() {
    generate_register();
    rerun_if_glob("tests/snapshot/*/*", "tests/snapshot");
}
