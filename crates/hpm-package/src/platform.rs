//! Platform type for multi-architecture packaging.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Canonical platform identifiers recognized by HPM.
///
/// The arch-suffixed variants (`<os>-x86_64` / `<os>-aarch64`) plus the OS-
/// agnostic `Universal` match the TumbleTrove registry's `build.platform`
/// enum, so a packed archive can be registered without renaming.
///
/// The legacy `macos-universal` identifier was dropped in v0.13 — pick the
/// concrete arch (`MacosX86_64` / `MacosAarch64`) or `Universal` for OS-
/// agnostic content. Existing manifests carrying `macos-universal` fail to
/// parse and must be migrated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Platform {
    LinuxX86_64,
    LinuxAarch64,
    MacosX86_64,
    MacosAarch64,
    WindowsX86_64,
    WindowsAarch64,
    /// OS-agnostic content (pure-Python / data packages). Matches the
    /// TumbleTrove API's `"universal"` build platform.
    Universal,
}

impl Platform {
    /// Detect the current host platform.
    ///
    /// Returns the arch-suffixed variant matching `std::env::consts::OS`
    /// and `std::env::consts::ARCH`. Never returns [`Platform::Universal`] —
    /// that is an intent-bearing identifier a user opts into, not a state a
    /// host machine is in.
    pub fn current() -> Option<Self> {
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("linux", "x86_64") => Some(Self::LinuxX86_64),
            ("linux", "aarch64") => Some(Self::LinuxAarch64),
            ("macos", "x86_64") => Some(Self::MacosX86_64),
            ("macos", "aarch64") => Some(Self::MacosAarch64),
            ("windows", "x86_64") => Some(Self::WindowsX86_64),
            ("windows", "aarch64") => Some(Self::WindowsAarch64),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LinuxX86_64 => "linux-x86_64",
            Self::LinuxAarch64 => "linux-aarch64",
            Self::MacosX86_64 => "macos-x86_64",
            Self::MacosAarch64 => "macos-aarch64",
            Self::WindowsX86_64 => "windows-x86_64",
            Self::WindowsAarch64 => "windows-aarch64",
            Self::Universal => "universal",
        }
    }

    /// Short OS identifier used for platform-scoped manifest sections
    /// (e.g. `[scripts.platform.<os>]`).
    ///
    /// Returns `None` for [`Platform::Universal`], which has no OS to scope
    /// against — platform-scoped script overrides simply don't apply to a
    /// universal target.
    pub fn os_key(&self) -> Option<&'static str> {
        match self {
            Self::LinuxX86_64 | Self::LinuxAarch64 => Some("linux"),
            Self::MacosX86_64 | Self::MacosAarch64 => Some("macos"),
            Self::WindowsX86_64 | Self::WindowsAarch64 => Some("windows"),
            Self::Universal => None,
        }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Platform {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "linux-x86_64" => Ok(Self::LinuxX86_64),
            "linux-aarch64" => Ok(Self::LinuxAarch64),
            "macos-x86_64" => Ok(Self::MacosX86_64),
            "macos-aarch64" => Ok(Self::MacosAarch64),
            "windows-x86_64" => Ok(Self::WindowsX86_64),
            "windows-aarch64" => Ok(Self::WindowsAarch64),
            "universal" => Ok(Self::Universal),
            _ => Err(format!(
                "unknown platform '{}'; expected one of: linux-x86_64, linux-aarch64, macos-x86_64, macos-aarch64, windows-x86_64, windows-aarch64, universal",
                s
            )),
        }
    }
}

impl TryFrom<String> for Platform {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<Platform> for String {
    fn from(p: Platform) -> Self {
        p.as_str().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_VARIANTS: &[Platform] = &[
        Platform::LinuxX86_64,
        Platform::LinuxAarch64,
        Platform::MacosX86_64,
        Platform::MacosAarch64,
        Platform::WindowsX86_64,
        Platform::WindowsAarch64,
        Platform::Universal,
    ];

    #[test]
    fn round_trip_all_variants() {
        for platform in ALL_VARIANTS {
            let s = platform.to_string();
            let parsed: Platform = s.parse().unwrap();
            assert_eq!(parsed, *platform);
        }
    }

    #[test]
    fn arch_suffixed_variants_parse() {
        // Spot-check: arch-suffixed identifiers match the TumbleTrove API
        // enum verbatim.
        assert_eq!(
            "linux-aarch64".parse::<Platform>().unwrap(),
            Platform::LinuxAarch64
        );
        assert_eq!(
            "macos-x86_64".parse::<Platform>().unwrap(),
            Platform::MacosX86_64
        );
        assert_eq!(
            "macos-aarch64".parse::<Platform>().unwrap(),
            Platform::MacosAarch64
        );
        assert_eq!(
            "windows-aarch64".parse::<Platform>().unwrap(),
            Platform::WindowsAarch64
        );
        assert_eq!(
            "universal".parse::<Platform>().unwrap(),
            Platform::Universal
        );
    }

    #[test]
    fn legacy_macos_universal_rejected() {
        // Removed in v0.13. Manifests still carrying it must migrate to
        // macos-x86_64 / macos-aarch64 (or universal for OS-agnostic content).
        let err = "macos-universal".parse::<Platform>().unwrap_err();
        assert!(err.contains("macos-universal"));
    }

    #[test]
    fn invalid_platform_rejected() {
        assert!("linux-arm64".parse::<Platform>().is_err());
        assert!("darwin-universal".parse::<Platform>().is_err());
        assert!("".parse::<Platform>().is_err());
        // Common typo / wrong-case forms are rejected — parsing is exact.
        assert!("Linux-X86_64".parse::<Platform>().is_err());
    }

    #[test]
    fn error_message_lists_new_identifiers() {
        let err = "bogus".parse::<Platform>().unwrap_err();
        assert!(err.contains("linux-aarch64"));
        assert!(err.contains("macos-aarch64"));
        assert!(err.contains("universal"));
    }

    #[test]
    fn serde_round_trip() {
        for platform in ALL_VARIANTS {
            let json = serde_json::to_string(platform).unwrap();
            assert_eq!(json, format!("\"{}\"", platform.as_str()));
            let parsed: Platform = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, *platform);
        }
    }

    #[test]
    fn os_key_groups_by_os_for_native_variants() {
        assert_eq!(Platform::LinuxX86_64.os_key(), Some("linux"));
        assert_eq!(Platform::LinuxAarch64.os_key(), Some("linux"));
        assert_eq!(Platform::MacosX86_64.os_key(), Some("macos"));
        assert_eq!(Platform::MacosAarch64.os_key(), Some("macos"));
        assert_eq!(Platform::WindowsX86_64.os_key(), Some("windows"));
        assert_eq!(Platform::WindowsAarch64.os_key(), Some("windows"));
    }

    #[test]
    fn os_key_is_none_for_universal() {
        // Universal is OS-agnostic — platform-scoped script overrides don't
        // apply to it.
        assert_eq!(Platform::Universal.os_key(), None);
    }

    #[test]
    fn current_platform_is_some() {
        // On any CI/dev machine this should detect something
        let current = Platform::current();
        assert!(current.is_some());
    }

    #[test]
    fn current_never_returns_universal() {
        // Auto-detection picks the concrete arch-suffixed variant for the
        // host. Universal is a user-declared intent, never auto-detected.
        let current = Platform::current().unwrap();
        assert_ne!(current, Platform::Universal);
    }
}
