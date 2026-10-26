//! Choose one version of each package so the set works together.
//!
//! A project names its packages, some pinned and some not. Taking the newest
//! version of each unpinned package on its own is what every caller did
//! before this module, and it breaks in two ways the registry can already
//! describe: a version that does not support the project's Houdini (its
//! `houdini_compat`), and a version whose requirements of another package in
//! the set (its `deps`) the version chosen for that package does not meet.
//! The second is how a TumbleTrove release that removed an API TumblePipe
//! 1.74 called reached every project still on 1.74, although 1.74 declared
//! `tumbletrove = ">=0.47, <0.48"` in its manifest.
//!
//! [`solve`] is pure: callers fetch each package's version list
//! ([`RegistrySet::get_versions`](super::RegistrySet::get_versions)) and pass
//! it in, so the rules are testable without a registry and every caller that
//! needs "the version this project uses" asks the same function.
//!
//! The rules:
//!
//! - **A pin is the user's decision.** It is selected as written, yanked or
//!   not, Houdini range or not, and even when the registry does not list it
//!   (the install reports that, not the solver). It still takes part in the
//!   requirements: an unpinned package moves to fit it, and two pins that
//!   cannot coexist are a [`SolveError::Conflict`].
//! - **An unpinned package takes the newest version that fits.** Candidates
//!   are the non-yanked versions whose `houdini_compat` admits the target
//!   Houdini, newest first, and the search backtracks, so the result is the
//!   first acceptable assignment in input order: earlier packages get their
//!   newest fitting version before later ones are considered.
//! - **Requirements constrain the set; they do not extend it.** A `deps`
//!   entry naming a package outside the set is ignored here. A caller that
//!   wants such packages installed adds them to the set and solves again
//!   until nothing new is required (TumbleTrove Desktop does), since only
//!   it knows which packages the user has switched off.

use super::types::RegistryEntry;
use hpm_package::HoudiniRange;
use semver::{Version, VersionReq};
use std::fmt;

/// The Houdini a project targets, for filtering versions by their
/// `houdini_compat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoudiniTarget {
    pub major: u32,
    pub minor: u32,
    /// The build, when the project names one. `None` evaluates a range
    /// against the newest build of the line (see
    /// [`HoudiniRange::matches_version`]).
    pub build: Option<u32>,
}

impl HoudiniTarget {
    /// Parse `MAJOR.MINOR` or `MAJOR.MINOR.BUILD`. A bare major is refused:
    /// it names a span of lines, not a target a range can be tested against.
    pub fn parse(s: &str) -> Option<Self> {
        let mut parts = s.trim().split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let build = match parts.next() {
            Some(p) => Some(p.parse().ok()?),
            None => None,
        };
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            build,
        })
    }

    /// Whether `entry` declares support for this Houdini. A version with no
    /// range supports every Houdini; a range that does not parse is admitted,
    /// as [`HoudiniRange::matches_version`] does, so a parser disagreement
    /// defers to Houdini's own `enable` evaluation instead of hiding a
    /// version.
    pub fn admits(&self, entry: &RegistryEntry) -> bool {
        match entry.houdini_compat.as_deref().map(str::trim) {
            None | Some("") => true,
            Some(range) => HoudiniRange::parse(range)
                .map(|r| r.matches_version(self.major, self.minor, self.build))
                .unwrap_or(true),
        }
    }
}

impl fmt::Display for HoudiniTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.build {
            Some(build) => write!(f, "{}.{}.{}", self.major, self.minor, build),
            None => write!(f, "{}.{}", self.major, self.minor),
        }
    }
}

/// One package of the set: its scoped path, its pin if any, and the
/// registry's version list for it (one entry per version, as
/// [`RegistrySet::get_versions`](super::RegistrySet::get_versions) returns).
#[derive(Debug, Clone, Copy)]
pub struct Root<'a> {
    pub name: &'a str,
    pub pin: Option<&'a str>,
    pub versions: &'a [RegistryEntry],
}

/// The version chosen for one [`Root`]. `entry` is `None` only for a pin the
/// registry does not list.
#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    pub name: String,
    pub version: String,
    pub entry: Option<RegistryEntry>,
}

