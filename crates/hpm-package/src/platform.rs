//! Platform type for multi-architecture packaging.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Canonical platform identifiers recognized by HPM.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Platform {
    LinuxX86_64,
    MacosUniversal,
    WindowsX86_64,
}

impl Platform {
    /// Detect the current host platform.
    pub fn current() -> Option<Self> {
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("linux", "x86_64") => Some(Self::LinuxX86_64),
            ("macos", _) => Some(Self::MacosUniversal),
            ("windows", "x86_64") => Some(Self::WindowsX86_64),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LinuxX86_64 => "linux-x86_64",
            Self::MacosUniversal => "macos-universal",
            Self::WindowsX86_64 => "windows-x86_64",
        }
    }

    /// Short OS identifier used for platform-scoped manifest sections
    /// (e.g. `[scripts.platform.<os>]`).
    pub fn os_key(&self) -> &'static str {
        match self {
            Self::LinuxX86_64 => "linux",
            Self::MacosUniversal => "macos",
            Self::WindowsX86_64 => "windows",
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
            "macos-universal" => Ok(Self::MacosUniversal),
            "windows-x86_64" => Ok(Self::WindowsX86_64),
            _ => Err(format!(
                "unknown platform '{}'; expected one of: linux-x86_64, macos-universal, windows-x86_64",
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

    #[test]
    fn round_trip_all_variants() {
        for platform in [
            Platform::LinuxX86_64,
            Platform::MacosUniversal,
            Platform::WindowsX86_64,
        ] {
            let s = platform.to_string();
            let parsed: Platform = s.parse().unwrap();
            assert_eq!(parsed, platform);
        }
    }

    #[test]
    fn invalid_platform_rejected() {
        assert!("linux-arm64".parse::<Platform>().is_err());
        assert!("darwin-universal".parse::<Platform>().is_err());
        assert!("".parse::<Platform>().is_err());
    }

    #[test]
    fn serde_round_trip() {
        let platform = Platform::MacosUniversal;
        let json = serde_json::to_string(&platform).unwrap();
        assert_eq!(json, "\"macos-universal\"");
        let parsed: Platform = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, platform);
    }

    #[test]
    fn current_platform_is_some() {
        // On any CI/dev machine this should detect something
        let current = Platform::current();
        assert!(current.is_some());
    }
}
