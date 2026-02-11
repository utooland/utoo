use deno_core::op2;

#[op2(fast)]
pub fn op_console_log(#[string] msg: &str) {
    println!("{msg}");
}

#[op2(fast)]
pub fn op_console_warn(#[string] msg: &str) {
    eprintln!("{msg}");
}

#[op2(fast)]
pub fn op_console_error(#[string] msg: &str) {
    eprintln!("{msg}");
}
