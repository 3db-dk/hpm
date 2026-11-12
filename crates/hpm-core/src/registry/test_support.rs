//! A registry over a fixed list of entries, for tests that need a
//! [`RegistrySet`](super::RegistrySet) without a network or a git index.

use super::types::{RegistryEntry, SearchResults};
use super::{Registry, RegistryError};
use async_trait::async_trait;

/// An entry for `package@version`, yanked or not, with a download URL that
/// is never reachable (a test that downloads fails loudly).
pub(crate) fn entry(package: &str, version: &str, yanked: bool) -> RegistryEntry {
    RegistryEntry {
        name: package.to_string(),
        version: version.to_string(),
        cksum: None,
        dl: format!("http://127.0.0.1:9/{package}-{version}.zip"),
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

/// A registry holding a fixed set of entries.
pub(crate) struct FakeRegistry {
    name: String,
    entries: Vec<RegistryEntry>,
}

impl FakeRegistry {
    /// A registry named `name` listing each `(package, version)`, none
    /// yanked.
    pub(crate) fn new(name: &str, packages: &[(&str, &str)]) -> Self {
        // The download URL names the registry, so a test can tell which
        // registry an entry came from.
        Self::with_entries(
            name,
            packages
                .iter()
                .map(|(p, v)| RegistryEntry {
                    dl: format!("https://{name}/{p}-{v}.tar.gz"),
                    ..entry(p, v, false)
                })
                .collect(),
        )
    }

    pub(crate) fn with_entries(name: &str, entries: Vec<RegistryEntry>) -> Self {
        Self {
            name: name.to_string(),
            entries,
        }
    }
}

#[async_trait]
impl Registry for FakeRegistry {
    async fn search(&self, _query: &str) -> Result<SearchResults, RegistryError> {
        Ok(SearchResults {
            packages: self.entries.clone(),
            total: self.entries.len(),
        })
    }

    async fn get_versions(&self, name: &str) -> Result<Vec<RegistryEntry>, RegistryError> {
        let found: Vec<_> = self
            .entries
            .iter()
            .filter(|e| e.name == name)
            .cloned()
            .collect();
        if found.is_empty() {
            return Err(RegistryError::PackageNotFound {
                name: name.to_string(),
            });
        }
        Ok(found)
    }

    async fn get_version(&self, name: &str, version: &str) -> Result<RegistryEntry, RegistryError> {
        self.entries
            .iter()
            .find(|e| e.name == name && e.version == version)
            .cloned()
            .ok_or_else(|| RegistryError::VersionNotFound {
                name: name.to_string(),
                version: version.to_string(),
            })
    }

    async fn refresh(&self) -> Result<(), RegistryError> {
        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }
}
