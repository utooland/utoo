//! Shared human/JSON result renderer.

use std::io::{self, Write};

use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::{Map, Value};

use super::invocation;

pub const SCHEMA_VERSION: u8 = 1;

pub fn emit<T, F>(command: &str, result: &T, human: F) -> Result<()>
where
    T: Serialize,
    F: FnOnce() -> Result<()>,
{
    if !invocation::json() {
        return human();
    }

    let value = serde_json::to_value(result).context("Failed to serialize command result")?;
    let mut output = Map::new();
    output.insert("schemaVersion".to_string(), Value::from(SCHEMA_VERSION));
    output.insert("command".to_string(), Value::from(command));
    match value {
        Value::Object(fields) => output.extend(fields),
        value => {
            output.insert("result".to_string(), value);
        }
    }

    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, &output).context("Failed to write JSON result")?;
    stdout.write_all(b"\n")?;
    Ok(())
}
