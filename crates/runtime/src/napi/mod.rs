// N-API implementation for utoo-runtime.
// Ported from Deno's ext/napi/.

#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]
#![allow(clippy::undocumented_unsafe_blocks)]

pub mod js_native_api;
pub mod node_api;
pub mod util;
pub mod uv;

use core::ptr::NonNull;
use std::cell::Cell;
use std::cell::RefCell;
pub use std::ffi::CStr;
pub use std::os::raw::c_char;
pub use std::os::raw::c_void;
use std::rc::Rc;
use std::thread_local;

use deno_core::ExternalOpsTracker;
use deno_core::V8CrossThreadTaskSpawner;
pub use deno_core::v8;
pub use value::napi_value;

pub mod function;
mod value;

// ---- napi_status constants ----

pub type napi_status = i32;
pub type napi_env = *mut c_void;
pub type napi_callback_info = *mut c_void;
pub type napi_deferred = *mut c_void;
pub type napi_ref = *mut c_void;
pub type napi_threadsafe_function = *mut c_void;
pub type napi_handle_scope = *mut c_void;
pub type napi_callback_scope = *mut c_void;
pub type napi_escapable_handle_scope = *mut c_void;
pub type napi_async_cleanup_hook_handle = *mut c_void;
pub type napi_async_work = *mut c_void;
pub type napi_async_context = *mut c_void;

pub const napi_ok: napi_status = 0;
pub const napi_invalid_arg: napi_status = 1;
pub const napi_object_expected: napi_status = 2;
pub const napi_string_expected: napi_status = 3;
pub const napi_name_expected: napi_status = 4;
pub const napi_function_expected: napi_status = 5;
pub const napi_number_expected: napi_status = 6;
pub const napi_boolean_expected: napi_status = 7;
pub const napi_array_expected: napi_status = 8;
pub const napi_generic_failure: napi_status = 9;
pub const napi_pending_exception: napi_status = 10;
pub const napi_cancelled: napi_status = 11;
pub const napi_escape_called_twice: napi_status = 12;
pub const napi_handle_scope_mismatch: napi_status = 13;
pub const napi_callback_scope_mismatch: napi_status = 14;
pub const napi_queue_full: napi_status = 15;
pub const napi_closing: napi_status = 16;
pub const napi_bigint_expected: napi_status = 17;
pub const napi_date_expected: napi_status = 18;
pub const napi_arraybuffer_expected: napi_status = 19;
pub const napi_detachable_arraybuffer_expected: napi_status = 20;
pub const napi_would_deadlock: napi_status = 21;
pub const napi_no_external_buffers_allowed: napi_status = 22;
pub const napi_cannot_run_js: napi_status = 23;

pub static ERROR_MESSAGES: &[&CStr] = &[
    c"",
    c"Invalid argument",
    c"An object was expected",
    c"A string was expected",
    c"A string or symbol was expected",
    c"A function was expected",
    c"A number was expected",
    c"A boolean was expected",
    c"An array was expected",
    c"Unknown failure",
    c"An exception is pending",
    c"The async work item was cancelled",
    c"napi_escape_handle already called on scope",
    c"Invalid handle scope usage",
    c"Invalid callback scope usage",
    c"Thread-safe function queue is full",
    c"Thread-safe function handle is closing",
    c"A bigint was expected",
    c"A date was expected",
    c"An arraybuffer was expected",
    c"A detachable arraybuffer was expected",
    c"Main thread would deadlock",
    c"External buffers are not allowed",
    c"Cannot run JavaScript",
];

pub const NAPI_AUTO_LENGTH: usize = usize::MAX;

thread_local! {
    pub static MODULE_TO_REGISTER: RefCell<Option<*const NapiModule>> = const { RefCell::new(None) };
}

pub type napi_addon_register_func =
    unsafe extern "C" fn(env: napi_env, exports: napi_value) -> napi_value;
