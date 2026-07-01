use anyhow::{Context, Result};
use deno_core::{JsRuntime, JsRuntimeForSnapshot};

use crate::loader::UtooModuleLoader;
use crate::ops;

deno_core::extension!(
    utoo_rt_ext,
    ops = [
        // Console
        ops::console::op_console_log,
        ops::console::op_console_warn,
        ops::console::op_console_error,
        // Process
        ops::process::op_exit,
        ops::process::op_cwd,
        ops::process::op_env_to_object,
        ops::process::op_exec_path,
        // CJS
        ops::cjs::op_cjs_resolve,
        ops::cjs::op_cjs_detect,
        ops::cjs::op_cjs_transpile,
        // FS — async
        ops::fs::op_fs_read_file,
        ops::fs::op_fs_read_text_file,
        ops::fs::op_fs_write_file,
        ops::fs::op_fs_append_file,
        ops::fs::op_fs_readdir,
        ops::fs::op_fs_mkdir,
        ops::fs::op_fs_stat,
        ops::fs::op_fs_lstat,
        ops::fs::op_fs_unlink,
        ops::fs::op_fs_rename,
        ops::fs::op_fs_copy_file,
        ops::fs::op_fs_rm,
        ops::fs::op_fs_access,
        ops::fs::op_fs_chmod,
        ops::fs::op_fs_realpath,
        // FS — sync
        ops::fs::op_fs_read_file_sync,
        ops::fs::op_fs_read_text_file_sync,
        ops::fs::op_fs_write_file_sync,
        ops::fs::op_fs_append_file_sync,
        ops::fs::op_fs_readdir_sync,
        ops::fs::op_fs_mkdir_sync,
        ops::fs::op_fs_stat_sync,
        ops::fs::op_fs_lstat_sync,
        ops::fs::op_fs_unlink_sync,
        ops::fs::op_fs_rename_sync,
        ops::fs::op_fs_copy_file_sync,
        ops::fs::op_fs_rm_sync,
        ops::fs::op_fs_exists_sync,
        ops::fs::op_fs_access_sync,
        ops::fs::op_fs_chmod_sync,
        ops::fs::op_fs_realpath_sync,
        // OS
        ops::os::op_os_hostname,
        ops::os::op_os_platform,
        ops::os::op_os_arch,
        ops::os::op_os_type,
        ops::os::op_os_release,
        ops::os::op_os_tmpdir,
        ops::os::op_os_homedir,
        ops::os::op_os_cpus,
        ops::os::op_os_uptime,
        // Crypto
        ops::crypto::op_crypto_hash_create,
        ops::crypto::op_crypto_hash_update,
        ops::crypto::op_crypto_hash_digest,
        ops::crypto::op_crypto_hmac_create,
        ops::crypto::op_crypto_hmac_update,
        ops::crypto::op_crypto_hmac_digest,
        ops::crypto::op_crypto_random_bytes,
        ops::crypto::op_crypto_aes_gcm_encrypt,
        ops::crypto::op_crypto_aes_gcm_decrypt,
        // Net
        ops::net::op_net_listen,
        ops::net::op_net_accept,
        ops::net::op_net_connect,
        ops::net::op_net_read,
        ops::net::op_net_write,
        ops::net::op_net_shutdown,
        ops::net::op_net_close,
        ops::net::op_net_local_addr,
        ops::net::op_net_remote_addr,
        // NAPI
        ops::napi::op_napi_open,
        // Child process
        ops::child_process::op_spawn,
        ops::child_process::op_spawn_wait,
        ops::child_process::op_spawn_stdin_write,
        ops::child_process::op_spawn_stdin_close,
        ops::child_process::op_spawn_stdout_read,
        ops::child_process::op_spawn_stderr_read,
        ops::child_process::op_spawn_kill,
        ops::child_process::op_spawn_close,
        ops::child_process::op_exec_sync,
        ops::child_process::op_fork_spawn,
        ops::child_process::op_ipc_stream_write,
        ops::child_process::op_ipc_stream_read_line,
        // IPC (Node.js fork channel)
        ops::ipc::op_ipc_has_channel,
        ops::ipc::op_ipc_send,
        ops::ipc::op_ipc_read_line,
    ],
    esm_entry_point = "ext:utoo_rt_ext/node/_init",
    esm = [
        "ext:utoo_rt_ext/node/events" = "src/js/node/events.js",
        "ext:utoo_rt_ext/node/util" = "src/js/node/util.js",
        "ext:utoo_rt_ext/node/assert" = "src/js/node/assert.js",
        "ext:utoo_rt_ext/node/querystring" = "src/js/node/querystring.js",
        "ext:utoo_rt_ext/node/string_decoder" = "src/js/node/string_decoder.js",
        "ext:utoo_rt_ext/node/stream" = "src/js/node/stream.js",
        "ext:utoo_rt_ext/node/net" = "src/js/node/net.js",
        "ext:utoo_rt_ext/node/http" = "src/js/node/http.js",
        "ext:utoo_rt_ext/node/https" = "src/js/node/https.js",
        "ext:utoo_rt_ext/node/http2" = "src/js/node/http2.js",
        "ext:utoo_rt_ext/node/async_hooks" = "src/js/node/async_hooks.js",
        "ext:utoo_rt_ext/node/crypto" = "src/js/node/crypto.js",
        "ext:utoo_rt_ext/node/zlib" = "src/js/node/zlib.js",
        "ext:utoo_rt_ext/node/fs_promises" = "src/js/node/fs_promises.js",
        "ext:utoo_rt_ext/node/fs" = "src/js/node/fs.js",
        "ext:utoo_rt_ext/node/path" = "src/js/node/path.js",
        "ext:utoo_rt_ext/node/os" = "src/js/node/os.js",
        "ext:utoo_rt_ext/node/url" = "src/js/node/url.js",
        "ext:utoo_rt_ext/node/buffer" = "src/js/node/buffer.js",
        "ext:utoo_rt_ext/node/v8" = "src/js/node/v8.js",
        "ext:utoo_rt_ext/node/cluster" = "src/js/node/cluster.js",
        "ext:utoo_rt_ext/node/child_process" = "src/js/node/child_process.js",
        "ext:utoo_rt_ext/node/tty" = "src/js/node/tty.js",
        "ext:utoo_rt_ext/node/dns" = "src/js/node/dns.js",
        "ext:utoo_rt_ext/node/dgram" = "src/js/node/dgram.js",
        "ext:utoo_rt_ext/node/tls" = "src/js/node/tls.js",
        "ext:utoo_rt_ext/node/worker_threads" = "src/js/node/worker_threads.js",
        "ext:utoo_rt_ext/node/perf_hooks" = "src/js/node/perf_hooks.js",
        "ext:utoo_rt_ext/node/module" = "src/js/node/module.js",
        "ext:utoo_rt_ext/node/readline" = "src/js/node/readline.js",
        "ext:utoo_rt_ext/node/diagnostics_channel" = "src/js/node/diagnostics_channel.js",
        "ext:utoo_rt_ext/node/console" = "src/js/node/console.js",
        "ext:utoo_rt_ext/node/timers" = "src/js/node/timers.js",
        "ext:utoo_rt_ext/node/timers_promises" = "src/js/node/timers_promises.js",
        "ext:utoo_rt_ext/node/process" = "src/js/node/process.js",
        "ext:utoo_rt_ext/node/constants" = "src/js/node/constants.js",
        "ext:utoo_rt_ext/node/domain" = "src/js/node/domain.js",
        "ext:utoo_rt_ext/node/util_types" = "src/js/node/util_types.js",
        "ext:utoo_rt_ext/node/stream_promises" = "src/js/node/stream_promises.js",
        "ext:utoo_rt_ext/node/stream_web" = "src/js/node/stream_web.js",
        "ext:utoo_rt_ext/node/stream_consumers" = "src/js/node/stream_consumers.js",
        "ext:utoo_rt_ext/node/inspector" = "src/js/node/inspector.js",
        "ext:utoo_rt_ext/node/dns_promises" = "src/js/node/dns_promises.js",
        "ext:utoo_rt_ext/node/path_posix" = "src/js/node/path_posix.js",
        "ext:utoo_rt_ext/node/vm" = "src/js/node/vm.js",
        "ext:utoo_rt_ext/node/repl" = "src/js/node/repl.js",
        "ext:utoo_rt_ext/node/_init" = "src/js/node/_init.js",
    ],
    js = ["src/js/bootstrap.js", "src/js/cjs_loader.js"],
    state = |state| {
        state.put(crate::napi::NapiState {
            env_cleanup_hooks: std::rc::Rc::new(std::cell::RefCell::new(vec![])),
        });
    },
);

