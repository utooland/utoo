/// One-time system tuning: fd limits, thread pools, etc.
pub fn init() {
    #[cfg(unix)]
    {
        raise_fd_limit();
        ignore_sigpipe();
    }

    // Windows default thread stack is 1MB, insufficient for libdeflater + tar
    // + rayon work-stealing.
    #[cfg(target_os = "windows")]
    rayon::ThreadPoolBuilder::new()
        .stack_size(8 * 1024 * 1024)
        .build_global()
        .ok();
}

/// Ignore SIGPIPE so a broken downstream pipe never terminates the process.
///
/// Filters like ripgrep and fd reset SIGPIPE to `SIG_DFL` so they exit
/// quietly when their consumer goes away (`rg foo | head`). A package
/// manager is different: it performs stateful, side-effecting work
/// (writing `node_modules/`, lockfiles, the store) and must NOT be killed
/// just because something closed its stdout/stderr — e.g. `utoo install |
/// head`, a CI log collector closing the read end, or a supervising
/// process that stops reading. Getting signalled mid-install can leave a
/// half-written tree.
///
/// With `SIG_IGN`, a write to a broken pipe instead returns `EPIPE`
/// (surfaced as `io::ErrorKind::BrokenPipe`), which the I/O layers
/// (`tracing`, `indicatif`) absorb, so the install keeps running. This is
/// also the Rust runtime's own startup default; we set it explicitly to
/// document the intent and to be robust against that default changing.
#[cfg(unix)]
fn ignore_sigpipe() {
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
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

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;

    #[test]
    fn test_ignore_sigpipe_sets_sig_ign() {
        ignore_sigpipe();
        unsafe {
            // signal() returns the previous handler — after ignoring it should be SIG_IGN,
            // so broken pipes surface as BrokenPipe errors instead of killing the process.
            let prev = libc::signal(libc::SIGPIPE, libc::SIG_IGN);
            assert_eq!(prev, libc::SIG_IGN);
        }
    }
}