pub type napi_register_module_v1 =
    unsafe extern "C" fn(env: napi_env, exports: napi_value) -> napi_value;

#[repr(C)]
#[derive(Clone)]
pub struct NapiModule {
    pub nm_version: i32,
    pub nm_flags: u32,
    nm_filename: *const c_char,
    pub nm_register_func: napi_addon_register_func,
    nm_modname: *const c_char,
    nm_priv: *mut c_void,
    reserved: [*mut c_void; 4],
}

// ---- Value type constants ----

pub type napi_valuetype = i32;

pub const napi_undefined: napi_valuetype = 0;
pub const napi_null: napi_valuetype = 1;
pub const napi_boolean: napi_valuetype = 2;
pub const napi_number: napi_valuetype = 3;
pub const napi_string: napi_valuetype = 4;
pub const napi_symbol: napi_valuetype = 5;
pub const napi_object: napi_valuetype = 6;
pub const napi_function: napi_valuetype = 7;
pub const napi_external: napi_valuetype = 8;
pub const napi_bigint: napi_valuetype = 9;

// ---- Threadsafe function modes ----

pub type napi_threadsafe_function_release_mode = i32;
pub const napi_tsfn_release: napi_threadsafe_function_release_mode = 0;
pub const napi_tsfn_abort: napi_threadsafe_function_release_mode = 1;

pub type napi_threadsafe_function_call_mode = i32;
pub const napi_tsfn_nonblocking: napi_threadsafe_function_call_mode = 0;
pub const napi_tsfn_blocking: napi_threadsafe_function_call_mode = 1;

// ---- Key collection constants ----

pub type napi_key_collection_mode = i32;
pub const napi_key_include_prototypes: napi_key_collection_mode = 0;
pub const napi_key_own_only: napi_key_collection_mode = 1;

pub type napi_key_filter = i32;
pub const napi_key_all_properties: napi_key_filter = 0;
pub const napi_key_writable: napi_key_filter = 1;
pub const napi_key_enumerable: napi_key_filter = 1 << 1;
pub const napi_key_configurable: napi_key_filter = 1 << 2;
pub const napi_key_skip_strings: napi_key_filter = 1 << 3;
pub const napi_key_skip_symbols: napi_key_filter = 1 << 4;

pub type napi_key_conversion = i32;
pub const napi_key_keep_numbers: napi_key_conversion = 0;
pub const napi_key_numbers_to_strings: napi_key_conversion = 1;

// ---- TypedArray type constants ----

pub type napi_typedarray_type = i32;
pub const napi_int8_array: napi_typedarray_type = 0;
pub const napi_uint8_array: napi_typedarray_type = 1;
pub const napi_uint8_clamped_array: napi_typedarray_type = 2;
pub const napi_int16_array: napi_typedarray_type = 3;
pub const napi_uint16_array: napi_typedarray_type = 4;
pub const napi_int32_array: napi_typedarray_type = 5;
pub const napi_uint32_array: napi_typedarray_type = 6;
pub const napi_float32_array: napi_typedarray_type = 7;
pub const napi_float64_array: napi_typedarray_type = 8;
pub const napi_bigint64_array: napi_typedarray_type = 9;
pub const napi_biguint64_array: napi_typedarray_type = 10;

// ---- Type tag ----

#[repr(C)]
#[derive(Clone, Copy, PartialEq)]
pub struct napi_type_tag {
    pub lower: u64,
    pub upper: u64,
}

// ---- Callback types ----

pub type napi_callback = unsafe extern "C" fn(
    env: napi_env,
    info: napi_callback_info,
) -> napi_value<'static>;

pub type napi_finalize = unsafe extern "C" fn(
    env: napi_env,
    data: *mut c_void,
    finalize_hint: *mut c_void,
);

pub type napi_async_execute_callback =
    unsafe extern "C" fn(env: napi_env, data: *mut c_void);