/// Create the utoo_rt_ext extension. Used by the snapshot generator and benchmarks.
pub fn create_ext() -> deno_core::Extension {
    utoo_rt_ext::init()
}

pub async fn run_script(script_path: &str) -> Result<()> {
    run_script_with_args(script_path, &[]).await
}

pub async fn run_script_with_args(script_path: &str, script_args: &[String]) -> Result<()> {
    let abs_path = std::path::absolute(script_path)
        .with_context(|| format!("Invalid path: {script_path}"))?;

    let main_module =
        deno_core::ModuleSpecifier::from_file_path(&abs_path).map_err(|_| {
            anyhow::anyhow!("Cannot convert path to module specifier: {}", abs_path.display())
        })?;

    // Build the process.argv JSON array
    let argv_json = {
        let mut argv = vec![
            serde_json::Value::String("utoo-runtime".to_string()),
            serde_json::Value::String(abs_path.to_string_lossy().into_owned()),
        ];
        for arg in script_args {
            argv.push(serde_json::Value::String(arg.clone()));
        }
        serde_json::to_string(&argv)?
    };

    let mut runtime = JsRuntime::new(deno_core::RuntimeOptions {
        module_loader: Some(std::rc::Rc::new(UtooModuleLoader)),
        extensions: vec![utoo_rt_ext::init()],
        ..Default::default()
    });

    // Set process.argv and execPath (bootstrap defaults are stale)
    runtime.execute_script(
        "<utoo:init-argv>",
        format!(
            "globalThis.process.argv = {argv_json};\
             globalThis.process.execPath = Deno.core.ops.op_exec_path();",
        ),
    )?;

    let mod_id = runtime
        .load_main_es_module(&main_module)
        .await
        .with_context(|| format!("Failed to load {}", abs_path.display()))?;

    let result = runtime.mod_evaluate(mod_id);
    runtime
        .run_event_loop(deno_core::PollEventLoopOptions::default())
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("Event loop error")?;
    result
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("Module evaluation error")?;

    Ok(())
}