/// Why a version was ruled out, for [`SolveError::Conflict`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    pub version: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SolveError {
    /// Every version of an unpinned package is yanked.
    #[error("every version of {name} is yanked")]
    NoUsableVersion { name: String },
    /// No non-yanked version of an unpinned package supports the target.
    #[error("no version of {name} supports Houdini {houdini}")]
    NoVersionForHoudini { name: String, houdini: String },
    /// The packages' requirements of each other cannot all be met. `name` is
    /// the package the search could not place; `rejected` says why its
    /// candidates were refused.
    #[error(
        "no version of {name} works with the other packages: {}",
        describe(rejected)
    )]
    Conflict {
        name: String,
        rejected: Vec<Rejection>,
    },
    /// The search gave up. Not reachable at the sizes a project has; it
    /// bounds a pathological registry rather than looping on it.
    #[error("the package versions could not be resolved within {0} steps")]
    TooComplex(usize),
}

fn describe(rejected: &[Rejection]) -> String {
    const SHOWN: usize = 3;
    let mut parts: Vec<String> = rejected
        .iter()
        .take(SHOWN)
        .map(|r| format!("{} {}", r.version, r.reason))
        .collect();
    if rejected.len() > SHOWN {
        parts.push(format!("and {} older", rejected.len() - SHOWN));
    }
    parts.join("; ")
}

/// Search budget, in candidate checks.
const MAX_STEPS: usize = 200_000;

struct Candidate<'a> {
    version: String,
    semver: Option<Version>,
    entry: Option<&'a RegistryEntry>,
}

impl Candidate<'_> {
    fn deps(&self) -> &[super::types::RegistryDep] {
        self.entry.map(|e| e.deps.as_slice()).unwrap_or(&[])
    }
}

/// Choose a version for every root. See the module docs for the rules.
/// Selections come back in the order of `roots`; a name given twice is
/// solved once, at its first position.
pub fn solve(
    roots: &[Root<'_>],
    houdini: Option<HoudiniTarget>,
) -> Result<Vec<Selection>, SolveError> {
    let mut unique: Vec<&Root<'_>> = Vec::new();
    for root in roots {
        if !unique.iter().any(|r| r.name == root.name) {
            unique.push(root);
        }
    }

    let mut candidates: Vec<Vec<Candidate<'_>>> = Vec::with_capacity(unique.len());
    for root in &unique {
        candidates.push(candidates_for(root, houdini)?);
    }

    // Pins first: they cannot move, so placing them early makes every
    // unpinned package's check against them immediate.
    let mut order: Vec<usize> = (0..unique.len()).collect();
    order.sort_by_key(|&i| unique[i].pin.is_none());

    let mut search = Search {
        names: unique.iter().map(|r| r.name).collect(),
        candidates: &candidates,
        order: &order,
        chosen: vec![0; unique.len()],
        steps: 0,
        deepest: None,
    };
    match search.place(0) {
        Ok(true) => {}
        Ok(false) => {
            let (pos, rejected) = search.deepest.take().unwrap_or_default();
            return Err(SolveError::Conflict {
                name: unique[order[pos]].name.to_string(),
                rejected,
            });
        }
        Err(e) => return Err(e),
    }

    Ok((0..unique.len())
        .map(|i| {
            let c = &candidates[i][search.chosen[i]];
            Selection {
                name: unique[i].name.to_string(),
                version: c.version.clone(),
                entry: c.entry.cloned(),
            }
        })
        .collect())
}

fn candidates_for<'a>(
    root: &Root<'a>,
    houdini: Option<HoudiniTarget>,
) -> Result<Vec<Candidate<'a>>, SolveError> {
    if let Some(pin) = root.pin {
        let entry = root.versions.iter().find(|e| e.version == pin);
        return Ok(vec![Candidate {
            version: pin.to_string(),
            semver: Version::parse(pin).ok(),
            entry,
        }]);
    }

    let live: Vec<&RegistryEntry> = root.versions.iter().filter(|e| !e.yanked).collect();
    if live.is_empty() {
        return Err(SolveError::NoUsableVersion {
            name: root.name.to_string(),
        });
    }
    let mut fitting: Vec<Candidate<'a>> = live
        .into_iter()
        .filter(|e| houdini.is_none_or(|h| h.admits(e)))
        .map(|e| Candidate {
            version: e.version.clone(),
            semver: Version::parse(&e.version).ok(),
            entry: Some(e),
        })
        .collect();
    if fitting.is_empty() {
        return Err(SolveError::NoVersionForHoudini {
            name: root.name.to_string(),
            houdini: houdini.map(|h| h.to_string()).unwrap_or_default(),
        });
    }
    // Newest first. A version that is not semver cannot meet a requirement
    // and sorts last, keeping the registry's own order among such versions.
    fitting.sort_by(|a, b| match (&a.semver, &b.semver) {
        (Some(x), Some(y)) => y.cmp(x),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    Ok(fitting)
}

struct Search<'s, 'a> {
    names: Vec<&'s str>,
    candidates: &'s [Vec<Candidate<'a>>],
    order: &'s [usize],
    /// Candidate index per root (indexed like `names`), valid for the roots
    /// placed so far.
    chosen: Vec<usize>,
    steps: usize,
    /// The furthest position the search failed at, with why each candidate
    /// there was refused — the most informative failure to report.
    deepest: Option<(usize, Vec<Rejection>)>,
}

