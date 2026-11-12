//! The one grammar for a dependency's version requirement in hpm.

use super::types::RegistryEntry;
use semver::{Version, VersionReq};

/// A dependency's version requirement, as `hpm.toml` and a registry's
/// `deps` write it.
///
/// A full version (`"1.2.3"`) is exactly that version. Anything else is
/// Cargo's grammar (`"^1"`, `"~1.2"`, `">=2, <3"`, `"*"`, and a partial
/// `"1.2"`, which Cargo reads as `^1.2`).
///
/// Which listed version a requirement selects follows one rule
/// ([`Self::select`]): an exact version is selected as written, yanked or
/// not (pinning a yanked release is the user's decision); a range selects
/// the highest version it admits that is not yanked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionRequirement {
    Exact(Version),
    Range(VersionReq),
}

impl VersionRequirement {
    pub fn parse(req: &str) -> Result<Self, semver::Error> {
        let req = req.trim();
        match Version::parse(req) {
            Ok(exact) => Ok(Self::Exact(exact)),
            Err(_) => VersionReq::parse(req).map(Self::Range),
        }
    }

    /// Whether `version` meets the requirement.
    pub fn matches(&self, version: &Version) -> bool {
        match self {
            Self::Exact(exact) => version == exact,
            Self::Range(range) => range.matches(version),
        }
    }

    /// The entry this requirement selects from `entries`: an exact
    /// version's entry whether or not it is yanked, or the highest
    /// non-yanked entry a range admits.
    pub fn select<'a>(&self, entries: &'a [RegistryEntry]) -> Option<&'a RegistryEntry> {
        match self {
            Self::Exact(exact) => entries
                .iter()
                .find(|e| Version::parse(&e.version).is_ok_and(|v| &v == exact)),
            Self::Range(range) => super::highest_matching(entries, range),
        }
    }

    /// The range `hpm update` searches for a newer release: it reads a
    /// pinned `1.2.3` as "compatible with 1.2.3" (`^1.2.3`) and offers the
    /// newest such release. Everywhere else a pin is exact.
    pub fn as_update_range(&self) -> VersionReq {
        match self {
            Self::Exact(exact) => VersionReq::parse(&format!("^{exact}"))
                .expect("a caret over a valid version parses"),
            Self::Range(range) => range.clone(),
        }
    }
}

impl std::fmt::Display for VersionRequirement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exact(exact) => write!(f, "{exact}"),
            Self::Range(range) => write!(f, "{range}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_version_is_exact_and_a_partial_one_is_cargo() {
        let v = |s: &str| Version::parse(s).unwrap();
        let exact = VersionRequirement::parse("1.2.3").unwrap();
        assert!(exact.matches(&v("1.2.3")));
        assert!(!exact.matches(&v("1.2.4")));
        let partial = VersionRequirement::parse("1.2").unwrap();
        assert!(partial.matches(&v("1.9.0")));
        assert!(!partial.matches(&v("2.0.0")));
    }

    /// `hpm update` reads a pin as caret, and a range as itself.
    #[test]
    fn the_update_range_reads_a_pin_as_caret() {
        assert_eq!(
            VersionRequirement::parse("1.2.3")
                .unwrap()
                .as_update_range(),
            VersionReq::parse("^1.2.3").unwrap()
        );
        assert_eq!(
            VersionRequirement::parse(">=1, <2")
                .unwrap()
                .as_update_range(),
            VersionReq::parse(">=1, <2").unwrap()
        );
    }
}