/// Build an application-level snapshot.
///
/// Runs the user's entry script in a `JsRuntimeForSnapshot` with
/// `__utoo_snapshot_mode = true`. In this mode, `net.Server.listen()` captures
/// args without actually binding a port, so the event loop drains naturally
/// once framework initialization completes. The entire V8 heap (runtime +
/// framework + app state) is then serialized to `output_path`.
pub async fn build_app_snapshot(script_path: &str, output_path: &str) -> Result<()> {
    let abs_path = std::path::absolute(script_path)
        .with_context(|| format!("Invalid path: {script_path}"))?;

    let main_module =
        deno_core::ModuleSpecifier::from_file_path(&abs_path).map_err(|_| {
            anyhow::anyhow!("Cannot convert path to module specifier: {}", abs_path.display())
        })?;

    // Build app snapshot from scratch (don't layer on runtime snapshot).
    let mut runtime = JsRuntimeForSnapshot::new(deno_core::RuntimeOptions {
        module_loader: Some(std::rc::Rc::new(UtooModuleLoader)),
        extensions: vec![utoo_rt_ext::init()],
        ..Default::default()
    });

    // Enable snapshot mode.
    //
    // V8 snapshot crash root cause: Error.stackTraceLimit is a V8-managed
    // "special" property. Any JS write to it (even the same value) converts
    // it from V8's internal representation to a regular JS data property.
    // On restore, V8 tries to re-add it via AddProperty which asserts the
    // property must not already exist → LinearSearch crash.
    //
    // Fix strategy:
    // 1. Save original Error.captureStackTrace (V8 native) before replacing
    // 2. Replace captureStackTrace with a no-op that sets obj.stack = []
    //    (so depd's getStack() cleanup path runs and restores prepareStackTrace)
    // 3. Let modules freely modify Error.stackTraceLimit during loading
    // 4. Before snapshot: delete Error.stackTraceLimit and restore
    //    Error.captureStackTrace to the original V8 native
    runtime.execute_script(
        "<utoo:snapshot-init>",
        format!(
            "globalThis.__utoo_snapshot_mode = true;\
             globalThis.__utoo_building_snapshot = true;\
             globalThis.process.argv = ['utoo-runtime', {}];\
             globalThis.__utoo_origCaptureStackTrace = Error.captureStackTrace;\
             Error.captureStackTrace = function(obj) {{\
               var fake = {{\
                 getFileName: function() {{ return '<snapshot>'; }},\
                 getLineNumber: function() {{ return 0; }},\
                 getColumnNumber: function() {{ return 0; }},\
                 getFunctionName: function() {{ return null; }},\
                 getTypeName: function() {{ return null; }},\
                 getMethodName: function() {{ return null; }},\
                 isEval: function() {{ return false; }},\
                 getEvalOrigin: function() {{ return ''; }},\
                 getThis: function() {{ return null; }},\
                 toString: function() {{ return '<snapshot>:0:0'; }}\
               }};\
               obj.stack = [fake, fake, fake, fake, fake, fake, fake, fake, fake, fake];\
             }};",
            serde_json::to_string(abs_path.to_string_lossy().as_ref())?
        ),
    )?;

    // V8 startup-snapshot isolates have WebAssembly disabled (compiled WASM cannot
    // be serialized into a snapshot). Some dependencies eagerly compile WASM at
    // module load and reject when it is unavailable (e.g. undici compiling llhttp
    // via `await WebAssembly.compile`). Those resources re-initialize lazily at
    // runtime (where WASM is available), so a WASM-unavailable rejection during the
    // snapshot build is benign and must not abort it. Suppress *only* those; all
    // other unhandled rejections stay fatal. The default handler (() => false) is
    // restored on deserialize in run_from_app_snapshot.
    runtime.execute_script(
        "<utoo:snapshot-reject-guard>",
        "Deno.core.setUnhandledPromiseRejectionHandler(function(promise, err){\
           var msg = (err && (err.message || String(err))) || '';\
           if (msg.indexOf('WebAssembly') !== -1) {\
             try { console.error('[snapshot-build] suppressed WASM-unavailable rejection:', msg); } catch (e) {}\
             return true;\
           }\
           return false;\
         });",
    )?;

    eprintln!("Loading {}", abs_path.display());

    let mod_id = runtime
        .load_main_es_module(&main_module)
        .await
        .with_context(|| format!("Failed to load {}", abs_path.display()))?;

    let _result = runtime.mod_evaluate(mod_id);

    // Drive the event loop until the entry registers its restore path (a
    // deserialize main function or a captured server) rather than waiting for a
    // full drain. egg's snapshot mode loads all metadata, cleans up
    // non-serializable resources, registers the deserialize main, then *returns*
    // while timers/handles (logger flush, schedule) may still be alive — exactly
    // as Node's `--build-snapshot` snapshots right after the entry runs, not
    // after a libuv drain. Waiting for a full drain here would hang forever. A
    // wall-clock cap guards an entry that never registers its restore path.
    {
        use std::task::Poll;
        let outcome = tokio::time::timeout(
            std::time::Duration::from_secs(120),
            std::future::poll_fn(|cx| {
                match runtime.poll_event_loop(cx, deno_core::PollEventLoopOptions::default()) {
                    Poll::Ready(r) => Poll::Ready(r),
                    Poll::Pending => {
                        let ready = runtime
                            .execute_script(
                                "<utoo:ready-poll>",
                                "!!globalThis.__utoo_deserialize_main || \
                                 !!(globalThis.__utoo_snapshot_servers && \
                                    globalThis.__utoo_snapshot_servers.length > 0)",
                            )
                            .ok()
                            .map(|v| v.open(runtime.v8_isolate()).is_true())
                            .unwrap_or(false);
                        if ready {
                            Poll::Ready(Ok(()))
                        } else {
                            Poll::Pending
                        }
                    }
                }
            }),
        )
        .await;

        match outcome {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                return Err(anyhow::anyhow!("{e}")).context("Event loop error");
            }
            Err(_elapsed) => anyhow::bail!(
                "Snapshot build timed out (120s): the entry never registered a restore \
                 path. Framework init may be stuck awaiting a connection that snapshot \
                 mode should defer."
            ),
        }
    }

    // Run v8.startupSnapshot serialize callbacks, then verify the entry
    // registered a restore path: either a deserialize main function
    // (v8.startupSnapshot flow, e.g. egg's snapshot lifecycle) or a captured
    // server via the listen()-interception flow.
    let ready: bool = runtime
        .execute_script(
            "<utoo:check>",
            "(function(){\
               var cbs = globalThis.__utoo_serialize_cbs || [];\
               for (var i = 0; i < cbs.length; i++) {\
                 try { cbs[i][0](cbs[i][1]); } catch (e) {}\
               }\
               return !!globalThis.__utoo_deserialize_main ||\
                 !!(globalThis.__utoo_snapshot_servers && globalThis.__utoo_snapshot_servers.length > 0);\
             })()",
        )?
        .open(runtime.v8_isolate())
        .is_true();

    if !ready {
        anyhow::bail!(
            "Snapshot entry did not register a restore path. Call \
             v8.startupSnapshot.setDeserializeMainFunction(fn) (recommended) or \
             server.listen() so the application snapshot has something to resume."
        );
    }

    // Clean up V8-managed properties before snapshot.
    // Delete Error.stackTraceLimit (may have been converted from V8-internal
    // to regular JS property by module code like depd). V8 will re-add it
    // fresh with its default value during Context::FromSnapshot.
    // Also restore Error.captureStackTrace to the original V8 native.
    runtime.execute_script(
        "<utoo:snapshot-cleanup>",
        "delete Error.stackTraceLimit;\
         if (globalThis.__utoo_origCaptureStackTrace) {\
           Error.captureStackTrace = globalThis.__utoo_origCaptureStackTrace;\
           delete globalThis.__utoo_origCaptureStackTrace;\
         }",
    )?;

    eprintln!("Taking snapshot...");
    let snapshot = runtime.snapshot();

    std::fs::write(output_path, &snapshot)
        .with_context(|| format!("Failed to write snapshot to {output_path}"))?;

    eprintln!(
        "Application snapshot written to {} ({:.1} KB)",
        output_path,
        snapshot.len() as f64 / 1024.0
    );

    Ok(())
}

