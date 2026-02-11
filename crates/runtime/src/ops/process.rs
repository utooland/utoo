use std::collections::HashMap;

use deno_core::op2;
use deno_error::JsErrorBox;

#[op2(fast)]
pub fn op_exit(code: i32) {
    std::process::exit(code);
}

#[op2]
#[string]
pub fn op_cwd() -> Result<String, JsErrorBox> {
    std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| JsErrorBox::generic(e.to_string()))
}

#[op2]
#[serde]
pub fn op_env_to_object() -> HashMap<String, String> {
    std::env::vars().collect()
}
