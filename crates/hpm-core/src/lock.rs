//! Lock file management for HPM.
//!
//! The lock file (`hpm.lock`) records the exact versions and checksums of all
//! dependencies resolved during installation. This ensures reproducible builds
//! across different machines and time.

use crate::package_source::PackageSource;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

/// The lock file structure, representing resolved dependencies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockFile {
    /// Lock file format version
    pub version: u32,

    /// The root package metadata
    pub package: LockPackageInfo,

    /// Resolved HPM dependencies with exact versions and checksums
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies: BTreeMap<String, LockedDependency>,

    /// Resolved Python dependencies with exact versions
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub python_dependencies: BTreeMap<String, LockedPythonDependency>,

    /// Metadata about when and how this lock file was generated
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<LockMetadata>,
}

/// Information about the root package
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockPackageInfo {
    pub name: String,
    pub version: String,
}

/// A locked HPM dependency with exact version and verification data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedDependency {
    /// Exact resolved version
    pub version: String,

    /// SHA256 checksum of the package contents
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,

    /// Source information
    pub source: PackageSource,

    /// Transitive dependencies (just names, versions are in the main dependencies map)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
}

/// Re-export PackageSource as DependencySource for backward compatibility
pub use crate::package_source::PackageSource as DependencySource;

/// A locked Python dependency
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedPythonDependency {
    /// Exact resolved version
    pub version: String,

    /// SHA256 checksum of the wheel/sdist
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,

    /// Source URL (PyPI or custom index)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,

    /// Platform markers (if any)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markers: Option<String>,
}

/// Metadata about the lock file generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockMetadata {
    /// Timestamp when the lock file was generated (ISO 8601)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated_at: Option<String>,

    /// HPM version that generated this lock file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hpm_version: Option<String>,

    /// Platform the lock file was generated on
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
}

/// Errors that can occur during lock file operations
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    #[error("Failed to read lock file: {path}")]
    Read {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to parse lock file: {path}")]
    Parse {
        path: std::path::PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },

    #[error("Failed to serialize lock file")]
    Serialize(#[from] toml::ser::Error),

    #[error("Failed to write lock file: {path}")]
    Write {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Checksum mismatch for {package}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        package: String,
        expected: String,
        actual: String,
    },

    #[error("Lock file version {version} is not supported (max: {max_supported})")]
    UnsupportedVersion { version: u32, max_supported: u32 },
}

impl LockFile {
    /// Current lock file format version
    pub const CURRENT_VERSION: u32 = 1;

    /// Create a new empty lock file for the given package
    pub fn new(name: String, version: String) -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            package: LockPackageInfo { name, version },
            dependencies: BTreeMap::new(),
            python_dependencies: BTreeMap::new(),
            metadata: Some(LockMetadata {
                generated_at: Some(chrono_now()),
                hpm_version: Some(env!("CARGO_PKG_VERSION").to_string()),
                platform: Some(current_platform()),
            }),
        }
    }

    /// Load a lock file from the given path
    pub fn load(path: &Path) -> Result<Self, LockError> {
        let content = std::fs::read_to_string(path).map_err(|e| LockError::Read {
            path: path.to_path_buf(),
            source: e,
        })?;

        let lock_file: Self = toml::from_str(&content).map_err(|e| LockError::Parse {
            path: path.to_path_buf(),
            source: Box::new(e),
        })?;

        // Check version compatibility
        if lock_file.version > Self::CURRENT_VERSION {
            return Err(LockError::UnsupportedVersion {
                version: lock_file.version,
                max_supported: Self::CURRENT_VERSION,
            });
        }

        Ok(lock_file)
    }

    /// Save the lock file to the given path
    pub fn save(&self, path: &Path) -> Result<(), LockError> {
        let content = self.to_toml()?;
        std::fs::write(path, content).map_err(|e| LockError::Write {
            path: path.to_path_buf(),
            source: e,
        })?;
        Ok(())
    }

    /// Convert to TOML string with proper formatting
    pub fn to_toml(&self) -> Result<String, LockError> {
        let mut output = String::new();

        // Header comment
        output.push_str("# HPM Lock File\n");
        output.push_str("# This file is auto-generated. Do not edit manually.\n\n");

        // Serialize the lock file
        let toml_content = toml::to_string_pretty(self)?;
        output.push_str(&toml_content);

        Ok(output)
    }

    /// Add a locked dependency
    pub fn add_dependency(&mut self, name: String, dependency: LockedDependency) {
        self.dependencies.insert(name, dependency);
    }

    /// Add a locked Python dependency
    pub fn add_python_dependency(&mut self, name: String, dependency: LockedPythonDependency) {
        self.python_dependencies.insert(name, dependency);
    }

    /// Get a locked dependency by name
    pub fn get_dependency(&self, name: &str) -> Option<&LockedDependency> {
        self.dependencies.get(name)
    }

    /// Get a locked Python dependency by name
    pub fn get_python_dependency(&self, name: &str) -> Option<&LockedPythonDependency> {
        self.python_dependencies.get(name)
    }

    /// Check if the lock file has any dependencies
    pub fn is_empty(&self) -> bool {
        self.dependencies.is_empty() && self.python_dependencies.is_empty()
    }

    /// Verify all checksums in the lock file against installed packages
    pub fn verify_checksums(&self, packages_dir: &Path) -> Result<(), LockError> {
        for (name, dep) in &self.dependencies {
            if let Some(expected_checksum) = &dep.checksum {
                let package_dir = packages_dir.join(format!("{}@{}", name, dep.version));
                if package_dir.exists() {
                    let actual_checksum = compute_directory_checksum(&package_dir)?;
                    if &actual_checksum != expected_checksum {
                        return Err(LockError::ChecksumMismatch {
                            package: name.clone(),
                            expected: expected_checksum.clone(),
                            actual: actual_checksum,
                        });
                    }
                }
            }
        }
        Ok(())
    }

    /// Check if dependencies have changed compared to another lock file
    pub fn has_changes(&self, other: &LockFile) -> bool {
        if self.package.name != other.package.name || self.package.version != other.package.version
        {
            return true;
        }

        if self.dependencies.len() != other.dependencies.len()
            || self.python_dependencies.len() != other.python_dependencies.len()
        {
            return true;
        }

        for (name, dep) in &self.dependencies {
            match other.dependencies.get(name) {
                Some(other_dep) if dep.version == other_dep.version => {}
                _ => return true,
            }
        }

        for (name, dep) in &self.python_dependencies {
            match other.python_dependencies.get(name) {
                Some(other_dep) if dep.version == other_dep.version => {}
                _ => return true,
            }
        }

        false
    }
}

