use std::io::Read;

use anyhow::{Context, Result};
use pack_core::mode::Mode;
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{ResolvedVc, TurboTasks, Vc};
use turbo_tasks_backend::{BackendOptions, TurboTasksBackend, noop_backing_storage};
use turbo_tasks_fs::{File, FileContent, FileSystem, FileSystemPath, NullFileSystem};
use turbo_tasks_hash::HashAlgorithm;
use turbopack_browser::BrowserChunkingContext;
use turbopack_core::{
    asset::{Asset, AssetContent, no_hash_salt},
    chunk::ContentHashing,
    environment::{BrowserEnvironment, Environment, ExecutionEnvironment},
    output::{OutputAsset, OutputAssetsReference},
    source_map::{GenerateSourceMap, SourceMapType},
    virtual_source::VirtualSource,
};
use turbopack_ecmascript::single_file_ecmascript_output::SingleFileEcmascriptOutput;

const SOURCE: &str = "export const value = 42;\n";

#[tokio::test]
async fn single_file_names_change_with_maps_without_rewriting_source() -> Result<()> {
    let tasks = TurboTasks::new(TurboTasksBackend::new(
        BackendOptions::default(),
        noop_backing_storage(),
    ));
    tasks
        .run_once(async {
            verify_single_file_output_hashing()
                .read_strongly_consistent()
                .await?;
            Ok(())
        })
        .await
}

async fn context(
    root: FileSystemPath,
    maps: bool,
    explicit_template: bool,
    salt: &str,
) -> Result<ResolvedVc<BrowserChunkingContext>> {
    let output_root = root.join("output")?;
    let environment = Environment::new(ExecutionEnvironment::Browser(
        BrowserEnvironment {
            dom: true,
            web_worker: false,
            service_worker: false,
            browserslist_query: rcstr!("last 1 Chrome version"),
        }
        .resolved_cell(),
    ))
    .to_resolved()
    .await?;
    let mut builder = BrowserChunkingContext::builder(
        root,
        output_root.clone(),
        rcstr!(".."),
        output_root.clone(),
        output_root.clone(),
        output_root,
        environment,
        Mode::Production.runtime_type(),
    )
    .emitted_source_map_type(maps.then_some(SourceMapType::Full))
    .debug_ids(true)
    .hash_salt(ResolvedVc::cell(RcStr::from(salt)))
    .chunk_content_hashing(ContentHashing::Direct { length: 13 });
    if explicit_template {
        // A fixed prefix keeps source-ident renames from changing the filename on their own.
        builder = builder.chunk_filename(rcstr!("single.[contenthash:8].js"));
    }
    builder.build().to_resolved().await
}

async fn file_text(content: Vc<FileContent>) -> Result<String> {
    let content = content.await?;
    let FileContent::Content(file) = &*content else {
        anyhow::bail!("expected file content");
    };
    let mut text = String::new();
    file.read().read_to_string(&mut text)?;
    Ok(text)
}

#[turbo_tasks::function(operation, root)]
async fn verify_single_file_output_hashing() -> Result<()> {
    let root = NullFileSystem.cell().root().owned().await?;
    let content = AssetContent::file(FileContent::Content(File::from(SOURCE)).cell())
        .to_resolved()
        .await?;
    let first_source = VirtualSource::new(root.join("first.js")?, *content)
        .to_resolved()
        .await?;
    let renamed_source = VirtualSource::new(root.join("renamed.js")?, *content)
        .to_resolved()
        .await?;

    for maps in [false, true] {
        for (explicit_template, salt) in [(true, ""), (false, ""), (false, "map-test-salt")] {
            let context = context(root.clone(), maps, explicit_template, salt).await?;
            let first =
                SingleFileEcmascriptOutput::new(Vc::upcast(*context), Vc::upcast(*first_source));
            let repeated =
                SingleFileEcmascriptOutput::new(Vc::upcast(*context), Vc::upcast(*first_source));
            let renamed =
                SingleFileEcmascriptOutput::new(Vc::upcast(*context), Vc::upcast(*renamed_source));
            let algorithm = if explicit_template {
                HashAlgorithm::Xxh3Hash64Hex
            } else {
                HashAlgorithm::Xxh3Hash128Base38
            };
            let hash_salt = if explicit_template {
                no_hash_salt()
            } else {
                Vc::cell(RcStr::from(salt))
            };
            let hash = first.content_hash(hash_salt, algorithm).owned().await?;
            let hash = hash.context("expected a file hash")?;
            assert_eq!(
                repeated.content_hash(hash_salt, algorithm).owned().await?,
                Some(hash.clone()),
            );
            let first_path = first.path().await?;
            assert_eq!(first_path.path, repeated.path().await?.path);
            let expected_name = if explicit_template {
                format!("single.{}.js", &hash[..8])
            } else {
                format!("{}.js", &hash[..13])
            };
            assert_eq!(
                first_path.path,
                root.join("output")?.join(&expected_name)?.path
            );

            // Renaming only the source changes its map while keeping output JS bytes identical.
            let first_map = file_text(first.generate_source_map(SourceMapType::Full)).await?;
            assert_ne!(
                first_map,
                file_text(renamed.generate_source_map(SourceMapType::Full)).await?
            );
            let renamed_path = renamed.path().await?;
            let references = first.references().await?;
            let references = references.assets.await?;
            if maps {
                assert_ne!(first_path.path, renamed_path.path);
                assert_eq!(references.len(), 1);
                let map = references[0];
                assert_eq!(
                    map.path().await?.path,
                    RcStr::from(format!("{}.map", first_path.path))
                );
                assert_eq!(file_text(map.content().file_content()).await?, first_map);
                let json: serde_json::Value = serde_json::from_str(&first_map)?;
                assert_eq!(json["version"], 3);
                assert_eq!(json["sourcesContent"][0], SOURCE);
            } else {
                assert_eq!(first_path.path, renamed_path.path);
                assert!(references.is_empty());
                assert_eq!(
                    Some(hash),
                    first_source
                        .content_hash(hash_salt, algorithm)
                        .owned()
                        .await?,
                );
            }
            assert_eq!(file_text(first.content().file_content()).await?, SOURCE);
            assert_eq!(file_text(renamed.content().file_content()).await?, SOURCE);

            let changed_content = AssetContent::file(
                FileContent::Content(File::from(SOURCE.replace("42", "43"))).cell(),
            )
            .to_resolved()
            .await?;
            let changed_source = VirtualSource::new(root.join("first.js")?, *changed_content)
                .to_resolved()
                .await?;
            let changed =
                SingleFileEcmascriptOutput::new(Vc::upcast(*context), Vc::upcast(*changed_source));
            assert_ne!(first_path.path, changed.path().await?.path);
        }
    }
    Ok(())
}
