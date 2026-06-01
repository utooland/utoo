use std::sync::Arc;

use anyhow::Result;
use serde::Serialize;
use turbo_tasks::{Completion, Effects, OperationVc, ReadRef, Vc, VcValueType, take_effects};
use turbopack_core::issue::{
    CollectibleIssuesExt, IssueFilter, IssueSeverity, PlainIssue, StyledString,
};

use crate::endpoint::{
    Endpoint, EndpointIssuesAndDiags, OptionEndpoint, endpoint_server_changed_operation,
};

#[turbo_tasks::function(operation, root)]
pub async fn subscribe_issues_and_diags_operation(
    endpoint_op: OperationVc<OptionEndpoint>,
    should_include_issues: bool,
) -> Result<Vc<EndpointIssuesAndDiags>> {
    let changed_op = endpoint_server_changed_operation(endpoint_op);

    if should_include_issues {
        let (changed_value, issues, effects) =
            strongly_consistent_catch_collectables(changed_op).await?;
        Ok(EndpointIssuesAndDiags {
            changed: changed_value,
            issues,
            effects,
        }
        .cell())
    } else {
        let changed_value = changed_op.read_strongly_consistent().await?;
        Ok(EndpointIssuesAndDiags {
            changed: Some(changed_value),
            issues: Arc::new(vec![]),
            effects: Arc::new(Effects::default()),
        }
        .cell())
    }
}

#[turbo_tasks::function(operation, root)]
pub async fn endpoint_client_changed_operation(
    endpoint: OperationVc<OptionEndpoint>,
) -> Result<Vc<Completion>> {
    Ok(if let Some(endpoint) = *endpoint.connect().await? {
        endpoint.client_changed()
    } else {
        Completion::new()
    })
}

// Await the source and return fatal issues if there are any, otherwise
// propagate any actual error results.
pub async fn strongly_consistent_catch_collectables<R: VcValueType + Send>(
    source_op: OperationVc<R>,
) -> Result<(
    Option<ReadRef<R>>,
    Arc<Vec<ReadRef<PlainIssue>>>,
    Arc<Effects>,
)> {
    let result = source_op.read_strongly_consistent().await;
    let issues = get_issues(source_op).await?;
    let effects = Arc::new(take_effects(source_op).await?);

    let result = if result.is_err() && issues.iter().any(|i| i.severity <= IssueSeverity::Error) {
        None
    } else {
        Some(result?)
    };

    Ok((result, issues, effects))
}

pub async fn get_issues<T: Send>(source: OperationVc<T>) -> Result<Arc<Vec<ReadRef<PlainIssue>>>> {
    let issues = source.peek_issues();
    Ok(Arc::new(
        issues.get_plain_issues(IssueFilter::everything()).await?,
    ))
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StyledStringSerialize<'a> {
    Line {
        value: Vec<StyledStringSerialize<'a>>,
    },
    Stack {
        value: Vec<StyledStringSerialize<'a>>,
    },
    Text {
        value: &'a str,
    },
    Code {
        value: &'a str,
    },
    Strong {
        value: &'a str,
    },
}

impl<'a> From<&'a StyledString> for StyledStringSerialize<'a> {
    fn from(value: &'a StyledString) -> Self {
        match value {
            StyledString::Line(parts) => StyledStringSerialize::Line {
                value: parts.iter().map(|p| p.into()).collect(),
            },
            StyledString::Stack(parts) => StyledStringSerialize::Stack {
                value: parts.iter().map(|p| p.into()).collect(),
            },
            StyledString::Text(string) => StyledStringSerialize::Text { value: string },
            StyledString::Code(string) => StyledStringSerialize::Code { value: string },
            StyledString::Strong(string) => StyledStringSerialize::Strong { value: string },
        }
    }
}
