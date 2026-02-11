use deno_core::op2;
use serde::Serialize;

#[op2]
#[string]
pub fn op_os_hostname() -> String {
    #[cfg(unix)]
    {
        let mut buf = [0u8; 256];
        let ret = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut _, buf.len()) };
        if ret == 0 {
            let cstr = unsafe { std::ffi::CStr::from_ptr(buf.as_ptr() as *const _) };
            return cstr.to_string_lossy().into_owned();
        }
    }
    String::new()
}

#[op2]
#[string]
pub fn op_os_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "windows") {
        "win32"
    } else {
        "linux"
    }
}

#[op2]
#[string]
pub fn op_os_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        std::env::consts::ARCH
    }
}

#[op2]
#[string]
pub fn op_os_type() -> &'static str {
    if cfg!(target_os = "macos") {
        "Darwin"
    } else if cfg!(target_os = "windows") {
        "Windows_NT"
    } else {
        "Linux"
    }
}

#[op2]
#[string]
pub fn op_os_release() -> String {
    #[cfg(unix)]
    {
        let mut uname = std::mem::MaybeUninit::<libc::utsname>::uninit();
        let ret = unsafe { libc::uname(uname.as_mut_ptr()) };
        if ret == 0 {
            let uname = unsafe { uname.assume_init() };
            let release = unsafe { std::ffi::CStr::from_ptr(uname.release.as_ptr()) };
            return release.to_string_lossy().into_owned();
        }
    }
    String::new()
}

#[op2]
#[string]
pub fn op_os_tmpdir() -> String {
    std::env::temp_dir().to_string_lossy().into_owned()
}

#[op2]
#[string]
pub fn op_os_homedir() -> String {
    #[cfg(unix)]
    {
        std::env::var("HOME").unwrap_or_default()
    }
    #[cfg(not(unix))]
    {
        std::env::var("USERPROFILE").unwrap_or_default()
    }
}

#[derive(Serialize)]
pub struct CpuInfo {
    pub model: String,
    pub speed: u64,
}

#[op2]
#[serde]
pub fn op_os_cpus() -> Vec<CpuInfo> {
    let count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    (0..count)
        .map(|_| CpuInfo {
            model: String::new(),
            speed: 0,
        })
        .collect()
}

#[op2(fast)]
pub fn op_os_uptime() -> f64 {
    #[cfg(target_os = "macos")]
    {
        // Use sysctl kern.boottime on macOS
        use std::time::{SystemTime, UNIX_EPOCH};
        let mut boottime = libc::timeval {
            tv_sec: 0,
            tv_usec: 0,
        };
        let mut size = std::mem::size_of::<libc::timeval>();
        let mut mib = [libc::CTL_KERN, libc::KERN_BOOTTIME];
        let ret = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                2,
                &mut boottime as *mut _ as *mut _,
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if ret == 0 {
            if let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) {
                return now.as_secs_f64() - boottime.tv_sec as f64;
            }
        }
        0.0
    }
    #[cfg(target_os = "linux")]
    {
        let mut info = std::mem::MaybeUninit::<libc::sysinfo>::uninit();
        let ret = unsafe { libc::sysinfo(info.as_mut_ptr()) };
        if ret == 0 {
            let info = unsafe { info.assume_init() };
            return info.uptime as f64;
        }
        0.0
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        0.0
    }
}
