//! Types for Python dependency management

use anyhow::Result;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

/// Python version specification
///
/// Represents a Python version with major, minor, and optional patch components.
/// Used for Houdini-to-Python version mapping and dependency resolution.
///
/// # Examples
///
/// ```rust
/// use hpm_python::PythonVersion;
///
/// // Create a Python version
/// let version = PythonVersion::new(3, 9, Some(12));
/// assert_eq!(version.to_string(), "3.9.12");
///
/// // Parse from string
/// let parsed: PythonVersion = "3.10".parse().unwrap();
/// assert_eq!(parsed.major, 3);
/// assert_eq!(parsed.minor, 10);
/// assert_eq!(parsed.patch, None);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PythonVersion {
    /// Major version number (e.g., 3 for Python 3.x)
    pub major: u8,
    /// Minor version number (e.g., 9 for Python 3.9.x)
    pub minor: u8,
    /// Optional patch version number (e.g., 12 for Python 3.9.12)
    pub patch: Option<u8>,
}

impl fmt::Display for PythonVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(patch) = self.patch {
            write!(f, "{}.{}.{}", self.major, self.minor, patch)
        } else {
            write!(f, "{}.{}", self.major, self.minor)
        }
    }
}

impl PythonVersion {
    /// Create a new Python version
    ///
    /// # Arguments
    ///
    /// * `major` - Major version number (e.g., 3)
    /// * `minor` - Minor version number (e.g., 9)
    /// * `patch` - Optional patch version number (e.g., Some(12))
    ///
    /// # Examples
    ///
    /// ```rust
    /// use hpm_python::PythonVersion;
    ///
    /// let version = PythonVersion::new(3, 9, Some(12));
    /// assert_eq!(version.major, 3);
    /// assert_eq!(version.minor, 9);
    /// assert_eq!(version.patch, Some(12));
    /// ```
    pub fn new(major: u8, minor: u8, patch: Option<u8>) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl FromStr for PythonVersion {
    type Err = anyhow::Error;

    fn from_str(version: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = version.split('.').collect();
        if parts.is_empty() {
            return Err(anyhow::anyhow!("Empty version string"));
        }
        if parts.len() > 3 {
            return Err(anyhow::anyhow!("Too many version components"));
        }
        let major = parts[0].parse()?;
        let minor = if parts.len() > 1 {
            parts[1].parse()?
        } else {
            0
        };
        let patch = if parts.len() > 2 {
            Some(parts[2].parse()?)
        } else {
            None
        };
        Ok(Self::new(major, minor, patch))
    }
}

/// Version specification for a Python package
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VersionSpec {
    pub spec: String,
}

impl VersionSpec {
    pub fn new(spec: impl Into<String>) -> Self {
        Self { spec: spec.into() }
    }

    pub fn any() -> Self {
        Self::new("*")
    }
}

impl fmt::Display for VersionSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.spec)
    }
}

/// Python package dependency
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PythonDependency {
    pub name: String,
    pub version: VersionSpec,
    pub optional: bool,
    pub extras: Vec<String>,
}

impl PythonDependency {
    pub fn new(name: impl Into<String>, version: VersionSpec) -> Self {
        Self {
            name: name.into(),
            version,
            optional: false,
            extras: Vec::new(),
        }
    }

    pub fn with_extras(mut self, extras: Vec<String>) -> Self {
        self.extras = extras;
        self
    }

    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }
}

/// Collection of Python dependencies
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PythonDependencies {
    pub dependencies: IndexMap<String, PythonDependency>,
    pub python_version: Option<PythonVersion>,
}

impl PythonDependencies {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_dependency(&mut self, dep: PythonDependency) {
        self.dependencies.insert(dep.name.clone(), dep);
    }

    pub fn set_python_version(&mut self, version: PythonVersion) {
        self.python_version = Some(version);
    }

    pub fn merge(&mut self, other: &Self) -> Result<()> {
        for (name, dep) in &other.dependencies {
            if let Some(existing) = self.dependencies.get(name) {
                if existing.version != dep.version {
                    return Err(anyhow::anyhow!(
                        "Conflicting versions for package {}: {} vs {}",
                        name,
                        existing.version,
                        dep.version
                    ));
                }
            } else {
                self.dependencies.insert(name.clone(), dep.clone());
            }
        }

        if let Some(other_py) = &other.python_version {
            if let Some(existing_py) = &self.python_version {
                if existing_py != other_py {
                    return Err(anyhow::anyhow!(
                        "Conflicting Python versions: {} vs {}",
                        existing_py,
                        other_py
                    ));
                }
            } else {
                self.python_version = Some(other_py.clone());
            }
        }

        Ok(())
    }
}

/// Resolved Python dependency set with exact versions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedDependencySet {
    pub packages: BTreeMap<String, String>,
    pub python_version: PythonVersion,
}

