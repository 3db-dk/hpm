//! Locating Houdini's per-user preferences directory.
//!
//! Houdini scans several directories for `packages/*.json` at startup; the
//! first is the user preferences directory, which is per-version and lives
//! outside any project. Writing a manifest there makes a package load in
//! every session of that Houdini version, with no launcher and no project.
//!
//! This is the one place in hpm that knows anything about a Houdini
//! *installation's* conventions rather than about hpm's own storage.
//!
//! Reference: <https://www.sidefx.com/docs/houdini/basics/config.html>

use hpm_package::IoOp;
use std::path::PathBuf;
use thiserror::Error;

/// A Houdini `major.minor` release line, e.g. `21.0`.
///
/// Only the two leading components matter: the preferences directory is
/// shared by every build within a `major.minor` line, so `21.0.729` and
/// `21.0.440` both read `houdini21.0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HoudiniVersion {
    pub major: u32,
    pub minor: u32,
    /// Build number, when the user supplied a full version string.
    ///
    /// Not part of the preferences directory — that is per `major.minor` —
    /// but it is what lets a `[compat].houdini` range with a build-level
    /// bound (`">=20.5.445"`) be answered precisely instead of approximated.
    pub build: Option<u32>,
}

#[derive(Debug, Error)]
pub enum HoudiniPrefsError {
    #[error(
        "'{0}' is not a Houdini version. Expected 'major.minor' (e.g. '21.0'), \
         optionally with a build (e.g. '21.0.729')."
    )]
    VersionParse(String),

    #[error(
        "Cannot locate the Houdini user preferences directory: no home directory. \
         Set HOUDINI_USER_PREF_DIR, or set {var} to your home directory."
    )]
    NoHome { var: &'static str },

    #[error(transparent)]
    Io(#[from] IoOp),
}

impl HoudiniVersion {
    /// Parse `major.minor` or `major.minor.build`, keeping the first two
    /// components. A bare major (`"21"`) is accepted as `21.0` — Houdini's
    /// own directory naming always carries a minor.
    pub fn parse(input: &str) -> Result<Self, HoudiniPrefsError> {
        let bad = || HoudiniPrefsError::VersionParse(input.to_string());
        let mut parts = input.split('.');
        let major = parts.next().ok_or_else(bad)?;
        if major.is_empty() {
            return Err(bad());
        }
        let major: u32 = major.parse().map_err(|_| bad())?;
        let minor = match parts.next() {
            None => 0,
            Some(m) => m.parse().map_err(|_| bad())?,
        };
        // The third component is the build number. It is kept (compat ranges
        // may bound on it) and must be numeric, so a typo like "21.0.x" is
        // rejected rather than silently treated as 21.0.
        let build = match parts.next() {
            None => None,
            Some(b) => Some(b.parse::<u32>().map_err(|_| bad())?),
        };
        for extra in parts {
            extra
                .parse::<u32>()
                .map_err(|_| HoudiniPrefsError::VersionParse(input.to_string()))?;
        }
        Ok(Self {
            major,
            minor,
            build,
        })
    }

    /// Rendered as Houdini writes it in directory names: `21.0`.
    pub fn as_dir_component(&self) -> String {
        format!("{}.{}", self.major, self.minor)
    }
}

impl std::fmt::Display for HoudiniVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.build {
            Some(build) => write!(f, "{}.{}.{}", self.major, self.minor, build),
            None => write!(f, "{}.{}", self.major, self.minor),
        }
    }
}

/// The `HOUDINI_USER_PREF_DIR` placeholder Houdini expands to `major.minor`.
const HVER_PLACEHOLDER: &str = "__HVER__";

