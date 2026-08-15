#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("http {status} for {url}")]
    Http { url: String, status: u16 },
    #[error("{kind:?} {id} not found")]
    NotFound { kind: ResourceKind, id: u32 },
    #[error("no download url for mod {mod_id} file {file_id}")]
    NoDownloadUrl { mod_id: u32, file_id: u32 },
    #[error("no compatible file for mod {mod_id} on {game_version}")]
    NoCompatibleFile { mod_id: u32, game_version: String },
    #[error("checksum mismatch for file {file_id}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        file_id: u32,
        expected: String,
        actual: String,
    },
    #[error("decode {url}: {message}")]
    Decode { url: String, message: String },
    #[error("manifest: {message}")]
    Manifest { message: String },
    #[error("zip: {message}")]
    Zip { message: String },
    #[error("invalid query: {message}")]
    InvalidQuery { message: &'static str },
    #[error("client builder: {message}")]
    Builder { message: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    Mod,
    File,
}