impl Search<'_, '_> {
    fn place(&mut self, pos: usize) -> Result<bool, SolveError> {
        if pos == self.order.len() {
            return Ok(true);
        }
        let root = self.order[pos];
        let mut rejected = Vec::new();
        for (ci, cand) in self.candidates[root].iter().enumerate() {
            self.steps += 1;
            if self.steps > MAX_STEPS {
                return Err(SolveError::TooComplex(MAX_STEPS));
            }
            if let Some(reason) = self.clash(root, cand, pos) {
                rejected.push(Rejection {
                    version: cand.version.clone(),
                    reason,
                });
                continue;
            }
            self.chosen[root] = ci;
            if self.place(pos + 1)? {
                return Ok(true);
            }
        }
        if self.deepest.as_ref().is_none_or(|(p, _)| pos >= *p) && !rejected.is_empty() {
            self.deepest = Some((pos, rejected));
        }
        Ok(false)
    }

    /// The first requirement between `cand` (for `root`) and the roots
    /// already placed that does not hold, in words.
    fn clash(&self, root: usize, cand: &Candidate<'_>, pos: usize) -> Option<String> {
        let name = self.names[root];
        for &other in &self.order[..pos] {
            let other_name = self.names[other];
            let placed = &self.candidates[other][self.chosen[other]];
            for dep in cand.deps().iter().filter(|d| d.name == other_name) {
                if !meets(&dep.req, placed) {
                    return Some(format!(
                        "requires {} {}, but {} is selected",
                        other_name, dep.req, placed.version
                    ));
                }
            }
            for dep in placed.deps().iter().filter(|d| d.name == name) {
                if !meets(&dep.req, cand) {
                    return Some(format!(
                        "is excluded by {} {}, which requires {}",
                        other_name, placed.version, dep.req
                    ));
                }
            }
        }
        None
    }
}

/// Whether `cand`'s version meets `req`. A requirement or version that does
/// not parse is not met: a constraint nobody can read cannot be shown to
/// hold, and the registry validates both on the way in.
fn meets(req: &str, cand: &Candidate<'_>) -> bool {
    match (requirement(req), &cand.semver) {
        (Some(req), Some(v)) => req.matches(v),
        _ => false,
    }
}

