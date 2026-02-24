use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;

use deno_core::{op2, JsBuffer, OpState, Resource, ResourceId};
use deno_error::JsErrorBox;
use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};

struct ChildProcessResource {
    child: RefCell<Option<Child>>,
    stdin_rid: Option<ResourceId>,
    stdout_rid: Option<ResourceId>,
    stderr_rid: Option<ResourceId>,
}

impl Resource for ChildProcessResource {
    fn name(&self) -> Cow<'_, str> {
        "childProcess".into()
    }
}

struct ChildStdinResource {
    inner: RefCell<Option<tokio::process::ChildStdin>>,
}

impl Resource for ChildStdinResource {
    fn name(&self) -> Cow<'_, str> {
        "childStdin".into()
    }
}

struct ChildStdoutResource {
    inner: RefCell<Option<tokio::process::ChildStdout>>,
}

impl Resource for ChildStdoutResource {
    fn name(&self) -> Cow<'_, str> {
        "childStdout".into()
    }
}

struct ChildStderrResource {
    inner: RefCell<Option<tokio::process::ChildStderr>>,
}

impl Resource for ChildStderrResource {
    fn name(&self) -> Cow<'_, str> {
        "childStderr".into()
    }
}

#[derive(Serialize)]
pub struct SpawnResult {
    pub rid: ResourceId,
    pub pid: u32,
    pub stdin_rid: Option<ResourceId>,
    pub stdout_rid: Option<ResourceId>,
    pub stderr_rid: Option<ResourceId>,
}

#[derive(Serialize)]
pub struct ForkSpawnResult {
    pub rid: ResourceId,
    pub pid: u32,
    pub stdin_rid: Option<ResourceId>,
    pub stdout_rid: Option<ResourceId>,
    pub stderr_rid: Option<ResourceId>,
    pub ipc_rid: ResourceId,
}

#[derive(Serialize)]
pub struct WaitResult {
    pub code: Option<i32>,
    pub success: bool,
}

/// Resource wrapping the parent end of an IPC socketpair.
struct IpcStreamResource {
    reader: RefCell<tokio::io::BufReader<tokio::io::ReadHalf<tokio::net::UnixStream>>>,
    writer: RefCell<tokio::io::WriteHalf<tokio::net::UnixStream>>,
}

impl Resource for IpcStreamResource {
    fn name(&self) -> Cow<'_, str> {
        "ipcStream".into()
    }
}

/// Spawn a child process.
/// `stdio` is a 3-element array: [stdin, stdout, stderr]
/// Each element: "pipe" | "inherit" | "null"
#[op2]
#[serde]
pub fn op_spawn(
    state: &mut OpState,
    #[string] cmd: &str,
    #[serde] args: Vec<String>,
    #[serde] stdio: Vec<String>,
    #[string] cwd: &str,
    #[serde] env: Vec<(String, String)>,
) -> Result<SpawnResult, JsErrorBox> {
    let mut command = Command::new(cmd);
    command.args(&args);

    if !cwd.is_empty() {
        command.current_dir(cwd);
    }

    for (k, v) in &env {
        command.env(k, v);
    }

    // Configure stdio
    let stdin_cfg = stdio.first().map(|s| s.as_str()).unwrap_or("pipe");
    let stdout_cfg = stdio.get(1).map(|s| s.as_str()).unwrap_or("pipe");
    let stderr_cfg = stdio.get(2).map(|s| s.as_str()).unwrap_or("pipe");

    command.stdin(match stdin_cfg {
        "pipe" => std::process::Stdio::piped(),
        "inherit" => std::process::Stdio::inherit(),
        _ => std::process::Stdio::null(),
    });
    command.stdout(match stdout_cfg {
        "pipe" => std::process::Stdio::piped(),
        "inherit" => std::process::Stdio::inherit(),
        _ => std::process::Stdio::null(),
    });
    command.stderr(match stderr_cfg {
        "pipe" => std::process::Stdio::piped(),
        "inherit" => std::process::Stdio::inherit(),
        _ => std::process::Stdio::null(),
    });

    let mut child = command
        .spawn()
        .map_err(|e| JsErrorBox::generic(format!("Failed to spawn '{}': {}", cmd, e)))?;

    let pid = child.id().unwrap_or(0);

    // Extract stdio handles and register as resources
    let stdin_rid = child.stdin.take().map(|s| {
        state.resource_table.add(ChildStdinResource {
            inner: RefCell::new(Some(s)),
        })
    });
    let stdout_rid = child.stdout.take().map(|s| {
        state.resource_table.add(ChildStdoutResource {
            inner: RefCell::new(Some(s)),
        })
    });
    let stderr_rid = child.stderr.take().map(|s| {
        state.resource_table.add(ChildStderrResource {
            inner: RefCell::new(Some(s)),
        })
    });

    let rid = state.resource_table.add(ChildProcessResource {
        child: RefCell::new(Some(child)),
        stdin_rid,
        stdout_rid,
        stderr_rid,
    });

    Ok(SpawnResult {
        rid,
        pid,
        stdin_rid,
        stdout_rid,
        stderr_rid,
    })
}

