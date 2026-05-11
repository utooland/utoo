/// One-time system tuning: fd limits, thread pools, etc.
pub fn init() {
    #[cfg(unix)]
    {
        raise_fd_limit();
        reset_sigpipe();
    }

    init_rayon_pool();
}

/// Configure the global rayon pool with a floor of 8 worker threads.
///
/// Rayon defaults to `num_cpus`, which is 2 on GHA ubuntu-latest.
/// Manifest parse and extract dispatch dozens of short blocking JSON
/// ops per fetch wave; with pool=2 these queue serially and a 5ms
/// parse stretches to ~30ms wall as it waits for a worker. Floor of
/// 8 oversubscribes the 2-core image but the work is still bounded
/// by host CPU — the extra slots replace FIFO queueing with parallel
/// dispatch. On bigger hosts `num_cpus` already exceeds 8 so this
/// is a no-op there.
fn init_rayon_pool() {
    let parallelism = std::thread::available_parallelism()
        .map(std::num::NonZero::get)
        .unwrap_or(2);
    let threads = parallelism.max(8);

    let builder = rayon::ThreadPoolBuilder::new().num_threads(threads);

    #[cfg(target_os = "windows")]
    let builder = builder.stack_size(8 * 1024 * 1024);

    builder.build_global().ok();
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
