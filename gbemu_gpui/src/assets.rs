use rust_embed::Embed;

#[derive(Embed)]
#[folder = "$CARGO_MANIFEST_DIR/assets/MaterialDesign-SVG-7.4.47/svg/"]
#[include = "*.svg"]
pub struct Icons;

impl gpui::AssetSource for Icons {
    fn load(&self, path: &str) -> gpui::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        if let Some(path) = path.strip_prefix("icons/") {
            Ok(Icons::get(&format!("{path}.svg")).map(|file| file.data))
        } else {
            Ok(None)
        }
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<gpui::SharedString>> {
        if path.starts_with("icons/") {
            Ok(Icons::iter()
                .map(|path: std::borrow::Cow<'static, str>| gpui::SharedString::from(path))
                .collect())
        } else {
            Ok(vec![])
        }
    }
}
