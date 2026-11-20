use std::{
    env,
    ffi::{OsStr, OsString},
    os::windows::process::CommandExt,
    path::Path,
    process::Command,
};

/// Keep the manifest script as shell syntax. Only forwarded CLI arguments
/// are literals and need escaping. cmd reads a raw command line after /c.
pub(super) async fn command(
    shell: Option<OsString>,
    script: &str,
    args: &[&str],
    cwd: &Path,
    path: &str,
) -> Command {
    let shell = shell
        .or_else(|| env::var_os("ComSpec").filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "cmd.exe".into());
    let mut command = Command::new(&shell);
    let is_cmd = Path::new(&shell).file_name().is_some_and(|name| {
        name.eq_ignore_ascii_case("cmd") || name.eq_ignore_ascii_case("cmd.exe")
    });
    let mut line = script.to_owned();
    if is_cmd {
        let batch = !args.is_empty() && starts_with_batch_file(script, cwd, path).await;
        for arg in args {
            line.push(' ');
            let escaped = escape_cmd_arg(arg);
            if batch && !arg.is_empty() {
                line.push_str(&escape_meta(&escaped));
            } else {
                line.push_str(&escaped);
            }
        }
        command
            .args(["/d", "/s", "/c"])
            .raw_arg(format!("\"{line}\""));
    } else {
        for arg in args {
            line.push_str(" '");
            line.push_str(&arg.replace('\'', "'\\''"));
            line.push('\'');
        }
        command.arg("-c").arg(line);
    }
    command
}

// cmd reparses arguments once more when a package binary is a .cmd shim.
async fn starts_with_batch_file(script: &str, cwd: &Path, path: &str) -> bool {
    let mut token = String::new();
    let mut quoted = false;
    let mut chars = script.trim_start().chars();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => quoted = !quoted,
            '^' if !quoted => token.extend(chars.next()),
            ch if !quoted && ch.is_whitespace() => break,
            ch => token.push(ch),
        }
    }
    let program = Path::new(&token);
    let is_batch = |ext: &OsStr| ext.eq_ignore_ascii_case("bat") || ext.eq_ignore_ascii_case("cmd");
    if let Some(ext) = program.extension()
        && is_batch(ext)
    {
        return true;
    }
    if token.is_empty() {
        return false;
    }
    let extensions = env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    let has_path = program.components().count() > 1;
    for dir in std::iter::once(cwd.to_owned()).chain(env::split_paths(path)) {
        let base = cwd.join(dir).join(program);
        if program.extension().is_some() && tokio::fs::metadata(&base).await.is_ok() {
            return false;
        }
        for ext in extensions.split(';').filter(|ext| !ext.is_empty()) {
            let mut candidate = base.as_os_str().to_owned();
            candidate.push(ext);
            if tokio::fs::metadata(candidate)
                .await
                .is_ok_and(|m| m.is_file())
            {
                return is_batch(OsStr::new(ext.trim_start_matches('.')));
            }
        }
        if has_path {
            break;
        }
    }
    false
}

fn escape_meta(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if " !%^&()<>|\"".contains(ch) {
            out.push('^');
        }
        out.push(ch);
    }
    out
}

// Quote for the executable's argv parser, then escape cmd's metacharacters.
fn escape_cmd_arg(arg: &str) -> String {
    if arg.is_empty() {
        return "\"\"".into();
    }
    if !arg.contains([' ', '\t', '\n', '\u{b}', '"']) {
        return escape_meta(arg);
    }
    let mut out = String::from("\"");
    let mut slashes = 0;
    for ch in arg.chars() {
        if ch == '\\' {
            slashes += 1;
            continue;
        }
        out.extend(std::iter::repeat_n(
            '\\',
            if ch == '"' { 2 * slashes + 1 } else { slashes },
        ));
        slashes = 0;
        out.push(ch);
    }
    out.extend(std::iter::repeat_n('\\', slashes * 2));
    out.push('"');
    escape_meta(&out)
}
