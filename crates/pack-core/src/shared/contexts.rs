use std::str::FromStr;

use anyhow::Result;
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{FxIndexMap, Vc};
use turbo_tasks_env::EnvMap;
use turbopack_core::{
    compile_time_info::{
        CompileTimeDefineValue, CompileTimeDefines, DefinableNameSegment, FreeVarReference,
        FreeVarReferences,
    },
    free_var_references,
};

use crate::config::{ProviderConfig, ProviderConfigValue};

fn parse_define_value(value: &RcStr) -> CompileTimeDefineValue {
    match serde_json::Value::from_str(value) {
        Ok(value) => value.into(),
        Err(_) => CompileTimeDefineValue::Evaluate(value.clone()),
    }
}

fn merge_nested_define(
    current: &mut CompileTimeDefineValue,
    path: &[DefinableNameSegment],
    value: CompileTimeDefineValue,
) -> bool {
    if path.is_empty() {
        *current = value;
        return true;
    }

    match current {
        CompileTimeDefineValue::Object(entries) => {
            let DefinableNameSegment::Name(segment) = &path[0] else {
                return false;
            };

            if path.len() == 1 {
                if let Some((_, existing)) = entries.iter_mut().find(|(key, _)| key == segment) {
                    *existing = value;
                } else {
                    entries.push((segment.clone(), value));
                }
                return true;
            }

            let child =
                if let Some((_, existing)) = entries.iter_mut().find(|(key, _)| key == segment) {
                    existing
                } else {
                    entries.push((segment.clone(), CompileTimeDefineValue::Object(vec![])));
                    &mut entries.last_mut().expect("just inserted nested define").1
                };

            merge_nested_define(child, &path[1..], value)
        }
        _ => false,
    }
}

fn defines_from_ref(define_env: &FxIndexMap<RcStr, RcStr>) -> CompileTimeDefines {
    let mut defines = FxIndexMap::default();

    for (k, v) in define_env {
        let key = k
            .split('.')
            .map(|s| DefinableNameSegment::Name(s.into()))
            .collect::<Vec<_>>();
        let value = parse_define_value(v);

        defines.entry(key).or_insert(value);
    }

    let define_entries = defines
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<Vec<_>>();

    for (key, value) in define_entries {
        for prefix_len in 1..key.len() {
            let (prefix, suffix) = key.split_at(prefix_len);
            if let Some((_, parent)) = defines
                .iter_mut()
                .find(|(candidate, _)| candidate.as_slice() == prefix)
            {
                let _ = merge_nested_define(parent, suffix, value.clone());
            }
        }
    }

    CompileTimeDefines(defines)
}

#[turbo_tasks::function]
pub async fn defines(define_env: Vc<EnvMap>) -> Result<Vc<CompileTimeDefines>> {
    Ok(defines_from_ref(&*define_env.await?).cell())
}

#[turbo_tasks::function]
pub async fn free_vars(
    define_env: Vc<EnvMap>,
    provider_config: Vc<ProviderConfig>,
) -> Result<Vc<FreeVarReferences>> {
    let mut free_vars = free_var_references!(..defines_from_ref(&*define_env.await?).into_iter());

    // Add provider configurations as FreeVarReference::EcmaScriptModule
    // This implements webpack's ProvidePlugin behavior
    let provider = provider_config.await?;
    for (var_name, value) in provider.iter() {
        let (request, export) = match value {
            ProviderConfigValue::Module(module_name) => {
                // Simple module import: { $: 'jquery' } -> import $ from 'jquery'
                (module_name.clone(), Some(rcstr!("default")))
            }
            ProviderConfigValue::NamedExport(parts) => {
                // Named export import: { Buffer: ['buffer', 'Buffer'] }
                // -> import { Buffer } from 'buffer'
                let request = if let Some(r) = parts.first() {
                    r.clone()
                } else {
                    continue;
                };
                let export = parts.get(1).cloned().or(Some(rcstr!("default")));
                (request, export)
            }
        };

        // Support nested variable names like "process.env"
        let name_segments: Vec<DefinableNameSegment> = var_name
            .split('.')
            .map(|s| DefinableNameSegment::Name(s.into()))
            .collect();

        free_vars.0.insert(
            name_segments,
            FreeVarReference::EcmaScriptModule {
                request,
                lookup_path: None,
                export,
            },
        );
    }

    Ok(free_vars.cell())
}