impl ResolvedDependencySet {
    pub fn new(python_version: PythonVersion) -> Self {
        Self {
            packages: BTreeMap::new(),
            python_version,
        }
    }

    pub fn add_package(&mut self, name: impl Into<String>, version: impl Into<String>) {
        self.packages.insert(name.into(), version.into());
    }

    /// Generate a unique hash for this dependency set
    pub fn hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(format!("python:{}", self.python_version));
        for (name, version) in &self.packages {
            hasher.update(format!("{}:{}", name, version));
        }
        format!("{:x}", hasher.finalize())[..16].to_string()
    }
}

/// Virtual environment metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VenvMetadata {
    pub hash: String,
    pub dependency_set: ResolvedDependencySet,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub used_by_packages: Vec<String>,
    pub path: PathBuf,
}

impl VenvMetadata {
    pub fn new(hash: String, dependency_set: ResolvedDependencySet, path: PathBuf) -> Self {
        Self {
            hash,
            dependency_set,
            created_at: chrono::Utc::now(),
            used_by_packages: Vec::new(),
            path,
        }
    }

    pub fn add_package_reference(&mut self, package_name: impl Into<String>) {
        let package = package_name.into();
        if !self.used_by_packages.contains(&package) {
            self.used_by_packages.push(package);
        }
    }

    pub fn remove_package_reference(&mut self, package_name: &str) {
        self.used_by_packages.retain(|p| p != package_name);
    }

    pub fn is_orphaned(&self) -> bool {
        self.used_by_packages.is_empty()
    }
}

/// Information about an orphaned virtual environment
#[derive(Debug, Clone)]
pub struct OrphanedVenv {
    pub hash: String,
    pub path: PathBuf,
    pub size: u64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_used: Option<chrono::DateTime<chrono::Utc>>,
}

/// Python dependency management errors
#[derive(Debug, thiserror::Error)]
pub enum PythonError {
    #[error("Conflicting Python dependencies: {conflicts:?}")]
    ConflictingDependencies { conflicts: Vec<String> },

    #[error("Python version {required} not compatible with Houdini")]
    IncompatiblePythonVersion { required: String },

    #[error("Failed to resolve dependencies: {message}")]
    ResolutionFailed { message: String },

    #[error("UV binary not found and extraction failed")]
    UvNotAvailable,

    #[error("Virtual environment creation failed: {path}")]
    VenvCreationFailed { path: PathBuf },

    #[error("Virtual environment not found: {hash}")]
    VenvNotFound { hash: String },

    #[error("Invalid Python version format: {version}")]
    InvalidPythonVersion { version: String },

    #[error("Package installation failed: {package}")]
    InstallationFailed { package: String },
}

