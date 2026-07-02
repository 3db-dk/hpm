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

    /// True when the package declares support for a concrete native platform
    /// (any [`Platform`] other than [`Platform::Universal`]) — i.e. it ships
    /// per-platform native binaries (HDK/DSO). Pure-data / pure-Python
    /// packages declare nothing here, or only `universal`, and return `false`.
    ///
    /// Used to steer dev installs away from link-mode for native packages: a
    /// junction/symlink makes the workspace build output the very DSO a
    /// running Houdini has memory-mapped, blocking in-place rebuilds.
    pub fn declares_native_platforms(&self) -> bool {
        self.platforms.iter().any(|p| *p != Platform::Universal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_platforms_is_not_native() {
        assert!(!CompatConfig::default().declares_native_platforms());
    }

    #[test]
    fn universal_only_is_not_native() {
        let compat = CompatConfig {
            platforms: vec![Platform::Universal],
            ..Default::default()
        };
        assert!(!compat.declares_native_platforms());
    }

    #[test]
    fn any_concrete_platform_is_native() {
        let compat = CompatConfig {
            platforms: vec![Platform::Universal, Platform::WindowsX86_64],
            ..Default::default()
        };
        assert!(compat.declares_native_platforms());
    }
}
