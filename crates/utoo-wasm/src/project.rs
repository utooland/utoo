use std::path::PathBuf;
use std::str::FromStr;
#[cfg(feature = "utoo-pack")]
use std::sync::Arc;

use anyhow::Context;
use opfs_project::DirEntry as RawDirEntry;
use serde_wasm_bindgen::to_value;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsValue;

#[cfg(feature = "utoo-pack")]
use super::{
    pack::{PackProject, PartialProjectOptions, TurbopackResult},
    tokio_runtime::TOKIO_RUNTIME,
};

use parking_lot::RwLock;

#[wasm_bindgen]
pub struct Project {
    #[cfg(feature = "utoo-pack")]
    pack_project: RwLock<Option<Arc<PackProject>>>,
}

#[wasm_bindgen]
impl Project {
    #[wasm_bindgen(constructor)]
    pub fn new(cwd: String) -> Project {
        opfs_project::set_cwd(&cwd);
        Project {
            #[cfg(feature = "utoo-pack")]
            pack_project: RwLock::new(None),
        }
    }

    #[wasm_bindgen(getter)]
    pub fn cwd(&self) -> String {
        opfs_project::get_cwd().to_string_lossy().to_string()
    }

    #[wasm_bindgen]
    pub async fn install(&self, package_lock: String) -> Result<(), String> {
        opfs_project::package_manager::install_deps(&package_lock)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    #[cfg(feature = "utoo-pack")]
    #[wasm_bindgen]
    #[allow(clippy::await_holding_refcell_ref)]
    pub async fn build(&self) -> Result<JsValue, String> {
        self.init_pack_project().await.map_err(|e| e.to_string())?;

        let pack_project = match self.pack_project.read().as_ref() {
            Some(pack_project) => pack_project.clone(),
            None => return Err("invalid pack project".to_string()),
        };

        TOKIO_RUNTIME
            .spawn(async move { pack_project.build().await })
            .await
            .map_err(|e| e.to_string())?
            .map_or_else(
                |e| Err(e.to_string()),
                |turbopack_result| {
                    serde_wasm_bindgen::to_value(&turbopack_result).map_err(|e| e.to_string())
                },
            )
    }

    #[allow(clippy::await_holding_refcell_ref)]
    async fn init_pack_project(&self) -> anyhow::Result<()> {
        if self.pack_project.read().is_none() {
            use pack_api::project::ProjectOptions;
            use turbo_rcstr::RcStr;

            let config = self.read_to_string("utoopack.json").await.ok();

            let partial_options = PartialProjectOptions {
                project_path: ".".into(),
                config,
            };
            let project_path: RcStr = partial_options.project_path.into();

            let mode = "production";

            let config = partial_options.config.map_or(
                anyhow::Result::<RcStr>::Ok(format!(r#"{{ "mode": {mode}}}"#).into()),
                |config| {
                    use std::str::FromStr;

                    let mut val = serde_json::value::Value::from_str(&config)?;
                    if let serde_json::value::Value::Object(map) = &mut val {
                        map.insert("mode".to_string(), mode.into());
                    }
                    Ok(val.to_string().into())
                },
            )?;
            let options = ProjectOptions {
                root_path: project_path.clone(),
                project_path: project_path.clone(),
                config,
                build_id: project_path.clone(),
                ..Default::default()
            };

            let pack_context = TOKIO_RUNTIME
                .spawn(PackProject::initialize(options))
                .await
                .context("fail to initialize pack project")??;

            let mut pack_project_guard = self.pack_project.write();
            *pack_project_guard = Some(Arc::new(pack_context));
        }

        Ok(())
    }

    #[cfg(not(feature = "utoo-pack"))]
    #[wasm_bindgen]
    pub async fn build(&self) -> Result<(), JsValue> {
        Err(JsValue::from_str(
            "Build functionality requires the 'utoo-pack' feature to be enabled",
        ))
    }

    #[wasm_bindgen]
    pub async fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        opfs_project::read(path).await.map_err(|e| e.to_string())
    }

    #[wasm_bindgen(js_name = readToString)]
    pub async fn read_to_string(&self, path: &str) -> Result<String, String> {
        let buf = opfs_project::read(path).await.map_err(|e| e.to_string())?;
        Ok(unsafe { String::from_utf8_unchecked(buf) })
    }

    #[wasm_bindgen]
    pub async fn write(&self, path: &str, content: &[u8]) -> Result<(), String> {
        opfs_project::write(path, content)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    #[wasm_bindgen(js_name = "writeString")]
    pub async fn write_string(&self, path: &str, content: &str) -> Result<(), String> {
        opfs_project::write(path, content)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    #[wasm_bindgen(js_name = readDir)]
    pub async fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, String> {
        let read_dir = opfs_project::read_dir(path)
            .await
            .map_err(|e| e.to_string())?;

        let ret = read_dir
            .into_iter()
            .map(|e| e.map_or_else(Err, DirEntry::try_from))
            .collect::<Result<Vec<_>, std::io::Error>>()
            .map_err(|e| e.to_string())?;

        Ok(ret)
    }

    #[wasm_bindgen(js_name = createDir)]
    pub async fn create_dir(&self, path: &str) -> Result<(), String> {
        opfs_project::create_dir(path)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    #[wasm_bindgen(js_name = createDirAll)]
    pub async fn create_dir_all(&self, path: &str) -> Result<(), String> {
        opfs_project::create_dir_all(path)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    #[wasm_bindgen(js_name = copyFile)]
    pub async fn copy_file(&self, src: &str, dst: &str) -> Result<(), String> {
        opfs_project::copy(src, dst)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[wasm_bindgen(inspectable)]
#[derive(Debug, Clone)]
pub struct DirEntry {
    #[wasm_bindgen(getter_with_clone)]
    pub name: String,
    #[wasm_bindgen]
    pub r#type: DirEntryType,
}

#[wasm_bindgen]
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum DirEntryType {
    File = "file",
    Directory = "directory",
}

impl TryFrom<RawDirEntry> for DirEntry {
    type Error = std::io::Error;

    fn try_from(v: RawDirEntry) -> Result<Self, Self::Error> {
        Ok(DirEntry {
            r#type: {
                let file_type = v.file_type()?;
                if file_type.is_dir() {
                    DirEntryType::Directory
                } else if file_type.is_file() {
                    DirEntryType::File
                } else {
                    return Err(std::io::Error::from(std::io::ErrorKind::Unsupported));
                }
            },
            name: v.file_name().to_string_lossy().to_string(),
        })
    }
}
