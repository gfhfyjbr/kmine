use super::types::CatalogError;
use crate::ids::Loader;

/// Parse a CF-style manifest `minecraft.modLoaders[].id` into loader + version.
///
/// Fabric ids may append `-{minecraft_version}`; that suffix is stripped only when it
/// matches the pack's Minecraft version.
pub fn parse_manifest_loader(
    id: &str,
    minecraft_version: &str,
) -> Result<(Loader, String), CatalogError> {
    let (prefix, rest) = id
        .split_once('-')
        .ok_or_else(|| CatalogError::UnsupportedLoader { raw: id.into() })?;
    let loader = match prefix {
        "forge" => Loader::Forge,
        "fabric" => Loader::Fabric,
        "neoforge" => Loader::NeoForge,
        "quilt" => Loader::Quilt,
        _ => return Err(CatalogError::UnsupportedLoader { raw: id.into() }),
    };
    let version = match rest.rsplit_once('-') {
        Some((head, tail)) if tail == minecraft_version => head.to_string(),
        _ => rest.to_string(),
    };
    if version.is_empty() {
        return Err(CatalogError::UnsupportedLoader { raw: id.into() });
    }
    Ok((loader, version))
}
