use crate::Engine;
use crate::error::EngineError;
use crate::http::HttpFiles;
use crate::ids::AccountId;
use crate::types::PrepareMode;
use base64::Engine as _;
use image::RgbaImage;
use image::imageops::FilterType;
use serde::Deserialize;
use std::io;
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

const SESSION_PROFILE: &str = "https://sessionserver.mojang.com/session/minecraft/profile";
const FACE_SIZE: u32 = 8;
const FACE_OUT: u32 = 64;

impl Engine {
    pub fn cached_skin_face(&self, id: AccountId) -> Option<PathBuf> {
        let path = face_path(&self.paths.cache_skins, id);
        nonempty_file(&path).then_some(path)
    }

    pub async fn ensure_skin_face(&self, id: AccountId) -> Result<PathBuf, EngineError> {
        if let Some(path) = self.cached_skin_face(id) {
            return Ok(path);
        }
        std::fs::create_dir_all(&self.paths.cache_skins)
            .map_err(|e| EngineError::io(&self.paths.cache_skins, e))?;
        let dest = face_path(&self.paths.cache_skins, id);
        let http = HttpFiles::new()?;
        let cancel = CancellationToken::new();
        let url = fetch_skin_url(&http, id, &cancel).await?;
        let raw_path = self
            .paths
            .cache_skins
            .join(format!("{}.skin.png", id.0.as_simple()));
        http.download_sha1(&url, &raw_path, None, None, &cancel, PrepareMode::Warm)
            .await?;
        let bytes = std::fs::read(&raw_path).map_err(|e| EngineError::io(&raw_path, e))?;
        let face = extract_face(&bytes).map_err(|e| EngineError::io(&raw_path, e))?;
        face.save(&dest)
            .map_err(|e| EngineError::io(&dest, io::Error::other(e.to_string())))?;
        Ok(dest)
    }
}

fn face_path(dir: &Path, id: AccountId) -> PathBuf {
    dir.join(format!("{}.png", id.0.as_simple()))
}

fn nonempty_file(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.len() > 0)
}

async fn fetch_skin_url(
    http: &HttpFiles,
    id: AccountId,
    cancel: &CancellationToken,
) -> Result<String, EngineError> {
    let url = format!("{SESSION_PROFILE}/{}", id.0.as_simple());
    let profile: SessionProfile = http.get_json(&url, cancel).await?;
    let encoded = profile
        .properties
        .into_iter()
        .find(|prop| prop.name == "textures")
        .map(|prop| prop.value)
        .ok_or_else(|| {
            EngineError::io(
                PathBuf::from(&url),
                io::Error::new(io::ErrorKind::NotFound, "profile missing textures"),
            )
        })?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|err| EngineError::io(PathBuf::from(&url), io::Error::other(err.to_string())))?;
    let blob: TextureBlob = serde_json::from_slice(&decoded)
        .map_err(|err| EngineError::io(PathBuf::from(&url), io::Error::other(err.to_string())))?;
    blob.textures
        .skin
        .map(|skin| skin.url)
        .filter(|url| !url.is_empty())
        .ok_or_else(|| {
            EngineError::io(
                PathBuf::from(&url),
                io::Error::new(io::ErrorKind::NotFound, "profile missing skin url"),
            )
        })
}

pub fn extract_face(png: &[u8]) -> io::Result<RgbaImage> {
    let skin = image::load_from_memory(png)
        .map_err(|err| io::Error::other(err.to_string()))?
        .to_rgba8();
    if skin.width() < 48 || skin.height() < 16 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "skin texture too small",
        ));
    }
    let mut face = RgbaImage::new(FACE_SIZE, FACE_SIZE);
    blit(&skin, &mut face, 8, 8);
    overlay_hat(&skin, &mut face);
    Ok(image::imageops::resize(
        &face,
        FACE_OUT,
        FACE_OUT,
        FilterType::Nearest,
    ))
}

fn blit(src: &RgbaImage, dest: &mut RgbaImage, sx: u32, sy: u32) {
    for y in 0..FACE_SIZE {
        for x in 0..FACE_SIZE {
            dest.put_pixel(x, y, *src.get_pixel(sx + x, sy + y));
        }
    }
}

fn overlay_hat(src: &RgbaImage, dest: &mut RgbaImage) {
    for y in 0..FACE_SIZE {
        for x in 0..FACE_SIZE {
            let hat = *src.get_pixel(40 + x, 8 + y);
            if hat[3] > 0 {
                dest.put_pixel(x, y, hat);
            }
        }
    }
}

#[derive(Deserialize)]
struct SessionProfile {
    #[serde(default)]
    properties: Vec<SessionProp>,
}

#[derive(Deserialize)]
struct SessionProp {
    name: String,
    value: String,
}

#[derive(Deserialize)]
struct TextureBlob {
    textures: TextureMap,
}

#[derive(Deserialize)]
struct TextureMap {
    #[serde(rename = "SKIN")]
    skin: Option<TextureUrl>,
}

#[derive(Deserialize)]
struct TextureUrl {
    url: String,
}

#[cfg(test)]
mod tests {
    use super::extract_face;
    use image::{Rgba, RgbaImage};

    #[test]
    fn extract_face_uses_head_and_hat() {
        let mut skin = RgbaImage::new(64, 64);
        for y in 8..16 {
            for x in 8..16 {
                skin.put_pixel(x, y, Rgba([10, 20, 30, 255]));
            }
        }
        skin.put_pixel(40, 8, Rgba([200, 0, 0, 255]));
        let mut png = Vec::new();
        skin.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let face = extract_face(&png).unwrap();
        assert_eq!(face.dimensions(), (64, 64));
        assert_eq!(*face.get_pixel(0, 0), Rgba([200, 0, 0, 255]));
        assert_eq!(*face.get_pixel(8, 0), Rgba([10, 20, 30, 255]));
    }
}
