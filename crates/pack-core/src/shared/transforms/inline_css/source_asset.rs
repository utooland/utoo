use anyhow::{Result, bail};
use indoc::formatdoc;
use lightningcss::{
    stylesheet::{MinifyOptions, ParserOptions, PrinterOptions, StyleSheet},
    targets::{BrowserslistConfig, Features, Targets},
};
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{ResolvedVc, Vc};
use turbo_tasks_fs::{FileContent, rope::Rope};
use turbopack_core::{
    asset::{Asset, AssetContent},
    context::AssetContext,
    ident::AssetIdent,
    source::Source,
};
use turbopack_ecmascript::utils::StringifyJs;

use super::module::InjectType;

/// A source asset that transforms a CSS file into JavaScript code which
/// injects the styles into the DOM at runtime.
#[turbo_tasks::value(shared)]
pub struct InlineCssFileSource {
    pub css: ResolvedVc<Box<dyn Source>>,
    pub asset_context: ResolvedVc<Box<dyn AssetContext>>,
    pub insert: RcStr,
    pub inject_type: InjectType,
    pub minify: bool,
}

#[turbo_tasks::value_impl]
impl Source for InlineCssFileSource {
    #[turbo_tasks::function]
    async fn ident(&self) -> Result<Vc<AssetIdent>> {
        let ident = self
            .css
            .ident()
            .owned()
            .await?
            .with_modifier(rcstr!("inline css"))
            .rename_as("*.js");

        Ok(ident.into_vc())
    }

    #[turbo_tasks::function]
    async fn description(&self) -> Result<Vc<RcStr>> {
        let inner = self.css.description().await?;
        Ok(Vc::cell(format!("inline css transform of {inner}").into()))
    }
}

#[turbo_tasks::value_impl]
impl Asset for InlineCssFileSource {
    #[turbo_tasks::function]
    async fn content(&self) -> Result<Vc<AssetContent>> {
        let content = self.css.content().await?;
        let AssetContent::File(content) = *content else {
            bail!("Input source is not a file and cannot be transformed into inline CSS");
        };
        let FileContent::Content(file) = &*content.await? else {
            bail!("Input source has no content for inline CSS transformation");
        };
        let raw_css = file.content().to_str()?.into_owned();

        // Transform CSS using lightningcss: parse, apply transforms (nesting,
        // vendor prefixes, etc.), then print the transformed output.
        let environment = self.asset_context.compile_time_info().environment();
        let browserslist_query = environment.browserslist_query().owned().await?;
        let browserslist_browsers = lightningcss::targets::Browsers::from_browserslist_with_config(
            browserslist_query.split(','),
            BrowserslistConfig {
                ignore_unknown_versions: true,
                ..Default::default()
            },
        )?;
        let targets = Targets {
            browsers: browserslist_browsers,
            include: Features::Nesting | Features::MediaRangeSyntax,
            ..Default::default()
        };

        let css_text = {
            let parsed = StyleSheet::parse(
                &raw_css,
                ParserOptions {
                    error_recovery: true,
                    ..Default::default()
                },
            );
            match parsed {
                Ok(mut ss) => {
                    // minify() applies transforms: lowers nesting, adds vendor
                    // prefixes, etc.
                    let _ = ss.minify(MinifyOptions {
                        targets,
                        ..Default::default()
                    });
                    ss.to_css(PrinterOptions {
                        minify: self.minify,
                        targets,
                        ..Default::default()
                    })?
                    .code
                }
                Err(e) => {
                    bail!("Failed to parse CSS: {}", e);
                }
            }
        };

        let ident = self.css.ident().await?;
        let ident_str = ident.path.to_string();
        let content_js = StringifyJs(css_text.as_str());
        let insert_js = StringifyJs(&*self.insert);
        let id_js = StringifyJs(&*ident_str);

        let js = match self.inject_type {
            InjectType::Link => {
                let api_import =
                    StringifyJs("@utoo/pack-runtime/inline_css/injectStylesIntoLinkTag.js");
                formatdoc! {"
                    import api from {api_import};
                    var content = {content_js};

                    var options = {{}};
                    options.insert = {insert_js};

                    var update = api([[{id_js}, content, undefined, undefined]], options);

                    export default {{}};
                "}
            }

            InjectType::LazyStyle | InjectType::LazySingletonStyle => {
                let is_singleton = matches!(self.inject_type, InjectType::LazySingletonStyle);
                let api_import =
                    StringifyJs("@utoo/pack-runtime/inline_css/injectStylesIntoStyleTag.js");
                formatdoc! {"
                    import api from {api_import};
                    var content = {content_js};

                    var refs = 0;
                    var update;
                    var options = {{}};

                    options.insert = {insert_js};
                    options.singleton = {is_singleton};

                    var exported = {{}};

                    exported.locals = {{}};
                    exported.use = function() {{
                      if (!(refs++)) {{
                        update = api([[{id_js}, content, undefined, undefined]], options);
                      }}
                      return exported;
                    }};
                    exported.unuse = function() {{
                      if (refs > 0 && !--refs) {{
                        update();
                        update = null;
                      }}
                    }};

                    export default exported;
                "}
            }

            InjectType::Style | InjectType::SingletonStyle => {
                let is_singleton = matches!(self.inject_type, InjectType::SingletonStyle);
                let api_import =
                    StringifyJs("@utoo/pack-runtime/inline_css/injectStylesIntoStyleTag.js");
                formatdoc! {"
                    import api from {api_import};
                    var content = {content_js};

                    var options = {{}};

                    options.insert = {insert_js};
                    options.singleton = {is_singleton};

                    var update = api([[{id_js}, content, undefined, undefined]], options);

                    export default {{}};
                "}
            }
        };

        Ok(AssetContent::File(FileContent::Content(Rope::from(js).into()).resolved_cell()).cell())
    }
}
