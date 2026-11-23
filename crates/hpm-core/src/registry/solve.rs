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
//!   Houdini and that have a build for the host platform (or a universal
//!   one), newest first, and the search backtracks, so the result is the
//!   first acceptable assignment in input order: earlier packages get their
//!   newest fitting version before later ones are considered. Input order
//!   is the order of the packages passed in, never the order a version
//!   lists its `deps` in: those are taken by name, then requirement, since
//!   the same release reaches callers in the publisher's order from a
//!   registry and sorted from a manifest read offline, and a set with
//!   several answers must not resolve differently between them. The platform
//!   rule is the one install applies when it picks a build
//!   ([`has_build_for`](super::has_build_for)): a version the solver chose
//!   but install could not download for this machine would fail the whole
//!   install with `NoCompatibleBuild`, even when an older version covers
//!   the host. A pinned version with no host build is still selected, and
//!   install names it.
//! - **Requirements constrain the set; they extend it only with what the
//!   caller offers.** A `deps` entry naming a package the caller did not
//!   pass is ignored. [`solve_with`] also takes packages that are
//!   *available*: one of those is part of the answer exactly when a chosen
//!   version requires it, directly or through another, and it constrains
//!   nothing while it is not. A caller that wants required packages
//!   installed offers them, and offers again whatever the answer's versions
//!   require that it had not fetched yet, until nothing is missing. Only the
//!   caller knows which packages the user has switched off, so it decides
//!   what to offer.
//!
//!   Whether a required package is present has to be decided in the search.
//!   A caller that added required packages to the set as ordinary roots,
//!   and dropped them again when a later answer no longer required them,
//!   never finished when a required package's own requirements moved the
//!   version that required it: the package was dropped, the newer version
//!   came back, and it was required again.

use super::types::{RegistryDep, RegistryEntry};
use hpm_package::{HoudiniRange, Platform};
use semver::Version;
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

/// A version of the package [`SolveError::Conflict`] could not place, and
/// everything that ruled it out against the versions selected at the time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    pub version: String,
    /// At least one. The first is the one the error's text names.
    pub causes: Vec<Cause>,
}

/// One reason a [`Rejection`]'s version could not be chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cause {
    /// `by` requires the unplaced package with `requirement`, and this
    /// version does not meet it.
    ExcludedBy { by: Selected, requirement: String },
    /// This version requires `selected.name` with `requirement`, and the
    /// version selected for it does not meet it.
    Requires {
        requirement: String,
        selected: Selected,
    },
    /// This version requires `name` with `requirement`, an available package
    /// with no version to offer at all. `error` says why: it is a
    /// [`SolveError::NoUsableVersion`], [`SolveError::NoVersionForHoudini`]
    /// or [`SolveError::NoVersionForPlatform`] for `name`.
    Unavailable {
        name: String,
        requirement: String,
        error: Box<SolveError>,
    },
}

impl fmt::Display for Cause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Cause::ExcludedBy { by, requirement } => write!(
                f,
                "is excluded by {} {}, which requires {requirement}",
                by.name, by.version
            ),
            Cause::Requires {
                requirement,
                selected,
            } => write!(
                f,
                "requires {} {requirement}, but {} is selected",
                selected.name, selected.version
            ),
            Cause::Unavailable { name, error, .. } => write!(f, "requires {name}, and {error}"),
        }
    }
}

/// A package the search had selected when it refused a version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selected {
    pub name: String,
    pub version: String,
    pub origin: Origin,
}

/// How a package came into the set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// One of the roots: a package the project lists itself.
    Listed { pinned: bool },
    /// An available package that a selected version required. `chain` runs
    /// from the root that started it to the version that requires this
    /// package, so `chain[0]` is always [`Origin::Listed`]. When several
    /// selected versions require the package, this is the one the search
    /// came to it through.
    Required { chain: Vec<PackageVersion> },
}

/// A package at a version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageVersion {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SolveError {
    /// Every version of an unpinned package is yanked.
    #[error("every version of {name} is yanked")]
    NoUsableVersion { name: String },
    /// No non-yanked version of an unpinned package supports the target.
    #[error("no version of {name} supports Houdini {houdini}")]
    NoVersionForHoudini { name: String, houdini: String },
    /// Versions of an unpinned package support the target Houdini, but none
    /// of them has a build for the host platform.
    #[error("no version of {name} has a build for {platform}")]
    NoVersionForPlatform { name: String, platform: String },
    /// The packages' requirements of each other cannot all be met. `name` is
    /// the package the search could not place and `origin` how it came into
    /// the set. `rejected` has one entry per version it could take, newest
    /// first, each with every requirement between that version and the
    /// versions selected at the time that does not hold. All of those
    /// selected versions belong to one assignment, so together they rule
    /// out every version of `name`; they are a witness, not the only one.
    #[error(
        "no version of {name} works with the other packages: {}",
        describe(rejected)
    )]
    Conflict {
        name: String,
        origin: Origin,
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
        .map(|r| match r.causes.first() {
            Some(cause) => format!("{} {cause}", r.version),
            None => r.version.clone(),
        })
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
    /// The entry's `deps`, by name, then requirement. The order they are
    /// listed in is the publisher's or the reader's (a registry serves
    /// manifest order, a working copy read offline comes out sorted), so
    /// the search must not depend on it: it decides which required package
    /// is placed first, and so which of several answers is found and what
    /// a conflict names.
    deps: Vec<&'a RegistryDep>,
}

impl<'a> Candidate<'a> {
    fn new(version: String, entry: Option<&'a RegistryEntry>) -> Self {
        let mut deps: Vec<&RegistryDep> =
            entry.map(|e| e.deps.iter().collect()).unwrap_or_default();
        deps.sort_by(|a, b| (&a.name, &a.req).cmp(&(&b.name, &b.req)));
        Candidate {
            semver: Version::parse(&version).ok(),
            version,
            entry,
            deps,
        }
    }

    fn deps(&self) -> &[&'a RegistryDep] {
        &self.deps
    }
}

