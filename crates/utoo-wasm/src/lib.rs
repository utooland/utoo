#![cfg(all(target_family = "wasm", target_os = "unknown"))]

use opfs_project::cwd::{self, get_cwd};
use opfs_project::package_manager;
use opfs_project::{opfs, DirEntry};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsValue;

#[wasm_bindgen]
pub struct Project {}

#[wasm_bindgen]
impl Project {
    #[wasm_bindgen(constructor)]
    pub fn new(cwd: String) -> Project {
        cwd::set_cwd(cwd.clone());
        Project {}
    }

    #[wasm_bindgen(getter)]
    pub fn cwd(&self) -> String {
        cwd::get_cwd()
    }

    #[wasm_bindgen]
    pub async fn install(&self, package_lock: String) -> Result<(), JsValue> {
        package_manager::install_deps(&package_lock)
            .await
            .map_err(|e| JsValue::from_str(&format!("install_deps error: {e}")))?;
        Ok(())
    }

    #[wasm_bindgen]
    pub async fn build(&self) -> Result<(), JsValue> {
        // TODO: 
        todo!()
    }

    #[wasm_bindgen(js_name = readFile)]
    pub async fn read_file(&self, path: String) -> Result<String, JsValue> {
        let content = opfs::read_with_fuse_link(&path)
            .await
            .map_err(|e| JsValue::from_str(&format!("read error: {e}")))?;
        Ok(unsafe { String::from_utf8_unchecked(content) })
    }

    #[wasm_bindgen(js_name = writeFile)]
    pub async fn write_file(&self, path: String, content: String) -> Result<(), JsValue> {
        opfs::write(&path, &content)
            .await
            .map_err(|e| JsValue::from_str(&format!("write error: {e}")))?;
        Ok(())
    }

    #[wasm_bindgen(js_name = readDir)]
    pub async fn read_dir(&self, path: String) -> Result<Vec<DirEntry>, JsValue> {
        let res = opfs::read_dir(&path)
            .await
            .map_err(|e| JsValue::from_str(&format!("read_dir error: {e}")))?;
        Ok(res)
    }

    #[wasm_bindgen(js_name = createDir)]
    pub async fn create_dir(&self, path: String) -> Result<(), JsValue> {
        opfs::create_dir(&path)
            .await
            .map_err(|e| JsValue::from_str(&format!("read_dir error: {e}")))?;
        Ok(())
    }

    #[wasm_bindgen(js_name = createDirAll)]
    pub async fn create_dir_all(&self, path: String) -> Result<(), JsValue> {
        opfs::create_dir_all(&path)
            .await
            .map_err(|e| JsValue::from_str(&format!("read_dir error: {e}")))?;
        Ok(())
    }

    #[wasm_bindgen(js_name = copyFile)]
    pub async fn copy_file(&self, src: String, dst: String) -> Result<(), JsValue> {
        opfs::copy(&src, &dst)
            .await
            .map_err(|e| JsValue::from_str(&format!("copy error: {e}")))?;
        Ok(())
    }
}
