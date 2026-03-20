/// Inline CSS module — a Rust-native reimplementation of webpack's style-loader
/// that injects CSS into the DOM at runtime via `<style>` or `<link>` tags.
///
/// Reference: <https://webpack.js.org/loaders/style-loader/>
pub(crate) mod module;
pub(crate) mod source_asset;

pub use module::InlineCssModuleType;