/// Wait for a child process to exit.
#[op2]
#[serde]
pub async fn op_spawn_wait(
    state: Rc<RefCell<OpState>>,
    #[smi] rid: ResourceId,
) -> Result<WaitResult, JsErrorBox> {
    let resource = state
        .borrow()
        .resource_table
        .get::<ChildProcessResource>(rid)
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;

    let mut child_opt = resource.child.borrow_mut();
    let child = child_opt
        .as_mut()
        .ok_or_else(|| JsErrorBox::generic("Child process already consumed"))?;

    let status = child
        .wait()
        .await
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;

    Ok(WaitResult {
        code: status.code(),
        success: status.success(),
    })
}

/// Write to child stdin.
#[op2]
#[smi]
pub async fn op_spawn_stdin_write(
    state: Rc<RefCell<OpState>>,
    #[smi] rid: ResourceId,
    #[buffer] data: JsBuffer,
) -> Result<u32, JsErrorBox> {
    let resource = state
        .borrow()
        .resource_table
        .get::<ChildStdinResource>(rid)
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;

    let mut inner = resource.inner.borrow_mut();
    let stdin = inner
        .as_mut()
        .ok_or_else(|| JsErrorBox::generic("Stdin already closed"))?;

    let n = stdin
        .write(data.as_ref())
        .await
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;
    Ok(n as u32)
}

/// Close child stdin (signals EOF to the child).
#[op2(fast)]
pub fn op_spawn_stdin_close(
    state: &mut OpState,
    #[smi] rid: ResourceId,
) -> Result<(), JsErrorBox> {
    let resource = state
        .resource_table
        .get::<ChildStdinResource>(rid)
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;
    resource.inner.borrow_mut().take(); // Drop the stdin handle
    Ok(())
}

/// Read from child stdout.
#[op2]
#[serde]
pub async fn op_spawn_stdout_read(
    state: Rc<RefCell<OpState>>,
    #[smi] rid: ResourceId,
    #[smi] len: u32,
) -> Result<Vec<u8>, JsErrorBox> {
    let resource = state
        .borrow()
        .resource_table
        .get::<ChildStdoutResource>(rid)
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;

    let mut inner = resource.inner.borrow_mut();
    let stdout = inner
        .as_mut()
        .ok_or_else(|| JsErrorBox::generic("Stdout already closed"))?;

    let mut buf = vec![0u8; len as usize];
    let n = stdout
        .read(&mut buf)
        .await
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;
    buf.truncate(n);
    Ok(buf)
}

/// Read from child stderr.
#[op2]
#[serde]
pub async fn op_spawn_stderr_read(
    state: Rc<RefCell<OpState>>,
    #[smi] rid: ResourceId,
    #[smi] len: u32,
) -> Result<Vec<u8>, JsErrorBox> {
    let resource = state
        .borrow()
        .resource_table
        .get::<ChildStderrResource>(rid)
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;

    let mut inner = resource.inner.borrow_mut();
    let stderr = inner
        .as_mut()
        .ok_or_else(|| JsErrorBox::generic("Stderr already closed"))?;

    let mut buf = vec![0u8; len as usize];
    let n = stderr
        .read(&mut buf)
        .await
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;
    buf.truncate(n);
    Ok(buf)
}

/// Kill a child process.
#[op2(fast)]
pub fn op_spawn_kill(
    state: &mut OpState,
    #[smi] rid: ResourceId,
    #[smi] signal: i32,
) -> Result<(), JsErrorBox> {
    let resource = state
        .resource_table
        .get::<ChildProcessResource>(rid)
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;

    let mut child_opt = resource.child.borrow_mut();
    let child = child_opt
        .as_mut()
        .ok_or_else(|| JsErrorBox::generic("Child process already consumed"))?;

    // On Unix, send signal via libc. Default to SIGTERM.
    #[cfg(unix)]
    {
        let _ = signal; // We use the signal parameter
        if let Some(pid) = child.id() {
            let sig = if signal == 0 { 15 } else { signal }; // SIGTERM = 15
            unsafe {
                libc::kill(pid as libc::pid_t, sig);
            }
        }
    }

    #[cfg(not(unix))]
    {
        let _ = signal;
        child
            .start_kill()
            .map_err(|e| JsErrorBox::generic(e.to_string()))?;
    }

    Ok(())
}

