use std::io::Write;

use anyhow::{Result, bail};
use base64::{display::Base64Display, engine::general_purpose::STANDARD};
use turbo_rcstr::rcstr;
use turbo_tasks::{ResolvedVc, Vc};
use turbo_tasks_fs::{FileContent, rope::RopeBuilder};
use turbopack_core::{
    asset::{Asset, AssetContent},
    ident::AssetIdent,
    source::Source,
};
use turbopack_ecmascript::utils::StringifyJs;
use turbopack_image::process::{BlurPlaceholderOptions, get_meta_data};

use super::module::BlurPlaceholderMode;

#[turbo_tasks::function]
fn blur_options() -> Vc<BlurPlaceholderOptions> {
    BlurPlaceholderOptions {
        quality: 70,
        size: 8,
    }
    .cell()
}

/// An source asset that transforms an image into javascript code which exports
/// an object with meta information like width, height and a blur placeholder.
#[turbo_tasks::value(shared)]
pub struct StructuredImageFileSource {
    pub image: ResolvedVc<Box<dyn Source>>,
    pub blur_placeholder_mode: BlurPlaceholderMode,
    pub inline_limit: Option<u64>,
}

#[turbo_tasks::value_impl]
impl Source for StructuredImageFileSource {
    #[turbo_tasks::function]
    async fn ident(&self) -> Result<Vc<AssetIdent>> {
        let ident = self
            .image
            .ident()
            .owned()
            .await?
            .with_modifier(rcstr!("structured image object"))
            .rename_as("*.mjs");

        Ok(ident.into_vc())
    }

    #[turbo_tasks::function]
    async fn description(&self) -> Result<Vc<turbo_rcstr::RcStr>> {
        let inner = self.image.description().await?;
        Ok(Vc::cell(
            format!("structured image transform of {inner}").into(),
        ))
    }
}

#[turbo_tasks::value_impl]
impl Asset for StructuredImageFileSource {
    #[turbo_tasks::function]
    async fn content(&self) -> Result<Vc<AssetContent>> {
        let content = self.image.content().await?;
        let AssetContent::File(content) = *content else {
            bail!("Input source is not a file and can't be transformed into image information");
        };
        let mut result = RopeBuilder::from("");

        if let Some(inline_limit) = self.inline_limit {
            if let FileContent::Content(file) = &*content.await? {
                if (file.content().len() as u64) < inline_limit {
                    if let Some(ext) = self.image.ident().await?.path.extension() {
                        if let Some(mime) = mime_guess::from_ext(ext).first() {
                            let data = file.content().to_bytes();
                            let data_url = format!(
                                "data:{mime};base64,{}",
                                Base64Display::new(&data, &STANDARD)
                            );
                            writeln!(result, "export default {};", StringifyJs(&data_url))?;

                            return Ok(AssetContent::File(
                                FileContent::Content(result.build().into()).resolved_cell(),
                            )
                            .cell());
                        } else {
                            bail!("Failed to inline source without known mime type");
                        }
                    } else {
                        bail!("Failed to inline source without known extension");
                    }
                }
            } else {
                bail!("Failed to inline unknown file source");
            }
        }

        let blur_options = blur_options();
        match self.blur_placeholder_mode {
            BlurPlaceholderMode::DataUrl => {
                writeln!(result, "import src from \"IMAGE\";",)?;
                let info = get_meta_data(*self.image, *content, Some(blur_options)).await?;
                writeln!(
                    result,
                    "export default {{ src, width: {width}, height: {height}, blurDataURL: \
                     {blur_data_url}, blurWidth: {blur_width}, blurHeight: {blur_height} }}",
                    width = StringifyJs(&info.width),
                    height = StringifyJs(&info.height),
                    blur_data_url =
                        StringifyJs(&info.blur_placeholder.as_ref().map(|p| p.data_url.as_str())),
                    blur_width =
                        StringifyJs(&info.blur_placeholder.as_ref().map_or(0, |p| p.width)),
                    blur_height =
                        StringifyJs(&info.blur_placeholder.as_ref().map_or(0, |p| p.height),),
                )?;
            }
            BlurPlaceholderMode::None => {
                writeln!(result, "import src from \"IMAGE\";",)?;
                let info = get_meta_data(*self.image, *content, None).await?;
                writeln!(
                    result,
                    "export default {{ src, width: {width}, height: {height} }}",
                    width = StringifyJs(&info.width),
                    height = StringifyJs(&info.height),
                )?;
            }
        };
        Ok(AssetContent::File(FileContent::Content(result.build().into()).resolved_cell()).cell())
    }
}
