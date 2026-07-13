use anyhow::Result;
pub use modularize_imports::ModularizeImportPackageConfig;
use turbo_rcstr::RcStr;
use turbo_tasks::ResolvedVc;
use turbopack::module_options::{ModuleRule, ModuleRuleEffect, ModuleType, RuleCondition};
use turbopack_core::reference_type::{
    CssReferenceSubType, ReferenceTypeCondition, UrlReferenceSubType,
};
use turbopack_core::source_transform::SourceTransform;
use turbopack_ecmascript::{CustomTransformer, EcmascriptInputTransform};

use image::{StructuredImageModuleType, module::BlurPlaceholderMode};
use inline_css::{InlineCssModuleType, module::InjectType};
use wasm::StaticWasmModuleType;

pub mod classic_jsx_react_import;
pub mod css_modules;
pub mod default_export_namer;
pub mod emotion;
pub mod image;
pub mod inline_css;
pub mod jsx_dev_filename;
pub mod modularize_imports;
pub mod remove_console;
pub mod styled_components;
pub mod styled_jsx;
pub mod swc_ecma_transform_plugins;
pub mod wasm;
pub mod webpack_public_path;

pub async fn get_image_rule(inline_limit: Option<u64>) -> Result<ModuleRule> {
    Ok(ModuleRule::new(
        RuleCondition::All(vec![
            RuleCondition::not(RuleCondition::ReferenceType(ReferenceTypeCondition::Url(
                Some(UrlReferenceSubType::Undefined),
            ))),
            RuleCondition::any(vec![
                RuleCondition::ResourcePathEndsWith(".jpg".to_string()),
                RuleCondition::ResourcePathEndsWith(".jpeg".to_string()),
                RuleCondition::ResourcePathEndsWith(".png".to_string()),
                RuleCondition::ResourcePathEndsWith(".apng".to_string()),
                RuleCondition::ResourcePathEndsWith(".gif".to_string()),
                RuleCondition::ResourcePathEndsWith(".svg".to_string()),
                RuleCondition::ResourcePathEndsWith(".bmp".to_string()),
                RuleCondition::ResourcePathEndsWith(".ico".to_string()),
                RuleCondition::ResourcePathEndsWith(".webp".to_string()),
                RuleCondition::ResourcePathEndsWith(".avif".to_string()),
            ]),
        ]),
        vec![ModuleRuleEffect::ModuleType(ModuleType::Custom(
            ResolvedVc::upcast(
                StructuredImageModuleType::new(inline_limit, BlurPlaceholderMode::DataUrl)
                    .to_resolved()
                    .await?,
            ),
        ))],
    ))
}

/// Returns a module rule for WASM files that outputs them as static assets.
pub async fn get_wasm_rule() -> Result<ModuleRule> {
    Ok(ModuleRule::new(
        RuleCondition::All(vec![
            RuleCondition::not(RuleCondition::ReferenceType(ReferenceTypeCondition::Url(
                Some(UrlReferenceSubType::Undefined),
            ))),
            RuleCondition::ResourcePathEndsWith(".wasm".to_string()),
        ]),
        vec![ModuleRuleEffect::ModuleType(ModuleType::Custom(
            ResolvedVc::upcast(StaticWasmModuleType::new().to_resolved().await?),
        ))],
    ))
}