/// Close/cleanup a child process resource.
#[op2(fast)]
pub fn op_spawn_close(state: &mut OpState, #[smi] rid: ResourceId) -> Result<(), JsErrorBox> {
    // Try to remove all related resources
    if let Ok(proc) = state.resource_table.take::<ChildProcessResource>(rid) {
        if let Some(stdin_rid) = proc.stdin_rid {
            let _ = state.resource_table.take::<ChildStdinResource>(stdin_rid);
        }
        if let Some(stdout_rid) = proc.stdout_rid {
            let _ = state.resource_table.take::<ChildStdoutResource>(stdout_rid);
        }
        if let Some(stderr_rid) = proc.stderr_rid {
            let _ = state.resource_table.take::<ChildStderrResource>(stderr_rid);
        }
    }
    Ok(())
}

/// Execute a command synchronously and return its stdout.
#[derive(Serialize)]
pub struct ExecSyncResult {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub status: i32,
}

#[op2]
#[serde]
pub fn op_exec_sync(
    #[string] cmd: &str,
    #[serde] args: Vec<String>,
    #[string] cwd: &str,
    #[serde] env: Vec<(String, String)>,
    #[string] shell: &str,
) -> Result<ExecSyncResult, JsErrorBox> {
    let mut command = if !shell.is_empty() {
        let mut c = std::process::Command::new(shell);
        c.arg("-c");
        let full = if args.is_empty() {
            cmd.to_string()
        } else {
            format!("{} {}", cmd, args.join(" "))
        };
        c.arg(full);
        c
    } else {
        let mut c = std::process::Command::new(cmd);
        c.args(&args);
        c
    };

    if !cwd.is_empty() {
        command.current_dir(cwd);
    }

    for (k, v) in &env {
        command.env(k, v);
    }

    let output = command
        .output()
        .map_err(|e| JsErrorBox::generic(format!("execSync failed: {}", e)))?;

    let status = output.status.code().unwrap_or(-1);

    Ok(ExecSyncResult {
        stdout: output.stdout,
        stderr: output.stderr,
        status,
    })
}