/// Parse a `deps` requirement. A bare full version (`"1.2.3"`) means exactly
/// that version, as it does everywhere in an hpm manifest
/// ([`RegistrySet::resolve`](super::RegistrySet::resolve) looks one up
/// directly); Cargo's grammar alone would read it as `^1.2.3`. Anything else
/// is Cargo's grammar.
pub(crate) fn requirement(req: &str) -> Option<VersionReq> {
    let req = req.trim();
    if Version::parse(req).is_ok() {
        return VersionReq::parse(&format!("={req}")).ok();
    }
    VersionReq::parse(req).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::types::RegistryDep;
    use proptest::prelude::*;

    fn entry(
        name: &str,
        version: &str,
        compat: Option<&str>,
        deps: &[(&str, &str)],
    ) -> RegistryEntry {
        RegistryEntry {
            name: name.to_string(),
            version: version.to_string(),
            cksum: None,
            dl: format!("https://pkg.example/{name}-{version}.zip"),
            sig: None,
            kid: None,
            houdini_compat: compat.map(str::to_string),
            platform: None,
            yanked: false,
            description: None,
            author: None,
            created_at: None,
            deps: deps
                .iter()
                .map(|(n, r)| RegistryDep {
                    name: n.to_string(),
                    req: r.to_string(),
                })
                .collect(),
        }
    }

    const TP: &str = "tumblehead/tumblepipe";
    const TT: &str = "tumbletrove/tumbletrove";

    /// The registry as it stood on 2026-10-19, with the requirements the
    /// manifests declared (and the registry did not yet carry).
    fn incident() -> (Vec<RegistryEntry>, Vec<RegistryEntry>) {
        let tumblepipe = vec![
            entry(TP, "1.75.2", Some(">=22,<=22"), &[(TT, ">=0.48")]),
            entry(TP, "1.74.3", Some(">=21,<=22"), &[(TT, ">=0.47,<0.48")]),
            entry(TP, "1.74.2", Some(">=21,<=22"), &[(TT, ">=0.47,<0.48")]),
        ];
        let tumbletrove = vec![
            entry(TT, "0.48.0", Some(">=21"), &[]),
            entry(TT, "0.47.0", Some(">=21"), &[]),
            entry(TT, "0.46.1", Some(">=21"), &[]),
        ];
        (tumblepipe, tumbletrove)
    }

    fn versions(sel: &[Selection]) -> Vec<(&str, &str)> {
        sel.iter()
            .map(|s| (s.name.as_str(), s.version.as_str()))
            .collect()
    }

    #[test]
    fn houdini_22_takes_the_newest_of_both() {
        let (tp, tt) = incident();
        let roots = [
            Root {
                name: TT,
                pin: None,
                versions: &tt,
            },
            Root {
                name: TP,
                pin: None,
                versions: &tp,
            },
        ];
        let sel = solve(&roots, HoudiniTarget::parse("22.0.368")).unwrap();
        assert_eq!(versions(&sel), [(TT, "0.48.0"), (TP, "1.75.2")]);
    }

    #[test]
    fn houdini_21_falls_back_to_the_1_74_line_and_the_tumbletrove_it_needs() {
        let (tp, tt) = incident();
        // TumbleTrove first, as the desktop prepends its defaults: its newest
        // version has to give way to a requirement placed after it.
        let roots = [
            Root {
                name: TT,
                pin: None,
                versions: &tt,
            },
            Root {
                name: TP,
                pin: None,
                versions: &tp,
            },
        ];
        let sel = solve(&roots, HoudiniTarget::parse("21.0.512")).unwrap();
        assert_eq!(versions(&sel), [(TT, "0.47.0"), (TP, "1.74.3")]);
    }

    #[test]
    fn a_pinned_tumblepipe_pulls_tumbletrove_back_on_any_houdini() {
        let (tp, tt) = incident();
        let roots = [
            Root {
                name: TT,
                pin: None,
                versions: &tt,
            },
            Root {
                name: TP,
                pin: Some("1.74.3"),
                versions: &tp,
            },
        ];
        let sel = solve(&roots, HoudiniTarget::parse("22.0")).unwrap();
        assert_eq!(versions(&sel), [(TT, "0.47.0"), (TP, "1.74.3")]);
    }

    #[test]
    fn two_pins_that_cannot_coexist_are_a_conflict_that_names_the_requirement() {
        let (tp, tt) = incident();
        let roots = [
            Root {
                name: TT,
                pin: Some("0.48.0"),
                versions: &tt,
            },
            Root {
                name: TP,
                pin: Some("1.74.3"),
                versions: &tp,
            },
        ];
        let err = solve(&roots, None).unwrap_err();
        let SolveError::Conflict { name, rejected } = &err else {
            panic!("expected a conflict, got {err:?}");
        };
        assert_eq!(name, TP);
        assert_eq!(rejected[0].version, "1.74.3");
        assert!(err.to_string().contains(">=0.47,<0.48"), "{err}");
        assert!(err.to_string().contains("0.48.0"), "{err}");
    }

    #[test]
    fn a_pin_the_registry_does_not_list_is_kept_and_constrains_nothing() {
        let (tp, tt) = incident();
        let roots = [
            Root {
                name: TT,
                pin: None,
                versions: &tt,
            },
            Root {
                name: TP,
                pin: Some("9.9.9"),
                versions: &tp,
            },
        ];
        let sel = solve(&roots, None).unwrap();
        assert_eq!(versions(&sel), [(TT, "0.48.0"), (TP, "9.9.9")]);
        assert!(sel[1].entry.is_none());
    }

    #[test]
    fn a_yanked_pin_is_honoured_and_a_yanked_latest_is_skipped() {
        let mut tt = incident().1;
        tt[0].yanked = true;
        let latest = solve(
            &[Root {
                name: TT,
                pin: None,
                versions: &tt,
            }],
            None,
        )
        .unwrap();
        assert_eq!(latest[0].version, "0.47.0");
        let pinned = solve(
            &[Root {
                name: TT,
                pin: Some("0.48.0"),
                versions: &tt,
            }],
            None,
        )
        .unwrap();
        assert_eq!(pinned[0].version, "0.48.0");
    }

    #[test]
    fn a_pin_ignores_the_houdini_range() {
        let (tp, _) = incident();
        let sel = solve(
            &[Root {
                name: TP,
                pin: Some("1.75.2"),
                versions: &tp,
            }],
            HoudiniTarget::parse("21.0"),
        )
        .unwrap();
        assert_eq!(sel[0].version, "1.75.2");
    }

    #[test]
    fn nothing_for_the_houdini_is_its_own_error() {
        let (tp, _) = incident();
        let err = solve(
            &[Root {
                name: TP,
                pin: None,
                versions: &tp,
            }],
            HoudiniTarget::parse("20.5"),
        )
        .unwrap_err();
        assert_eq!(
            err,
            SolveError::NoVersionForHoudini {
                name: TP.to_string(),
                houdini: "20.5".to_string()
            }
        );
    }

    #[test]
    fn a_requirement_on_a_package_outside_the_set_is_ignored() {
        let (tp, _) = incident();
        let sel = solve(
            &[Root {
                name: TP,
                pin: None,
                versions: &tp,
            }],
            None,
        )
        .unwrap();
        assert_eq!(sel[0].version, "1.75.2");
    }

    /// `"0.47.0"` in a manifest's `[dependencies]` names that version, as it
    /// does for `hpm install`; Cargo alone would let 0.47.5 through.
    #[test]
    fn a_bare_full_version_is_exact() {
        let tt = vec![
            entry(TT, "0.47.5", None, &[]),
            entry(TT, "0.47.0", None, &[]),
        ];
        let tp = vec![entry(TP, "1.74.3", None, &[(TT, "0.47.0")])];
        let roots = [
            Root {
                name: TT,
                pin: None,
                versions: &tt,
            },
            Root {
                name: TP,
                pin: None,
                versions: &tp,
            },
        ];
        assert_eq!(
            versions(&solve(&roots, None).unwrap()),
            [(TT, "0.47.0"), (TP, "1.74.3")]
        );
        assert!(
            requirement("0.47")
                .unwrap()
                .matches(&Version::new(0, 47, 5))
        );
    }

    #[test]
    fn houdini_target_parsing() {
        assert_eq!(
            HoudiniTarget::parse("21.0.512"),
            Some(HoudiniTarget {
                major: 21,
                minor: 0,
                build: Some(512)
            })
        );
        assert_eq!(
            HoudiniTarget::parse("20.5"),
            Some(HoudiniTarget {
                major: 20,
                minor: 5,
                build: None
            })
        );
        for bad in ["", "21", "21.x", "21.0.1.2", "21.0."] {
            assert_eq!(HoudiniTarget::parse(bad), None, "{bad:?}");
        }
    }

    // ---- Property: the search agrees with brute force -------------------
    //
    // Names and versions come from small shared pools and every requirement
    // names a pool package with a bound drawn from the pool's versions, so
    // requirements actually bite: random strings would almost never name a
    // package in the set and the property would pass vacuously.

    const NAMES: [&str; 3] = ["a/x", "a/y", "a/z"];
    const VERSIONS: [&str; 4] = ["0.1.0", "0.2.0", "1.0.0", "1.1.0"];
    const COMPATS: [Option<&str>; 3] = [None, Some(">=21"), Some(">=22,<=22")];

    fn arb_req() -> impl Strategy<Value = String> {
        (
            prop::sample::select(&[">=", "<", "="][..]),
            prop::sample::select(&VERSIONS[..]),
        )
            .prop_map(|(op, v)| format!("{op}{v}"))
    }

    fn arb_entry(name: &'static str) -> impl Strategy<Value = RegistryEntry> {
        (
            prop::sample::select(&VERSIONS[..]),
            prop::sample::select(&COMPATS[..]),
            any::<bool>(),
            prop::collection::vec((prop::sample::select(&NAMES[..]), arb_req()), 0..3),
        )
            .prop_map(move |(v, compat, yanked, deps)| {
                let deps: Vec<(&str, &str)> = deps
                    .iter()
                    .filter(|(n, _)| *n != name)
                    .map(|(n, r)| (*n, r.as_str()))
                    .collect();
                let mut e = entry(name, v, compat, &deps);
                e.yanked = yanked && v != "0.1.0";
                e
            })
    }

    fn arb_versions(name: &'static str) -> impl Strategy<Value = Vec<RegistryEntry>> {
        prop::collection::vec(arb_entry(name), 1..5).prop_map(|mut es| {
            // One entry per version, as get_versions returns.
            let mut seen = std::collections::HashSet::new();
            es.retain(|e| seen.insert(e.version.clone()));
            es
        })
    }

    fn arb_problem() -> impl Strategy<
        Value = (
            Vec<Vec<RegistryEntry>>,
            Vec<Option<String>>,
            Option<&'static str>,
        ),
    > {
        (
            (
                arb_versions(NAMES[0]),
                arb_versions(NAMES[1]),
                arb_versions(NAMES[2]),
            )
                .prop_map(|(a, b, c)| vec![a, b, c]),
            prop::collection::vec(
                prop::option::weighted(
                    0.3,
                    prop::sample::select(&VERSIONS[..]).prop_map(str::to_string),
                ),
                3,
            ),
            prop::sample::select(&[None, Some("21.0"), Some("22.0.1")][..]),
        )
    }

    /// Every assignment brute force considers acceptable, in the solver's
    /// preference order (input order, newest first).
    fn brute_force(roots: &[Root<'_>], houdini: Option<HoudiniTarget>) -> Option<Vec<String>> {
        let per_root: Vec<Vec<Candidate<'_>>> = roots
            .iter()
            .map(|r| candidates_for(r, houdini).ok())
            .collect::<Option<_>>()?;
        let mut idx = vec![0usize; roots.len()];
        loop {
            let ok = (0..roots.len()).all(|i| {
                (0..roots.len()).filter(|&j| j != i).all(|j| {
                    per_root[i][idx[i]]
                        .deps()
                        .iter()
                        .filter(|d| d.name == roots[j].name)
                        .all(|d| meets(&d.req, &per_root[j][idx[j]]))
                })
            });
            if ok {
                return Some(
                    (0..roots.len())
                        .map(|i| per_root[i][idx[i]].version.clone())
                        .collect(),
                );
            }
            // Odometer over candidate indices, last root fastest: the order a
            // depth-first search in input order visits assignments in. The
            // solver places pins first, which does not change that order,
            // since a pin has exactly one candidate.
            let mut k = roots.len();
            loop {
                if k == 0 {
                    return None;
                }
                k -= 1;
                idx[k] += 1;
                if idx[k] < per_root[k].len() {
                    break;
                }
                idx[k] = 0;
            }
        }
    }

    proptest! {
        #[test]
        fn prop_solve_matches_brute_force((lists, pins, houdini) in arb_problem()) {
            let roots: Vec<Root<'_>> = (0..3)
                .map(|i| Root { name: NAMES[i], pin: pins[i].as_deref(), versions: &lists[i] })
                .collect();
            let target = houdini.and_then(HoudiniTarget::parse);
            let expected = brute_force(&roots, target);
            match solve(&roots, target) {
                Ok(sel) => {
                    let got: Vec<String> = sel.iter().map(|s| s.version.clone()).collect();
                    prop_assert_eq!(Some(got), expected);
                }
                Err(SolveError::Conflict { .. }) => {
                    // Candidates existed for every root, but no combination fit.
                    let had_candidates = roots.iter().all(|r| candidates_for(r, target).is_ok());
                    prop_assert!(had_candidates);
                    prop_assert_eq!(expected, None);
                }
                Err(e) => {
                    prop_assert!(roots.iter().any(|r| candidates_for(r, target).is_err()), "{e}");
                    prop_assert_eq!(expected, None);
                }
            }
        }
    }
}
