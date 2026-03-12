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

fn defines_from_ref(define_env: &FxIndexMap<RcStr, RcStr>) -> CompileTimeDefines {
    let mut defines = FxIndexMap::default();

    for (k, v) in define_env {
        defines
            .entry(
                k.split('.')
                    .map(|s| DefinableNameSegment::Name(s.into()))
                    .collect::<Vec<_>>(),
            )
            .or_insert_with(|| {
                let val = serde_json::Value::from_str(v);
                match val {
                    Ok(v) => v.into(),
                    _ => CompileTimeDefineValue::Evaluate(v.clone()),
                }
            });
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