impl LockedDependency {
    /// Create a new locked dependency from a Git repository.
    ///
    /// # Arguments
    /// * `version` - The resolved version (from package manifest)
    /// * `url` - The Git repository URL
    /// * `commit` - The full commit hash (40 hex characters)
    /// * `checksum` - SHA256 checksum of the extracted package contents
    pub fn from_git(
        version: String,
        url: String,
        commit: String,
        checksum: Option<String>,
    ) -> Self {
        Self {
            version,
            checksum,
            source: PackageSource::Git { url, commit },
            dependencies: Vec::new(),
        }
    }

    /// Create a new locked dependency from a local path.
    ///
    /// # Arguments
    /// * `version` - The resolved version (from package manifest)
    /// * `path` - Path to the package directory
    /// * `checksum` - SHA256 checksum of the package contents
    pub fn from_path(version: String, path: impl Into<std::path::PathBuf>, checksum: Option<String>) -> Self {
        Self {
            version,
            checksum,
            source: PackageSource::Path { path: path.into() },
            dependencies: Vec::new(),
        }
    }

    /// Add a transitive dependency reference.
    pub fn add_dependency(&mut self, name: String) {
        if !self.dependencies.contains(&name) {
            self.dependencies.push(name);
        }
    }

    /// Check if this is a Git dependency.
    pub fn is_git(&self) -> bool {
        self.source.is_git()
    }

    /// Check if this is a path dependency.
    pub fn is_path(&self) -> bool {
        self.source.is_path()
    }
}

impl LockedPythonDependency {
    /// Create a new locked Python dependency
    pub fn new(version: String) -> Self {
        Self {
            version,
            checksum: None,
            source: None,
            markers: None,
        }
    }

    /// Set the checksum
    pub fn with_checksum(mut self, checksum: String) -> Self {
        self.checksum = Some(checksum);
        self
    }

    /// Set the source URL
    pub fn with_source(mut self, source: String) -> Self {
        self.source = Some(source);
        self
    }

