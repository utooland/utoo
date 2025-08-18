use opfs_project::cwd::{get_cwd, set_cwd};
use opfs_project::package_manager;
use opfs_project::{opfs, DirEntry};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsValue;

#[cfg(feature = "utoo-pack")]
use crate::pack::{build, PartialProjectOptions};

#[wasm_bindgen]
pub struct Project {}

#[wasm_bindgen]
impl Project {
    #[wasm_bindgen(constructor)]
    pub fn new(cwd: String) -> Project {
        set_cwd(cwd.into());
        Project {}
    }

    #[wasm_bindgen(getter)]
    pub fn cwd(&self) -> String {
        get_cwd().to_string_lossy().to_string()
    }

    #[wasm_bindgen]
    pub async fn install(&self, package_lock: String) -> Result<(), JsValue> {
        package_manager::install_deps(&package_lock)
            .await
            .map_err(|e| JsValue::from_str(&format!("install_deps error: {e}")))?;
        Ok(())
    }

    #[cfg(feature = "utoo-pack")]
    #[wasm_bindgen]
    pub async fn build(&self) -> Result<(), JsValue> {
        let config = self.read_to_string("./utoopack.json").await.ok();

        let partial_options = PartialProjectOptions {
            project_path: ".".into(),
            config,
        };
        build(partial_options).await
    }

    #[cfg(not(feature = "utoo-pack"))]
    #[wasm_bindgen]
    pub async fn build(&self) -> Result<(), JsValue> {
        Err(JsValue::from_str(
            "Build functionality requires the 'utoo-pack' feature to be enabled",
        ))
    }

    #[wasm_bindgen]
    pub async fn read(&self, path: &str) -> Result<Vec<u8>, JsValue> {
        opfs::read_with_fuse_link(path)
            .await
            .map_err(|e| JsValue::from_str(&format!("read error: {e}")))
    }

    #[wasm_bindgen(js_name = readToString)]
    pub async fn read_to_string(&self, path: &str) -> Result<String, JsValue> {
        let buf = opfs::read_with_fuse_link(path)
            .await
            .map_err(|e| JsValue::from_str(&format!("read error: {e}")))?;
        Ok(unsafe { String::from_utf8_unchecked(buf) })
    }

    #[wasm_bindgen]
    pub async fn write(&self, path: &str, content: &[u8]) -> Result<(), JsValue> {
        opfs::write_bytes(path, content)
            .await
            .map_err(|e| JsValue::from_str(&format!("write error: {e}")))?;
        Ok(())
    }

    #[wasm_bindgen(js_name = "writeString")]
    pub async fn write_string(&self, path: &str, content: &str) -> Result<(), JsValue> {
        opfs::write(path, content)
            .await
            .map_err(|e| JsValue::from_str(&format!("write error: {e}")))?;
        Ok(())
    }

    #[wasm_bindgen(js_name = readDir)]
    pub async fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, JsValue> {
        let res = opfs::read_dir(path)
            .await
            .map_err(|e| JsValue::from_str(&format!("read_dir error: {e}")))?;
        Ok(res)
    }

    #[wasm_bindgen(js_name = createDir)]
    pub async fn create_dir(&self, path: &str) -> Result<(), JsValue> {
        opfs::create_dir(path)
            .await
            .map_err(|e| JsValue::from_str(&format!("create_dir error: {e}")))?;
        Ok(())
    }

    #[wasm_bindgen(js_name = createDirAll)]
    pub async fn create_dir_all(&self, path: &str) -> Result<(), JsValue> {
        opfs::create_dir_all(path)
            .await
            .map_err(|e| JsValue::from_str(&format!("create_dir_all error: {e}")))?;
        Ok(())
    }

    #[wasm_bindgen(js_name = copyFile)]
    pub async fn copy_file(&self, src: &str, dst: &str) -> Result<(), JsValue> {
        opfs::copy(src, dst)
            .await
            .map_err(|e| JsValue::from_str(&format!("copy error: {e}")))?;
        Ok(())
    }
}
