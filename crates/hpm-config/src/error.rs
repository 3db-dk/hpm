//! Error type for configuration loading and persistence.
//!
//! Configuration is just a TOML-on-disk file, with no failure modes beyond
//! what `hpm_package::TomlFileError` already models. `ConfigError` is kept
//! as a name for backwards source-level clarity at call sites but is the
//! shared TOML file error.

pub use hpm_package::TomlFileError as ConfigError;
