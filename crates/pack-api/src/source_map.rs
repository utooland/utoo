use anyhow::{Result, bail};
use turbo_rcstr::RcStr;
use turbo_tasks::Vc;
use turbo_tasks_fs::FileContent;
use turbo_unix_path::sys_to_unix;
use url::Url;

use crate::project::ProjectContainer;

#[turbo_tasks::function]
pub async fn get_source_map_rope(
    container: Vc<ProjectContainer>,
    source_url: RcStr,
) -> Result<Vc<FileContent>> {
    let (file_path_sys, module) = match Url::parse(&source_url) {
        Ok(url) => match url.scheme() {
            "file" => {
                #[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
                let path: String = match url.to_file_path() {
                    Ok(path) => path.to_string_lossy().into(),
                    Err(_) => {
                        bail!("Failed to convert file URL to file path: {url}");
                    }
                };
                #[cfg(all(target_family = "wasm", target_os = "unknown"))]
                let path: String = urlencoding::decode(url.path())?.into_owned();

                let module = url.query_pairs().find(|(k, _)| k == "id");
                (
                    path,
                    match module {
                        Some(module) => Some(urlencoding::decode(&module.1)?.into_owned().into()),
                        None => None,
                    },
                )
            }
            _ => bail!("Unknown url scheme '{}'", url.scheme()),
        },
        Err(_) => (source_url.to_string(), None),
    };

    let chunk_base_unix =
        match file_path_sys.strip_prefix(container.project().dist_dir_absolute().await?.as_str()) {
            Some(relative_path) => sys_to_unix(relative_path),
            None => {
                // File doesn't exist within the dist dir
                return Ok(FileContent::NotFound.cell());
            }
        };

    let server_path = container
        .project()
        .node_root()
        .await?
        .join(&chunk_base_unix)?;

    let client_path = container
        .project()
        .client_root()
        .await?
        .join(&chunk_base_unix)?;

    let mut map = container.get_source_map(server_path, module.clone());

    if !map.await?.is_content() {
        // If the chunk doesn't exist as a server chunk, try a client chunk.
        // TODO: Properly tag all server chunks and use the `isServer` query param.
        // Currently, this is inaccurate as it does not cover RSC server
        // chunks.
        map = container.get_source_map(client_path, module);
        if !map.await?.is_content() {
            bail!("chunk/module '{}' is missing a sourcemap", source_url);
        }
    }

    Ok(map)
}
