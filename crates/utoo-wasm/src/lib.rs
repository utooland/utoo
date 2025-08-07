use opfs_project::cwd;
use opfs_project::opfs_fs;
use opfs_project::package_manager;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsValue;

#[wasm_bindgen(js_name = "fs")]
pub struct Opfs {}

#[wasm_bindgen]
impl Opfs {
    #[wasm_bindgen(js_name = "read")]
    pub async fn read(path: String) -> Result<JsValue, JsValue> {
        let content = opfs_fs::read(&path)
            .await
            .map_err(|e| JsValue::from_str(&format!("read error: {e}")))?;
        // Convert bytes to string, assuming UTF-8 encoding
        let content_str = String::from_utf8(content)
            .map_err(|e| JsValue::from_str(&format!("utf8 decode error: {e}")))?;
        Ok(JsValue::from_str(&content_str))
    }

    #[wasm_bindgen(js_name = "write")]
    pub async fn write(path: String, content: String) -> Result<(), JsValue> {
        opfs_fs::write(&path, &content)
            .await
            .map_err(|e| JsValue::from_str(&format!("write error: {e}")))?;
        Ok(())
    }

    #[wasm_bindgen(js_name = "read_dir")]
    pub async fn read_dir(path: String) -> Result<JsValue, JsValue> {
        let res = opfs_fs::read_dir(&path)
            .await
            .map_err(|e| JsValue::from_str(&format!("read_dir error: {e}")))?;
        Ok(JsValue::from_str(&serde_json::to_string(&res).unwrap()))
    }

    #[wasm_bindgen(js_name = "copy")]
    pub async fn copy(src: String, dst: String) -> Result<(), JsValue> {
        opfs_fs::copy(&src, &dst)
            .await
            .map_err(|e| JsValue::from_str(&format!("copy error: {e}")))?;
        Ok(())
    }
}

#[wasm_bindgen(js_name = "cwd")]
pub struct Cwd {}

#[wasm_bindgen]
impl Cwd {
    #[wasm_bindgen(js_name = "set")]
    pub async fn set(path: String) -> Result<(), JsValue> {
        cwd::set_cwd(&path)
            .await
            .map_err(|e| JsValue::from_str(&format!("set cwd error: {e}")))?;
        Ok(())
    }

    #[wasm_bindgen(js_name = "get")]
    pub async fn get() -> Result<JsValue, JsValue> {
        let res = cwd::get_cwd()
            .await
            .map_err(|e| JsValue::from_str(&format!("get cwd error: {e}")))?;
        Ok(JsValue::from_str(&res))
    }
}

/// download all tgz packages to OPFS
#[wasm_bindgen]
pub async fn install_deps(lock_content: String, _pkg: String) -> Result<JsValue, JsValue> {
    let results = package_manager::install_deps(&lock_content, &_pkg)
        .await
        .map_err(|e| JsValue::from_str(&format!("install_deps error: {e}")))?;
    Ok(JsValue::from_str(&serde_json::to_string(&results).unwrap()))
}