/// Resolve the user preferences directory for `version`.
///
/// `override_dir` is the value of `HOUDINI_USER_PREF_DIR` when set. Houdini
/// honours it ahead of the platform default and expands `__HVER__` inside it
/// to `major.minor`, so hpm has to do the same or it would write to a
/// directory the user's Houdini never reads.
///
/// Taken as a parameter rather than read from the environment here so the
/// mapping stays a pure function — process-wide env mutation is not safe to
/// do from tests running in parallel.
pub fn user_pref_dir_with_override(
    version: HoudiniVersion,
    override_dir: Option<&str>,
) -> Result<PathBuf, HoudiniPrefsError> {
    if let Some(raw) = override_dir.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(PathBuf::from(
            raw.replace(HVER_PLACEHOLDER, &version.as_dir_component()),
        ));
    }

    let home = hpm_package::user_home().ok_or(HoudiniPrefsError::NoHome {
        var: if cfg!(windows) { "USERPROFILE" } else { "HOME" },
    })?;

    // Houdini's per-platform conventions. These differ in shape, not just in
    // prefix: macOS nests the version under a `houdini` directory, the other
    // two suffix it onto the directory name.
    Ok(if cfg!(target_os = "macos") {
        home.join("Library")
            .join("Preferences")
            .join("houdini")
            .join(version.as_dir_component())
    } else if cfg!(windows) {
        home.join("Documents")
            .join(format!("houdini{}", version.as_dir_component()))
    } else {
        home.join(format!("houdini{}", version.as_dir_component()))
    })
}

/// [`user_pref_dir_with_override`] using the process's `HOUDINI_USER_PREF_DIR`.
pub fn user_pref_dir(version: HoudiniVersion) -> Result<PathBuf, HoudiniPrefsError> {
    let override_dir = std::env::var("HOUDINI_USER_PREF_DIR").ok();
    user_pref_dir_with_override(version, override_dir.as_deref())
}

/// The `packages/` directory Houdini scans inside the preferences directory.
pub fn user_packages_dir(version: HoudiniVersion) -> Result<PathBuf, HoudiniPrefsError> {
    Ok(user_pref_dir(version)?.join("packages"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_major_minor() {
        let v = HoudiniVersion::parse("21.0").unwrap();
        assert_eq!((v.major, v.minor), (21, 0));
        assert_eq!(v.as_dir_component(), "21.0");
    }

    #[test]
    fn parses_full_build_string_keeping_major_minor() {
        // Users copy the build number straight out of Houdini's About box.
        let v = HoudiniVersion::parse("21.0.729").unwrap();
        assert_eq!(v.as_dir_component(), "21.0");
    }

    #[test]
    fn bare_major_defaults_to_zero_minor() {
        assert_eq!(
            HoudiniVersion::parse("22").unwrap().as_dir_component(),
            "22.0"
        );
    }

    #[test]
    fn rejects_non_numeric() {
        for bad in ["", "houdini21", "21.x", "21.0.x", "-1", "21..0"] {
            assert!(
                HoudiniVersion::parse(bad).is_err(),
                "{bad:?} should not parse"
            );
        }
    }

    #[test]
    fn override_wins_over_platform_default() {
        let v = HoudiniVersion::parse("21.0").unwrap();
        let dir = user_pref_dir_with_override(v, Some("/custom/prefs")).unwrap();
        assert_eq!(dir, PathBuf::from("/custom/prefs"));
    }

    /// Houdini expands `__HVER__` in HOUDINI_USER_PREF_DIR. If hpm didn't,
    /// it would write to a literal `__HVER__` directory that Houdini never
    /// reads, and the package would silently fail to load.
    #[test]
    fn override_expands_hver_placeholder() {
        let v = HoudiniVersion::parse("22.0").unwrap();
        let dir = user_pref_dir_with_override(v, Some("/studio/prefs/__HVER__")).unwrap();
        assert_eq!(dir, PathBuf::from("/studio/prefs/22.0"));
    }

    #[test]
    fn blank_override_falls_back_to_default() {
        let v = HoudiniVersion::parse("21.0").unwrap();
        let from_blank = user_pref_dir_with_override(v, Some("   ")).unwrap();
        let from_unset = user_pref_dir_with_override(v, None).unwrap();
        assert_eq!(from_blank, from_unset);
    }

    #[test]
    fn default_matches_the_platform_convention() {
        let v = HoudiniVersion::parse("21.0").unwrap();
        let dir = user_pref_dir_with_override(v, None).unwrap();
        let shown = dir.to_string_lossy().replace('\\', "/");

        if cfg!(target_os = "macos") {
            assert!(
                shown.ends_with("Library/Preferences/houdini/21.0"),
                "got {shown}"
            );
        } else if cfg!(windows) {
            assert!(shown.ends_with("Documents/houdini21.0"), "got {shown}");
        } else {
            assert!(shown.ends_with("houdini21.0"), "got {shown}");
        }
    }
}