pub type napi_async_complete_callback =
    unsafe extern "C" fn(env: napi_env, status: napi_status, data: *mut c_void);

pub type napi_threadsafe_function_call_js = unsafe extern "C" fn(
    env: napi_env,
    js_callback: napi_value,
    context: *mut c_void,
    data: *mut c_void,
);

pub type napi_async_cleanup_hook = unsafe extern "C" fn(
    handle: napi_async_cleanup_hook_handle,
    data: *mut c_void,
);

pub type napi_cleanup_hook = unsafe extern "C" fn(data: *mut c_void);

// ---- Property attributes ----

pub type napi_property_attributes = i32;
pub const napi_default: napi_property_attributes = 0;
pub const napi_writable: napi_property_attributes = 1 << 0;
pub const napi_enumerable: napi_property_attributes = 1 << 1;
pub const napi_configurable: napi_property_attributes = 1 << 2;
pub const napi_static: napi_property_attributes = 1 << 10;
pub const napi_default_method: napi_property_attributes = napi_writable | napi_configurable;
pub const napi_default_jsproperty: napi_property_attributes =
    napi_enumerable | napi_configurable | napi_writable;

// ---- Property descriptor ----

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct napi_property_descriptor<'a> {
    pub utf8name: *const c_char,
    pub name: napi_value<'a>,
    pub method: Option<napi_callback>,
    pub getter: Option<napi_callback>,
    pub setter: Option<napi_callback>,
    pub value: napi_value<'a>,
    pub attributes: napi_property_attributes,
    pub data: *mut c_void,
}

// ---- Error info ----

#[repr(C)]
#[derive(Debug)]
pub struct napi_extended_error_info {
    pub error_message: *const c_char,
    pub engine_reserved: *mut c_void,
    pub engine_error_code: i32,
    pub error_code: Cell<napi_status>,
}

// ---- Node version ----

#[repr(C)]
#[derive(Debug)]
pub struct napi_node_version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub release: *const c_char,
}

// ---- NapiState ----

pub struct NapiState {
    pub env_cleanup_hooks: Rc<RefCell<Vec<(napi_cleanup_hook, *mut c_void)>>>,
}

impl Drop for NapiState {
    fn drop(&mut self) {
        let hooks = {
            let h = self.env_cleanup_hooks.borrow_mut();
            h.clone()
        };

        // Hooks are supposed to be run in LIFO order
        let hooks_to_run = hooks.into_iter().rev();

        for hook in hooks_to_run {
            if !self
                .env_cleanup_hooks
                .borrow()
                .iter()
                .any(|pair| std::ptr::fn_addr_eq(pair.0, hook.0) && pair.1 == hook.1)
            {
                continue;
            }

            unsafe {
                (hook.0)(hook.1);
            }

            {
                self.env_cleanup_hooks.borrow_mut().retain(|pair| {
                    !(std::ptr::fn_addr_eq(pair.0, hook.0) && pair.1 == hook.1)
                });
            }
        }
    }
}

// ---- Instance data ----

#[repr(C)]
#[derive(Debug)]
pub struct InstanceData {
    pub data: *mut c_void,
    pub finalize_cb: Option<napi_finalize>,
    pub finalize_hint: *mut c_void,
}

// ---- EnvShared ----

#[repr(C)]
#[derive(Debug)]
pub struct EnvShared {
    pub instance_data: Option<InstanceData>,
    pub napi_wrap: v8::Global<v8::Private>,
    pub type_tag: v8::Global<v8::Private>,
    pub finalize: Option<napi_finalize>,
    pub finalize_hint: *mut c_void,
    pub filename: String,
}

impl EnvShared {
    pub fn new(
        napi_wrap: v8::Global<v8::Private>,
        type_tag: v8::Global<v8::Private>,
        filename: String,
    ) -> Self {
        Self {
            instance_data: None,
            napi_wrap,
            type_tag,
            finalize: None,
            finalize_hint: std::ptr::null_mut(),
            filename,
        }
    }
}

