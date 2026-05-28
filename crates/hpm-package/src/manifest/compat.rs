//! `[compat]` configuration: Houdini version range and supported platforms.

use crate::env_value::HoudiniRange;
use crate::platform::Platform;
use serde::{Deserialize, Serialize};

/// Target-environment compatibility for the package.
///
/// `houdini` is a Cargo-style version requirement (`"20.5"`, `"^21"`,
/// `">=20.5, <22"`). Bare versions alias caret semantics: `"20.5"` means
/// `>=20.5, <21`. See [`HoudiniRange`] for the supported grammar.
///
/// `platforms` declares which native platforms this package supports.
/// Pure-data / pure-Python packages omit it (or use `["universal"]`);
/// HDK or DSO packages list the platforms they ship binaries for. The
/// per-platform staging rules live under `[stage.platform.<plat>]`.
///
/// An absent `[compat]` section, or an absent `houdini` field, leaves the
/// package's Houdini compatibility unconstrained — the generated package
/// manifest emits no `enable` clause.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompatConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub houdini: Option<HoudiniRange>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub platforms: Vec<Platform>,
}

impl CompatConfig {
    pub fn is_empty(&self) -> bool {
        self.houdini.is_none() && self.platforms.is_empty()
    }

    /// Lower bound of the Houdini range, used for Python ABI selection.
    pub fn houdini_min(&self) -> Option<String> {
        self.houdini.as_ref().and_then(HoudiniRange::lower_bound)
    }
}