/// Choose a version for every root. See the module docs for the rules.
/// Selections come back in the order of `roots`; a name given twice is
/// solved once, at its first position.
///
/// Versions are held to the platform this process runs on
/// ([`Platform::current`]); see [`solve_for_host`] to name another.
pub fn solve(
    roots: &[Root<'_>],
    houdini: Option<HoudiniTarget>,
) -> Result<Vec<Selection>, SolveError> {
    solve_with(roots, &[], houdini)
}

/// [`solve`], with packages that join the set only when required.
///
/// Each of `available` is part of the answer exactly when a version chosen
/// for a root requires it, directly or through other available packages. One
/// that is not required is left out and constrains nothing. One that is
/// required is chosen like a root: its newest version that fits, held to
/// every requirement on it, and its own requirements hold the rest. A
/// version whose requirement no version of an available package can meet is
/// not chosen, so a root falls back to an older version whose requirements
/// can be met.
///
/// An available package with no usable version (all yanked, none for the
/// target Houdini, or none with a build for the host) is not an error by
/// itself: it rules out the versions that
/// require it. A name that is also a root is a root.
///
/// Selections come back with the roots first, in their order, then the
/// required packages in the order the search came to need them, each
/// version's requirements taken by name. Neither the answer nor a
/// conflict's explanation depends on the order a version lists its `deps`
/// in. Every root
/// is settled before any available package is, so the roots get the best
/// versions for which the rest can be completed.
///
/// Versions are held to the platform this process runs on
/// ([`Platform::current`]), the one install downloads for.
pub fn solve_with(
    roots: &[Root<'_>],
    available: &[Root<'_>],
    houdini: Option<HoudiniTarget>,
) -> Result<Vec<Selection>, SolveError> {
    solve_for_host(roots, available, houdini, Platform::current())
}

/// [`solve_with`] for the platform `host` rather than this process's. `None`
/// is a host hpm does not recognise, which only a universal build serves, as
/// at install.
pub fn solve_for_host(
    roots: &[Root<'_>],
    available: &[Root<'_>],
    houdini: Option<HoudiniTarget>,
    host: Option<Platform>,
) -> Result<Vec<Selection>, SolveError> {
    let mut unique: Vec<&Root<'_>> = Vec::new();
    for root in roots {
        if !unique.iter().any(|r| r.name == root.name) {
            unique.push(root);
        }
    }
    let listed = unique.len();
    for root in available {
        if !unique.iter().any(|r| r.name == root.name) {
            unique.push(root);
        }
    }

    let mut candidates: Vec<Vec<Candidate<'_>>> = Vec::with_capacity(unique.len());
    let mut unusable: Vec<Option<SolveError>> = Vec::with_capacity(unique.len());
    for (i, root) in unique.iter().enumerate() {
        match candidates_for(root, houdini, host) {
            Ok(c) => {
                candidates.push(c);
                unusable.push(None);
            }
            Err(e) if i >= listed => {
                candidates.push(Vec::new());
                unusable.push(Some(e));
            }
            Err(e) => return Err(e),
        }
    }

    // Pins first: they cannot move, so placing them early makes every
    // unpinned package's check against them immediate.
    let mut agenda: Vec<usize> = (0..listed).collect();
    agenda.sort_by_key(|&i| unique[i].pin.is_none());

    let mut search = Search {
        names: unique.iter().map(|r| r.name).collect(),
        pinned: unique[..listed].iter().map(|r| r.pin.is_some()).collect(),
        via: vec![None; unique.len()],
        candidates: &candidates,
        unusable: &unusable,
        listed,
        agenda,
        chosen: vec![0; unique.len()],
        steps: 0,
        deepest: None,
    };
    match search.place(0) {
        Ok(true) => {}
        Ok(false) => {
            // A failure always records its position: the search fails only
            // where every candidate of a package clashed (an available
            // package with nothing to offer is never placed).
            let deepest = search
                .deepest
                .take()
                .expect("a failed search records where it failed");
            return Err(SolveError::Conflict {
                name: unique[deepest.package].name.to_string(),
                origin: deepest.origin,
                rejected: deepest.rejected,
            });
        }
        Err(e) => return Err(e),
    }

    let required = search.agenda.iter().copied().filter(|&i| i >= listed);
    Ok((0..listed)
        .chain(required)
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
    host: Option<Platform>,
) -> Result<Vec<Candidate<'a>>, SolveError> {
    if let Some(pin) = root.pin {
        // The pinned version's row install would use, as the listing view
        // (`dedupe_builds_by_version`) keeps it: among its live rows if it
        // has any, the host's build, else a universal one, else the first.
        let rows: Vec<&RegistryEntry> = root.versions.iter().filter(|e| e.version == pin).collect();
        let live: Vec<&RegistryEntry> = rows.iter().copied().filter(|e| !e.yanked).collect();
        let pool = if live.is_empty() { &rows } else { &live };
        let entry = pool
            .iter()
            .copied()
            .find(|e| built_for(e, host))
            .or_else(|| pool.iter().copied().find(|e| super::has_build_for(e, None)))
            .or_else(|| pool.first().copied());
        return Ok(vec![Candidate::new(pin.to_string(), entry)]);
    }

    let live: Vec<&RegistryEntry> = root.versions.iter().filter(|e| !e.yanked).collect();
    if live.is_empty() {
        return Err(SolveError::NoUsableVersion {
            name: root.name.to_string(),
        });
    }
    let for_houdini: Vec<&RegistryEntry> = live
        .into_iter()
        .filter(|e| houdini.is_none_or(|h| h.admits(e)))
        .collect();
    if for_houdini.is_empty() {
        return Err(SolveError::NoVersionForHoudini {
            name: root.name.to_string(),
            houdini: houdini.map(|h| h.to_string()).unwrap_or_default(),
        });
    }
    // One entry per version is the version view, whose survivor is the
    // host's build when the version has one (`dedupe_builds_by_version`);
    // a list of raw build rows works too, keeping the rows this host can
    // install. Either way, the rule is install's. A version is one
    // candidate however many of its rows the host can install, standing
    // for the row install would pick (`select_build`: the first built for
    // the host, else the first universal one); two candidates of one
    // version would be searched twice and named twice in a conflict.
    let mut fitting: Vec<Candidate<'a>> = Vec::new();
    for e in for_houdini
        .into_iter()
        .filter(|e| super::has_build_for(e, host))
    {
        match fitting.iter_mut().find(|c| c.version == e.version) {
            Some(c) => {
                if c.entry.is_some_and(|kept| !built_for(kept, host)) && built_for(e, host) {
                    *c = Candidate::new(e.version.clone(), Some(e));
                }
            }
            None => fitting.push(Candidate::new(e.version.clone(), Some(e))),
        }
    }
    if fitting.is_empty() {
        return Err(SolveError::NoVersionForPlatform {
            name: root.name.to_string(),
            platform: host
                .map(|p| p.as_str().to_string())
                .unwrap_or_else(Platform::host_id),
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

/// Whether `entry` is a build for `host` itself, which install prefers to a
/// universal one (`select_build`).
fn built_for(entry: &RegistryEntry, host: Option<Platform>) -> bool {
    host.is_some_and(|h| entry.platform.as_ref().is_some_and(|t| t.matches(h)))
}

struct Search<'s, 'a> {
    /// Every package: the roots, then the available ones.
    names: Vec<&'s str>,
    candidates: &'s [Vec<Candidate<'a>>],
    /// Per root, whether it is pinned.
    pinned: Vec<bool>,
    /// Per available package on the agenda, the package whose placed
    /// version put it there. Entries for packages off the agenda are stale.
    via: Vec<Option<usize>>,
    /// Per package, why it has no candidates. Only an available package can
    /// be in that state; for a root it is the solve's error.
    unusable: &'s [Option<SolveError>],
    /// How many of `names` are roots.
    listed: usize,
    /// The packages to place, in order: the roots, then each available
    /// package as a placed version comes to require it. What a candidate
    /// adds is taken back when the search moves off that candidate.
    agenda: Vec<usize>,
    /// Candidate index per package (indexed like `names`), valid for the
    /// packages placed so far.
    chosen: Vec<usize>,
    steps: usize,
    /// The furthest position the search failed at and the package there,
    /// with why each of its candidates was refused: the most informative
    /// failure to report.
    deepest: Option<Failure>,
}

struct Failure {
    pos: usize,
    package: usize,
    origin: Origin,
    rejected: Vec<Rejection>,
}

/// A [`Cause`] as the search finds it: indices only, so refusing a
/// candidate costs no names or chains. Only the failure that gets reported
/// is spelled out ([`Search::spell`]). `dep` indexes the requiring
/// candidate's `deps`.
enum Clash {
    Unavailable { dep: usize, package: usize },
    Requires { dep: usize, other: usize },
    ExcludedBy { dep: usize, other: usize },
}

impl Search<'_, '_> {
    fn place(&mut self, pos: usize) -> Result<bool, SolveError> {
        if pos == self.agenda.len() {
            return Ok(true);
        }
        let root = self.agenda[pos];
        let candidates = self.candidates;
        let mut rejected = Vec::new();
        for (ci, cand) in candidates[root].iter().enumerate() {
            self.steps += 1;
            if self.steps > MAX_STEPS {
                return Err(SolveError::TooComplex(MAX_STEPS));
            }
            let clashes = self.clashes(root, cand, pos);
            if !clashes.is_empty() {
                rejected.push((ci, clashes));
                continue;
            }
            self.chosen[root] = ci;
            let planned = self.agenda.len();
            for dep in cand.deps() {
                if let Some(required) = self.available(&dep.name)
                    && !self.agenda.contains(&required)
                {
                    self.agenda.push(required);
                    self.via[required] = Some(root);
                }
            }
            if self.place(pos + 1)? {
                return Ok(true);
            }
            self.agenda.truncate(planned);
        }
        // The packages before `pos` are still placed as they were when
        // each candidate was refused, so the clashes can be spelled now.
        if self.deepest.as_ref().is_none_or(|d| pos >= d.pos) && !rejected.is_empty() {
            let rejected = rejected
                .into_iter()
                .map(|(ci, clashes)| {
                    let cand = &candidates[root][ci];
                    Rejection {
                        version: cand.version.clone(),
                        causes: clashes.iter().map(|c| self.spell(cand, c)).collect(),
                    }
                })
                .collect();
            self.deepest = Some(Failure {
                pos,
                package: root,
                origin: self.origin(root),
                rejected,
            });
        }
        Ok(false)
    }

    /// The index of the available package called `name`, if there is one.
    fn available(&self, name: &str) -> Option<usize> {
        (self.listed..self.names.len()).find(|&i| self.names[i] == name)
    }

    /// How `package` came into the set. Valid for a package on the agenda
    /// whose requirers are placed: every package before the current
    /// position, and the one at it.
    fn origin(&self, package: usize) -> Origin {
        if package < self.listed {
            return Origin::Listed {
                pinned: self.pinned[package],
            };
        }
        let mut chain = Vec::new();
        let mut at = package;
        while at >= self.listed {
            let requirer = self.via[at].expect("a required package records its requirer");
            chain.push(PackageVersion {
                name: self.names[requirer].to_string(),
                version: self.placed(requirer).version.clone(),
            });
            at = requirer;
        }
        chain.reverse();
        Origin::Required { chain }
    }

    fn placed(&self, package: usize) -> &Candidate<'_> {
        &self.candidates[package][self.chosen[package]]
    }

    fn selected(&self, package: usize) -> Selected {
        Selected {
            name: self.names[package].to_string(),
            version: self.placed(package).version.clone(),
            origin: self.origin(package),
        }
    }

    /// Every requirement between `cand` (for `root`) and the packages
    /// already placed that does not hold, and every requirement of `cand`
    /// on an available package that has no version to offer. Empty when
    /// `cand` fits.
    fn clashes(&self, root: usize, cand: &Candidate<'_>, pos: usize) -> Vec<Clash> {
        let name = self.names[root];
        let mut clashes = Vec::new();
        for (dep, d) in cand.deps().iter().enumerate() {
            if let Some(package) = self.available(&d.name)
                && self.unusable[package].is_some()
            {
                clashes.push(Clash::Unavailable { dep, package });
            }
        }
        for &other in &self.agenda[..pos] {
            let other_name = self.names[other];
            let placed = self.placed(other);
            for (dep, d) in cand.deps().iter().enumerate() {
                if d.name == other_name && !meets(&d.req, placed) {
                    clashes.push(Clash::Requires { dep, other });
                }
            }
            for (dep, d) in placed.deps().iter().enumerate() {
                if d.name == name && !meets(&d.req, cand) {
                    clashes.push(Clash::ExcludedBy { dep, other });
                }
            }
        }
        clashes
    }

    /// `clash`, found for `cand`, as the [`Cause`] it reports. Valid while
    /// the packages it names are placed as they were when it was found.
    fn spell(&self, cand: &Candidate<'_>, clash: &Clash) -> Cause {
        match *clash {
            Clash::Unavailable { dep, package } => {
                let d = &cand.deps()[dep];
                Cause::Unavailable {
                    name: d.name.clone(),
                    requirement: d.req.clone(),
                    error: Box::new(
                        self.unusable[package]
                            .clone()
                            .expect("an unavailable package records why"),
                    ),
                }
            }
            Clash::Requires { dep, other } => Cause::Requires {
                requirement: cand.deps()[dep].req.clone(),
                selected: self.selected(other),
            },
            Clash::ExcludedBy { dep, other } => Cause::ExcludedBy {
                by: self.selected(other),
                requirement: self.placed(other).deps()[dep].req.clone(),
            },
        }
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

/// Parse a `deps` requirement with hpm's one grammar
/// ([`VersionRequirement`](super::requirement::VersionRequirement)).
pub(crate) fn requirement(req: &str) -> Option<super::requirement::VersionRequirement> {
    super::requirement::VersionRequirement::parse(req).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::types::{PlatformTag, RegistryDep};
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
        let SolveError::Conflict {
            name,
            origin,
            rejected,
        } = &err
        else {
            panic!("expected a conflict, got {err:?}");
        };
        assert_eq!(name, TP);
        assert_eq!(*origin, Origin::Listed { pinned: true });
        assert_eq!(rejected[0].version, "1.74.3");
        assert_eq!(
            rejected[0].causes,
            [Cause::Requires {
                requirement: ">=0.47,<0.48".to_string(),
                selected: Selected {
                    name: TT.to_string(),
                    version: "0.48.0".to_string(),
                    origin: Origin::Listed { pinned: true },
                },
            }]
        );
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

    /// The registry lists 2.0.0 for Windows only and 1.0.0 for every
    /// platform; a Linux host takes 1.0.0, which install can download.
    fn split_by_platform() -> Vec<RegistryEntry> {
        let mut windows_only = entry(TT, "2.0.0", None, &[]);
        windows_only.platform = Some(PlatformTag::Platform(Platform::WindowsX86_64));
        let mut linux = entry(TT, "1.0.0", None, &[]);
        linux.platform = Some(PlatformTag::Platform(Platform::LinuxX86_64));
        let universal = entry(TT, "0.9.0", None, &[]);
        vec![windows_only, linux, universal]
    }

    #[test]
    fn an_unpinned_package_skips_a_version_with_no_build_for_the_host() {
        let tt = split_by_platform();
        let linux = solve_for_host(&[root(TT, &tt)], &[], None, Some(Platform::LinuxX86_64));
        assert_eq!(linux.unwrap()[0].version, "1.0.0");
        let windows = solve_for_host(&[root(TT, &tt)], &[], None, Some(Platform::WindowsX86_64));
        assert_eq!(windows.unwrap()[0].version, "2.0.0");
        // A universal build serves every host, a host hpm does not know too.
        let mac = solve_for_host(&[root(TT, &tt)], &[], None, Some(Platform::MacosAarch64));
        assert_eq!(mac.unwrap()[0].version, "0.9.0");
        let unknown = solve_for_host(&[root(TT, &tt)], &[], None, None);
        assert_eq!(unknown.unwrap()[0].version, "0.9.0");
    }

    #[test]
    fn a_pin_with_no_build_for_the_host_is_kept() {
        let tt = split_by_platform();
        let pinned = Root {
            name: TT,
            pin: Some("2.0.0"),
            versions: &tt,
        };
        let sel = solve_for_host(&[pinned], &[], None, Some(Platform::LinuxX86_64)).unwrap();
        assert_eq!(sel[0].version, "2.0.0");
    }

    #[test]
    fn nothing_for_the_host_is_its_own_error() {
        let tt = &split_by_platform()[..1];
        let err = solve_for_host(&[root(TT, tt)], &[], None, Some(Platform::LinuxX86_64));
        assert_eq!(
            err.unwrap_err(),
            SolveError::NoVersionForPlatform {
                name: TT.to_string(),
                platform: "linux-x86_64".to_string(),
            }
        );
    }

    /// An offered package's newest version for another platform is passed
    /// over, and one with nothing for the host rules out its requirers.
    #[test]
    fn an_offered_package_is_held_to_the_host_too() {
        let tt = split_by_platform();
        let tp = vec![
            entry(TP, "2.0.0", None, &[(TT, ">=2")]),
            entry(TP, "1.0.0", None, &[(TT, ">=1")]),
        ];
        let sel = solve_for_host(
            &[root(TP, &tp)],
            &[root(TT, &tt)],
            None,
            Some(Platform::LinuxX86_64),
        );
        assert_eq!(versions(&sel.unwrap()), [(TP, "1.0.0"), (TT, "1.0.0")]);

        let only_windows = &tt[..1];
        let err = solve_for_host(
            &[root(TP, &tp[1..])],
            &[root(TT, only_windows)],
            None,
            Some(Platform::LinuxX86_64),
        )
        .unwrap_err();
        assert!(matches!(err, SolveError::Conflict { .. }), "{err}");
        assert!(
            err.to_string().contains("has a build for linux-x86_64"),
            "{err}"
        );
    }

    /// Raw build rows (one per platform) select the same version as the
    /// deduplicated view, and the host's row.
    #[test]
    fn raw_build_rows_select_the_hosts_row() {
        let mut linux = entry(TT, "2.0.0", None, &[]);
        linux.platform = Some(PlatformTag::Platform(Platform::LinuxX86_64));
        linux.dl = "https://pkg.example/linux.zip".into();
        let mut windows = linux.clone();
        windows.platform = Some(PlatformTag::Platform(Platform::WindowsX86_64));
        windows.dl = "https://pkg.example/windows.zip".into();
        let rows = vec![windows, linux];
        let sel =
            solve_for_host(&[root(TT, &rows)], &[], None, Some(Platform::LinuxX86_64)).unwrap();
        assert_eq!(
            sel[0].entry.as_ref().unwrap().dl,
            "https://pkg.example/linux.zip"
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
    /// The host the property tests solve for, and the build platforms an
    /// entry may carry: universal, the host's, another's.
    const HOST: Platform = Platform::LinuxX86_64;
    const PLATFORMS: [Option<Platform>; 3] = [None, Some(HOST), Some(Platform::WindowsX86_64)];

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
            prop::sample::select(&PLATFORMS[..]),
        )
            .prop_map(move |(v, compat, yanked, deps, platform)| {
                let deps: Vec<(&str, &str)> = deps
                    .iter()
                    .filter(|(n, _)| *n != name)
                    .map(|(n, r)| (*n, r.as_str()))
                    .collect();
                let mut e = entry(name, v, compat, &deps);
                e.yanked = yanked && v != "0.1.0";
                e.platform = platform.map(PlatformTag::Platform);
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
            .map(|r| candidates_for(r, houdini, Some(HOST)).ok())
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
            match solve_for_host(&roots, &[], target, Some(HOST)) {
                Ok(sel) => {
                    let got: Vec<String> = sel.iter().map(|s| s.version.clone()).collect();
                    prop_assert_eq!(Some(got), expected);
                }
                Err(SolveError::Conflict { .. }) => {
                    // Candidates existed for every root, but no combination fit.
                    let had_candidates = roots.iter().all(|r| candidates_for(r, target, Some(HOST)).is_ok());
                    prop_assert!(had_candidates);
                    prop_assert_eq!(expected, None);
                }
                Err(e) => {
                    prop_assert!(roots.iter().any(|r| candidates_for(r, target, Some(HOST)).is_err()), "{e}");
                    prop_assert_eq!(expected, None);
                }
            }
        }
    }

    // ---- Packages that join only when required ---------------------------

    const MODEL: &str = "b/model";
    const PIPE: &str = "a/pipe";
    const UTIL: &str = "b/util";

    fn root<'a>(name: &'a str, versions: &'a [RegistryEntry]) -> Root<'a> {
        Root {
            name,
            pin: None,
            versions,
        }
    }

    #[test]
    fn a_required_package_joins_after_the_roots() {
        let pipe = vec![entry(PIPE, "2.0.0", None, &[(UTIL, ">=1")])];
        let util = vec![
            entry(UTIL, "1.1.0", None, &[]),
            entry(UTIL, "0.9.0", None, &[]),
        ];
        let model = vec![entry(MODEL, "1.0.0", None, &[])];
        let sel = solve_with(
            &[root(PIPE, &pipe), root(MODEL, &model)],
            &[root(UTIL, &util)],
            None,
        )
        .unwrap();
        assert_eq!(
            versions(&sel),
            [(PIPE, "2.0.0"), (MODEL, "1.0.0"), (UTIL, "1.1.0")]
        );
    }

    /// An available package nobody requires is not in the answer, and what
    /// it requires holds nobody: here it would otherwise pull Pipe back.
    #[test]
    fn an_available_package_nobody_requires_is_left_out_and_constrains_nothing() {
        let pipe = vec![
            entry(PIPE, "2.0.0", None, &[]),
            entry(PIPE, "1.0.0", None, &[]),
        ];
        let util = vec![entry(UTIL, "1.0.0", None, &[(PIPE, "<2")])];
        let sel = solve_with(&[root(PIPE, &pipe)], &[root(UTIL, &util)], None).unwrap();
        assert_eq!(versions(&sel), [(PIPE, "2.0.0")]);
    }

    /// The newest Model requires Pipe, and Pipe works only with the older
    /// Model. The newest Model's requirements cannot be met, so the answer
    /// is the older Model, which needs no Pipe. Adding Pipe as a root and
    /// dropping it when the answer stopped requiring it alternated between
    /// the two for ever.
    #[test]
    fn a_required_package_that_rules_out_its_requirer_moves_the_requirer() {
        let model = vec![
            entry(MODEL, "2.0.0", None, &[(PIPE, ">=1")]),
            entry(MODEL, "1.0.0", None, &[]),
        ];
        let pipe = vec![entry(PIPE, "1.0.0", None, &[(MODEL, "<2")])];
        let sel = solve_with(&[root(MODEL, &model)], &[root(PIPE, &pipe)], None).unwrap();
        assert_eq!(versions(&sel), [(MODEL, "1.0.0")]);
    }

    /// The same without a cycle: the newest Pipe needs a Util that another
    /// root rules out, so Pipe falls back to the version that needs none.
    #[test]
    fn a_requirement_another_root_rules_out_moves_the_requirer() {
        let pipe = vec![
            entry(PIPE, "2.0.0", None, &[(UTIL, ">=2")]),
            entry(PIPE, "1.0.0", None, &[]),
        ];
        let model = vec![entry(MODEL, "1.0.0", None, &[(UTIL, "<2")])];
        let util = vec![
            entry(UTIL, "2.0.0", None, &[]),
            entry(UTIL, "1.0.0", None, &[]),
        ];
        let sel = solve_with(
            &[root(PIPE, &pipe), root(MODEL, &model)],
            &[root(UTIL, &util)],
            None,
        )
        .unwrap();
        assert_eq!(
            versions(&sel),
            [(PIPE, "1.0.0"), (MODEL, "1.0.0"), (UTIL, "1.0.0")]
        );
    }

    #[test]
    fn requirements_are_followed_through_required_packages() {
        let pipe = vec![entry(PIPE, "1.0.0", None, &[(MODEL, ">=1")])];
        let model = vec![entry(MODEL, "1.0.0", None, &[(UTIL, "<1")])];
        let util = vec![
            entry(UTIL, "1.0.0", None, &[]),
            entry(UTIL, "0.9.0", None, &[]),
        ];
        let sel = solve_with(
            &[root(PIPE, &pipe)],
            &[root(UTIL, &util), root(MODEL, &model)],
            None,
        )
        .unwrap();
        assert_eq!(
            versions(&sel),
            [(PIPE, "1.0.0"), (MODEL, "1.0.0"), (UTIL, "0.9.0")]
        );
    }

    /// A required package with nothing for the target Houdini rules out the
    /// versions that require it; when that leaves none, the conflict says
    /// which requirement and why.
    #[test]
    fn a_required_package_with_no_usable_version_rules_out_its_requirers() {
        let util = vec![entry(UTIL, "1.0.0", Some(">=22,<=22"), &[])];
        let both = vec![
            entry(PIPE, "2.0.0", None, &[(UTIL, ">=1")]),
            entry(PIPE, "1.0.0", None, &[]),
        ];
        let target = HoudiniTarget::parse("21.0");
        let sel = solve_with(&[root(PIPE, &both)], &[root(UTIL, &util)], target).unwrap();
        assert_eq!(versions(&sel), [(PIPE, "1.0.0")]);

        let only = vec![entry(PIPE, "2.0.0", None, &[(UTIL, ">=1")])];
        let err = solve_with(&[root(PIPE, &only)], &[root(UTIL, &util)], target).unwrap_err();
        let text = err.to_string();
        assert!(matches!(err, SolveError::Conflict { ref name, .. } if name == PIPE));
        assert!(
            text.contains("requires b/util, and no version of b/util supports Houdini 21.0"),
            "{text}"
        );
    }

    #[test]
    fn a_name_offered_both_ways_is_a_root() {
        let util = vec![entry(UTIL, "1.0.0", None, &[])];
        let sel = solve_with(&[root(UTIL, &util)], &[root(UTIL, &util)], None).unwrap();
        assert_eq!(versions(&sel), [(UTIL, "1.0.0")]);
    }

    // ---- Property: solve_with agrees with brute force --------------------
    //
    // The same pools as above, with each package drawn as a root or as
    // available, so a requirement often names an available package and that
    // package's own requirements often name a root.

    /// One package's place in a brute-force assignment: the candidate chosen
    /// for it, or `None` for an available package left out.
    type Assignment = Vec<Option<usize>>;

    /// Whether `assignment` is an answer: every root present, every
    /// requirement of a present version on a package in play met by a
    /// present version, and the available packages present exactly the ones
    /// reachable from the roots through those requirements.
    fn is_answer(
        names: &[&str],
        is_root: &[bool],
        per_package: &[Vec<Candidate<'_>>],
        assignment: &Assignment,
    ) -> bool {
        let n = names.len();
        let index = |name: &str| names.iter().position(|x| *x == name);
        if (0..n).any(|i| is_root[i] && assignment[i].is_none()) {
            return false;
        }
        let mut reachable = is_root.to_vec();
        let mut queue: Vec<usize> = (0..n).filter(|&i| is_root[i]).collect();
        while let Some(i) = queue.pop() {
            let Some(ci) = assignment[i] else {
                return false;
            };
            for dep in per_package[i][ci].deps() {
                let Some(j) = index(&dep.name) else { continue };
                match assignment[j] {
                    Some(cj) if meets(&dep.req, &per_package[j][cj]) => {}
                    _ => return false,
                }
                if !reachable[j] {
                    reachable[j] = true;
                    queue.push(j);
                }
            }
        }
        (0..n).all(|i| reachable[i] == assignment[i].is_some())
    }

    /// Every answer, by brute force over all assignments.
    fn all_answers(
        names: &[&str],
        is_root: &[bool],
        per_package: &[Vec<Candidate<'_>>],
    ) -> Vec<Assignment> {
        let n = names.len();
        // Per package: one more state than it has candidates, the last
        // meaning "left out".
        let states: Vec<usize> = per_package.iter().map(|c| c.len() + 1).collect();
        let mut found = Vec::new();
        let mut idx = vec![0usize; n];
        loop {
            let assignment: Assignment = (0..n)
                .map(|i| (idx[i] < per_package[i].len()).then_some(idx[i]))
                .collect();
            if is_answer(names, is_root, per_package, &assignment) {
                found.push(assignment);
            }
            let mut k = n;
            loop {
                if k == 0 {
                    return found;
                }
                k -= 1;
                idx[k] += 1;
                if idx[k] < states[k] {
                    break;
                }
                idx[k] = 0;
            }
        }
    }

    proptest! {
        /// What `solve_with` returns is an answer, its roots' versions are
        /// the best any answer has (earlier roots first, newest first), and
        /// it refuses only when there is no answer at all.
        #[test]
        fn prop_solve_with_matches_brute_force(
            (lists, pins, houdini) in arb_problem(),
            is_root in prop::collection::vec(any::<bool>(), 3),
        ) {
            let target = houdini.and_then(HoudiniTarget::parse);
            let make = |i: usize| Root {
                name: NAMES[i],
                pin: if is_root[i] { pins[i].as_deref() } else { None },
                versions: &lists[i],
            };
            let roots: Vec<Root<'_>> = (0..3).filter(|&i| is_root[i]).map(make).collect();
            let available: Vec<Root<'_>> = (0..3).filter(|&i| !is_root[i]).map(make).collect();
            let root_error = roots.iter().any(|r| candidates_for(r, target, Some(HOST)).is_err());

            // An available package with nothing to offer has no candidates,
            // which is exactly what leaves "left out" as its only state.
            let per_package: Vec<Vec<Candidate<'_>>> = (0..3)
                .map(|i| candidates_for(&make(i), target, Some(HOST)).unwrap_or_default())
                .collect();
            let answers = if root_error {
                Vec::new()
            } else {
                all_answers(&NAMES, &is_root, &per_package)
            };
            let root_choices = |a: &Assignment| -> Vec<usize> {
                (0..3).filter(|&i| is_root[i]).map(|i| a[i].unwrap()).collect()
            };

            match solve_for_host(&roots, &available, target, Some(HOST)) {
                Ok(sel) => {
                    let got: Assignment = (0..3)
                        .map(|i| {
                            sel.iter().find(|s| s.name == NAMES[i]).map(|s| {
                                per_package[i].iter().position(|c| c.version == s.version).unwrap()
                            })
                        })
                        .collect();
                    prop_assert!(answers.contains(&got), "not an answer: {:?}", versions(&sel));
                    let best = answers.iter().map(root_choices).min().unwrap();
                    prop_assert_eq!(root_choices(&got), best, "a better choice for the roots exists");
                    let root_names: Vec<&str> = roots.iter().map(|r| r.name).collect();
                    let first: Vec<&str> = sel.iter().take(roots.len()).map(|s| s.name.as_str()).collect();
                    prop_assert_eq!(first, root_names, "roots first, in order");
                }
                Err(SolveError::Conflict { .. }) => {
                    prop_assert!(!root_error);
                    prop_assert!(answers.is_empty(), "refused, but {:?} is an answer", answers[0]);
                }
                Err(e) => prop_assert!(root_error, "{e}"),
            }
        }
    }

    // ---- The order requirements are listed in ----------------------------

    /// The same release reaches the solver with its `deps` in different
    /// orders: a registry serves the publisher's manifest order, a working
    /// copy or installed manifest read offline comes out sorted. The search
    /// took the first answer in the order a version listed its `deps`, so
    /// when a set had several answers, the CLI and an application that
    /// embeds hpm could install different versions of one project.
    #[test]
    fn the_order_a_version_lists_its_requirements_in_changes_nothing() {
        const RIG_A: &str = "a/rig";
        let rig = vec![
            entry(RIG_A, "3.0.0", None, &[(MODEL, "<2")]),
            entry(RIG_A, "1.0.0", None, &[]),
        ];
        let model = vec![
            entry(MODEL, "2.0.0", None, &[]),
            entry(MODEL, "1.0.0", None, &[]),
        ];
        let solve_pipe = |deps: &[(&str, &str)]| {
            let pipe = vec![entry(PIPE, "1.0.0", None, deps)];
            let sel = solve_for_host(
                &[root(PIPE, &pipe)],
                &[root(RIG_A, &rig), root(MODEL, &model)],
                None,
                Some(HOST),
            )
            .unwrap();
            versions(&sel)
                .into_iter()
                .map(|(n, v)| (n.to_string(), v.to_string()))
                .collect::<Vec<_>>()
        };
        let model_first = solve_pipe(&[(MODEL, "<3"), (RIG_A, "*")]);
        let rig_first = solve_pipe(&[(RIG_A, "*"), (MODEL, "<3")]);
        assert_eq!(model_first, rig_first);
        // Both are answers; the one found is the one for the requirements
        // by name: a/rig is placed before b/model and takes its newest.
        assert_eq!(
            rig_first,
            [(PIPE, "1.0.0"), (RIG_A, "3.0.0"), (MODEL, "1.0.0")]
                .map(|(n, v)| (n.to_string(), v.to_string()))
        );
    }

    /// A world built to have several answers often: one listed package
    /// requiring two others, which require each other, so the order the two
    /// are placed in decides which answer the search finds first. The two
    /// are sometimes listed too, and requirements may name one package
    /// twice. Names, the root flags, then the versions per name.
    fn arb_ordered_world()
    -> impl Strategy<Value = ([&'static str; 3], [bool; 3], Vec<Vec<RegistryEntry>>)> {
        const WORLD: [&str; 3] = ["a/pipe", "a/rig", "b/model"];
        // Mostly loose requirements: a tight one leaves a single answer.
        let dep = |names: &'static [&'static str]| {
            (
                prop::sample::select(names),
                prop_oneof![1 => Just("*".to_string()), 2 => arb_req()],
            )
        };
        let versions = |name: &'static str, deps: BoxedStrategy<Vec<(&'static str, String)>>| {
            prop::collection::vec((prop::sample::select(&VERSIONS[..]), deps), 1..5).prop_map(
                move |vs| {
                    let mut seen = std::collections::HashSet::new();
                    vs.into_iter()
                        .filter(|(v, _)| seen.insert(*v))
                        .map(|(v, deps)| {
                            let deps: Vec<(&str, &str)> =
                                deps.iter().map(|(n, r)| (*n, r.as_str())).collect();
                            entry(name, v, None, &deps)
                        })
                        .collect::<Vec<_>>()
                },
            )
        };
        (
            versions(
                WORLD[0],
                prop::collection::vec(dep(&WORLD[1..]), 1..4).boxed(),
            ),
            versions(
                WORLD[1],
                prop::collection::vec(dep(&WORLD[2..]), 0..3).boxed(),
            ),
            versions(
                WORLD[2],
                prop::collection::vec(dep(&WORLD[1..2]), 0..3).boxed(),
            ),
            prop::bool::weighted(0.2),
            prop::bool::weighted(0.2),
        )
            .prop_map(|(pipe, rig, model, rig_listed, model_listed)| {
                (
                    WORLD,
                    [true, rig_listed, model_listed],
                    vec![pipe, rig, model],
                )
            })
    }

    /// `world`'s solve: what it selects, in order, or why it refuses.
    fn solve_world(
        names: &[&str; 3],
        is_root: &[bool; 3],
        lists: &[Vec<RegistryEntry>],
    ) -> Result<Vec<(String, String)>, SolveError> {
        let make = |i: usize| root(names[i], &lists[i]);
        let roots: Vec<Root<'_>> = (0..3).filter(|&i| is_root[i]).map(make).collect();
        let available: Vec<Root<'_>> = (0..3).filter(|&i| !is_root[i]).map(make).collect();
        solve_for_host(&roots, &available, None, Some(HOST))
            .map(|sel| sel.into_iter().map(|s| (s.name, s.version)).collect())
    }

    /// Guards the property below against passing vacuously: the worlds it
    /// draws must often have more than one answer, or no listing order
    /// could change the result. Without the solver's sort, the property
    /// failed within its first few hundred cases.
    #[test]
    fn the_ordered_worlds_often_have_several_answers() {
        use proptest::strategy::ValueTree;
        let mut runner = proptest::test_runner::TestRunner::deterministic();
        let strategy = arb_ordered_world();
        let draws = 400;
        let several = (0..draws)
            .filter(|_| {
                let (names, is_root, lists) = strategy.new_tree(&mut runner).unwrap().current();
                let per_package: Vec<Vec<Candidate<'_>>> = (0..3)
                    .map(|i| candidates_for(&root(names[i], &lists[i]), None, Some(HOST)).unwrap())
                    .collect();
                all_answers(&names, &is_root, &per_package).len() > 1
            })
            .count();
        assert!(
            several * 5 >= draws,
            "only {several} of {draws} worlds have several answers"
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(1024))]

        /// Listing every version's requirements in another order (reversed,
        /// or rotated by one) changes neither what is selected, nor its
        /// order, nor the error and everything a conflict names.
        #[test]
        fn prop_the_order_requirements_are_listed_in_changes_nothing(
            (names, is_root, lists) in arb_ordered_world(),
        ) {
            let relisted = |reorder: fn(&mut Vec<RegistryDep>)| -> Vec<Vec<RegistryEntry>> {
                lists
                    .iter()
                    .map(|vs| {
                        vs.iter()
                            .map(|e| {
                                let mut e = e.clone();
                                reorder(&mut e.deps);
                                e
                            })
                            .collect()
                    })
                    .collect()
            };
            let listed = solve_world(&names, &is_root, &lists);
            let reversed = solve_world(&names, &is_root, &relisted(|d| d.reverse()));
            let rotated = solve_world(&names, &is_root, &relisted(|d| {
                if !d.is_empty() {
                    d.rotate_left(1)
                }
            }));
            prop_assert_eq!(&listed, &reversed);
            prop_assert_eq!(&listed, &rotated);
        }
    }

    /// A general world (pins, yanked versions, Houdini ranges, platforms,
    /// some packages only available) with every list it is made of
    /// reordered: each version's `deps`, each package's versions, and the
    /// available packages.
    type Relisted = (
        Vec<Vec<RegistryEntry>>,
        Vec<Option<String>>,
        Option<&'static str>,
        Vec<bool>,
        Vec<Vec<RegistryEntry>>,
        Vec<usize>,
    );

    fn arb_relisted_problem() -> impl Strategy<Value = Relisted> {
        (arb_problem(), prop::collection::vec(any::<bool>(), 3)).prop_flat_map(
            |((lists, pins, houdini), is_root)| {
                let relisted: Vec<_> = lists
                    .iter()
                    .map(|versions| {
                        let entries: Vec<_> = versions
                            .iter()
                            .map(|e| {
                                let e = e.clone();
                                Just(e.deps.clone())
                                    .prop_shuffle()
                                    .prop_map(move |deps| RegistryEntry { deps, ..e.clone() })
                            })
                            .collect();
                        entries.prop_flat_map(|es| Just(es).prop_shuffle())
                    })
                    .collect();
                (
                    Just(lists),
                    Just(pins),
                    Just(houdini),
                    Just(is_root),
                    relisted,
                    Just(vec![0usize, 1, 2]).prop_shuffle(),
                )
            },
        )
    }

    /// Solve NAMES with `lists`: the roots in NAMES order, the rest offered
    /// as available in `offer` order. What is selected, in order, or why
    /// not.
    fn solve_listed(
        lists: &[Vec<RegistryEntry>],
        pins: &[Option<String>],
        houdini: Option<&str>,
        is_root: &[bool],
        offer: &[usize],
        extra: &[Root<'_>],
    ) -> Result<Vec<(String, String)>, SolveError> {
        let make = |i: usize| Root {
            name: NAMES[i],
            pin: if is_root[i] { pins[i].as_deref() } else { None },
            versions: &lists[i],
        };
        let roots: Vec<Root<'_>> = (0..3).filter(|&i| is_root[i]).map(make).collect();
        let mut available: Vec<Root<'_>> = offer
            .iter()
            .copied()
            .filter(|&i| !is_root[i])
            .map(make)
            .collect();
        available.extend_from_slice(extra);
        solve_for_host(
            &roots,
            &available,
            houdini.and_then(HoudiniTarget::parse),
            Some(HOST),
        )
        .map(|sel| sel.into_iter().map(|s| (s.name, s.version)).collect())
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        /// The order of the lists a caller passes in, other than the roots,
        /// is how a registry or a reader happens to produce them: a Git
        /// index and an API registry list versions differently, and a
        /// caller offers available packages in whatever order it fetched
        /// them. None of it changes what is selected, its order, or the
        /// error.
        #[test]
        fn prop_the_order_a_world_is_listed_in_changes_nothing(
            (lists, pins, houdini, is_root, relisted, offer) in arb_relisted_problem(),
        ) {
            let listed = solve_listed(&lists, &pins, houdini, &is_root, &[0, 1, 2], &[]);
            let reordered = solve_listed(&relisted, &pins, houdini, &is_root, &offer, &[]);
            prop_assert_eq!(listed, reordered);
        }

        /// Offering a package nothing requires, even one with no usable
        /// version, or offering one twice or as a root as well, changes
        /// nothing: an available package takes part only when required.
        #[test]
        fn prop_offering_more_than_is_required_changes_nothing(
            (lists, pins, houdini) in arb_problem(),
            is_root in prop::collection::vec(any::<bool>(), 3),
            unrequired in arb_versions("c/unrequired"),
            twice in 0usize..3,
        ) {
            let listed = solve_listed(&lists, &pins, houdini, &is_root, &[0, 1, 2], &[]);
            let extra = [
                Root { name: "c/unrequired", pin: None, versions: &unrequired },
                Root { name: NAMES[twice], pin: None, versions: &lists[twice] },
            ];
            let offered = solve_listed(&lists, &pins, houdini, &is_root, &[0, 1, 2], &extra);
            prop_assert_eq!(listed, offered);
        }
    }

    // ---- What a conflict names ------------------------------------------

    const RIG: &str = "b/rig";
    const WOBL: &str = "b/wobl";
    const KIT: &str = "c/kit";
    const LICENSE: &str = "c/license";

    /// Two listed packages need a shared package nobody lists, at versions
    /// no release of it meets. The conflict names the shared package, says
    /// who pulled it in, and names both requirers with their requirements,
    /// as listed packages a user can act on.
    #[test]
    fn a_conflict_on_a_shared_package_names_the_listed_packages_that_require_it() {
        let rig = vec![entry(RIG, "3.0.0", None, &[(KIT, ">=1.3")])];
        let wobl = vec![entry(WOBL, "0.9.0", None, &[(KIT, "<1.3")])];
        let kit = vec![
            entry(KIT, "1.3.0", None, &[]),
            entry(KIT, "1.2.0", None, &[]),
        ];
        let err = solve_with(
            &[root(RIG, &rig), root(WOBL, &wobl)],
            &[root(KIT, &kit)],
            None,
        )
        .unwrap_err();
        let SolveError::Conflict {
            name,
            origin,
            rejected,
        } = &err
        else {
            panic!("expected a conflict, got {err:?}");
        };
        assert_eq!(name, KIT);
        let rig_3 = PackageVersion {
            name: RIG.to_string(),
            version: "3.0.0".to_string(),
        };
        assert_eq!(
            *origin,
            Origin::Required {
                chain: vec![rig_3.clone()]
            }
        );
        let listed = |name: &str, version: &str| Selected {
            name: name.to_string(),
            version: version.to_string(),
            origin: Origin::Listed { pinned: false },
        };
        assert_eq!(
            rejected,
            &[
                Rejection {
                    version: "1.3.0".to_string(),
                    causes: vec![Cause::ExcludedBy {
                        by: listed(WOBL, "0.9.0"),
                        requirement: "<1.3".to_string(),
                    }],
                },
                Rejection {
                    version: "1.2.0".to_string(),
                    causes: vec![Cause::ExcludedBy {
                        by: listed(RIG, "3.0.0"),
                        requirement: ">=1.3".to_string(),
                    }],
                },
            ]
        );
        assert_eq!(
            err.to_string(),
            "no version of c/kit works with the other packages: \
             1.3.0 is excluded by b/wobl 0.9.0, which requires <1.3; \
             1.2.0 is excluded by b/rig 3.0.0, which requires >=1.3"
        );
    }

    /// A requirer two levels down is named with the chain that pulled it in,
    /// and a version every selected requirer rules out names all of them.
    #[test]
    fn a_deeper_requirer_carries_its_chain_and_every_cause_is_named() {
        let rig = vec![entry(RIG, "3.0.0", None, &[(KIT, ">=1")])];
        let wobl = vec![entry(WOBL, "0.9.0", None, &[(LICENSE, "<2")])];
        let kit = vec![entry(KIT, "1.3.0", None, &[(LICENSE, ">=2")])];
        let license = vec![
            entry(LICENSE, "2.0.0", None, &[]),
            entry(LICENSE, "1.0.0", None, &[]),
        ];
        let err = solve_with(
            &[root(RIG, &rig), root(WOBL, &wobl)],
            &[root(KIT, &kit), root(LICENSE, &license)],
            None,
        )
        .unwrap_err();
        let SolveError::Conflict {
            name,
            origin,
            rejected,
        } = &err
        else {
            panic!("expected a conflict, got {err:?}");
        };
        assert_eq!(name, LICENSE);
        let pv = |name: &str, version: &str| PackageVersion {
            name: name.to_string(),
            version: version.to_string(),
        };
        // Wobl is placed before Kit, so the search comes to License
        // through Wobl.
        assert_eq!(
            *origin,
            Origin::Required {
                chain: vec![pv(WOBL, "0.9.0")]
            }
        );
        let kit_by = Selected {
            name: KIT.to_string(),
            version: "1.3.0".to_string(),
            origin: Origin::Required {
                chain: vec![pv(RIG, "3.0.0")],
            },
        };
        assert_eq!(rejected[0].version, "2.0.0");
        assert_eq!(
            rejected[1].causes,
            [Cause::ExcludedBy {
                by: kit_by,
                requirement: ">=2".to_string(),
            }]
        );
    }

    #[test]
    fn a_required_package_with_nothing_to_offer_is_named_with_its_own_error() {
        let rig = vec![entry(RIG, "3.0.0", None, &[(KIT, ">=1")])];
        let mut kit = vec![entry(KIT, "1.3.0", None, &[])];
        kit[0].yanked = true;
        let err = solve_with(&[root(RIG, &rig)], &[root(KIT, &kit)], None).unwrap_err();
        let SolveError::Conflict { rejected, .. } = &err else {
            panic!("expected a conflict, got {err:?}");
        };
        assert_eq!(
            rejected[0].causes,
            [Cause::Unavailable {
                name: KIT.to_string(),
                requirement: ">=1".to_string(),
                error: Box::new(SolveError::NoUsableVersion {
                    name: KIT.to_string()
                }),
            }]
        );
    }

    /// Whether `version` meets `req`, by the grammar alone.
    fn holds(req: &str, version: &str) -> bool {
        requirement(req)
            .zip(Version::parse(version).ok())
            .is_some_and(|(r, v)| r.matches(&v))
    }

    proptest! {
        /// A conflict tells the truth, and enough of it: every version of
        /// the unplaced package it could take is named; every cause is a
        /// requirement that the registry really declares and that really
        /// fails; every selected package it names has one version and the
        /// origin it really has; so the named requirements together rule out
        /// every version of the unplaced package.
        #[test]
        fn prop_a_conflict_names_real_requirements_that_rule_out_every_version(
            (lists, pins, houdini) in arb_problem(),
            is_root in prop::collection::vec(any::<bool>(), 3),
        ) {
            let target = houdini.and_then(HoudiniTarget::parse);
            let (roots, available) = split(&lists, &pins, &is_root);
            let Err(SolveError::Conflict { name, origin, rejected }) =
                solve_for_host(&roots, &available, target, Some(HOST))
            else {
                return Ok(());
            };
            let index = |n: &str| NAMES.iter().position(|&m| m == n).unwrap();
            let make = |i: usize| Root {
                name: NAMES[i],
                pin: if is_root[i] { pins[i].as_deref() } else { None },
                versions: &lists[i],
            };
            let deps_of = |n: &str, v: &str| -> Vec<RegistryDep> {
                lists[index(n)].iter().find(|e| e.version == v).map(|e| e.deps.clone()).unwrap_or_default()
            };
            let requires = |n: &str, v: &str, on: &str, req: &str| {
                deps_of(n, v).iter().any(|d| d.name == on && d.req == req)
            };

            // Every candidate is named, in the solver's order, with a cause.
            let candidates: Vec<String> = candidates_for(&make(index(&name)), target, Some(HOST))
                .unwrap()
                .into_iter()
                .map(|c| c.version)
                .collect();
            let named: Vec<String> = rejected.iter().map(|r| r.version.clone()).collect();
            prop_assert_eq!(named, candidates);

            let mut assignment: Vec<(String, String)> = Vec::new();
            let mut check_origin = |pkg: &str, version: Option<&str>, origin: &Origin|
                -> Result<(), TestCaseError>
            {
                if let Some(v) = version {
                    match assignment.iter().find(|(n, _)| n == pkg) {
                        Some((_, w)) => prop_assert_eq!(w.as_str(), v, "{} named at two versions", pkg),
                        None => assignment.push((pkg.to_string(), v.to_string())),
                    }
                }
                let i = index(pkg);
                match origin {
                    Origin::Listed { pinned } => {
                        prop_assert!(is_root[i], "{} is not listed", pkg);
                        prop_assert_eq!(*pinned, pins[i].is_some());
                        if let (Some(pin), Some(v)) = (&pins[i], version) {
                            prop_assert_eq!(pin.as_str(), v);
                        }
                    }
                    Origin::Required { chain } => {
                        prop_assert!(!is_root[i], "{} is listed", pkg);
                        prop_assert!(!chain.is_empty());
                        prop_assert!(is_root[index(&chain[0].name)], "chain starts unlisted");
                        for link in chain.windows(2) {
                            prop_assert!(!is_root[index(&link[1].name)]);
                            prop_assert!(deps_of(&link[0].name, &link[0].version).iter().any(|d| d.name == link[1].name));
                        }
                        let last = chain.last().unwrap();
                        prop_assert!(deps_of(&last.name, &last.version).iter().any(|d| d.name == pkg));
                    }
                }
                Ok(())
            };
            check_origin(&name, None, &origin)?;

            for r in &rejected {
                prop_assert!(!r.causes.is_empty(), "{} {} has no cause", name, r.version);
                for cause in &r.causes {
                    match cause {
                        Cause::ExcludedBy { by, requirement } => {
                            prop_assert!(requires(&by.name, &by.version, &name, requirement));
                            prop_assert!(!holds(requirement, &r.version));
                            check_origin(&by.name, Some(&by.version), &by.origin)?;
                        }
                        Cause::Requires { requirement, selected } => {
                            prop_assert!(requires(&name, &r.version, &selected.name, requirement));
                            prop_assert!(!holds(requirement, &selected.version));
                            check_origin(&selected.name, Some(&selected.version), &selected.origin)?;
                        }
                        Cause::Unavailable { name: on, requirement, error } => {
                            prop_assert!(requires(&name, &r.version, on, requirement));
                            prop_assert!(!is_root[index(on)]);
                            prop_assert_eq!(
                                Err(error.as_ref().clone()),
                                candidates_for(&make(index(on)), target, Some(HOST)).map(|_| ())
                            );
                        }
                    }
                }
            }
            prop_assert!(assignment.iter().all(|(n, _)| *n != name), "the unplaced package is named as selected");
        }
    }

    proptest! {
        /// Pinning every root at the version the solver chose gives back
        /// the same answer, the available packages included: following the
        /// advice to pin what works never moves anything.
        #[test]
        fn prop_pinning_the_answer_keeps_it(
            (lists, pins, is_root, houdini, host) in arb_row_problem(),
        ) {
            let target = houdini.and_then(HoudiniTarget::parse);
            let (roots, available) = split(&lists, &pins, &is_root);
            let Ok(sel) = solve_for_host(&roots, &available, target, host) else { return Ok(()); };
            let pinned: Vec<Root<'_>> = roots.iter().map(|r| Root {
                pin: Some(sel.iter().find(|s| s.name == r.name).unwrap().version.as_str()),
                ..*r
            }).collect();
            let again = solve_for_host(&pinned, &available, target, host);
            prop_assert_eq!(again.map(|a| versions(&a).into_iter().map(|(n,v)|(n.to_string(),v.to_string())).collect::<Vec<_>>()),
                Ok(versions(&sel).into_iter().map(|(n,v)|(n.to_string(),v.to_string())).collect::<Vec<_>>()));
        }
    }

    // ---- Build rows and the host platform --------------------------------
    //
    // The brute force above takes its candidates from `candidates_for`, so
    // it cannot catch a mistake in the filter itself. These properties check
    // the answer against install's rule directly, and against the listing
    // view (`dedupe_builds_by_version`) the solver is normally fed.

    /// Build platforms a row may carry, for a host drawn from the same pool.
    const BUILD_PLATFORMS: [Option<&str>; 5] = [
        None,
        Some("universal"),
        Some("linux-x86_64"),
        Some("windows-x86_64"),
        Some("macos-aarch64"),
    ];
    const HOSTS: [Option<Platform>; 4] = [
        Some(Platform::LinuxX86_64),
        Some(Platform::WindowsX86_64),
        Some(Platform::MacosAarch64),
        None,
    ];

    /// A package's raw build rows, as a registry serves them: one to three
    /// rows per version, differing only in platform, download and yanked,
    /// the version's other metadata shared.
    fn arb_rows(name: &'static str) -> impl Strategy<Value = Vec<RegistryEntry>> {
        let builds = prop::collection::vec(
            (prop::sample::select(&BUILD_PLATFORMS[..]), any::<bool>()),
            1..4,
        );
        prop::collection::vec((arb_entry(name), builds), 1..5).prop_map(|versions| {
            let mut seen = std::collections::HashSet::new();
            let mut rows = Vec::new();
            for (base, builds) in versions {
                if !seen.insert(base.version.clone()) {
                    continue;
                }
                for (i, (platform, yanked)) in builds.into_iter().enumerate() {
                    let mut row = base.clone();
                    row.platform = platform.map(|p| PlatformTag::from(p.to_string()));
                    row.yanked = yanked;
                    row.dl = format!("{}#{i}", base.dl);
                    rows.push(row);
                }
            }
            rows
        })
    }

    /// Per package its rows, pin and whether it is a root; then the Houdini
    /// and the host.
    type RowProblem = (
        Vec<Vec<RegistryEntry>>,
        Vec<Option<String>>,
        Vec<bool>,
        Option<&'static str>,
        Option<Platform>,
    );

    fn arb_row_problem() -> impl Strategy<Value = RowProblem> {
        (
            (arb_rows(NAMES[0]), arb_rows(NAMES[1]), arb_rows(NAMES[2]))
                .prop_map(|(a, b, c)| vec![a, b, c]),
            prop::collection::vec(
                prop::option::weighted(
                    0.3,
                    prop::sample::select(&VERSIONS[..]).prop_map(str::to_string),
                ),
                3,
            ),
            prop::collection::vec(any::<bool>(), 3),
            prop::sample::select(&[None, Some("21.0"), Some("22.0.1")][..]),
            prop::sample::select(&HOSTS[..]),
        )
    }

    /// Roots and available packages from a problem: `is_root[i]` places
    /// package `i`; only a root carries its pin.
    fn split<'a>(
        lists: &'a [Vec<RegistryEntry>],
        pins: &'a [Option<String>],
        is_root: &[bool],
    ) -> (Vec<Root<'a>>, Vec<Root<'a>>) {
        let make = |i: usize| Root {
            name: NAMES[i],
            pin: if is_root[i] { pins[i].as_deref() } else { None },
            versions: &lists[i],
        };
        (
            (0..3).filter(|&i| is_root[i]).map(make).collect(),
            (0..3).filter(|&i| !is_root[i]).map(make).collect(),
        )
    }

    proptest! {
        /// Install's rule, checked without the solver's own filter: every
        /// version chosen for an unpinned or offered package is live,
        /// supports the Houdini, and is a row install can download on the
        /// host; every pin is chosen as written.
        #[test]
        fn prop_every_unpinned_choice_is_installable(
            (lists, pins, is_root, houdini, host) in arb_row_problem(),
        ) {
            let target = houdini.and_then(HoudiniTarget::parse);
            let (roots, available) = split(&lists, &pins, &is_root);
            let Ok(sel) = solve_for_host(&roots, &available, target, host) else {
                return Ok(());
            };
            for s in &sel {
                if let Some(pin) = roots.iter().find(|r| r.name == s.name).and_then(|r| r.pin) {
                    prop_assert_eq!(&s.version, pin);
                    continue;
                }
                let e = s.entry.as_ref().expect("an unpinned choice is a listed row");
                prop_assert!(!e.yanked, "{} {} is yanked", s.name, s.version);
                prop_assert!(target.is_none_or(|h| h.admits(e)));
                let installable = match &e.platform {
                    None => true,
                    Some(tag) => tag.is_universal() || host.is_some_and(|h| tag.matches(h)),
                };
                prop_assert!(installable, "{} {} has no build for {:?}", s.name, s.version, host);
            }
        }

        /// Solving the raw build rows and solving the listing view of them
        /// (what `RegistrySet::get_versions` returns, and what callers pass)
        /// give the same result: the same versions, standing for the same
        /// build rows, or the same error, a conflict naming each version
        /// once.
        #[test]
        fn prop_raw_rows_and_the_listing_solve_alike(
            (lists, pins, is_root, houdini, host) in arb_row_problem(),
        ) {
            let target = houdini.and_then(HoudiniTarget::parse);
            let listed: Vec<Vec<RegistryEntry>> = lists
                .iter()
                .map(|rows| crate::registry::dedupe_builds_by_version(rows.clone(), host))
                .collect();
            let (raw_roots, raw_available) = split(&lists, &pins, &is_root);
            let (roots, available) = split(&listed, &pins, &is_root);
            let raw = solve_for_host(&raw_roots, &raw_available, target, host);
            let view = solve_for_host(&roots, &available, target, host);
            prop_assert_eq!(raw, view);
        }

        /// A universal build serves every host: when every row is universal,
        /// the host makes no difference to the answer.
        #[test]
        fn prop_universal_builds_ignore_the_host(
            (mut lists, pins, is_root, houdini, host) in arb_row_problem(),
        ) {
            for rows in &mut lists {
                for (i, row) in rows.iter_mut().enumerate() {
                    row.platform = (i % 2 == 0).then(|| PlatformTag::from("universal".to_string()));
                }
            }
            let target = houdini.and_then(HoudiniTarget::parse);
            let (roots, available) = split(&lists, &pins, &is_root);
            let here = solve_for_host(&roots, &available, target, host);
            let linux = solve_for_host(&roots, &available, target, Some(Platform::LinuxX86_64));
            prop_assert_eq!(here, linux);
        }
    }
}
