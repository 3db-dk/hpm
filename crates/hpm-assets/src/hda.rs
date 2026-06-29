//! Offline parser for Houdini HDA/OTL digital-asset libraries.
//!
//! Implementation lands in the next commit once the container format is wired
//! up; this stub keeps the crate building behind the public surface.

use crate::asset::Asset;

/// Errors from parsing an HDA/OTL library.
#[derive(Debug, thiserror::Error)]
pub enum HdaParseError {
    /// The bytes are not a recognized HDA/OTL container.
    #[error("not a recognized HDA/OTL library: {0}")]
    NotAnHda(String),
    /// The container index was malformed.
    #[error("malformed HDA index: {0}")]
    MalformedIndex(String),
}

/// Parse the operators defined in an HDA/OTL library from its raw bytes.
///
/// `source_file` is the archive-relative path stamped onto each emitted asset.
pub fn parse_hda_bytes(_bytes: &[u8], _source_file: &str) -> Result<Vec<Asset>, HdaParseError> {
    Ok(Vec::new())
}