/// Run from an application-level snapshot.
///
/// Restores the V8 heap (including fully initialized framework state), then
/// re-binds the server to the port that was captured during snapshot build.
pub async fn run_from_app_snapshot(snapshot_path: &str, _script_path: &str) -> Result<()> {
    let snapshot_data = std::fs::read(snapshot_path)
        .with_context(|| format!("Failed to read snapshot: {snapshot_path}"))?;

    // Leak to get &'static [u8] (lives for the process lifetime)
    let snapshot_static: &'static [u8] = Box::leak(snapshot_data.into_boxed_slice());

    let mut runtime = JsRuntime::new(deno_core::RuntimeOptions {
        module_loader: Some(std::rc::Rc::new(UtooModuleLoader)),
        extensions: vec![utoo_rt_ext::init()],
        startup_snapshot: Some(snapshot_static),
        ..Default::default()
    });

    // Restore Rust-side state and resume the server
    runtime.execute_script(
        "<utoo:resume>",
        r#"
        Deno.core.setNextTickCallback(globalThis.__utoo_nextTickDrainer);
        globalThis.__utoo_snapshot_mode = false;
        globalThis.__utoo_building_snapshot = false;

        // Restore deno_core's default fatal handling for unhandled rejections
        // (the snapshot build had installed a WASM-unavailable suppressor).
        Deno.core.setUnhandledPromiseRejectionHandler(function(){ return false; });

        // Run v8.startupSnapshot deserialize callbacks.
        var __dcbs = globalThis.__utoo_deserialize_cbs || [];
        for (var __i = 0; __i < __dcbs.length; __i++) {
            try { __dcbs[__i][0](__dcbs[__i][1]); } catch (e) {}
        }

        if (typeof globalThis.__utoo_deserialize_main === 'function') {
            // v8.startupSnapshot flow: the entry registered a deserialize main
            // function that resumes the framework lifecycle and binds the server.
            globalThis.__utoo_deserialize_main(globalThis.__utoo_deserialize_data);
        } else if (globalThis.__utoo_snapshot_servers) {
            // listen()-interception flow: re-bind the captured servers.
            for (const entry of globalThis.__utoo_snapshot_servers) {
                entry.server.listen(entry.port, entry.host);
            }
            globalThis.__utoo_snapshot_servers = null;
        }
        "#,
    )?;

    runtime
        .run_event_loop(deno_core::PollEventLoopOptions::default())
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("Event loop error")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[tokio::test(flavor = "current_thread")]
    async fn run_simple_js() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "const x = 1 + 2;").unwrap();
        let path = f.path().to_str().unwrap();
        // Rename to .js so the loader treats it as JS
        let js_path = format!("{}.js", path);
        std::fs::copy(path, &js_path).unwrap();
        run_script(&js_path).await.unwrap();
        std::fs::remove_file(&js_path).ok();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_ts_with_types() {
        let mut f = tempfile::NamedTempFile::with_suffix(".ts").unwrap();
        write!(f, "const x: number = 42;").unwrap();
        let path = f.path().to_str().unwrap();
        run_script(path).await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_console_log() {
        let mut f = tempfile::NamedTempFile::with_suffix(".js").unwrap();
        write!(f, "console.log('hello', 42);").unwrap();
        let path = f.path().to_str().unwrap();
        run_script(path).await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_relative_import() {
        let dir = tempfile::tempdir().unwrap();
        let lib_path = dir.path().join("lib.ts");
        let app_path = dir.path().join("app.ts");
        std::fs::write(&lib_path, r#"export const name: string = "utoo";"#).unwrap();
        std::fs::write(
            &app_path,
            r#"import { name } from "./lib.ts"; console.log(name);"#,
        )
        .unwrap();
        run_script(app_path.to_str().unwrap()).await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_node_path() {
        let mut f = tempfile::NamedTempFile::with_suffix(".js").unwrap();
        write!(
            f,
            r#"import path from "node:path";
            console.log(path.join("/foo", "bar", "baz.ts"));
            console.log(path.dirname("/foo/bar/baz.ts"));
            console.log(path.basename("/foo/bar/baz.ts"));
            console.log(path.extname("/foo/bar/baz.ts"));
            "#,
        )
        .unwrap();
        let path = f.path().to_str().unwrap();
        run_script(path).await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_node_fs_sync() {
        let dir = tempfile::tempdir().unwrap();
        let test_file = dir.path().join("test.txt");
        let script_path = dir.path().join("test.js");
        std::fs::write(
            &script_path,
            format!(
                r#"import {{ readFileSync, writeFileSync }} from "node:fs";
                writeFileSync("{}", "hello from utoo");
                const data = readFileSync("{}", "utf8");
                console.log(data);
                "#,
                test_file.display(),
                test_file.display(),
            ),
        )
        .unwrap();
        run_script(script_path.to_str().unwrap()).await.unwrap();
        assert_eq!(std::fs::read_to_string(&test_file).unwrap(), "hello from utoo");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_node_fs_promises() {
        let dir = tempfile::tempdir().unwrap();
        let test_file = dir.path().join("test_async.txt");
        let script_path = dir.path().join("test.js");
        std::fs::write(
            &script_path,
            format!(
                r#"import {{ readFile, writeFile }} from "node:fs/promises";
                await writeFile("{}", "async hello");
                const data = await readFile("{}", "utf8");
                console.log(data);
                "#,
                test_file.display(),
                test_file.display(),
            ),
        )
        .unwrap();
        run_script(script_path.to_str().unwrap()).await.unwrap();
        assert_eq!(std::fs::read_to_string(&test_file).unwrap(), "async hello");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_bare_fs_import() {
        let dir = tempfile::tempdir().unwrap();
        let test_file = dir.path().join("bare.txt");
        let script_path = dir.path().join("test.js");
        std::fs::write(
            &script_path,
            format!(
                r#"import fs from "fs";
                fs.writeFileSync("{}", "bare import");
                const data = fs.readFileSync("{}", "utf8");
                console.log(data);
                "#,
                test_file.display(),
                test_file.display(),
            ),
        )
        .unwrap();
        run_script(script_path.to_str().unwrap()).await.unwrap();
        assert_eq!(std::fs::read_to_string(&test_file).unwrap(), "bare import");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_node_os() {
        let mut f = tempfile::NamedTempFile::with_suffix(".js").unwrap();
        write!(
            f,
            r#"import os from "node:os";
            console.log(os.platform());
            console.log(os.arch());
            console.log(os.type());
            console.log(os.homedir());
            "#,
        )
        .unwrap();
        let path = f.path().to_str().unwrap();
        run_script(path).await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_node_buffer() {
        let mut f = tempfile::NamedTempFile::with_suffix(".js").unwrap();
        write!(
            f,
            r#"import {{ Buffer }} from "node:buffer";
            const buf = Buffer.from("hello", "utf-8");
            console.log(buf.toString("utf-8"));
            console.log(buf.toString("hex"));
            console.log(Buffer.isBuffer(buf));
            "#,
        )
        .unwrap();
        let path = f.path().to_str().unwrap();
        run_script(path).await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_node_url() {
        let mut f = tempfile::NamedTempFile::with_suffix(".js").unwrap();
        write!(
            f,
            r#"import {{ URL }} from "node:url";
            const u = new URL("https://example.com/path?q=1");
            console.log(u.hostname);
            console.log(u.pathname);
            "#,
        )
        .unwrap();
        let path = f.path().to_str().unwrap();
        run_script(path).await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_node_fs_callback() {
        let dir = tempfile::tempdir().unwrap();
        let test_file = dir.path().join("cb.txt");
        let script_path = dir.path().join("test.js");
        std::fs::write(&test_file, "callback data").unwrap();
        std::fs::write(
            &script_path,
            format!(
                r#"import fs from "node:fs";
                fs.readFile("{}", "utf8", (err, data) => {{
                    if (err) throw err;
                    console.log(data);
                }});
                "#,
                test_file.display(),
            ),
        )
        .unwrap();
        run_script(script_path.to_str().unwrap()).await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_ts_interface_stripping() {
        let mut f = tempfile::NamedTempFile::with_suffix(".ts").unwrap();
        write!(
            f,
            r#"
            interface User {{ name: string; age: number }}
            const user: User = {{ name: "Alice", age: 30 }};
            console.log(user.name);
            "#,
        )
        .unwrap();
        let path = f.path().to_str().unwrap();
        run_script(path).await.unwrap();
    }

    // -----------------------------------------------------------------------
    // CJS / require() tests
    // -----------------------------------------------------------------------

    #[tokio::test(flavor = "current_thread")]
    async fn cjs_esm_imports_cjs_file() {
        let dir = tempfile::tempdir().unwrap();
        // Mark directory as ESM so the entry point is treated as ESM
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"type": "module"}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("lib.cjs"),
            "module.exports = { x: 42 };",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("test.js"),
            r#"import lib from "./lib.cjs";
            console.log(lib.x);
            if (lib.x !== 42) throw new Error("expected 42");
            "#,
        )
        .unwrap();
        run_script(dir.path().join("test.js").to_str().unwrap())
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cjs_require_builtin() {
        let dir = tempfile::tempdir().unwrap();
        // No package.json -> .js defaults to CJS
        std::fs::write(
            dir.path().join("test.js"),
            r#"const path = require("path");
            const result = path.join("a", "b");
            console.log(result);
            if (result !== "a/b") throw new Error("expected a/b");
            "#,
        )
        .unwrap();
        run_script(dir.path().join("test.js").to_str().unwrap())
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cjs_require_chain() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("c.js"),
            "module.exports = { value: 'from_c' };",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("b.js"),
            r#"const c = require("./c");
            module.exports = { value: "from_b+" + c.value };
            "#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("a.js"),
            r#"const b = require("./b");
            console.log(b.value);
            if (b.value !== "from_b+from_c") throw new Error("chain failed");
            "#,
        )
        .unwrap();
        run_script(dir.path().join("a.js").to_str().unwrap())
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cjs_circular_require() {
        let dir = tempfile::tempdir().unwrap();
        // a requires b, b requires a (gets partial exports)
        std::fs::write(
            dir.path().join("a.js"),
            r#"exports.fromA = "hello";
            const b = require("./b");
            exports.fromB = b.fromB;
            "#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("b.js"),
            r#"const a = require("./a");
            exports.fromB = "world";
            exports.gotFromA = a.fromA;
            "#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("main.js"),
            r#"const a = require("./a");
            console.log(a.fromA, a.fromB);
            if (a.fromA !== "hello") throw new Error("expected hello");
            if (a.fromB !== "world") throw new Error("expected world");
            "#,
        )
        .unwrap();
        run_script(dir.path().join("main.js").to_str().unwrap())
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cjs_require_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("data.json"),
            r#"{"name": "test", "version": "1.0.0"}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("test.js"),
            r#"const data = require("./data.json");
            console.log(data.name, data.version);
            if (data.name !== "test") throw new Error("expected test");
            "#,
        )
        .unwrap();
        run_script(dir.path().join("test.js").to_str().unwrap())
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cjs_node_modules_package() {
        let dir = tempfile::tempdir().unwrap();
        // Set up a fake npm package
        let pkg_dir = dir.path().join("node_modules/mypkg");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join("package.json"),
            r#"{"name": "mypkg", "main": "./lib.js"}"#,
        )
        .unwrap();
        std::fs::write(
            pkg_dir.join("lib.js"),
            "module.exports = { hello: 'from mypkg' };",
        )
        .unwrap();
        // ESM entry point imports the CJS package
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"type": "module"}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("test.js"),
            r#"import mypkg from "mypkg";
            console.log(mypkg.hello);
            if (mypkg.hello !== "from mypkg") throw new Error("expected from mypkg");
            "#,
        )
        .unwrap();
        run_script(dir.path().join("test.js").to_str().unwrap())
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cjs_require_typescript() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("lib.ts"),
            r#"const greeting: string = "hello ts";
            module.exports = { greeting };
            "#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("test.js"),
            r#"const lib = require("./lib.ts");
            console.log(lib.greeting);
            if (lib.greeting !== "hello ts") throw new Error("expected hello ts");
            "#,
        )
        .unwrap();
        run_script(dir.path().join("test.js").to_str().unwrap())
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cjs_require_node_builtin_with_prefix() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("test.js"),
            r#"const fs = require("node:fs");
            const path = require("node:path");
            const result = path.basename("/foo/bar/baz.txt");
            console.log(result);
            if (result !== "baz.txt") throw new Error("expected baz.txt");
            "#,
        )
        .unwrap();
        run_script(dir.path().join("test.js").to_str().unwrap())
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_timers() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"type": "module"}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("test.js"),
            r#"
            let count = 0;
            setTimeout(() => { count++; }, 10);
            setTimeout((a) => { count++; if (a !== "x") throw new Error("bad arg"); }, 20, "x");
            const cid = setTimeout(() => { throw new Error("should be cancelled"); }, 5);
            clearTimeout(cid);
            let ticks = 0;
            const iv = setInterval(() => {
                ticks++;
                if (ticks >= 2) { clearInterval(iv); count++; }
            }, 15);
            process.nextTick(() => { count++; });
            queueMicrotask(() => { count++; });
            setTimeout(() => {
                if (count !== 5) throw new Error("expected 5, got " + count);
            }, 200);
            "#,
        )
        .unwrap();
        run_script(dir.path().join("test.js").to_str().unwrap())
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn run_crypto() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"type": "module"}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("test.js"),
            r#"
            import crypto from "node:crypto";
            const h = crypto.createHash("sha256").update("hello").digest("hex");
            if (h !== "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
                throw new Error("sha256 fail: " + h);
            const hm = crypto.createHmac("sha256", "key").update("data").digest("hex");
            if (typeof hm !== "string" || hm.length !== 64) throw new Error("hmac fail");
            const r = crypto.randomBytes(32);
            if (r.length !== 32) throw new Error("randomBytes fail");
            const uuid = crypto.randomUUID();
            if (uuid.length !== 36) throw new Error("uuid fail");
            "#,
        )
        .unwrap();
        run_script(dir.path().join("test.js").to_str().unwrap())
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cjs_process_cwd_env() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"type": "module"}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("test.js"),
            r#"
            const cwd = process.cwd();
            console.log("cwd:", cwd);
            if (typeof cwd !== "string" || cwd.length === 0) throw new Error("bad cwd");
            if (typeof process.env !== "object") throw new Error("bad env");
            if (process.platform !== "darwin" && process.platform !== "linux")
                throw new Error("bad platform: " + process.platform);
            console.log("process ok");
            "#,
        )
        .unwrap();
        run_script(dir.path().join("test.js").to_str().unwrap())
            .await
            .unwrap();
    }
}
