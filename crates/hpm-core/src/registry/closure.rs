//! A project's whole package set: the packages it lists, and every package
//! the versions chosen for them require, directly or through each other.
//!
//! [`solve`] chooses versions for packages it is handed, and
//! lets a package join the set when a chosen version requires it, but only
//! among the packages it was offered. Someone has to find those: ask a
//! registry for each package the answer requires and nobody has fetched,
//! and solve again, until the answer requires nothing new.
//! [`select_closed`] is that loop, shared by every caller that installs a
//! project (`hpm install`, and an application that embeds hpm), so they all
//! arrive at the same set.
//!
//! Where versions come from is the caller's ([`VersionSource`]): a registry
//! online, what is installed offline, a map in tests. So is which packages
//! the project may not have (`excluded`) and how a failure is worded.
//!
//! A package supplied from a directory on this machine (a path dependency,
//! a dev override) takes part as [`working_copy`]: the one version its
//! `hpm.toml` declares, held like a pin, requiring what that file's
//! registry dependencies require, so it brings the packages its release
//! would, and the rest of the set moves to fit it as it would to fit that
//! release.

use super::solve::{self, HoudiniTarget, Root, Selection, SolveError};
use super::types::{RegistryDep, RegistryEntry};
use hpm_package::{DependencySpec, PackageManifest};
use std::collections::HashSet;
use std::future::Future;
use tracing::{info, warn};

/// One package of the set: its scoped path, its pin, and its versions. A
/// package nobody could list (a pin the registry does not have) carries no
/// versions; a pin is still selected as written, and constrains nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidates {
    pub name: String,
    pub pin: Option<String>,
    pub versions: Vec<RegistryEntry>,
    /// Supplied by a directory on this machine ([`working_copy`]): `pin` is
    /// the version its `hpm.toml` declares, not one the user wrote.
    pub on_disk: bool,
}

impl Candidates {
    fn root(&self) -> Root<'_> {
        Root {
            name: &self.name,
            pin: self.pin.as_deref(),
            versions: &self.versions,
        }
    }
}

/// The requirements a release of `manifest` registers: its registry
/// dependencies with a version, by name. `path` and `url` dependencies name
/// a machine's directory or a file, not a registry package, so they are
/// not registered and not followed.
pub fn registry_requirements(manifest: &PackageManifest) -> Vec<RegistryDep> {
    let mut deps: Vec<RegistryDep> = manifest
        .dependencies
        .iter()
        .filter_map(|(name, spec)| match spec {
            DependencySpec::Registry { version, .. } if !version.trim().is_empty() => {
                Some(RegistryDep {
                    name: name.clone(),
                    req: version.trim().to_string(),
                })
            }
            _ => None,
        })
        .collect();
    deps.sort_by(|a, b| (&a.name, &a.req).cmp(&(&b.name, &b.req)));
    deps
}

/// The registry entry a release of `manifest` would have, known as `name`:
/// its version and [`registry_requirements`], with nothing to download.
/// For a package whose `hpm.toml` is at hand (a working copy, a version
/// already installed), so its requirements need no registry.
pub fn entry_of(name: &str, manifest: &PackageManifest) -> RegistryEntry {
    RegistryEntry {
        name: name.to_string(),
        version: manifest.package.version.clone(),
        cksum: None,
        dl: String::new(),
        sig: None,
        kid: None,
        houdini_compat: None,
        platform: None,
        yanked: false,
        description: None,
        author: None,
        created_at: None,
        deps: registry_requirements(manifest),
    }
}

/// A package supplied from a directory on this machine, as the set sees it:
/// the one version its `hpm.toml` declares, held like a pin (it is what the
/// user pointed at), requiring what that file's registry dependencies
/// require. Requirements on it hold like requirements on any pin: a chosen
/// version that needs another version of it is a conflict. `name` is the
/// name the project knows it by.
pub fn working_copy(name: &str, manifest: &PackageManifest) -> Candidates {
    Candidates {
        name: name.to_string(),
        pin: Some(manifest.package.version.clone()),
        versions: vec![entry_of(name, manifest)],
        on_disk: true,
    }
}

