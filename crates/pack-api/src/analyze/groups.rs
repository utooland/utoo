//! Build-time output groups with joined async and worker triggers.

use anyhow::Result;
use rustc_hash::{FxHashMap, FxHashSet};
use turbo_tasks::{FxIndexSet, ResolvedVc, ValueToString, ValueToStringRef, Vc};
use turbopack_core::{module::Module, output::OutputAsset};

use super::{
    AnalyzeChunkGroupCoverage, AnalyzeChunkGroupData, AnalyzeChunkGroups, AnalyzeDataBuilder,
    AnalyzeModuleIndex, ChunkLoadCandidate, ChunkLoadTrigger,
};

pub(super) async fn collect_groups(
    builder: &mut AnalyzeDataBuilder,
    candidates: Vec<ChunkLoadCandidate>,
    asset_indices: &FxHashMap<ResolvedVc<Box<dyn OutputAsset>>, Vec<u32>>,
    chunk_groups: Vc<AnalyzeChunkGroups>,
    module_index: &AnalyzeModuleIndex,
) -> Result<()> {
    let emitted_modules = builder
        .output_files
        .iter()
        .flat_map(|file| file.module_indices.iter().copied())
        .collect();
    record_load_groups(
        builder,
        candidates,
        asset_indices,
        module_index,
        &emitted_modules,
    )
    .await?;
    record_groups(builder, asset_indices, chunk_groups, module_index).await?;
    // Pack discovers service-worker registrations but does not build their
    // standalone payloads. Preserve known groups without claiming full coverage.
    for &(importer, _) in &module_index.worker_entries {
        if !emitted_ancestors(vec![importer], module_index, &emitted_modules)
            .await?
            .is_empty()
        {
            builder.chunk_group_coverage = AnalyzeChunkGroupCoverage::Unknown;
            break;
        }
    }
    for (id, group) in builder.chunk_groups.iter_mut().enumerate() {
        group.id = id as u32;
    }
    Ok(())
}

async fn record_load_groups(
    builder: &mut AnalyzeDataBuilder,
    candidates: Vec<ChunkLoadCandidate>,
    asset_indices: &FxHashMap<ResolvedVc<Box<dyn OutputAsset>>, Vec<u32>>,
    module_index: &AnalyzeModuleIndex,
    emitted_modules: &FxHashSet<u32>,
) -> Result<()> {
    let mut load_groups = FxHashMap::default();
    for candidate in candidates {
        let Some(target_indices) = asset_indices.get(&candidate.target) else {
            builder.chunk_group_coverage = AnalyzeChunkGroupCoverage::Unknown;
            continue;
        };
        let (kind, triggers) = match candidate.trigger {
            ChunkLoadTrigger::Async(index) => ("async", vec![index]),
            ChunkLoadTrigger::Worker(loader) => (
                "worker",
                emitted_ancestors(
                    module_index
                        .sync_dependents
                        .get(&loader)
                        .cloned()
                        .unwrap_or_default(),
                    module_index,
                    emitted_modules,
                )
                .await?,
            ),
        };
        let triggers = if triggers.is_empty() {
            builder.chunk_group_coverage = AnalyzeChunkGroupCoverage::Unknown;
            vec![None]
        } else {
            triggers.into_iter().map(Some).collect()
        };
        for &target in target_indices {
            if target == candidate.source && matches!(candidate.trigger, ChunkLoadTrigger::Async(_))
            {
                continue;
            }
            for &trigger in &triggers {
                // Workers use isolated root availability, so the same loader in
                // multiple chunks still describes one group within this entry.
                // Async loaders remain source-specific for chunk availability.
                let source = match candidate.trigger {
                    ChunkLoadTrigger::Async(_) => Some(candidate.source),
                    ChunkLoadTrigger::Worker(_) => None,
                };
                let key = (source, candidate.trigger, trigger);
                let group_index = *load_groups.entry(key).or_insert_with(|| {
                    let index = builder.chunk_groups.len();
                    builder.chunk_groups.push(AnalyzeChunkGroupData {
                        id: index as u32,
                        kind: kind.into(),
                        trigger_module_index: trigger,
                        output_file_indices: vec![],
                    });
                    index
                });
                let outputs = &mut builder.chunk_groups[group_index].output_file_indices;
                if !outputs.contains(&target) {
                    outputs.push(target);
                }
            }
        }
    }
    Ok(())
}

async fn record_groups(
    builder: &mut AnalyzeDataBuilder,
    asset_indices: &FxHashMap<ResolvedVc<Box<dyn OutputAsset>>, Vec<u32>>,
    chunk_groups: Vc<AnalyzeChunkGroups>,
    module_index: &AnalyzeModuleIndex,
) -> Result<()> {
    for group in chunk_groups.await?.iter() {
        let id = builder.chunk_groups.len() as u32;
        let mut output_indices = FxIndexSet::default();
        for &asset in group.assets.await?.iter() {
            let Some(indices) = asset_indices.get(&asset) else {
                let path = asset.path().await?.to_string_ref().await?;
                anyhow::bail!("chunk-group asset {path} not present among emitted output files");
            };
            output_indices.extend(indices.iter().copied());
        }
        let trigger_module_index = if let Some(module) = group.trigger {
            let ident = module.ident().to_string().owned().await?;
            Some(*module_index.by_ident.get(&ident).ok_or_else(|| {
                anyhow::anyhow!(
                    "{} group trigger {ident} is missing from the analyzer module index",
                    group.kind
                )
            })?)
        } else {
            None
        };
        builder.chunk_groups.push(AnalyzeChunkGroupData {
            id,
            kind: group.kind.clone(),
            trigger_module_index,
            output_file_indices: output_indices.into_iter().collect(),
        });
    }
    Ok(())
}

/// Registrations can live in generated `<locals>`. Find the enclosing emitted
/// items in this entry without crossing async, traced, or isolated worker edges.
async fn emitted_ancestors(
    mut pending: Vec<ResolvedVc<Box<dyn Module>>>,
    module_index: &AnalyzeModuleIndex,
    emitted_modules: &FxHashSet<u32>,
) -> Result<Vec<u32>> {
    let mut triggers = FxIndexSet::default();
    let mut seen = FxHashSet::default();
    while let Some(module) = pending.pop() {
        if !seen.insert(module) {
            continue;
        }
        let ident = module.ident().to_string().owned().await?;
        let mut found = false;
        if let Some(&index) = module_index.by_ident.get(&ident) {
            found = emitted_modules.contains(&index);
            if found {
                triggers.insert(index);
            }
        }
        if !found && let Some(parents) = module_index.sync_dependents.get(&module) {
            pending.extend(parents.iter().copied());
        }
    }
    Ok(triggers.into_iter().collect())
}
