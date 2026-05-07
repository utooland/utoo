/// One-time system tuning: fd limits, thread pools, etc.
pub fn init() {
    #[cfg(unix)]
    {
        raise_fd_limit();
        reset_sigpipe();
    }

    init_rayon_pool();
}

/// Configure the global rayon pool. On 16+ core dev/CI machines a default
/// pool of `num_cpus` produces a write-storm during tarball extraction
/// that saturates the underlying disk's IO queue (GHA pcap experiments
/// showed util_max=92% + w_await peaks of 490ms paired with TCP retx=123
/// on the install hot path). Capping at 8 threads keeps headroom for the
/// rest of the runtime — tokio worker, IO completion handlers — and for
/// the disk's own queue draining without losing meaningful parallelism on
/// the typical 1-1000 file tarball.
///
/// Windows additionally raises the per-thread stack size: the default 1MB
/// is insufficient for libdeflater + tar parsing + rayon work-stealing.
fn init_rayon_pool() {
    let parallelism = std::thread::available_parallelism()
        .map(std::num::NonZero::get)
        .unwrap_or(2);
    let threads = parallelism.min(8);

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
