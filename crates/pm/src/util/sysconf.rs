/// One-time system tuning: fd limits, thread pools, etc.
pub fn init() {
    #[cfg(unix)]
    raise_fd_limit();

    // Ensure rayon threads have sufficient stack for libdeflater + tar +
    // deep recursive directory walks (e.g. ant-design-x node_modules) +
    // rayon work-stealing.  Windows defaults to 1 MB; Linux CI runners may
    // also have small defaults.
    rayon::ThreadPoolBuilder::new()
        .stack_size(8 * 1024 * 1024)
        .build_global()
        .ok();
}

/// Raise the open-file soft limit to the hard limit.
///
/// macOS ships with a default soft limit of 256, which is easily exhausted
/// during parallel tarball extraction and hardlinking. Both pnpm and bun
/// perform the same adjustment at startup.
#[cfg(unix)]
fn raise_fd_limit() {
    unsafe {
        let mut rlim = std::mem::MaybeUninit::<libc::rlimit>::zeroed().assume_init();
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut rlim) == 0 && rlim.rlim_cur < rlim.rlim_max {
            rlim.rlim_cur = rlim.rlim_max;
            libc::setrlimit(libc::RLIMIT_NOFILE, &rlim);
        }
    }
}
