use anyhow::Result;
use turbo_tasks::Vc;
use turbopack::module_options::ModuleRule;
use turbopack_ecmascript_plugins::transform::emotion::EmotionTransformer;

use crate::{
    config::Config,
    shared::transforms::{EcmascriptTransformStage, get_ecma_transform_rule},
};

pub async fn get_emotion_transform_rule(config: Vc<Config>) -> Result<Option<ModuleRule>> {
    let styles = config.styles().await?;
    if let Some(emotion_options) = styles.emotion.as_ref() {
        Ok(EmotionTransformer::new(emotion_options).map(|transformer| {
            get_ecma_transform_rule(Box::new(transformer), false, EcmascriptTransformStage::Main)
        }))
    } else {
        Ok(None)
    }
}
