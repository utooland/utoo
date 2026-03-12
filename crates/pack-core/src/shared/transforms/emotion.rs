use anyhow::Result;
use turbo_tasks::Vc;
use turbopack::module_options::ModuleRule;
use turbopack_ecmascript_plugins::transform::emotion::{
    EmotionTransformConfig, EmotionTransformer,
};

use crate::{
    config::Config,
    shared::transforms::{EcmascriptTransformStage, get_ecma_transform_rule},
};

pub async fn get_emotion_transform_rule(config: Vc<Config>) -> Result<Option<ModuleRule>> {
    let styles = config.styles().await?;
    let enabled = styles.emotion.unwrap_or(false);

    if !enabled {
        return Ok(None);
    }

    let module_rule =
        EmotionTransformer::new(&EmotionTransformConfig::default()).map(|transformer| {
            get_ecma_transform_rule(Box::new(transformer), false, EcmascriptTransformStage::Main)
        });

    Ok(module_rule)
}
