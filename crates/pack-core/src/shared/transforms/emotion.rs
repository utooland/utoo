use anyhow::Result;
use turbo_tasks::Vc;
use turbopack::module_options::ModuleRule;
use turbopack_ecmascript_plugins::transform::emotion::{
    EmotionLabelKind, EmotionTransformConfig, EmotionTransformer,
};

use crate::{
    config::Config,
    mode::Mode,
    shared::transforms::{EcmascriptTransformStage, get_ecma_transform_rule},
};

pub async fn get_emotion_transform_rule(
    config: Vc<Config>,
    mode: Vc<Mode>,
) -> Result<Option<ModuleRule>> {
    let styles = config.styles().await?;
    let Some(emotion) = styles.emotion.as_ref() else {
        return Ok(None);
    };

    if !emotion.is_enabled() {
        return Ok(None);
    }

    let is_dev = mode.await?.is_development();
    let utoo_defaults = EmotionTransformConfig {
        sourcemap: Some(is_dev),
        auto_label: Some(if is_dev {
            EmotionLabelKind::Always
        } else {
            EmotionLabelKind::Never
        }),
        ..Default::default()
    };
    let emotion_config = match emotion.options() {
        Some(options) => EmotionTransformConfig {
            // Priority: user config > utoo defaults > upstream defaults
            sourcemap: options.sourcemap.or(utoo_defaults.sourcemap),
            auto_label: options.auto_label.clone().or(utoo_defaults.auto_label),
            label_format: options.label_format.clone().or(utoo_defaults.label_format),
            import_map: options.import_map.clone().or(utoo_defaults.import_map),
        },
        None => utoo_defaults,
    };

    Ok(EmotionTransformer::new(&emotion_config).map(|transformer| {
        get_ecma_transform_rule(Box::new(transformer), false, EcmascriptTransformStage::Main)
    }))
}
