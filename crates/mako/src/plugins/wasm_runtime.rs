use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::sync::Arc;

use anyhow;
use wasmparser::{Import, Parser, Payload};

use crate::ast::file::{Content, JsContent};
use crate::compiler::Context;
use crate::plugin::{Plugin, PluginLoadParam};

pub struct WasmRuntimePlugin {}

const WASM_EXTENSIONS: [&str; 1] = ["wasm"];

impl Plugin for WasmRuntimePlugin {
    fn name(&self) -> &str {
        "wasm_runtime"
    }

    fn runtime_plugins(&self, context: &Arc<Context>) -> anyhow::Result<Vec<String>> {
        if context
            .assets_info
            .lock()
            .unwrap()
            .values()
            .any(|info| info.ends_with(".wasm"))
        {
            Ok(vec![
                include_str!("./wasm_runtime/wasm_runtime.js").to_string()
            ])
        } else {
            Ok(vec![])
        }
    }

    fn load(
        &self,
        param: &PluginLoadParam,
        _context: &Arc<Context>,
    ) -> anyhow::Result<Option<Content>> {
        let file = param.file;

        if WASM_EXTENSIONS.contains(&file.extname.as_str()) {
            let final_file_name = format!(
                "{}.{}.{}",
                file.get_file_stem(),
                file.get_content_hash()?,
                file.extname
            );
            _context.emit_assets(
                file.pathname.to_string_lossy().to_string(),
                final_file_name.clone(),
            );

            let mut buffer = Vec::new();
            File::open(&file.path)?.read_to_end(&mut buffer)?;
            // Parse wasm file to get imports
            let mut wasm_import_object_map: HashMap<&str, Vec<String>> = HashMap::new();
            Parser::new(0).parse_all(&buffer).for_each(|payload| {
                if let Ok(Payload::ImportSection(imports)) = payload {
                    imports.into_iter_with_offsets().for_each(|import| {
                        if let Ok((
                            _,
                            Import {
                                module,
                                name,
                                ty: _,
                            },
                        )) = import
                        {
                            if let Some(import_object) = wasm_import_object_map.get_mut(module) {
                                import_object.push(name.to_string());
                            } else {
                                wasm_import_object_map.insert(module, vec![name.to_string()]);
                            }
                        }
                    });
                }
            });

            let mut module_import_code = String::new();
            let mut wasm_import_object_code = String::new();

            for (index, (key, value)) in wasm_import_object_map.iter().enumerate() {
                module_import_code.push_str(&format!(
                    "import * as module{module_idx} from \"{module}\";\n",
                    module_idx = index,
                    module = key
                ));

                wasm_import_object_code.push_str(&format!(
                    "\"{module}\": {{ {names} }},",
                    module = key,
                    names = value
                        .iter()
                        .map(|name| format!("\"{}\": module{}[\"{}\"]", name, index, name))
                        .collect::<Vec<String>>()
                        .join(", ")
                ));
            }

            let mut content = String::new();
            content.push_str(&module_import_code);

            if wasm_import_object_code.is_empty() {
                content.push_str(&format!(
                    "module.exports = require._interopreRequireWasm(exports, \"{}\")",
                    final_file_name
                ));
            } else {
                content.push_str(&format!(
                    "module.exports = require._interopreRequireWasm(exports, \"{}\", {{{}}})",
                    final_file_name, wasm_import_object_code
                ));
            }

            return Ok(Some(Content::Js(JsContent {
                content,
                ..Default::default()
            })));
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::ast::file::File;
    use crate::compiler::Context;

    #[test]
    fn test_wasm_runtime_load_with_import_object() {
        let plugin = WasmRuntimePlugin {};
        let context = Arc::new(Context {
            ..Default::default()
        });
        let wasm_relative_path =
            std::path::Path::new("../../examples/import-resources/minus-wasm-pack/index_bg.wasm");
        let wasm_path = std::fs::canonicalize(wasm_relative_path).unwrap();
        let file = File::new(wasm_path.to_string_lossy().to_string(), context.clone());
        let param = PluginLoadParam { file: &file };
        let result: Option<Content> = plugin.load(&param, &context).unwrap();

        assert!(result.is_some());
        if let Some(Content::Js(js_content)) = result {
            assert!(js_content.content.contains("import * as module0 from"));
        }
    }

    #[test]
    fn test_wasm_runtime_load_without_import_object() {
        let plugin = WasmRuntimePlugin {};
        let context = Arc::new(Context {
            ..Default::default()
        });
        let wasm_relative_path = std::path::Path::new("../../examples/import-resources/add.wasm");
        let wasm_path = std::fs::canonicalize(wasm_relative_path).unwrap();
        let file = File::new(wasm_path.to_string_lossy().to_string(), context.clone());
        let param = PluginLoadParam { file: &file };
        let result = plugin.load(&param, &context).unwrap();
        assert!(result.is_some());
        if let Some(Content::Js(js_content)) = result {
            assert!(!js_content.content.contains("import * as module0 from"))
        }
    }
}