/// Returns a module rule for CSS files that outputs them as JS modules
/// injecting styles into the DOM at runtime.
pub async fn get_inline_css_rule(
    insert: RcStr,
    inject_type: InjectType,
    css_modules_pattern: Option<RcStr>,
    postcss_transform: Option<ResolvedVc<Box<dyn SourceTransform>>>,
) -> Result<ModuleRule> {
    let mut effects = vec![];

    if let Some(postcss_transform) = postcss_transform {
        effects.push(ModuleRuleEffect::SourceTransforms(ResolvedVc::cell(vec![
            postcss_transform,
        ])));
    }

    effects.push(ModuleRuleEffect::ModuleType(ModuleType::Custom(
        ResolvedVc::upcast(
            InlineCssModuleType::new(insert, inject_type, css_modules_pattern)
                .to_resolved()
                .await?,
        ),
    )));

    Ok(ModuleRule::new(
        RuleCondition::All(vec![
            RuleCondition::not(RuleCondition::ReferenceType(ReferenceTypeCondition::Url(
                Some(UrlReferenceSubType::Undefined),
            ))),
            // CSS module facade analysis must still see a CSS-processable asset.
            // If we inline the Analyze reference into JS here, class extraction
            // later fails with "inner asset should be CSS processable".
            RuleCondition::not(RuleCondition::ReferenceType(ReferenceTypeCondition::Css(
                Some(CssReferenceSubType::Analyze),
            ))),
            RuleCondition::ResourcePathEndsWith(".css".to_string()),
        ]),
        effects,
    ))
}

fn match_js_extension(enable_mdx_rs: bool) -> Vec<RuleCondition> {
    let mut conditions = vec![
        RuleCondition::ResourcePathEndsWith(".js".to_string()),
        RuleCondition::ResourcePathEndsWith(".jsx".to_string()),
        RuleCondition::All(vec![
            RuleCondition::ResourcePathEndsWith(".ts".to_string()),
            RuleCondition::Not(Box::new(RuleCondition::ResourcePathEndsWith(
                ".d.ts".to_string(),
            ))),
        ]),
        RuleCondition::ResourcePathEndsWith(".tsx".to_string()),
        RuleCondition::ResourcePathEndsWith(".mjs".to_string()),
        RuleCondition::ResourcePathEndsWith(".cjs".to_string()),
    ];

    if enable_mdx_rs {
        conditions.append(
            vec![
                RuleCondition::ResourcePathEndsWith(".md".to_string()),
                RuleCondition::ResourcePathEndsWith(".mdx".to_string()),
                RuleCondition::ContentTypeStartsWith("text/markdown".to_string()),
            ]
            .as_mut(),
        );
    }
    conditions
}

/// Returns a module rule condition matches to any ecmascript (with mdx if
/// enabled) except url reference type. This is a typical custom rule matching
/// condition for custom ecma specific transforms.
pub(crate) fn module_rule_match_js_no_url(enable_mdx_rs: bool) -> RuleCondition {
    let conditions = match_js_extension(enable_mdx_rs);

    RuleCondition::all(vec![
        RuleCondition::not(RuleCondition::ReferenceType(ReferenceTypeCondition::Url(
            Some(UrlReferenceSubType::Undefined),
        ))),
        RuleCondition::any(conditions),
    ])
}

pub(crate) enum EcmascriptTransformStage {
    /// Transforms to run first: transpile TypeScript, decorators, ...
    Preprocess,
    /// Transforms to execute on standard EcmaScript (plus JSX): styled-jsx, swc plugins, ...
    Main,
    #[allow(dead_code)]
    /// Transforms to run last: JSX, preset-env, scan for imports, ...
    Postprocess,
}

/// Create a new module rule for the given ecmatransform, runs against
/// any ecmascript (with mdx if enabled) except url reference type
pub(crate) fn get_ecma_transform_rule(
    transformer: Box<dyn CustomTransformer + Send + Sync>,
    enable_mdx_rs: bool,
    stage: EcmascriptTransformStage,
) -> ModuleRule {
    let transformer = EcmascriptInputTransform::Plugin(ResolvedVc::cell(transformer as _));
    let (preprocess, main, postprocess) = match stage {
        EcmascriptTransformStage::Preprocess => (vec![transformer], vec![], vec![]),
        EcmascriptTransformStage::Main => (vec![], vec![transformer], vec![]),
        EcmascriptTransformStage::Postprocess => (vec![], vec![], vec![transformer]),
    };

    ModuleRule::new(
        module_rule_match_js_no_url(enable_mdx_rs),
        vec![ModuleRuleEffect::ExtendEcmascriptTransforms {
            preprocess: ResolvedVc::cell(preprocess),
            main: ResolvedVc::cell(main),
            postprocess: ResolvedVc::cell(postprocess),
        }],
    )
}