/// Where [`select_closed`] asks for the versions of a package the set comes
/// to require.
pub trait VersionSource {
    /// Every version of `name` (one entry per version), or why there are
    /// none.
    fn versions(
        &mut self,
        name: &str,
    ) -> impl Future<Output = Result<Vec<RegistryEntry>, Unsupplied>>;
}

/// Why a [`VersionSource`] has no versions of a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unsupplied {
    /// The source failed: the choice is refused.
    Failed(String),
    /// There is no version of it here to choose (offline, and none is
    /// installed). It is offered with none, so a version that requires it
    /// is passed over for one whose requirements can be met; the choice is
    /// refused, naming it, only when nothing else works.
    Absent(String),
}

/// A required package [`Unsupplied::Absent`] had no version of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbsentPackage {
    pub name: String,
    /// `<name> <version>` of the chosen version that requires it.
    pub requirer: String,
    pub reason: String,
}

/// Why [`select_closed`] could not choose a set.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClosureError {
    /// The packages' requirements cannot all be met, or a package has no
    /// usable version.
    #[error(transparent)]
    Solve(#[from] SolveError),
    /// A chosen version requires a package the source failed to list.
    #[error(
        "{requirer} requires {name}, which the project does not list and no registry could supply: {reason}"
    )]
    Unsupplied {
        name: String,
        requirer: String,
        reason: String,
    },
    /// No answer works without packages the source has no version of. Named
    /// alone: the solver reads a package offered with no versions as one
    /// whose versions are all yanked, which is not why.
    #[error("{}", describe_absent(.0))]
    Absent(Vec<AbsentPackage>),
    /// The rounds did not settle. Not reachable by a real dependency chain,
    /// which settles in about as many rounds as it is deep.
    #[error("the project's package requirements did not settle within {0} rounds")]
    Unsettled(usize),
}

