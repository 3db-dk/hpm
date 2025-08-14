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