// ---- Env ----

#[repr(C)]
pub struct Env {
    context: NonNull<v8::Context>,
    pub isolate_ptr: v8::UnsafeRawIsolatePtr,
    pub open_handle_scopes: usize,
    pub shared: *mut EnvShared,
    pub async_work_sender: V8CrossThreadTaskSpawner,
    cleanup_hooks: Rc<RefCell<Vec<(napi_cleanup_hook, *mut c_void)>>>,
    external_ops_tracker: ExternalOpsTracker,
    pub last_error: napi_extended_error_info,
    pub last_exception: Option<v8::Global<v8::Value>>,
    pub global: v8::Global<v8::Object>,
    pub create_buffer: v8::Global<v8::Function>,
    pub report_error: v8::Global<v8::Function>,
}

unsafe impl Send for Env {}
unsafe impl Sync for Env {}

impl Env {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        isolate_ptr: v8::UnsafeRawIsolatePtr,
        context: v8::Global<v8::Context>,
        global: v8::Global<v8::Object>,
        create_buffer: v8::Global<v8::Function>,
        report_error: v8::Global<v8::Function>,
        sender: V8CrossThreadTaskSpawner,
        cleanup_hooks: Rc<RefCell<Vec<(napi_cleanup_hook, *mut c_void)>>>,
        external_ops_tracker: ExternalOpsTracker,
    ) -> Self {
        Self {
            isolate_ptr,
            context: context.into_raw(),
            global,
            create_buffer,
            report_error,
            shared: std::ptr::null_mut(),
            open_handle_scopes: 0,
            async_work_sender: sender,
            cleanup_hooks,
            external_ops_tracker,
            last_error: napi_extended_error_info {
                error_message: std::ptr::null(),
                engine_reserved: std::ptr::null_mut(),
                engine_error_code: 0,
                error_code: Cell::new(napi_ok),
            },
            last_exception: None,
        }
    }

    pub fn shared(&self) -> &EnvShared {
        unsafe { &*self.shared }
    }

    pub fn shared_mut(&mut self) -> &mut EnvShared {
        unsafe { &mut *self.shared }
    }

    pub fn add_async_work(&mut self, async_work: impl FnOnce() + Send + 'static) {
        self.async_work_sender.spawn(|_| async_work());
    }

    #[inline]
    pub fn isolate(&mut self) -> &mut v8::Isolate {
        unsafe {
            v8::Isolate::ref_from_raw_isolate_ptr_mut_unchecked(&mut self.isolate_ptr)
        }
    }

    pub fn context<'s>(&'s self) -> v8::Local<'s, v8::Context> {
        unsafe {
            std::mem::transmute::<NonNull<v8::Context>, v8::Local<v8::Context>>(self.context)
        }
    }

    pub fn threadsafe_function_ref(&mut self) {
        self.external_ops_tracker.ref_op();
    }

    pub fn threadsafe_function_unref(&mut self) {
        self.external_ops_tracker.unref_op();
    }

    pub fn add_cleanup_hook(&mut self, hook: napi_cleanup_hook, data: *mut c_void) {
        let mut hooks = self.cleanup_hooks.borrow_mut();
        if hooks
            .iter()
            .any(|pair| std::ptr::fn_addr_eq(pair.0, hook) && pair.1 == data)
        {
            panic!("Cannot register cleanup hook with same data twice");
        }
        hooks.push((hook, data));
    }

    pub fn remove_cleanup_hook(&mut self, hook: napi_cleanup_hook, data: *mut c_void) {
        let mut hooks = self.cleanup_hooks.borrow_mut();
        match hooks
            .iter()
            .rposition(|&pair| std::ptr::fn_addr_eq(pair.0, hook) && pair.1 == data)
        {
            Some(index) => {
                hooks.remove(index);
            }
            None => panic!("Cannot remove cleanup hook which was not registered"),
        }
    }
}
