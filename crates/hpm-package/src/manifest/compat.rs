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
#[serde(deny_unknown_fields)]
pub struct CompatConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub houdini: Option<HoudiniRange>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub platforms: Vec<Platform>,
    /// Oldest glibc the package's Linux binaries may require, as `"2.28"`.
    ///
    /// Absent means [`GlibcVersion::VFX_PLATFORM_BASELINE`]. `hpm pack`
    /// refuses to build a Linux archive whose ELF payload needs a newer glibc
    /// than this, because that failure is otherwise invisible until a user
    /// runs it: the loader rejects the binary outright, and the same package
    /// works on Windows, so it reads as a platform bug rather than a bad
    /// build. Raising it is a deliberate statement that the package does not
    /// support older hosts — not a way to silence the check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glibc: Option<GlibcVersion>,
}

/// A glibc `major.minor` version. Patch levels don't exist in glibc's symbol
/// versioning (`GLIBC_2.39`), so two components is the whole grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GlibcVersion {
    pub major: u32,
    pub minor: u32,
}

impl GlibcVersion {
    /// glibc 2.28 — the [VFX Reference Platform](https://vfxplatform.com/)
    /// requirement for CY2025 and CY2026, i.e. the Houdini 21 and Houdini 22
    /// series, and in practice an EL8-era host. Even the CY2027 draft only
    /// moves to 2.34, so this is the safe default for a Houdini package.
    pub const VFX_PLATFORM_BASELINE: GlibcVersion = GlibcVersion {
        major: 2,
        minor: 28,
    };

    pub const fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }
}

impl std::fmt::Display for GlibcVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

impl std::str::FromStr for GlibcVersion {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || {
            format!(
                "invalid glibc version {:?} (expected \"major.minor\", e.g. \"2.28\")",
                s
            )
        };
        let (major, minor) = s.trim().split_once('.').ok_or_else(invalid)?;
        Ok(Self {
            major: major.parse().map_err(|_| invalid())?,
            minor: minor.parse().map_err(|_| invalid())?,
        })
    }
}

impl Serialize for GlibcVersion {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for GlibcVersion {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

impl CompatConfig {
    pub fn is_empty(&self) -> bool {
        self.houdini.is_none() && self.platforms.is_empty() && self.glibc.is_none()
    }

    /// The glibc floor to hold Linux binaries to, defaulting to the VFX
    /// Reference Platform baseline when the manifest doesn't state one.
    pub fn glibc_floor(&self) -> GlibcVersion {
        self.glibc.unwrap_or(GlibcVersion::VFX_PLATFORM_BASELINE)
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