/// Spawn a child process with an IPC channel (Unix socketpair on fd 3).
/// This is the equivalent of Node.js `child_process.fork()`.
#[op2]
#[serde]
pub fn op_fork_spawn(
    state: &mut OpState,
    #[string] cmd: &str,
    #[serde] args: Vec<String>,
    #[serde] stdio: Vec<String>,
    #[string] cwd: &str,
    #[serde] env: Vec<(String, String)>,
) -> Result<ForkSpawnResult, JsErrorBox> {
    use std::os::unix::io::FromRawFd;
    #[allow(unused_imports)] // used by pre_exec() inside unsafe block
    use std::os::unix::process::CommandExt;

    // Create a Unix socketpair for IPC
    let mut fds: [libc::c_int; 2] = [0; 2];
    let ret = unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, fds.as_mut_ptr()) };
    if ret != 0 {
        return Err(JsErrorBox::generic(format!(
            "socketpair failed: {}",
            std::io::Error::last_os_error()
        )));
    }
    let parent_fd = fds[0];
    let child_fd = fds[1];

    let mut command = Command::new(cmd);
    command.args(&args);

    if !cwd.is_empty() {
        command.current_dir(cwd);
    }

    for (k, v) in &env {
        command.env(k, v);
    }
    // Tell the child which fd is the IPC channel
    command.env("NODE_CHANNEL_FD", "3");

    // Configure stdio (same as op_spawn)
    let stdin_cfg = stdio.first().map(|s| s.as_str()).unwrap_or("pipe");
    let stdout_cfg = stdio.get(1).map(|s| s.as_str()).unwrap_or("pipe");
    let stderr_cfg = stdio.get(2).map(|s| s.as_str()).unwrap_or("pipe");

    command.stdin(match stdin_cfg {
        "pipe" => std::process::Stdio::piped(),
        "inherit" => std::process::Stdio::inherit(),
        _ => std::process::Stdio::null(),
    });
    command.stdout(match stdout_cfg {
        "pipe" => std::process::Stdio::piped(),
        "inherit" => std::process::Stdio::inherit(),
        _ => std::process::Stdio::null(),
    });
    command.stderr(match stderr_cfg {
        "pipe" => std::process::Stdio::piped(),
        "inherit" => std::process::Stdio::inherit(),
        _ => std::process::Stdio::null(),
    });

    // Before spawning, set up fd 3 in the child to be the child end of the socketpair
    unsafe {
        let cfd = child_fd;
        command.pre_exec(move || {
            // If child_fd is not already 3, dup it to 3
            if cfd != 3 {
                libc::dup2(cfd, 3);
                libc::close(cfd);
            }
            // Clear close-on-exec for fd 3 so it survives exec
            libc::fcntl(3, libc::F_SETFD, 0);
            Ok(())
        });
    }

    let mut child = command
        .spawn()
        .map_err(|e| JsErrorBox::generic(format!("Failed to fork-spawn '{}': {}", cmd, e)))?;

    // Close the child end of the socketpair in the parent
    unsafe {
        libc::close(child_fd);
    }

    let pid = child.id().unwrap_or(0);

    // Extract stdio handles
    let stdin_rid = child.stdin.take().map(|s| {
        state.resource_table.add(ChildStdinResource {
            inner: RefCell::new(Some(s)),
        })
    });
    let stdout_rid = child.stdout.take().map(|s| {
        state.resource_table.add(ChildStdoutResource {
            inner: RefCell::new(Some(s)),
        })
    });
    let stderr_rid = child.stderr.take().map(|s| {
        state.resource_table.add(ChildStderrResource {
            inner: RefCell::new(Some(s)),
        })
    });

    let rid = state.resource_table.add(ChildProcessResource {
        child: RefCell::new(Some(child)),
        stdin_rid,
        stdout_rid,
        stderr_rid,
    });

    // Wrap the parent end of the socketpair as a tokio UnixStream
    let parent_std = unsafe { std::os::unix::net::UnixStream::from_raw_fd(parent_fd) };
    parent_std
        .set_nonblocking(true)
        .map_err(|e| JsErrorBox::generic(format!("set_nonblocking: {}", e)))?;
    let parent_stream = tokio::net::UnixStream::from_std(parent_std)
        .map_err(|e| JsErrorBox::generic(format!("UnixStream::from_std: {}", e)))?;
    let (read_half, write_half) = tokio::io::split(parent_stream);

    let ipc_rid = state.resource_table.add(IpcStreamResource {
        reader: RefCell::new(tokio::io::BufReader::new(read_half)),
        writer: RefCell::new(write_half),
    });

    Ok(ForkSpawnResult {
        rid,
        pid,
        stdin_rid,
        stdout_rid,
        stderr_rid,
        ipc_rid,
    })
}

/// Write a message (JSON line) to the parent end of an IPC channel.
#[op2]
#[smi]
pub async fn op_ipc_stream_write(
    state: Rc<RefCell<OpState>>,
    #[smi] rid: ResourceId,
    #[string] data: String,
) -> Result<u32, JsErrorBox> {
    let resource = state
        .borrow()
        .resource_table
        .get::<IpcStreamResource>(rid)
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;

    let msg = format!("{}\n", data);
    let mut writer = resource.writer.borrow_mut();
    writer
        .write_all(msg.as_bytes())
        .await
        .map_err(|e| JsErrorBox::generic(format!("IPC write: {}", e)))?;
    writer
        .flush()
        .await
        .map_err(|e| JsErrorBox::generic(format!("IPC flush: {}", e)))?;
    Ok(msg.len() as u32)
}

/// Read a newline-delimited message from the parent end of an IPC channel.
#[op2]
#[string]
pub async fn op_ipc_stream_read_line(
    state: Rc<RefCell<OpState>>,
    #[smi] rid: ResourceId,
) -> Result<String, JsErrorBox> {
    let resource = state
        .borrow()
        .resource_table
        .get::<IpcStreamResource>(rid)
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;

    let mut reader = resource.reader.borrow_mut();
    let mut line = String::new();
    let n = reader
        .read_line(&mut line)
        .await
        .map_err(|e| JsErrorBox::generic(format!("IPC read: {}", e)))?;
    if n == 0 {
        return Ok(String::new()); // EOF
    }
    if line.ends_with('\n') {
        line.pop();
    }
    if line.ends_with('\r') {
        line.pop();
    }
    Ok(line)
}
