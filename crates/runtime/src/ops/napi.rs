use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use deno_core::parking_lot::RwLock;
use deno_core::{op2, OpState, v8};
use deno_error::JsErrorBox;

use crate::napi::{Env, EnvShared, NapiModule, NapiState, MODULE_TO_REGISTER};

unsafe impl Sync for NapiModuleHandle {}
unsafe impl Send for NapiModuleHandle {}

#[derive(Clone, Copy)]
struct NapiModuleHandle(*const NapiModule);

static NAPI_LOADED_MODULES: std::sync::LazyLock<
    RwLock<HashMap<PathBuf, NapiModuleHandle>>,
> = std::sync::LazyLock::new(|| RwLock::new(HashMap::new()));

#[op2(reentrant)]
pub fn op_napi_open<'scope>(
    scope: &mut v8::PinScope<'scope, '_>,
    isolate: &mut v8::Isolate,
    op_state: Rc<RefCell<OpState>>,
    #[string] path: &str,
    global: v8::Local<'scope, v8::Object>,
    create_buffer: v8::Local<'scope, v8::Function>,
    report_error: v8::Local<'scope, v8::Function>,
) -> Result<v8::Local<'scope, v8::Value>, JsErrorBox> {
    // Resolve the path to a canonical file path
    let path_url = url::Url::parse(path)
        .ok()
        .and_then(|u| {
            if u.scheme() == "file" {
                u.to_file_path().ok()
            } else {
                None
            }
        })
        .unwrap_or_else(|| PathBuf::from(path));

    let path = std::fs::canonicalize(&path_url)
        .map_err(|_| JsErrorBox::type_error(format!("Invalid path: {}", path_url.display())))?;

    // Native addon loading requires NAPI symbols to be exported from the binary.
    // Until we properly export napi_* symbols, skip dlopen to avoid dyld crashes.
    // Set UTOO_ENABLE_NATIVE_ADDONS=1 to attempt loading anyway. This check MUST
    // run before any v8::Global is created below: those handles get leaked into a
    // Box::into_raw'd Env and would otherwise persist as un-serializable global
    // handles, breaking V8 startup-snapshot builds (CheckGlobalAndEternalHandles).
    if std::env::var("UTOO_ENABLE_NATIVE_ADDONS").is_err() {
        return Err(JsErrorBox::type_error(format!(
            "Native addon loading is not yet supported: {}",
            path.display()
        )));
    }

    // Create exports object
    let exports = v8::Object::new(scope);

    // Get the NapiState from op_state
    let napi_state = {
        let state = op_state.borrow();
        state.borrow::<NapiState>().env_cleanup_hooks.clone()
    };

    let isolate_ptr = unsafe { isolate.as_raw_isolate_ptr() };
    let context = scope.get_current_context();
    let context_global = v8::Global::new(scope, context);
    let global_global = v8::Global::new(scope, global);
    let create_buffer_global = v8::Global::new(scope, create_buffer);
    let report_error_global = v8::Global::new(scope, report_error);

    let async_work_sender = {
        let state = op_state.borrow();
        state.borrow::<deno_core::V8CrossThreadTaskSpawner>().clone()
    };

    let external_ops_tracker = {
        let state = op_state.borrow();
        state.external_ops_tracker.clone()
    };

    let env = Env::new(
        isolate_ptr,
        context_global,
        global_global,
        create_buffer_global,
        report_error_global,
        async_work_sender,
        napi_state,
        external_ops_tracker,
    );

    let env_ptr = Box::into_raw(Box::new(env));

    // Create and set up EnvShared
    let napi_wrap_name = v8::String::new(scope, "napi_wrap").unwrap();
    let napi_wrap = v8::Private::new(scope, Some(napi_wrap_name));
    let napi_wrap_global = v8::Global::new(scope, napi_wrap);

    let type_tag_name = v8::String::new(scope, "type_tag").unwrap();
    let type_tag = v8::Private::new(scope, Some(type_tag_name));
    let type_tag_global = v8::Global::new(scope, type_tag);

    let filename = path.to_string_lossy().to_string();
    let env_shared = EnvShared::new(napi_wrap_global, type_tag_global, filename);
    let env_shared_ptr = Box::into_raw(Box::new(env_shared));
    unsafe {
        (*env_ptr).shared = env_shared_ptr;
    }

    // Check the loaded modules cache first
    {
        let loaded = NAPI_LOADED_MODULES.read();
        if let Some(handle) = loaded.get(&path) {
            let module = unsafe { &*handle.0 };
            let exports_napi = exports.into();
            let result = unsafe {
                (module.nm_register_func)(env_ptr as _, exports_napi)
            };
            match *result {
                Some(v) => return Ok(v),
                None => return Ok(exports.into()),
            }
        }
    }

    // dlopen the .node file (only reached when UTOO_ENABLE_NATIVE_ADDONS=1; the
    // unsupported-addon guard ran at the top of this op before any globals).
    let library = unsafe { libloading::Library::new(&path) }
        .map_err(|e| JsErrorBox::type_error(format!("{e}")))?;

    // Check if MODULE_TO_REGISTER was populated (by the addon's constructor
    // calling napi_module_register during dlopen)
    let maybe_module = MODULE_TO_REGISTER.with(|cell| cell.borrow_mut().take());

    if let Some(module_ptr) = maybe_module {
        // Cache the module
        NAPI_LOADED_MODULES.write().insert(
            path.clone(),
            NapiModuleHandle(module_ptr),
        );

        let module = unsafe { &*module_ptr };
        let exports_napi = exports.into();
        let result = unsafe {
            (module.nm_register_func)(env_ptr as _, exports_napi)
        };

        // Prevent the library from being unloaded -- addons cannot be unloaded
        std::mem::forget(library);

        match *result {
            Some(v) => Ok(v),
            None => Ok(exports.into()),
        }
    } else {
        // Fall back to looking up napi_register_module_v1
        let register_func: libloading::Symbol<
            unsafe extern "C" fn(
                env: crate::napi::napi_env,
                exports: crate::napi::napi_value,
            ) -> crate::napi::napi_value,
        > = unsafe { library.get(b"napi_register_module_v1") }
            .map_err(|_| {
                JsErrorBox::type_error(format!(
                    "Unable to find register Node-API module at {}",
                    path.display()
                ))
            })?;

        let exports_napi = exports.into();
        let result = unsafe { register_func(env_ptr as _, exports_napi) };

        // Prevent the library from being unloaded
        std::mem::forget(library);

        match *result {
            Some(v) => Ok(v),
            None => Ok(exports.into()),
        }
    }
}
