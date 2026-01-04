use anyhow::{Result, bail};
use turbo_rcstr::RcStr;
use turbo_tasks::Vc;
use turbo_tasks_fs::FileContent;
use url::Url;

use crate::project::ProjectContainer;

#[turbo_tasks::function]
pub async fn get_source_map_rope(
    container: Vc<ProjectContainer>,
    file_path: RcStr,
) -> Result<Vc<FileContent>> {
    let (file, module) = match Url::parse(&file_path) {
        Ok(url) => match url.scheme() {
            "file" => {
                let path = urlencoding::decode(url.path())?.to_string();
                let module = url.query_pairs().find(|(k, _)| k == "id");
                (
                    path,
                    match module {
                        Some(module) => Some(urlencoding::decode(&module.1)?.into_owned().into()),
                        None => None,
                    },
                )
            }
            _ => bail!("Unknown url scheme"),
        },
        Err(_) => (file_path.to_string(), None),
    };

    let Some(chunk_base) = file.strip_prefix(
        &(format!(
            "{}/{}/",
            container.project().await?.project_path,
            container.project().dist_dir().await?
        )),
    ) else {
        // File doesn't exist within the dist dir
        return Ok(FileContent::NotFound.cell());
    };

    let server_path = container.project().node_root().await?.join(chunk_base)?;

    let client_path = container.project().client_root().await?.join(chunk_base)?;

    let mut map = container.get_source_map(server_path, module.clone());

    if map.await?.is_content() {
        // If the chunk doesn't exist as a server chunk, try a client chunk.
        // TODO: Properly tag all server chunks and use the `isServer` query param.
        // Currently, this is inaccurate as it does not cover RSC server
        // chunks.
        map = container.get_source_map(client_path, module);
        if map.await?.is_content() {
            bail!("chunk/module '{}' is missing a sourcemap", file_path);
        }
    }

    Ok(map)
}
