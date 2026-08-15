//! CurseForge client bits for kmine. Starts with pulling the Core `x-api-key`
//! out of an official app build; catalog HTTP can live here later.
//!
//! The key is plain text inside uncompressed `app.asar` JS. Feed a `.app` tree,
//! a raw `.asar`, a zip, a `.dmg`, a URL, or any in-memory blob. Nothing is
//! written to disk.

mod asar;
mod dmg;
mod extract;
mod fetch;

pub use extract::{CfCoreKey, CfKeyError, extract_from_bytes, extract_from_path};
pub use fetch::{extract_from_source, extract_from_url, LATEST_MAC_DMG};