fn describe_absent(absent: &[AbsentPackage]) -> String {
    absent
        .iter()
        .map(|a| {
            format!(
                "{} requires {}, which the project does not list: {}",
                a.requirer, a.name, a.reason
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Bound on [`select_closed`]'s rounds. Each round offers the solver what
/// the previous answer required and nobody had fetched yet.
pub const MAX_ROUNDS: usize = 16;

/// Choose a version for every package of `listed`, in order, and for every
/// package their chosen versions require, directly or through each other,
/// that `listed` does not name: unpinned, the newest that fits, like any
/// other ([`solve::solve_with`]).
///
/// `supplied` are packages the caller can provide without asking `source`
/// (a working copy of a package the project does not list, a version a lock
/// recorded): one is chosen exactly when a chosen version requires it, at
/// its pin if it has one, and `source` is never asked for it. `excluded`
/// names packages the set must not contain unless `listed` names them (one
/// the user switched off, one the account may not use): a requirement on
/// one is left unmet and logged, it is never fetched, and it is never
/// chosen, even when `supplied` offers it.
///
/// Whether a required package is part of the set is the solver's decision;
/// this only fetches. Each round offers every package fetched so far, and
/// what the answer requires that has not been fetched is fetched for the
/// next round. Nothing fetched is taken back, so the rounds end. Deciding it
/// here instead, by adding a required package and dropping it when a later
/// answer no longer required it, never ended when that package's own
/// requirements moved the version that required it.
///
/// Selections come back with `listed`'s first, in order, then the required
/// ones in the order the search came to need them.
pub async fn select_closed(
    listed: &[Candidates],
    supplied: &[Candidates],
    houdini: Option<HoudiniTarget>,
    source: &mut impl VersionSource,
    excluded: impl Fn(&str) -> bool,
) -> Result<Vec<Selection>, ClosureError> {
    let listed_names: HashSet<&str> = listed.iter().map(|c| c.name.as_str()).collect();
    let mut offered: Vec<Candidates> = supplied
        .iter()
        .filter(|c| !listed_names.contains(c.name.as_str()) && !excluded(&c.name))
        .cloned()
        .collect();
    let mut refused: HashSet<String> = HashSet::new();
    let mut absent: Vec<AbsentPackage> = Vec::new();
    for _ in 0..MAX_ROUNDS {
        let selections = match select(listed, &offered, houdini) {
            Ok(selections) => selections,
            Err(_) if !absent.is_empty() => return Err(ClosureError::Absent(absent)),
            Err(e) => return Err(e.into()),
        };

        // Every selection is listed or required, so what they require is
        // what the set requires.
        let mut missing: Vec<(String, String)> = Vec::new();
        for sel in &selections {
            for dep in sel.entry.iter().flat_map(|e| e.deps.iter()) {
                let name = dep.name.as_str();
                if listed_names.contains(name)
                    || offered.iter().any(|c| c.name == name)
                    || missing.iter().any(|(n, _)| n == name)
                {
                    continue;
                }
                if excluded(name) {
                    if refused.insert(name.to_string()) {
                        warn!(
                            "{} {} requires {}, which this project excludes",
                            sel.name, sel.version, name
                        );
                    }
                    continue;
                }
                missing.push((name.to_string(), format!("{} {}", sel.name, sel.version)));
            }
        }
        if missing.is_empty() {
            return Ok(selections);
        }
        for (name, requirer) in missing {
            let versions = match source.versions(&name).await {
                Ok(versions) => {
                    info!("Offering {name} to the project's packages: {requirer} requires it");
                    versions
                }
                Err(Unsupplied::Failed(reason)) => {
                    return Err(ClosureError::Unsupplied {
                        name,
                        requirer,
                        reason,
                    });
                }
                Err(Unsupplied::Absent(reason)) => {
                    info!("{requirer} requires {name}, which has no version to offer: {reason}");
                    absent.push(AbsentPackage {
                        name: name.clone(),
                        requirer,
                        reason,
                    });
                    Vec::new()
                }
            };
            offered.push(Candidates {
                name,
                pin: None,
                versions,
                on_disk: false,
            });
        }
    }
    Err(ClosureError::Unsettled(MAX_ROUNDS))
}

/// The versions a lock recorded, as [`select_keeping`] offers them: `listed`
/// is the project's packages with each unpinned one pinned at its locked
/// version (while that is one of its versions), and `supplied` the locked
/// versions of packages the project does not list.
#[derive(Debug, Clone, Copy)]
pub struct Kept<'a> {
    pub listed: &'a [Candidates],
    pub supplied: &'a [Candidates],
}

/// [`select_closed`], keeping the versions a lock recorded while they fit:
/// first with `kept`, and when no set fits those, with `fresh` (the
/// project's packages as written, nothing supplied), so every package the
/// project does not pin is chosen anew. With `frozen`, the set must fit the
/// lock: no second choice is made. `excluded` is [`select_closed`]'s, for
/// both choices.
pub async fn select_keeping(
    kept: Kept<'_>,
    fresh: &[Candidates],
    houdini: Option<HoudiniTarget>,
    source: &mut impl VersionSource,
    frozen: bool,
    excluded: impl Fn(&str) -> bool,
) -> Result<Vec<Selection>, ClosureError> {
    match select_closed(kept.listed, kept.supplied, houdini, source, &excluded).await {
        Ok(selections) => Ok(selections),
        Err(e) if frozen => Err(e),
        Err(e) => {
            info!("The versions the lock records no longer fit together ({e}); choosing again");
            select_closed(fresh, &[], houdini, source, &excluded).await
        }
    }
}

/// Choose a version for every package of `packages`, in order, and for each
/// of `offered` that a chosen version requires ([`solve::solve_with`]).
///
/// The order a version lists its requirements in does not matter: the
/// solver takes them by name, then requirement.
pub fn select(
    packages: &[Candidates],
    offered: &[Candidates],
    houdini: Option<HoudiniTarget>,
) -> Result<Vec<Selection>, SolveError> {
    let roots: Vec<Root<'_>> = packages.iter().map(Candidates::root).collect();
    let available: Vec<Root<'_>> = offered.iter().map(Candidates::root).collect();
    solve::solve_with(&roots, &available, houdini)
}

#[cfg(test)]
#[path = "closure_tests.rs"]
mod tests;
