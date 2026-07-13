use std::io::Write;

use anyhow::Result;
use indoc::writedoc;
use turbo_rcstr::RcStr;
use turbo_tasks::{ResolvedVc, Vc};
use turbopack_core::{
    code_builder::{Code, CodeBuilder},
    environment::Environment,
};
use turbopack_ecmascript::{StaticEcmascriptCode, utils::StringifyJs};
use turbopack_ecmascript_runtime::{RuntimeType, embed_file_path};

use super::{asset_context::get_runtime_asset_context, embed_js::embed_static_code};

/// Returns the code for the ECMAScript runtime.
#[turbo_tasks::function]
pub async fn get_library_runtime_code(
    environment: ResolvedVc<Environment>,
    chunk_base_path: Vc<Option<RcStr>>,
    chunk_suffix_path: Vc<Option<RcStr>>,
    _runtime_type: RuntimeType,
    output_root_to_root_path: Vc<RcStr>,
    generate_source_map: bool,
    runtime_root: Vc<Option<RcStr>>,
    runtime_export: Vc<Vec<RcStr>>,
    runtime_module_ids: Vc<Vec<RcStr>>,
    is_node_platform: bool,
) -> Result<Vc<Code>> {
    let asset_context = get_runtime_asset_context(*environment).resolve().await?;

    let shared_runtime_utils_code = StaticEcmascriptCode::new(
        asset_context,
        embed_file_path("shared/runtime/runtime-utils.ts".into())
            .owned()
            .await?,
        generate_source_map,
    )
    .code();

    let build_base_code = StaticEcmascriptCode::new(
        asset_context,
        embed_file_path("browser/runtime/base/build-base.ts".into())
            .owned()
            .await?,
        generate_source_map,
    )
    .code();

    let runtime_base_code = vec!["library/runtime-base.ts"];

    let mut code: CodeBuilder = CodeBuilder::default();
    let relative_root_path = output_root_to_root_path.await?;
    let chunk_base_path = &*chunk_base_path.await?;
    let chunk_base_path = chunk_base_path.as_ref().map_or_else(|| "", |f| f.as_str());
    let chunk_suffix_path = &*chunk_suffix_path.await?;
    let chunk_suffix_path = chunk_suffix_path
        .as_ref()
        .map_or_else(|| "", |f| f.as_str());

    writedoc!(
        code,
        r#"
            if (!Array.isArray(__UTOOPACK__)) {{
                return;
            }}

            const CHUNK_BASE_PATH = {};
            const CHUNK_SUFFIX_PATH = {};
            const RELATIVE_ROOT_PATH = {};
            const RUNTIME_PUBLIC_PATH = {};
        "#,
        StringifyJs(chunk_base_path),
        StringifyJs(chunk_suffix_path),
        StringifyJs(relative_root_path.as_str()),
        StringifyJs(chunk_base_path),
    )?;

    code.push_code(&*shared_runtime_utils_code.await?);
    for runtime_code in runtime_base_code {
        code.push_code(
            &*embed_static_code(asset_context, runtime_code.into(), generate_source_map).await?,
        );
    }
    code.push_code(&*build_base_code.await?);

    // Select the appropriate runtime backend based on target platform.
    // - Node.js: minimal backend without DOM APIs, includes externals utils
    // - Browser: minimal DOM-based backend with loadScript for script externals
    let runtime_backend = if is_node_platform {
        "library/runtime-backend-node.ts"
    } else {
        "library/runtime-backend-dom.ts"
    };

    code.push_code(
        &*embed_static_code(asset_context, runtime_backend.into(), generate_source_map).await?,
    );

    // Registering chunks and chunk lists depends on the BACKEND variable, which is set by the
    // specific runtime code, hence it must be appended after it.
    writedoc!(
        code,
        r#"
            const chunksToRegister = __UTOOPACK__;
            __UTOOPACK__ = {{ push: registerChunk }};
            chunksToRegister.forEach(registerChunk);
        "#
    )?;

    let runtime_root = &*runtime_root.await?;
    let runtime_export = &*runtime_export.await?;
    let runtime_export = if runtime_export.is_empty() {
        "".to_string()
    } else {
        runtime_export
            .iter()
            .map(|e| format!("[{}]", StringifyJs(e)))
            .collect::<Vec<String>>()
            .join("")
    };

    let runtime_module_ids = &*runtime_module_ids.await?;

    writedoc!(
        code,
        r#"
            function factory () {{
                const runtimeModuleIds = {};
                let exports;
                for (let i = 0; i < runtimeModuleIds.length; i++) {{
                    const module = moduleCache[runtimeModuleIds[i]];
                    if (module.error) throw module.error;
                    exports = module;
                }}
                if (exports) {{
                    // any ES module has to have `module.namespaceObject` defined.
                    if (exports.namespaceObject) return exports.namespaceObject;
                    // only ESM can be an async module, so we don't need to worry about exports being a promise here.
                    const raw = exports.exports;
                    return exports.namespaceObject = interopEsm(raw, createNS(raw), raw && raw.__esModule);
                }}
            }}

            if (typeof exports === 'object' && typeof module === 'object') {{
                module.exports = factory();
            }} else if (typeof exports === 'object') {{
        "#,
        StringifyJs(runtime_module_ids),
    )?;

    if let Some(runtime_root) = runtime_root {
        let runtime_root = StringifyJs(runtime_root);
        writedoc!(
            code,
            r#"
                exports[{}] = factory(){};
            }} else {{
                globalThis[{}] = factory(){};
            "#,
            runtime_root,
            runtime_export,
            runtime_root,
            runtime_export,
        )?;
    } else {
        writedoc!(
            code,
            r#"
                var a = factory();
                for(var i in a) exports[i] = a[i]{};
            }} else {{
                var a = factory();
                for(var i in a) globalThis[i] = a[i]{};
            "#,
            runtime_export,
            runtime_export,
        )?;
    }

    writedoc!(
        code,
        r#"
            }}
        "#
    )?;

    Ok(Code::cell(code.build()))
}
