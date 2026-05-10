/// One-time system tuning: fd limits, thread pools, etc.
pub fn init() {
    #[cfg(unix)]
    {
        raise_fd_limit();
        reset_sigpipe();
    }

    // Windows default thread stack is 1MB, insufficient for libdeflater
    // + tar + rayon work-stealing. On Unix the default 8MB stack is fine.
    //
    // Rayon thread count: prior iteration forced \`max(num_cpus, 8)\` on
    // the theory that resolve-path manifest parse benefits from extra
    // pool slots. Bench A/B showed that on 2-core GHA runners, 8 rayon
    // workers oversubscribe disk during install-path tarball extract
    // (par_chunks(64) × 8 = 512 in-flight writes) — utoo p3 degrades
    // sharply under CI contention while utoo-next (default num_cpus)
    // stays stable. Reverted to default to keep install-path stable;
    // resolve-path uses tokio's blocking pool (512 default slots),
    // which doesn't share rayon's contention.
    #[cfg(target_os = "windows")]
    rayon::ThreadPoolBuilder::new()
        .stack_size(8 * 1024 * 1024)
        .build_global()
        .ok();
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
}
