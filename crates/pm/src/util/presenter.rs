//! Shared human/JSON result renderer.

use std::io::{self, Write};

use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::error::ErrorKind;

use super::invocation;

pub const SCHEMA_VERSION: u8 = 1;

pub struct ErrorReport<'a> {
    pub command: Option<&'a str>,
    pub category: ErrorKind,
    pub code: i32,
    pub message: &'a str,
    pub suggestion: Option<&'a str>,
    pub required_by: Option<&'a [(String, String)]>,
    pub details: Option<&'a Value>,
}

pub fn write<T>(writer: &mut impl Write, command: &str, result: &T) -> Result<()>
where
    T: Serialize,
{
    let value = serde_json::to_value(result).context("Failed to serialize command result")?;
    let mut output = Map::new();
    output.insert("schemaVersion".to_string(), Value::from(SCHEMA_VERSION));
    output.insert("command".to_string(), Value::from(command));
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                anyhow::ensure!(
                    !output.contains_key(&key),
                    "command result uses reserved field `{key}`"
                );
                output.insert(key, value);
            }
        }
        value => {
            output.insert("result".to_string(), value);
        }
    }

    serde_json::to_writer(&mut *writer, &output).context("Failed to write JSON result")?;
    writer.write_all(b"\n")?;
    Ok(())
}

pub fn emit<T, F>(command: &str, result: &T, human: F) -> Result<()>
where
    T: Serialize,
    F: FnOnce() -> Result<()>,
{
    if !invocation::json() {
        return human();
    }

    let mut stdout = io::stdout().lock();
    write(&mut stdout, command, result)
}

pub fn write_error(writer: &mut impl Write, report: &ErrorReport<'_>) -> Result<()> {
    let mut error = Map::new();
    error.insert(
        "category".to_string(),
        serde_json::to_value(report.category)?,
    );
    error.insert("code".to_string(), Value::from(report.code));
    error.insert("message".to_string(), Value::from(report.message));
    if let Some(suggestion) = report.suggestion {
        error.insert("suggestion".to_string(), Value::from(suggestion));
    }
    if let Some(chain) = report.required_by {
        error.insert(
            "requiredBy".to_string(),
            chain
                .iter()
                .map(|(name, version)| {
                    serde_json::json!({
                        "name": name,
                        "version": version,
                    })
                })
                .collect::<Vec<_>>()
                .into(),
        );
    }
    if let Some(details) = report.details {
        error.insert("details".to_string(), details.clone());
    }

    let mut output = Map::new();
    output.insert("schemaVersion".to_string(), Value::from(SCHEMA_VERSION));
    if let Some(command) = report.command {
        output.insert("command".to_string(), Value::from(command));
    }
    output.insert("error".to_string(), Value::Object(error));

    serde_json::to_writer(&mut *writer, &output).context("Failed to write JSON error")?;
    writer.write_all(b"\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_result_fields_reserved_by_the_envelope() {
        let mut output = Vec::new();
        let error = write(
            &mut output,
            "test",
            &serde_json::json!({ "schemaVersion": 99 }),
        )
        .unwrap_err();
        assert!(error.to_string().contains("reserved field"));
    }

    #[test]
    fn error_dependency_chain_stays_structured() {
        let chain = vec![
            ("root".to_string(), "1.0.0".to_string()),
            ("dependency".to_string(), "^2".to_string()),
        ];
        let report = ErrorReport {
            command: Some("install"),
            category: ErrorKind::NotFound,
            code: 4,
            message: "package not found",
            suggestion: None,
            required_by: Some(&chain),
            details: None,
        };
        let mut output = Vec::new();
        write_error(&mut output, &report).unwrap();

        let value: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(value["error"]["requiredBy"][0]["name"], "root");
        assert_eq!(value["error"]["requiredBy"][1]["name"], "dependency");
        assert_eq!(value["error"]["requiredBy"][1]["version"], "^2");
    }
}
