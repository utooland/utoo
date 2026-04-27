/// One-time system tuning: fd limits, thread pools, etc.
pub fn init() {
    #[cfg(unix)]
    {
        raise_fd_limit();
        reset_sigpipe();
    }

    // Windows default thread stack is 1MB, insufficient for libdeflater + tar
    // + rayon work-stealing.
    #[cfg(target_os = "windows")]
    rayon::ThreadPoolBuilder::new()
        .stack_size(8 * 1024 * 1024)
        .build_global()
        .ok();
}

/// Conservative upper bound for filesystem-heavy parallel work.
///
/// Package linking is mostly blocking I/O. Letting one task per package enter
/// the blocking pool at once creates scheduler churn on large dependency
/// graphs, while a limit based on CPU parallelism still keeps disk queues full.
pub fn parallel_io_limit() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().saturating_mul(4))
        .unwrap_or(32)
        .clamp(16, 128)
}

/// Restore default SIGPIPE handling so broken pipes cause a clean exit
/// instead of a panic. Same approach as ripgrep and fd.
#[cfg(unix)]
fn reset_sigpipe() {
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
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
    fn test_reset_sigpipe_sets_sig_dfl() {
        reset_sigpipe();
        unsafe {
            // signal() returns the previous handler — after reset it should be SIG_DFL
            let prev = libc::signal(libc::SIGPIPE, libc::SIG_DFL);
            assert_eq!(prev, libc::SIG_DFL);
        }
    }

    #[test]
    fn test_parallel_io_limit_is_bounded() {
        let limit = parallel_io_limit();
        assert!((16..=128).contains(&limit));
    }
}
