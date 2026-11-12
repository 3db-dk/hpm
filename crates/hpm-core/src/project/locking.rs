//! Building `hpm.lock` from a sync's outcomes.

use crate::lock::{LockFile, LockedDependency, LockedPythonDependency, LockedSource};
use crate::registry::RegistrySet;
use hpm_package::{DependencySpec, PackageManifest};

use super::{InstallOutcome, ProjectError, ProjectManager};

/// The URL older hpm versions wrote as the source of a dependency they
/// found already installed and had no previous lock entry for. Read as
/// "source unknown", so the next lock resolves the real one.
const UNRESOLVED_URL: &str = "unresolved";

impl ProjectManager {
    /// The lockfile for this project after a sync, given the sync's
    /// `outcomes` and the previous lockfile, if any.
    ///
    /// A freshly fetched dependency records its checksum and source. One the
    /// sync found already installed carries no checksum or source of its
    /// own: those come from the previous lock entry, and when there is none
    /// (or it holds an older hpm's `"unresolved"` placeholder) the source is
    /// looked up in the project's registries, so the lock never records a
    /// URL nobody can fetch. That lookup is the only network this does, and
    /// its failure is an error.
    pub async fn lock_file(
        &self,
        manifest: &PackageManifest,
        outcomes: &[(String, InstallOutcome)],
        existing: Option<&LockFile>,
    ) -> Result<LockFile, ProjectError> {
        let mut lock = LockFile::new(
            manifest.package.name.clone(),
            manifest.package.version.clone(),
        );
        let mut registries: Option<RegistrySet> = None;

        for (name, outcome) in outcomes {
            let prior = existing.and_then(|l| l.get_dependency(name));
            let checksum = outcome
                .checksum
                .clone()
                .or_else(|| prior.and_then(|p| p.checksum.clone()));
            let known = outcome.source.clone().or_else(|| {
                prior.map(|p| p.source.clone()).filter(
                    |s| !matches!(s, LockedSource::Url { url, .. } if url == UNRESOLVED_URL),
                )
            });
            let version = outcome.package.version.clone();
            let source = match known {
                Some(source) => source,
                None => {
                    let pinned = match manifest.dependencies.get(name) {
                        Some(DependencySpec::Registry { registry, .. }) => registry.as_deref(),
                        _ => None,
                    };
                    let set = match &mut registries {
                        Some(set) => set,
                        None => registries.insert(
                            RegistrySet::for_project(
                                &manifest.registries,
                                &self.config,
                                self.auth_token.as_deref(),
                            )
                            .map_err(|e| ProjectError::RegistryConfiguration(Box::new(e)))?,
                        ),
                    };
                    let entry =
                        set.get_version_in(name, &version, pinned)
                            .await
                            .map_err(|source| ProjectError::RegistryResolution {
                                name: name.clone(),
                                version_req: version.clone(),
                                source: Box::new(source),
                            })?;
                    LockedSource::url(entry.dl, version.clone())
                }
            };
            lock.add_dependency(
                name.clone(),
                LockedDependency {
                    version,
                    checksum,
                    source,
                },
            );
        }

        for (name, spec) in &manifest.python_dependencies {
            lock.add_python_dependency(
                name.clone(),
                LockedPythonDependency::new(spec.version_req().to_string()),
            );
        }
        Ok(lock)
    }
}
