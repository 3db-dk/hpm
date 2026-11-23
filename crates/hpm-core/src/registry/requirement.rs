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

    // ---- Properties -------------------------------------------------------

    use crate::registry::types::RegistryEntry;
    use proptest::prelude::*;

    /// Small numbers, so generated requirements and versions meet often.
    fn arb_version() -> impl Strategy<Value = Version> {
        (
            0u64..3,
            0u64..3,
            0u64..3,
            prop::option::weighted(0.2, prop::sample::select(&["alpha", "beta.1"][..])),
        )
            .prop_map(|(major, minor, patch, pre)| {
                let mut v = Version::new(major, minor, patch);
                if let Some(pre) = pre {
                    v.pre = semver::Prerelease::new(pre).unwrap();
                }
                v
            })
    }

    /// Every shape the grammar takes: a bare full version, a partial one,
    /// an operator on either, `*`, and two comparators joined.
    fn arb_requirement() -> impl Strategy<Value = String> {
        let partial = prop_oneof![
            arb_version().prop_map(|v| v.to_string()),
            (0u64..3, 0u64..3).prop_map(|(a, b)| format!("{a}.{b}")),
            (0u64..3).prop_map(|a| a.to_string()),
        ];
        let op = prop::sample::select(&["", "=", ">", ">=", "<", "<=", "~", "^"][..]);
        let comparator = (op, partial).prop_map(|(op, v)| format!("{op}{v}"));
        prop_oneof![
            4 => comparator.clone(),
            1 => Just("*".to_string()),
            2 => (comparator.clone(), comparator).prop_map(|(a, b)| format!("{a}, {b}")),
        ]
    }

    fn row(version: &Version, yanked: bool) -> RegistryEntry {
        RegistryEntry {
            name: "acme/rock-tools".to_string(),
            version: version.to_string(),
            cksum: None,
            dl: format!("https://packages.studio.example/rock-tools-{version}.zip"),
            sig: None,
            kid: None,
            houdini_compat: None,
            platform: None,
            yanked,
            description: None,
            author: None,
            created_at: None,
            deps: Vec::new(),
        }
    }

    /// One row per version, as `get_versions` lists them.
    fn arb_listing() -> impl Strategy<Value = Vec<RegistryEntry>> {
        prop::collection::vec((arb_version(), prop::bool::weighted(0.3)), 0..8).prop_map(|rows| {
            let mut seen = std::collections::HashSet::new();
            rows.into_iter()
                .filter(|(v, _)| seen.insert(v.clone()))
                .map(|(v, yanked)| row(&v, yanked))
                .collect()
        })
    }

    proptest! {
        /// What a requirement prints, which is what hpm writes back into a
        /// manifest or a lock, reads back as the same requirement.
        #[test]
        fn prop_a_requirement_reads_back_as_written(req in arb_requirement()) {
            let parsed = VersionRequirement::parse(&req).unwrap();
            prop_assert_eq!(VersionRequirement::parse(&parsed.to_string()).unwrap(), parsed.clone());
            prop_assert_eq!(VersionRequirement::parse(&format!("  {req} ")).unwrap(), parsed);
        }

        /// A bare full version is exact: it meets that version and no other,
        /// whatever Cargo would read it as.
        #[test]
        fn prop_a_full_version_meets_only_itself(exact in arb_version(), other in arb_version()) {
            let req = VersionRequirement::parse(&exact.to_string()).unwrap();
            prop_assert!(matches!(req, VersionRequirement::Exact(_)));
            prop_assert_eq!(req.matches(&other), other == exact);
        }

        /// `select` follows `matches`: an exact requirement selects its own
        /// version, yanked or not; a range selects the highest version it
        /// admits that is not yanked. Nothing is selected only when no
        /// version qualifies, and the order the listing comes in does not
        /// matter.
        #[test]
        fn prop_select_takes_the_best_version_that_meets_the_requirement(
            req in arb_requirement(),
            (listing, shuffled) in arb_listing().prop_flat_map(|l| (Just(l.clone()), Just(l).prop_shuffle())),
        ) {
            let parsed = VersionRequirement::parse(&req).unwrap();
            let version = |e: &RegistryEntry| Version::parse(&e.version).unwrap();
            let qualifies = |e: &&RegistryEntry| {
                parsed.matches(&version(e))
                    && (matches!(parsed, VersionRequirement::Exact(_)) || !e.yanked)
            };
            let best = listing.iter().filter(qualifies).max_by_key(|e| version(e));
            let selected = parsed.select(&listing);
            prop_assert_eq!(selected.map(|e| &e.version), best.map(|e| &e.version));
            prop_assert_eq!(
                parsed.select(&shuffled).map(|e| &e.version),
                selected.map(|e| &e.version)
            );
        }

        /// `hpm update` searches a range that still admits what was pinned,
        /// and leaves a range as it is.
        #[test]
        fn prop_the_update_range_admits_what_the_requirement_does(req in arb_requirement()) {
            let parsed = VersionRequirement::parse(&req).unwrap();
            match &parsed {
                VersionRequirement::Exact(exact) => prop_assert!(parsed.as_update_range().matches(exact)),
                VersionRequirement::Range(range) => prop_assert_eq!(&parsed.as_update_range(), range),
            }
        }
    }
}
