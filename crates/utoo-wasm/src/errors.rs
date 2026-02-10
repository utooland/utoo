use std::io;
use wasm_bindgen::prelude::*;

// Helper macro to define constants and their JS getters (exported as functions)
macro_rules! define_error_consts {
    ($($name:ident, $fn_name:ident = $val:expr),* $(,)?) => {
        $(
            // Internal Rust constant
            pub const $name: &str = $val;

            // Exported JS function returning the string
            #[wasm_bindgen(js_name = $name)]
            pub fn $fn_name() -> String {
                $name.to_string()
            }
        )*
    };
}

define_error_consts! {
    ERR_NOT_FOUND, err_not_found = "NotFoundError",
    ERR_KEY_ALREADY_EXISTS, err_key_already_exists = "NoModificationAllowedError",
    ERR_NOT_ALLOWED, err_not_allowed = "NotAllowedError",
    ERR_NO_MODIFICATION_ALLOWED, err_no_modification_allowed = "NoModificationAllowedError",
    ERR_TYPE_MISMATCH, err_type_mismatch = "TypeMismatchError",
    ERR_QUOTA_EXCEEDED, err_quota_exceeded = "QuotaExceededError",
    ERR_INVALID_STATE, err_invalid_state = "InvalidStateError",
    ERR_ABORT, err_abort = "AbortError",
}

pub fn to_js_error<E>(e: E) -> JsError
where
    E: Into<anyhow::Error>,
{
    let err = e.into();

    if let Some(io_err) = err.downcast_ref::<io::Error>() {
        let prefix = match io_err.kind() {
            io::ErrorKind::NotFound => ERR_NOT_FOUND,
            io::ErrorKind::PermissionDenied => ERR_NOT_ALLOWED,
            io::ErrorKind::WouldBlock => ERR_NO_MODIFICATION_ALLOWED,
            io::ErrorKind::InvalidData => ERR_TYPE_MISMATCH,
            io::ErrorKind::StorageFull => ERR_QUOTA_EXCEEDED,
            io::ErrorKind::InvalidInput => ERR_INVALID_STATE,
            io::ErrorKind::Interrupted => ERR_ABORT,
            _ => "",
        };

        if !prefix.is_empty() {
            return JsError::new(&format!("{}: {}", prefix, io_err));
        }
    }

    JsError::new(&format!("{:#?}", err))
}
