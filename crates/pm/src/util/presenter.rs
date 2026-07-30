//! Shared human/JSON result renderer.

use std::io::{self, Write};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::error::ErrorKind;
use crate::model::cli_output::{
    ErrorDetails, ErrorOutput, FailureEnvelope, PartialResult, SCHEMA_VERSION, SuccessEnvelope,
};

use super::invocation;

pub struct ErrorReport<'a> {
    pub command: Option<&'a str>,
    pub subcommand: Option<&'a str>,
    pub category: ErrorKind,
    pub code: &'a str,
    pub exit_code: u8,
    pub message: &'a str,
    pub causes: &'a [String],
    pub suggestion: Option<&'a str>,
    pub partial_result: Option<&'a PartialResult>,
    pub details: Option<&'a ErrorDetails>,
    pub log_path: Option<String>,
}

pub fn write<T>(
    writer: &mut impl Write,
    command: &str,
    subcommand: Option<&str>,
    result: &T,
) -> Result<()>
where
    T: Serialize,
{
    let output = SuccessEnvelope {
        schema_version: SCHEMA_VERSION,
        command: command.to_string(),
        subcommand: subcommand.map(str::to_string),
        ok: true,
        duration_ms: invocation::duration_ms(),
        result,
    };
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
    write(&mut stdout, command, invocation::subcommand(), result)
}

pub fn write_error(writer: &mut impl Write, report: &ErrorReport<'_>) -> Result<()> {
    let output = FailureEnvelope {
        schema_version: SCHEMA_VERSION,
        command: report.command.map(str::to_string),
        subcommand: report.subcommand.map(str::to_string),
        ok: false,
        duration_ms: invocation::duration_ms(),
        error: ErrorOutput {
            category: report.category,
            code: report.code.to_string(),
            exit_code: report.exit_code,
            message: report.message.to_string(),
            causes: report.causes.to_vec(),
            suggestion: report.suggestion.map(str::to_string),
            partial_result: report.partial_result.cloned(),
            details: report.details.cloned(),
            log_path: report.log_path.clone(),
        },
    };

    serde_json::to_writer(&mut *writer, &output).context("Failed to write JSON error")?;
    writer.write_all(b"\n")?;
    Ok(())
}
