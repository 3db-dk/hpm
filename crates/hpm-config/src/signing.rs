//! Package-signing configuration: where to find the signing key when none is
//! passed on the command line or in the environment.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SigningConfig {
    /// Path to an Ed25519 PKCS#8 PEM signing key used as a fallback when neither
    /// `--key` nor `HPM_SIGNING_KEY` is set.
    pub key_path: Option<PathBuf>,
}