    /// Set platform markers
    pub fn with_markers(mut self, markers: String) -> Self {
        self.markers = Some(markers);
        self
    }
}

/// Compute SHA256 checksum of a directory's contents
fn compute_directory_checksum(dir: &Path) -> Result<String, LockError> {
    let mut hasher = Sha256::new();
    let mut entries: Vec<_> = walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().to_path_buf())
        .collect();

    // Sort for deterministic hashing
    entries.sort();

    for path in entries {
        // Include relative path in hash for structure integrity
        let relative_path = path
            .strip_prefix(dir)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/"); // Normalize path separators
        hasher.update(relative_path.as_bytes());

        // Hash file contents
        let contents = std::fs::read(&path).map_err(|e| LockError::Read {
            path: path.clone(),
            source: e,
        })?;
        hasher.update(&contents);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

/// Get current timestamp in ISO 8601 format
fn chrono_now() -> String {
    // Simple timestamp without external chrono dependency
    use std::time::SystemTime;

    let duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();

    // Format as ISO 8601 (approximate, without full timezone)
    let secs = duration.as_secs();
    let days_since_epoch = secs / 86400;
    let remaining_secs = secs % 86400;
    let hours = remaining_secs / 3600;
    let minutes = (remaining_secs % 3600) / 60;
    let seconds = remaining_secs % 60;

    // Calculate year/month/day from days since epoch (Jan 1, 1970)
    let (year, month, day) = days_to_ymd(days_since_epoch);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

/// Convert days since epoch to year/month/day
fn days_to_ymd(days: u64) -> (u32, u32, u32) {
    // Simplified date calculation
    let mut remaining = days as i64;
    let mut year = 1970;

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
    }

    let leap = is_leap_year(year);
    let days_in_months: [i64; 12] = if leap {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 1;
    for days_in_month in days_in_months {
        if remaining < days_in_month {
            break;
        }
        remaining -= days_in_month;
        month += 1;
    }

    (year, month, (remaining + 1) as u32)
}

fn is_leap_year(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Get the current platform identifier
fn current_platform() -> String {
    format!(
        "{}-{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_lock_file_creation() {
        let lock = LockFile::new("test-package".to_string(), "1.0.0".to_string());

        assert_eq!(lock.version, LockFile::CURRENT_VERSION);
        assert_eq!(lock.package.name, "test-package");
        assert_eq!(lock.package.version, "1.0.0");
        assert!(lock.dependencies.is_empty());
        assert!(lock.python_dependencies.is_empty());
        assert!(lock.metadata.is_some());
    }

    #[test]
    fn test_add_dependency() {
        let mut lock = LockFile::new("my-package".to_string(), "1.0.0".to_string());

        lock.add_dependency(
            "utility-nodes".to_string(),
            LockedDependency::from_git(
                "2.1.0".to_string(),
                "https://github.com/studio/utility-nodes".to_string(),
                "abc123def456789012345678901234567890abcd".to_string(),
                Some("checksum123".to_string()),
            ),
        );

        assert_eq!(lock.dependencies.len(), 1);
        let dep = lock.get_dependency("utility-nodes").unwrap();
        assert_eq!(dep.version, "2.1.0");
        assert_eq!(dep.checksum, Some("checksum123".to_string()));
        assert!(dep.is_git());
    }

    #[test]
    fn test_add_python_dependency() {
        let mut lock = LockFile::new("my-package".to_string(), "1.0.0".to_string());

        lock.add_python_dependency(
            "numpy".to_string(),
            LockedPythonDependency::new("1.24.0".to_string()).with_source("https://pypi.org".to_string()),
        );

        assert_eq!(lock.python_dependencies.len(), 1);
        let dep = lock.get_python_dependency("numpy").unwrap();
        assert_eq!(dep.version, "1.24.0");
        assert_eq!(dep.source, Some("https://pypi.org".to_string()));
    }

    #[test]
    fn test_lock_file_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let lock_path = temp_dir.path().join("hpm.lock");

        let mut lock = LockFile::new("test-package".to_string(), "2.0.0".to_string());
        lock.add_dependency(
            "dep-a".to_string(),
            LockedDependency::from_git(
                "1.0.0".to_string(),
                "https://github.com/studio/dep-a".to_string(),
                "abc123def456789012345678901234567890abcd".to_string(),
                None,
            ),
        );
        lock.add_python_dependency("requests".to_string(), LockedPythonDependency::new("2.28.0".to_string()));

        // Save
        lock.save(&lock_path).unwrap();
        assert!(lock_path.exists());

        // Load
        let loaded = LockFile::load(&lock_path).unwrap();
        assert_eq!(loaded.package.name, "test-package");
        assert_eq!(loaded.package.version, "2.0.0");
        assert_eq!(loaded.dependencies.len(), 1);
        assert_eq!(loaded.python_dependencies.len(), 1);
    }

    #[test]
    fn test_has_changes() {
        let lock1 = LockFile::new("pkg".to_string(), "1.0.0".to_string());
        let lock2 = LockFile::new("pkg".to_string(), "1.0.0".to_string());

        assert!(!lock1.has_changes(&lock2));

        let mut lock3 = LockFile::new("pkg".to_string(), "1.0.0".to_string());
        lock3.add_dependency(
            "new-dep".to_string(),
            LockedDependency::from_git(
                "1.0.0".to_string(),
                "https://github.com/studio/new-dep".to_string(),
                "abc123def456789012345678901234567890abcd".to_string(),
                None,
            ),
        );

        assert!(lock1.has_changes(&lock3));
    }

    #[test]
    fn test_locked_dependency_from_git() {
        let dep = LockedDependency::from_git(
            "1.0.0".to_string(),
            "https://github.com/user/repo.git".to_string(),
            "abc123def456789012345678901234567890abcd".to_string(),
            Some("sha256checksum".to_string()),
        );

        assert_eq!(dep.version, "1.0.0");
        assert_eq!(dep.checksum, Some("sha256checksum".to_string()));
        match dep.source {
            DependencySource::Git { url, commit } => {
                assert_eq!(url, "https://github.com/user/repo.git");
                assert_eq!(commit, "abc123def456789012345678901234567890abcd");
            }
            _ => panic!("Expected Git source"),
        }
    }

    #[test]
    fn test_locked_dependency_from_path() {
        let dep = LockedDependency::from_path(
            "0.1.0".to_string(),
            "../local-package",
            Some("checksum123".to_string()),
        );

        assert_eq!(dep.version, "0.1.0");
        assert_eq!(dep.checksum, Some("checksum123".to_string()));
        match &dep.source {
            PackageSource::Path { path } => {
                assert_eq!(path, &std::path::PathBuf::from("../local-package"));
            }
            _ => panic!("Expected Path source"),
        }
    }

    #[test]
    fn test_to_toml() {
        let mut lock = LockFile::new("my-package".to_string(), "1.0.0".to_string());
        lock.add_dependency(
            "test-dep".to_string(),
            LockedDependency::from_git(
                "2.0.0".to_string(),
                "https://github.com/studio/test-dep".to_string(),
                "abc123def456789012345678901234567890abcd".to_string(),
                Some("sha256abc".to_string()),
            ),
        );

        let toml = lock.to_toml().unwrap();
        assert!(toml.contains("# HPM Lock File"));
        assert!(toml.contains("my-package"));
        assert!(toml.contains("test-dep"));
        assert!(toml.contains("sha256abc"));
    }

    #[test]
    fn test_is_empty() {
        let lock = LockFile::new("pkg".to_string(), "1.0.0".to_string());
        assert!(lock.is_empty());

        let mut lock_with_deps = LockFile::new("pkg".to_string(), "1.0.0".to_string());
        lock_with_deps.add_dependency(
            "dep".to_string(),
            LockedDependency::from_git(
                "1.0.0".to_string(),
                "https://github.com/studio/dep".to_string(),
                "abc123def456789012345678901234567890abcd".to_string(),
                None,
            ),
        );
        assert!(!lock_with_deps.is_empty());
    }

    #[test]
    fn test_chrono_now_format() {
        let timestamp = chrono_now();
        // Should match ISO 8601 format: YYYY-MM-DDTHH:MM:SSZ
        assert!(timestamp.len() == 20);
        assert!(timestamp.contains('T'));
        assert!(timestamp.ends_with('Z'));
    }

    #[test]
    fn test_current_platform() {
        let platform = current_platform();
        // Should contain OS and arch
        assert!(platform.contains('-'));
        assert!(!platform.is_empty());
    }
}
