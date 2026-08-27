use anyhow::Result;
use turbo_tasks::{ResolvedVc, Vc};
use turbo_tasks_fs::{self, FileJsonContent, FileSystemPath};
use turbopack::module_options::{
    DecoratorsKind, DecoratorsOptions, JsxTransformOptions, TypescriptTransformOptions,
};
use turbopack_core::{
    file_source::FileSource,
    resolve::{FindContextFileResult, find_context_file, node::node_cjs_resolve_options},
    source::Source,
};
use turbopack_ecmascript::typescript::resolve::{read_from_tsconfigs, read_tsconfigs, tsconfig};

use crate::{config::Config, mode::Mode};

async fn get_typescript_options(
    project_path: FileSystemPath,
) -> Result<Option<Vec<(Vc<FileJsonContent>, ResolvedVc<Box<dyn Source>>)>>> {
    let tsconfig = find_context_file(project_path, tsconfig(), false);
    Ok(match tsconfig.await.ok().as_deref() {
        Some(FindContextFileResult::Found(path, _)) => read_tsconfigs(
            path.read(),
            ResolvedVc::upcast(FileSource::new(path.clone()).to_resolved().await?),
            node_cjs_resolve_options(path.root().owned().await?),
        )
        .await
        .ok(),
        Some(FindContextFileResult::NotFound(_)) | None => None,
    })
}

/// Build the transform options for specifically for the typescript's runtime
/// outputs
#[turbo_tasks::function]
pub async fn get_typescript_transform_options(
    project_path: FileSystemPath,
) -> Result<Vc<TypescriptTransformOptions>> {
    let tsconfig = get_typescript_options(project_path).await?;

    let use_define_for_class_fields = if let Some(ref tsconfig) = tsconfig {
        read_from_tsconfigs(tsconfig, |json, _| {
            json.get("compilerOptions")
                .and_then(|opts| opts.get("useDefineForClassFields"))
                .and_then(|v| v.as_bool())
        })
        .await?
        .unwrap_or(true)
    } else {
        true
    };
    let verbatim_module_syntax = if let Some(ref tsconfig) = tsconfig {
        read_from_tsconfigs(tsconfig, |json, _| {
            json.get("compilerOptions")
                .and_then(|opts| opts.get("verbatimModuleSyntax"))
                .and_then(|v| v.as_bool())
        })
        .await?
        .unwrap_or(false)
    } else {
        false
    };

    let ts_transform_options = TypescriptTransformOptions {
        use_define_for_class_fields,
        verbatim_module_syntax,
    };

    Ok(ts_transform_options.cell())
}

/// Build the transform options for the decorators.
/// **TODO** Currnently only typescript's legacy decorators are supported
#[turbo_tasks::function]
pub async fn get_decorators_transform_options(
    project_path: FileSystemPath,
) -> Result<Vc<DecoratorsOptions>> {
    let tsconfig = get_typescript_options(project_path).await?;

    let experimental_decorators = if let Some(ref tsconfig) = tsconfig {
        read_from_tsconfigs(tsconfig, |json, _| {
            json.get("compilerOptions")
                .and_then(|opts| opts.get("experimentalDecorators"))
                .and_then(|v| v.as_bool())
        })
        .await?
        .unwrap_or(false)
    } else {
        false
    };

    let decorators_kind = if experimental_decorators {
        Some(DecoratorsKind::Legacy)
    } else {
        // ref: https://devblogs.microsoft.com/typescript/announcing-typescript-5-0-rc/#differences-with-experimental-legacy-decorators
        // `without the flag, decorators will now be valid syntax for all new code.
        // Outside of --experimentalDecorators, they will be type-checked and emitted
        // differently with ts 5.0, new ecma decorators will be enabled
        // if legacy decorators are not enabled
        Some(DecoratorsKind::Ecma)
    };

    let emit_decorators_metadata = if let Some(ref tsconfig) = tsconfig {
        read_from_tsconfigs(tsconfig, |json, _| {
            json.get("compilerOptions")
                .and_then(|opts| opts.get("emitDecoratorMetadata"))
                .and_then(|v| v.as_bool())
        })
        .await?
        .unwrap_or(false)
    } else {
        false
    };

    let use_define_for_class_fields = if let Some(ref tsconfig) = tsconfig {
        read_from_tsconfigs(tsconfig, |json, _| {
            json.get("compilerOptions")
                .and_then(|opts| opts.get("useDefineForClassFields"))
                .and_then(|v| v.as_bool())
        })
        .await?
        .unwrap_or(true)
    } else {
        true
    };

    let decorators_transform_options = DecoratorsOptions {
        decorators_kind: decorators_kind.clone(),
        emit_decorators_metadata: if let Some(ref decorators_kind) = decorators_kind {
            match decorators_kind {
                DecoratorsKind::Legacy => emit_decorators_metadata,
                // ref: This new decorators proposal is not compatible with
                // --emitDecoratorMetadata, and it does not allow decorating parameters.
                // Future ECMAScript proposals may be able to help bridge that gap
                DecoratorsKind::Ecma => false,
            }
        } else {
            false
        },
        use_define_for_class_fields,
        ..Default::default()
    };

    Ok(decorators_transform_options.cell())
}

#[turbo_tasks::function]
pub async fn get_jsx_transform_options(
    mode: Vc<Mode>,
    config: Vc<Config>,
    enable_react_refresh: bool,
) -> Result<Vc<JsxTransformOptions>> {
    let react_config = config.react().await?;
    let emotion_enabled = config.styles().await?.emotion.as_ref().is_some();

    let import_source = react_config.import_source.clone().or_else(|| {
        if emotion_enabled {
            Some("@emotion/react".into())
        } else {
            Some("react".into())
        }
    });

    let react_transform_options = JsxTransformOptions {
        development: mode.await?.is_react_development(),
        import_source,
        runtime: react_config
            .runtime
            .as_ref()
            .map(|r| r.as_str().into())
            .or(Some("automatic".into())),
        react_refresh: enable_react_refresh,
    };

    Ok(react_transform_options.cell())
}
