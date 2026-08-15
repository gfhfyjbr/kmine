//! CurseForge Core client for Minecraft, plus official-app key extraction.
//!
//! The catalog client never writes to disk. The key extractor reads an official
//! build and also writes nothing.

mod asar;
mod client;
mod dmg;
mod error;
mod extract;
mod fetch;
mod fingerprint;
mod manifest;
mod search;
mod types;

pub use client::{Client, ClientBuilder};
pub use error::{Error, ResourceKind};
pub use extract::{CfCoreKey, CfKeyError, extract_from_bytes, extract_from_path};
pub use fetch::{LATEST_MAC_DMG, extract_from_source, extract_from_url};
pub use fingerprint::fingerprint;
pub use manifest::{Manifest, ManifestFile, ManifestLoader, ManifestMinecraft};
pub use search::{CategoryFilter, FileFilter, SearchQuery};
pub use types::*;