/// Result type for Python operations
pub type PythonResult<T> = Result<T, PythonError>;

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // Custom strategies for generating test data

    /// Strategy to generate valid Python version numbers
    fn python_version_strategy() -> impl Strategy<Value = PythonVersion> {
        (2u8..=3, 6u8..=12, prop::option::of(0u8..=20))
            .prop_map(|(major, minor, patch)| PythonVersion::new(major, minor, patch))
    }

    /// Strategy to generate valid Python package names
    fn python_package_name_strategy() -> impl Strategy<Value = String> {
        prop::string::string_regex("[a-z][a-z0-9_-]{1,50}")
            .unwrap()
            .prop_filter("Package name must be reasonable length", |name| {
                name.len() >= 2
                    && name.len() <= 50
                    && !name.starts_with('-')
                    && !name.ends_with('-')
            })
    }

    /// Strategy to generate version specifications
    fn version_spec_strategy() -> impl Strategy<Value = VersionSpec> {
        prop_oneof![
            Just(VersionSpec::any()),
            (0u32..100, 0u32..100, 0u32..100).prop_map(|(major, minor, patch)| VersionSpec::new(
                format!("{}.{}.{}", major, minor, patch)
            )),
            (0u32..100, 0u32..100, 0u32..100).prop_map(|(major, minor, patch)| VersionSpec::new(
                format!(">={}.{}.{}", major, minor, patch)
            )),
            (0u32..100, 0u32..100, 0u32..100).prop_map(|(major, minor, patch)| VersionSpec::new(
                format!("~={}.{}.{}", major, minor, patch)
            )),
        ]
    }

    /// Strategy to generate Python dependencies
    fn python_dependency_strategy() -> impl Strategy<Value = PythonDependency> {
        (
            python_package_name_strategy(),
            version_spec_strategy(),
            any::<bool>(),
            prop::collection::vec("(security|testing|dev|docs)", 0..4),
        )
            .prop_map(|(name, version, optional, extras)| {
                let mut dep = PythonDependency::new(name, version);
                if optional {
                    dep = dep.optional();
                }
                if !extras.is_empty() {
                    dep = dep.with_extras(extras);
                }
                dep
            })
    }

    /// Strategy to generate Python dependencies collection
    fn python_dependencies_strategy() -> impl Strategy<Value = PythonDependencies> {
        (
            prop::collection::hash_map(
                python_package_name_strategy(),
                python_dependency_strategy(),
                0..10,
            ),
            prop::option::of(python_version_strategy()),
        )
            .prop_map(|(deps, python_version)| {
                let mut py_deps = PythonDependencies::new();
                for (_, dep) in deps {
                    py_deps.add_dependency(dep);
                }
                if let Some(py_ver) = python_version {
                    py_deps.set_python_version(py_ver);
                }
                py_deps
            })
    }

    /// Strategy to generate resolved dependency sets
    fn resolved_dependency_set_strategy() -> impl Strategy<Value = ResolvedDependencySet> {
        (
            python_version_strategy(),
            prop::collection::btree_map(
                python_package_name_strategy(),
                version_spec_strategy().prop_map(|v| v.spec),
                0..10,
            ),
        )
            .prop_map(|(python_version, packages)| {
                let mut resolved = ResolvedDependencySet::new(python_version);
                for (name, version) in packages {
                    resolved.add_package(name, version);
                }
                resolved
            })
    }

    // Property-based tests

    proptest! {
        /// Test that Python version parsing and display is consistent
        #[test]
        fn prop_python_version_roundtrip(version in python_version_strategy()) {
            let version_str = version.to_string();
            let parsed: PythonVersion = version_str.parse().unwrap();
            prop_assert_eq!(version, parsed);
        }

        /// Test that Python version serialization is consistent
        #[test]
        fn prop_python_version_serialization(version in python_version_strategy()) {
            let json = serde_json::to_string(&version).unwrap();
            let deserialized: PythonVersion = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(version, deserialized);
        }

        /// Test that version specifications can be created and displayed consistently
        #[test]
        fn prop_version_spec_consistency(spec in version_spec_strategy()) {
            let spec_str = spec.to_string();
            let new_spec = VersionSpec::new(spec_str.clone());
            prop_assert_eq!(new_spec.spec.clone(), spec_str.clone());
            prop_assert_eq!(new_spec.to_string(), spec_str.clone());
        }

        /// Test that Python dependencies maintain consistency across operations
        #[test]
        fn prop_python_dependency_consistency(dep in python_dependency_strategy()) {
            // Test serialization roundtrip
            let json = serde_json::to_string(&dep).unwrap();
            let deserialized: PythonDependency = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(dep, deserialized);

        }

        /// Test that dependency collections can merge without conflicts when compatible
        #[test]
        fn prop_dependency_merge_compatible(deps1 in python_dependencies_strategy(), deps2 in python_dependencies_strategy()) {
            let mut merged = deps1.clone();

            // Only test merge if there are no conflicting package names
            let conflicting_packages: Vec<_> = deps1.dependencies.keys()
                .filter(|k| deps2.dependencies.contains_key(*k))
                .collect();

            // Check for Python version conflicts (versions must be exactly equal)
            let python_version_conflict = match (&deps1.python_version, &deps2.python_version) {
                (Some(v1), Some(v2)) => v1 != v2,
                _ => false,
            };

            if conflicting_packages.is_empty() && !python_version_conflict {
                let result = merged.merge(&deps2);
                prop_assert!(result.is_ok(), "Non-conflicting merge should succeed");

                // Verify all dependencies from both collections are present
                for (name, dep) in &deps1.dependencies {
                    prop_assert!(merged.dependencies.contains_key(name));
                    prop_assert_eq!(merged.dependencies.get(name).unwrap(), dep);
                }
                for (name, dep) in &deps2.dependencies {
                    prop_assert!(merged.dependencies.contains_key(name));
                    prop_assert_eq!(merged.dependencies.get(name).unwrap(), dep);
                }
            }
        }

        /// Test that resolved dependency sets generate consistent hashes
        #[test]
        fn prop_resolved_set_hash_consistency(resolved in resolved_dependency_set_strategy()) {
            let hash1 = resolved.hash();
            let hash2 = resolved.hash();
            prop_assert_eq!(hash1.clone(), hash2, "Hash should be consistent");

            // Hash should be 16 characters (truncated SHA-256)
            prop_assert_eq!(hash1.len(), 16);
            prop_assert!(hash1.chars().all(|c| c.is_ascii_hexdigit()));
        }

        /// Test that identical dependency sets produce identical hashes
        #[test]
        fn prop_resolved_set_hash_deterministic(resolved in resolved_dependency_set_strategy()) {
            // Create a clone
            let mut cloned = ResolvedDependencySet::new(resolved.python_version.clone());
            for (name, version) in &resolved.packages {
                cloned.add_package(name.clone(), version.clone());
            }

            prop_assert_eq!(resolved.hash(), cloned.hash(),
                          "Identical dependency sets should have identical hashes");
        }

        /// Test that different dependency sets produce different hashes
        #[test]
        fn prop_resolved_set_hash_uniqueness(
            resolved1 in resolved_dependency_set_strategy(),
            resolved2 in resolved_dependency_set_strategy()
        ) {
            // Only test if they're actually different
            let are_different = resolved1.python_version != resolved2.python_version ||
                              resolved1.packages != resolved2.packages;

            if are_different {
                prop_assert_ne!(resolved1.hash(), resolved2.hash(),
                              "Different dependency sets should have different hashes");
            }
        }

        /// Test that virtual environment metadata behaves correctly
        #[test]
        fn prop_venv_metadata_consistency(resolved in resolved_dependency_set_strategy()) {
            let hash = resolved.hash();
            let path = PathBuf::from(format!("/test/path/{}", hash));
            let mut metadata = VenvMetadata::new(hash.clone(), resolved, path.clone());

            prop_assert_eq!(metadata.hash.clone(), hash);
            prop_assert_eq!(metadata.path.clone(), path.clone());
            prop_assert!(metadata.is_orphaned(), "New metadata should be orphaned");

            // Test package reference management
            metadata.add_package_reference("test-package");
            prop_assert!(!metadata.is_orphaned(), "Should not be orphaned after adding reference");
            prop_assert!(metadata.used_by_packages.contains(&"test-package".to_string()));

            metadata.remove_package_reference("test-package");
            prop_assert!(metadata.is_orphaned(), "Should be orphaned after removing reference");
            prop_assert!(!metadata.used_by_packages.contains(&"test-package".to_string()));
        }
    }

    // Traditional unit tests for edge cases

    #[test]
    fn python_version_parsing_edge_cases() {
        // Valid cases
        assert_eq!(
            "3.9".parse::<PythonVersion>().unwrap(),
            PythonVersion::new(3, 9, None)
        );
        assert_eq!(
            "3.9.12".parse::<PythonVersion>().unwrap(),
            PythonVersion::new(3, 9, Some(12))
        );
        assert_eq!(
            "2.7".parse::<PythonVersion>().unwrap(),
            PythonVersion::new(2, 7, None)
        );

        // Invalid cases
        assert!("".parse::<PythonVersion>().is_err());
        assert!("invalid".parse::<PythonVersion>().is_err());
        assert!("3.9.12.1".parse::<PythonVersion>().is_err()); // Too many components
    }

    #[test]
    fn version_spec_creation() {
        let spec = VersionSpec::new(">=1.0.0");
        assert_eq!(spec.spec, ">=1.0.0");
        assert_eq!(spec.to_string(), ">=1.0.0");

        let any_spec = VersionSpec::any();
        assert_eq!(any_spec.spec, "*");
    }

    #[test]
    fn python_dependency_builder() {
        let dep = PythonDependency::new("requests", VersionSpec::new(">=2.25.0"))
            .with_extras(vec!["security".to_string(), "socks".to_string()])
            .optional();

        assert_eq!(dep.name, "requests");
        assert_eq!(dep.version.spec, ">=2.25.0");
        assert!(dep.optional);
        assert_eq!(dep.extras, vec!["security", "socks"]);
    }

    #[test]
    fn dependency_merge_conflicts() {
        let mut deps1 = PythonDependencies::new();
        deps1.add_dependency(PythonDependency::new("numpy", VersionSpec::new("1.20.0")));

        let mut deps2 = PythonDependencies::new();
        deps2.add_dependency(PythonDependency::new("numpy", VersionSpec::new("1.21.0")));

        let result = deps1.merge(&deps2);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Conflicting versions"));
    }

    #[test]
    fn python_version_conflicts() {
        let mut deps1 = PythonDependencies::new();
        deps1.set_python_version(PythonVersion::new(3, 9, None));

        let mut deps2 = PythonDependencies::new();
        deps2.set_python_version(PythonVersion::new(3, 10, None));

        let result = deps1.merge(&deps2);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Conflicting Python versions"));
    }

    #[test]
    fn resolved_dependency_set_operations() {
        let python_version = PythonVersion::new(3, 9, Some(12));
        let mut resolved = ResolvedDependencySet::new(python_version.clone());

        resolved.add_package("numpy", "1.21.0");
        resolved.add_package("requests", "2.25.1");

        assert_eq!(resolved.packages.len(), 2);
        assert_eq!(resolved.packages.get("numpy"), Some(&"1.21.0".to_string()));
        assert_eq!(
            resolved.packages.get("requests"),
            Some(&"2.25.1".to_string())
        );
        assert_eq!(resolved.python_version, python_version);

        // Hash should be consistent
        let hash1 = resolved.hash();
        let hash2 = resolved.hash();
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 16);
    }
}
