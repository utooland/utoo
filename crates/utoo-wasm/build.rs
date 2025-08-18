fn main() {
    #[cfg(feature = "utoo-pack")]
    {
        use turbo_tasks_build::generate_register;
        generate_register();
    }
}
