use std::os::unix::io::FromRawFd;

use deno_core::op2;
use deno_error::JsErrorBox;

/// Check if an IPC channel is available (NODE_CHANNEL_FD env var set by parent Node).
#[op2(fast)]
pub fn op_ipc_has_channel() -> bool {
    std::env::var("NODE_CHANNEL_FD").is_ok()
}

/// Send a JSON message over the IPC channel.
/// Writes `data + '\n'` to the fd specified by NODE_CHANNEL_FD.
/// This is the same protocol as Node.js JSON-mode IPC.
#[op2(fast)]
pub fn op_ipc_send(#[string] data: &str) -> Result<(), JsErrorBox> {
    let fd: i32 = std::env::var("NODE_CHANNEL_FD")
        .ok()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| JsErrorBox::generic("No IPC channel (NODE_CHANNEL_FD not set)"))?;

    let msg = format!("{}\n", data);
    let bytes = msg.as_bytes();
    let mut written = 0usize;
    while written < bytes.len() {
        let result = unsafe {
            libc::write(
                fd,
                bytes[written..].as_ptr() as *const libc::c_void,
                bytes.len() - written,
            )
        };
        if result < 0 {
            return Err(JsErrorBox::generic(format!(
                "IPC write failed: {}",
                std::io::Error::last_os_error()
            )));
        }
        written += result as usize;
    }
    Ok(())
}

/// Read one newline-delimited message from the IPC channel.
/// Returns empty string on EOF (parent closed the channel).
/// Blocks until a complete line is available (runs on tokio blocking pool).
#[op2]
#[string]
pub async fn op_ipc_read_line() -> Result<String, JsErrorBox> {
    use std::io::BufRead;
    use std::sync::Mutex;

    // Persistent BufReader -- initialized once from the raw fd, kept alive
    // for the process lifetime so buffered data isn't lost between calls.
    static IPC_READER: Mutex<Option<std::io::BufReader<std::fs::File>>> = Mutex::new(None);

    tokio::task::spawn_blocking(move || {
        let mut guard = IPC_READER
            .lock()
            .map_err(|e| JsErrorBox::generic(format!("IPC lock: {}", e)))?;

        if guard.is_none() {
            let fd: i32 = std::env::var("NODE_CHANNEL_FD")
                .ok()
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| JsErrorBox::generic("No IPC channel"))?;

            // SAFETY: fd is inherited from the parent Node process (socketpair).
            // We take ownership -- the File lives in this static for the process lifetime.
            let file = unsafe { std::fs::File::from_raw_fd(fd) };
            *guard = Some(std::io::BufReader::new(file));
        }

        let reader = guard.as_mut().unwrap();
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => Ok(String::new()), // EOF → empty string sentinel
            Ok(_) => {
                if line.ends_with('\n') {
                    line.pop();
                }
                if line.ends_with('\r') {
                    line.pop();
                }
                Ok(line)
            }
            Err(e) => Err(JsErrorBox::generic(format!("IPC read: {}", e))),
        }
    })
    .await
    .map_err(|e| JsErrorBox::generic(format!("IPC join: {}", e)))?
}
