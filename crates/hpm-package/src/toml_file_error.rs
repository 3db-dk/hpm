//! Shared error type for loading and saving TOML-on-disk files.
//!
//! Both `hpm_config::ConfigError` and `hpm_core::lock::LockError` carried
//! the same four variants — Read / Parse / Serialize / Write — with
//! identical `{ path, source }` shapes. Any TOML file we read or write
//! has the same three failure modes (IO at read/write time, parse failure,
//! serialize failure), so consolidating into one type lets us fix any
//! bug across all consumers in one place.

use crate::IoOp;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum TomlFileError {
    /// IO failure on read or write. `IoOp::op` distinguishes ("read TOML
    /// file", "write TOML file"), so transparent forwarding via Display
    /// remains specific.
    #[error(transparent)]
    Io(#[from] IoOp),

    /// The bytes were read but did not parse as TOML.
    #[error("Failed to parse TOML file: {path}")]
    Parse {
        path: PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },

    /// Serialization to TOML failed.
    #[error("Failed to serialize TOML")]
    Serialize(#[from] toml::ser::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;
    use std::io;
    use std::path::Path;

    #[test]
    fn io_forwards_to_iop_display() {
        let err: TomlFileError = IoOp::wrap(
            "read TOML file",
            Path::new("/etc/hpm/config.toml"),
            io::Error::from(io::ErrorKind::NotFound),
        )
        .into();
        // `#[error(transparent)]` forwards Display verbatim — caller-side
        // messages stay specific via the IoOp's `op` field.
        assert_eq!(
            err.to_string(),
            "failed to read TOML file /etc/hpm/config.toml"
        );
    }

    #[test]
    fn parse_carries_path_and_underlying_source() {
        let parse_err = toml::from_str::<toml::Value>("=").unwrap_err();
        let err = TomlFileError::Parse {
            path: PathBuf::from("/tmp/bad.toml"),
            source: Box::new(parse_err),
        };
        assert!(err.to_string().contains("/tmp/bad.toml"));
        assert!(err.source().is_some());
    }
}
