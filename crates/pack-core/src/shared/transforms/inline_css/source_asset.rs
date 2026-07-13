use anyhow::Result;
use indoc::formatdoc;
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{ResolvedVc, Vc};
use turbo_tasks_fs::{FileContent, rope::Rope};
use turbopack_core::{
    asset::{Asset, AssetContent},
    ident::AssetIdent,
    source::Source,
};
use turbopack_ecmascript::utils::StringifyJs;

use super::module::InjectType;

pub(super) const INLINE_CSS_CONTENT: &str = "INLINE_CSS_CONTENT";

/// A source asset that transforms a CSS file into JavaScript code which
/// injects the styles into the DOM at runtime.
#[turbo_tasks::value(shared)]
pub struct InlineCssFileSource {
    pub css: ResolvedVc<Box<dyn Source>>,
    pub insert: RcStr,
    pub inject_type: InjectType,
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
        let ident = self.css.ident().await?;
        let ident_str = ident.path.to_string();
        let content_import = StringifyJs(INLINE_CSS_CONTENT);
        let insert_js = StringifyJs(&*self.insert);
        let id_js = StringifyJs(&*ident_str);

        let js = match self.inject_type {
            InjectType::Link => {
                let api_import =
                    StringifyJs("@utoo/pack-runtime/inline_css/injectStylesIntoLinkTag.js");
                formatdoc! {"
                    import content from {content_import};
                    import api from {api_import};

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
                    import content from {content_import};
                    import api from {api_import};

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
                    import content from {content_import};
                    import api from {api_import};

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
