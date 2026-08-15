use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

const CUSTOM: &[(&str, &[u8])] = &[
    ("icons/pin.svg", include_bytes!("../assets/icons/pin.svg")),
    (
        "icons/pin-fill.svg",
        include_bytes!("../assets/icons/pin-fill.svg"),
    ),
    (
        "icons/trash.svg",
        include_bytes!("../assets/icons/trash.svg"),
    ),
];

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() {
            return Ok(None);
        }
        if let Some((_, bytes)) = CUSTOM.iter().find(|(name, _)| *name == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        gpui_component_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut items: Vec<SharedString> = CUSTOM
            .iter()
            .filter_map(|(name, _)| name.starts_with(path).then(|| (*name).into()))
            .collect();
        match gpui_component_assets::Assets.list(path) {
            Ok(rest) => items.extend(rest),
            Err(err) if items.is_empty() => return Err(err),
            Err(_) => {}
        }
        Ok(items)
    }
}
