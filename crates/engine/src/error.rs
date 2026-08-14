use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("Microsoft CLIENT_ID is not configured")]
    AuthNotConfigured,
    #[error("account session expired")]
    AuthExpired,
    #[error("auth failed: {message}")]
    AuthFailed { message: String },
    #[error("this Microsoft account does not own Minecraft")]
    MinecraftNotOwned,
    #[error("a login is already in progress")]
    LoginInProgress,
    #[error("no Microsoft account selected")]
    NoAccount,
    #[error("Minecraft version not found: {id}")]
    VersionNotFound { id: String },
    #[error("no {loader:?} build for Minecraft {minecraft}")]
    LoaderUnavailable {
        loader: crate::ids::Loader,
        minecraft: String,
    },
    #[error("checksum mismatch for {path:?}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },
    #[error("java binary not found")]
    JavaNotFound,
    #[error("sandbox unavailable: {reason}")]
    SandboxUnavailable { reason: String },
    #[error("cancelled")]
    Cancelled,
    #[error("instance is already preparing or running")]
    InstanceBusy,
    #[error("io error at {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("crypto failure")]
    Crypto,
    #[error("http {status} for {url}")]
    Http { url: String, status: u16 },
}

impl EngineError {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
