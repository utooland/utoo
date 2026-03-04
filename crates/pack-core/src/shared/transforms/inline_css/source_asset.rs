use std::io::Write;

use anyhow::{Result, bail};
use turbo_rcstr::{RcStr, rcstr};
use turbo_tasks::{ResolvedVc, Vc};
use turbo_tasks_fs::{FileContent, rope::RopeBuilder};
use turbopack_core::{
    asset::{Asset, AssetContent},
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
    pub insert: RcStr,
    pub inject_type: InjectType,
}

#[turbo_tasks::value_impl]
impl Source for InlineCssFileSource {
    #[turbo_tasks::function]
    fn ident(&self) -> Vc<AssetIdent> {
        self.css
            .ident()
            .with_modifier(rcstr!("inline css"))
            .rename_as("*.js".into())
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
        let css_text = file.content().to_str()?;
        let ident = self.css.ident().await?;
        let ident_str = ident.path.to_string();

        let mut result = RopeBuilder::from("");

        // The runtime API expects arrays of [id, css_content, media_query, source_map].
        // media_query and source_map are undefined as they are not used in the inline CSS case.
        match self.inject_type {
            InjectType::Link => {
                writeln!(
                    result,
                    "import api from {};",
                    StringifyJs("@utoo/pack-runtime/inline_css/injectStylesIntoLinkTag.js")
                )?;
                writeln!(result, "var content = {};", StringifyJs(css_text.as_ref()))?;
                writeln!(result)?;
                writeln!(result, "var options = {{}};")?;
                writeln!(result, "options.insert = {};", StringifyJs(&*self.insert))?;
                writeln!(result)?;
                writeln!(
                    result,
                    "var update = api([[{}, content, undefined, undefined]], options);",
                    StringifyJs(&*ident_str)
                )?;
                writeln!(result)?;
                writeln!(result, "export default {{}};")?;
            }

            InjectType::LazyStyle | InjectType::LazySingletonStyle => {
                let is_singleton = matches!(self.inject_type, InjectType::LazySingletonStyle);
                writeln!(
                    result,
                    "import api from {};",
                    StringifyJs("@utoo/pack-runtime/inline_css/injectStylesIntoStyleTag.js")
                )?;
                writeln!(result, "var content = {};", StringifyJs(css_text.as_ref()))?;
                writeln!(result)?;
                writeln!(result, "var refs = 0;")?;
                writeln!(result, "var update;")?;
                writeln!(result, "var options = {{}};")?;
                writeln!(result)?;
                writeln!(result, "options.insert = {};", StringifyJs(&*self.insert))?;
                writeln!(result, "options.singleton = {};", is_singleton)?;
                writeln!(result)?;
                writeln!(result, "var exported = {{}};")?;
                writeln!(result)?;
                writeln!(result, "exported.locals = {{}};")?;
                writeln!(result, "exported.use = function() {{")?;
                writeln!(result, "  if (!(refs++)) {{")?;
                writeln!(
                    result,
                    "    update = api([[{}, content, undefined, undefined]], options);",
                    StringifyJs(&*ident_str)
                )?;
                writeln!(result, "  }}")?;
                writeln!(result, "  return exported;")?;
                writeln!(result, "}};")?;
                writeln!(result, "exported.unuse = function() {{")?;
                writeln!(result, "  if (refs > 0 && !--refs) {{")?;
                writeln!(result, "    update();")?;
                writeln!(result, "    update = null;")?;
                writeln!(result, "  }}")?;
                writeln!(result, "}};")?;
                writeln!(result)?;
                writeln!(result, "export default exported;")?;
            }

            InjectType::Style | InjectType::SingletonStyle => {
                let is_singleton = matches!(self.inject_type, InjectType::SingletonStyle);
                writeln!(
                    result,
                    "import api from {};",
                    StringifyJs("@utoo/pack-runtime/inline_css/injectStylesIntoStyleTag.js")
                )?;
                writeln!(result, "var content = {};", StringifyJs(css_text.as_ref()))?;
                writeln!(result)?;
                writeln!(result, "var options = {{}};")?;
                writeln!(result)?;
                writeln!(result, "options.insert = {};", StringifyJs(&*self.insert))?;
                writeln!(result, "options.singleton = {};", is_singleton)?;
                writeln!(result)?;
                writeln!(
                    result,
                    "var update = api([[{}, content, undefined, undefined]], options);",
                    StringifyJs(&*ident_str)
                )?;
                writeln!(result)?;
                writeln!(result, "export default {{}};")?;
            }
        }

        Ok(AssetContent::File(FileContent::Content(result.build().into()).resolved_cell()).cell())
    }
}
