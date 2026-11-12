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
#[derive(Debug, PartialEq, Eq, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

    /// The platform a pack or build targets.
    ///
    /// A package without `platforms` builds one common archive (`None`) and
    /// refuses a requested platform. One with `platforms` targets the
    /// requested platform, or else `host` (the caller passes
    /// [`Platform::current`]), and refuses either when it is not declared.
    pub fn resolve_target_platform(
        &self,
        requested: Option<Platform>,
        host: Option<Platform>,
    ) -> Result<Option<Platform>, TargetPlatformError> {
        let declared = || DeclaredPlatforms(self.platforms.clone());
        match (requested, self.platforms.is_empty()) {
            (None, true) => Ok(None),
            (Some(platform), true) => Err(TargetPlatformError::NotPerPlatform { platform }),
            (Some(platform), false) if self.platforms.contains(&platform) => Ok(Some(platform)),
            (Some(platform), false) => Err(TargetPlatformError::NotDeclared {
                platform,
                declared: declared(),
            }),
            (None, false) => match host {
                None => Err(TargetPlatformError::HostUnknown),
                Some(host) if self.platforms.contains(&host) => Ok(Some(host)),
                Some(host) => Err(TargetPlatformError::HostNotDeclared {
                    host,
                    declared: declared(),
                }),
            },
        }
    }
}

/// The `[compat].platforms` list, rendered as `linux-x86_64, macos-aarch64`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredPlatforms(pub Vec<Platform>);

impl std::fmt::Display for DeclaredPlatforms {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.0.iter().map(Platform::as_str).collect();
        f.write_str(&names.join(", "))
    }
}

/// Why [`CompatConfig::resolve_target_platform`] found no target.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TargetPlatformError {
    #[error(
        "platform {platform} was requested but the package has no [compat].platforms; \
         it packs one archive for every platform"
    )]
    NotPerPlatform { platform: Platform },
    #[error("Platform '{platform}' is not declared in [compat].platforms ({declared})")]
    NotDeclared {
        platform: Platform,
        declared: DeclaredPlatforms,
    },
    #[error(
        "Host platform '{host}' is not declared in [compat].platforms ({declared}); \
         choose one of them as the target platform"
    )]
    HostNotDeclared {
        host: Platform,
        declared: DeclaredPlatforms,
    },
    #[error("Could not detect the host platform; choose the target platform explicitly")]
    HostUnknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every (requested, declared, host) combination over a small platform
    /// set, against the rule stated directly.
    #[test]
    fn resolve_target_platform_over_requested_declared_and_host() {
        use Platform::*;
        let options = [None, Some(LinuxX86_64), Some(MacosAarch64)];
        let declared_sets: [&[Platform]; 3] = [&[], &[LinuxX86_64], &[LinuxX86_64, MacosAarch64]];
        for declared in declared_sets {
            let compat = CompatConfig {
                platforms: declared.to_vec(),
                ..Default::default()
            };
            for requested in options {
                for host in options {
                    let got = compat.resolve_target_platform(requested, host);
                    let expected = if declared.is_empty() {
                        match requested {
                            None => Ok(None),
                            Some(platform) => Err(TargetPlatformError::NotPerPlatform { platform }),
                        }
                    } else {
                        let declared_list = DeclaredPlatforms(declared.to_vec());
                        match (requested, host) {
                            (Some(p), _) if declared.contains(&p) => Ok(Some(p)),
                            (Some(platform), _) => Err(TargetPlatformError::NotDeclared {
                                platform,
                                declared: declared_list,
                            }),
                            (None, None) => Err(TargetPlatformError::HostUnknown),
                            (None, Some(h)) if declared.contains(&h) => Ok(Some(h)),
                            (None, Some(host)) => Err(TargetPlatformError::HostNotDeclared {
                                host,
                                declared: declared_list,
                            }),
                        }
                    };
                    assert_eq!(got, expected, "{declared:?} {requested:?} {host:?}");
                }
            }
        }
    }

    #[test]
    fn target_platform_errors_name_the_declared_platforms() {
        let compat = CompatConfig {
            platforms: vec![Platform::LinuxAarch64, Platform::WindowsX86_64],
            ..Default::default()
        };
        let err = compat
            .resolve_target_platform(Some(Platform::MacosX86_64), None)
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "Platform 'macos-x86_64' is not declared in [compat].platforms (linux-aarch64, windows-x86_64)"
        );
    }

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
