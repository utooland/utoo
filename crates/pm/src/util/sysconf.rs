/// One-time system tuning: fd limits, thread pools, etc.
pub fn init() {
    #[cfg(unix)]
    {
        raise_fd_limit();
        reset_sigpipe();
    }

    init_rayon_pool();
}

/// Configure the global rayon pool size.
///
/// Rayon defaults to `num_cpus` workers, which is 2 on GHA ubuntu-latest.
/// Two workers are enough for the install-path's `par_chunks(64)` extract
/// (mostly disk-bound), but the resolve-path's manifest parse + extract
/// pipeline runs *many* short CPU bursts (parse: ~5ms, get_core_version:
/// ~1-3ms) dispatched from up to 64 concurrent fetches.
///
/// With pool=2, each fetch waits up to ~25ms in queue per dispatch —
/// fetch-breakdown instrumentation showed avg_parse jumping 5ms (CPU)
/// → 30ms (CPU + queue) just from the first dispatch. The second hop
/// (`extract_core_version_off_runtime`) has the same problem. `tokio
/// spawn_blocking` avoids the queue but its per-dispatch overhead
/// (round 3 measurement) was higher than rayon's queue wait at 64×.
///
/// Sizing the pool above the host CPU count for these short, blocking
/// JSON-shape operations gives the queue a chance to drain even when
/// 64 fetches dispatch concurrently. The work itself is bounded — at
/// most 2 are doing real CPU at once on a 2-core box; the extra pool
/// slots just hold pending tasks until a CPU is free, replacing FIFO
/// queueing with parallel dispatch.
///
/// Cap of 8 keeps the pool reasonable on bigger machines (where
/// `num_cpus` is already enough); the floor of 8 oversubscribes
/// only on the constrained 2-core CI image.
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
