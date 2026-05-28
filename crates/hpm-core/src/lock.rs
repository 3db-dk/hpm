//! Lock file management for HPM.
//!
//! The lock file (`hpm.lock`) records the exact versions and checksums of all
//! dependencies resolved during installation. This ensures reproducible builds
//! across different machines and time.

use crate::archive_fetcher::cas_install_dir;
use crate::path_util::relative_path_to_forward_slash;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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
    /// Full scoped package path, e.g. `creator/slug`
    pub name: String,
    pub version: String,
}

/// A locked HPM dependency with exact version and verification data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedDependency {
    /// Exact resolved version
    pub version: String,

    /// SHA256 checksum of the package contents
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,

    /// Where the package came from at install time. URL deps record the
    /// resolved download URL; path deps record the local source directory.
    /// Distinct from [`PackageSource`] (which is URL-only and feeds the
    /// fetcher) so the lockfile can faithfully reproduce both flavours.
    pub source: LockedSource,

    /// Transitive dependencies (just names, versions are in the main dependencies map)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
}

/// Origin of a locked dependency. Used only by the lockfile schema —
/// path deps don't go through `ArchiveFetcher`/`PackageSource`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum LockedSource {
    /// Resolved download URL plus the version it was fetched at.
    Url { url: String, version: String },
    /// Local path the user pointed `path = "..."` at.
    Path { path: std::path::PathBuf },
}

impl LockedSource {
    pub fn url(url: impl Into<String>, version: impl Into<String>) -> Self {
        Self::Url {
            url: url.into(),
            version: version.into(),
        }
    }
    pub fn path(path: impl Into<std::path::PathBuf>) -> Self {
        Self::Path { path: path.into() }
    }
    pub fn is_url(&self) -> bool {
        matches!(self, Self::Url { .. })
    }
    pub fn is_path(&self) -> bool {
        matches!(self, Self::Path { .. })
    }
}

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

impl LockMetadata {
    /// Calculate the number of days since the lock file was generated.
    ///
    /// Returns `None` if the timestamp is missing or cannot be parsed.
    pub fn days_since_generated(&self) -> Option<i64> {
        use std::time::SystemTime;

        let generated = self.generated_at.as_ref()?;

        // Parse ISO 8601 timestamp: YYYY-MM-DDTHH:MM:SSZ
        // Extract date parts
        if generated.len() < 10 {
            return None;
        }

        let year: i64 = generated[0..4].parse().ok()?;
        let month: i64 = generated[5..7].parse().ok()?;
        let day: i64 = generated[8..10].parse().ok()?;

        // Calculate days since epoch for the generated date
        let gen_days = ymd_to_days(year, month, day);

        // Get current days since epoch
        let now_secs = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .ok()?
            .as_secs();
        let now_days = (now_secs / 86400) as i64;

        Some(now_days - gen_days)
    }
}

/// Convert year/month/day to days since Unix epoch (Jan 1, 1970)
fn ymd_to_days(year: i64, month: i64, day: i64) -> i64 {
    // Simplified calculation - good enough for date comparison
    let mut days = 0i64;

    // Add days for complete years from 1970
    for y in 1970..year {
        days += if is_leap_year(y) { 366 } else { 365 };
    }

    // Add days for complete months
    let month_days = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    for m in 1..month {
        days += month_days[(m - 1) as usize] as i64;
        if m == 2 && is_leap_year(year) {
            days += 1;
        }
    }

    // Add remaining days
    days += day - 1;

    days
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
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

    /// Lock file claims a package is installed at `expected_dir`, but the
    /// directory doesn't exist. Distinct from `ChecksumMismatch` so callers
    /// can choose to repair (re-fetch) vs. fail loudly.
    #[error(
        "Package {package} is recorded in the lock file but is not installed at {}",
        expected_dir.display()
    )]
    PackageMissing {
        package: String,
        expected_dir: PathBuf,
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

        // Atomic write: stage to <path>.tmp, then rename. A crash mid-write
        // otherwise leaves a truncated `hpm.lock` — every subsequent
        // `install` warns and regenerates, which is visible churn for
        // something the user didn't do wrong.
        let mut tmp_path = path.as_os_str().to_os_string();
        tmp_path.push(".tmp");
        let tmp_path = std::path::PathBuf::from(tmp_path);
        std::fs::write(&tmp_path, content).map_err(|e| LockError::Write {
            path: tmp_path.clone(),
            source: e,
        })?;
        std::fs::rename(&tmp_path, path).map_err(|e| LockError::Write {
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

    /// Verify all checksums in the lock file against installed packages.
    ///
    /// `packages_dir` is the canonical `StorageManager` CAS root (typically
    /// `config.storage.packages_dir`). For each locked dependency that
    /// carries a checksum:
    ///
    /// - if the package directory is missing, returns
    ///   [`LockError::PackageMissing`].
    /// - if the directory is present but its checksum diverges, returns
    ///   [`LockError::ChecksumMismatch`].
    ///
    /// The lookup is by **bare slug** — scoped names like `creator/foo`
    /// reduce to `foo` so the lookup matches `install_into_cas`'s layout.
    pub fn verify_checksums(&self, packages_dir: &Path) -> Result<(), LockError> {
        for (name, dep) in &self.dependencies {
            let Some(expected_checksum) = &dep.checksum else {
                continue;
            };
            let package_dir = cas_install_dir(packages_dir, name, &dep.version);
            if !package_dir.exists() {
                return Err(LockError::PackageMissing {
                    package: name.clone(),
                    expected_dir: package_dir,
                });
            }
            let actual_checksum = compute_directory_checksum(&package_dir)?;
            if &actual_checksum != expected_checksum {
                return Err(LockError::ChecksumMismatch {
                    package: name.clone(),
                    expected: expected_checksum.clone(),
                    actual: actual_checksum,
                });
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
    /// Create a new locked dependency from a URL source.
    ///
    /// # Arguments
    /// * `version` - The resolved version (from package manifest)
    /// * `url` - The download URL
    /// * `checksum` - SHA256 checksum of the extracted package contents
    pub fn from_url(version: String, url: String, checksum: Option<String>) -> Self {
        Self {
            version: version.clone(),
            checksum,
            source: LockedSource::url(url, version),
            dependencies: Vec::new(),
        }
    }

    /// Create a new locked dependency from a local path.
    ///
    /// # Arguments
    /// * `version` - The resolved version (from package manifest)
    /// * `path` - Path to the package directory
    /// * `checksum` - SHA256 checksum of the package contents
    pub fn from_path(
        version: String,
        path: impl Into<std::path::PathBuf>,
        checksum: Option<String>,
    ) -> Self {
        Self {
            version,
            checksum,
            source: LockedSource::path(path),
            dependencies: Vec::new(),
        }
    }

    /// Add a transitive dependency reference.
    pub fn add_dependency(&mut self, name: String) {
        if !self.dependencies.contains(&name) {
            self.dependencies.push(name);
        }
    }

    /// Check if this is a URL dependency.
    pub fn is_url(&self) -> bool {
        self.source.is_url()
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
        // Include relative path in hash for structure integrity. Normalized
        // to `/` so the digest is identical for the same tree on any host.
        let relative_path = relative_path_to_forward_slash(path.strip_prefix(dir).unwrap_or(&path));
        hasher.update(relative_path.as_bytes());

        // Hash file contents
        let contents = std::fs::read(&path).map_err(|e| LockError::Read {
            path: path.clone(),
            source: e,
        })?;
        hasher.update(&contents);
    }

    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect())
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
    let mut year: u32 = 1970;

    loop {
        let days_in_year = if is_leap_year(year as i64) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
    }

    let leap = is_leap_year(year as i64);
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

// is_leap_year is defined above (line 156) with i64 signature

/// Get the current platform identifier
fn current_platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // Keep only time/platform-dependent tests that can't be property-tested
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

    /// Build a fake fetched package directory for verify_checksums tests:
    /// `packages_dir/<safe(name)>-<version>/<file>` with given content.
    /// Returns the per-package directory path.
    fn write_fake_package(
        packages_dir: &Path,
        name: &str,
        version: &str,
        content: &[u8],
    ) -> PathBuf {
        let dir = cas_install_dir(packages_dir, name, version);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("file.txt"), content).unwrap();
        dir
    }

    /// Regression: `verify_checksums` used to compute `<name>@<version>`
    /// and silently pass because the path never existed. Now a missing
    /// install must error explicitly so the next layout drift is loud.
    #[test]
    fn verify_checksums_errors_when_package_missing() {
        let temp_dir = TempDir::new().unwrap();
        let packages_dir = temp_dir.path();

        let mut lock = LockFile::new("root/pkg".to_string(), "1.0.0".to_string());
        lock.add_dependency(
            "creator/foo".to_string(),
            LockedDependency {
                version: "1.0.0".to_string(),
                checksum: Some("0".repeat(64)),
                source: LockedSource::url("https://example.com/foo.zip", "1.0.0"),
                dependencies: Vec::new(),
            },
        );

        match lock.verify_checksums(packages_dir).unwrap_err() {
            LockError::PackageMissing {
                package,
                expected_dir,
            } => {
                assert_eq!(package, "creator/foo");
                // Canonical CAS layout: <packages_dir>/<slug>@<version>/.
                assert!(expected_dir.ends_with("foo@1.0.0"));
            }
            other => panic!("Expected PackageMissing, got {:?}", other),
        }
    }

    /// Verify the matching-checksum path: package present, recorded
    /// checksum matches the on-disk hash, returns Ok.
    #[test]
    fn verify_checksums_passes_when_checksums_match() {
        let temp_dir = TempDir::new().unwrap();
        let packages_dir = temp_dir.path();

        let dir = write_fake_package(packages_dir, "creator/foo", "1.0.0", b"hello");
        let actual = compute_directory_checksum(&dir).unwrap();

        let mut lock = LockFile::new("root/pkg".to_string(), "1.0.0".to_string());
        lock.add_dependency(
            "creator/foo".to_string(),
            LockedDependency {
                version: "1.0.0".to_string(),
                checksum: Some(actual),
                source: LockedSource::url("https://example.com/foo.zip", "1.0.0"),
                dependencies: Vec::new(),
            },
        );

        lock.verify_checksums(packages_dir)
            .expect("matching checksum must verify");
    }

    /// Verify the mismatched-checksum path: package present, hash differs.
    #[test]
    fn verify_checksums_errors_on_mismatch() {
        let temp_dir = TempDir::new().unwrap();
        let packages_dir = temp_dir.path();

        write_fake_package(packages_dir, "creator/foo", "1.0.0", b"hello");

        let mut lock = LockFile::new("root/pkg".to_string(), "1.0.0".to_string());
        lock.add_dependency(
            "creator/foo".to_string(),
            LockedDependency {
                version: "1.0.0".to_string(),
                checksum: Some("0".repeat(64)),
                source: LockedSource::url("https://example.com/foo.zip", "1.0.0"),
                dependencies: Vec::new(),
            },
        );

        match lock.verify_checksums(packages_dir).unwrap_err() {
            LockError::ChecksumMismatch {
                package, expected, ..
            } => {
                assert_eq!(package, "creator/foo");
                assert_eq!(expected, "0".repeat(64));
            }
            other => panic!("Expected ChecksumMismatch, got {:?}", other),
        }
    }

    /// `LockFile::save` stages to `<path>.tmp` and renames. Verify that a
    /// stale `.tmp` from a previous interrupted save doesn't block the
    /// next save (the `.tmp` must be replaced, not refused), and that the
    /// final path reflects the new content rather than the stale staged
    /// bytes.
    #[test]
    fn save_overwrites_stale_tmp_file() {
        let temp_dir = TempDir::new().unwrap();
        let lock_path = temp_dir.path().join("hpm.lock");
        let tmp_path = temp_dir.path().join("hpm.lock.tmp");

        // Simulate an interrupted save: a leftover .tmp with garbage.
        std::fs::write(&tmp_path, b"garbage from a previous crash").unwrap();

        let lock = LockFile::new("acme/widget".to_string(), "1.0.0".to_string());
        lock.save(&lock_path).expect("save should succeed");

        assert!(lock_path.exists(), "lock file must exist after save");
        assert!(!tmp_path.exists(), "tmp file should have been renamed away");

        let loaded = LockFile::load(&lock_path).expect("load should succeed");
        assert_eq!(loaded.package.name, "acme/widget");
    }

    // Property-based tests cover all other functionality with better coverage
    use proptest::prelude::*;

    /// Strategy to generate valid package names
    fn package_name_strategy() -> impl Strategy<Value = String> {
        "[a-z][a-z0-9-]{1,20}".prop_filter("no double dashes", |s| !s.contains("--"))
    }

    /// Strategy to generate valid version strings
    fn version_string_strategy() -> impl Strategy<Value = String> {
        (0u32..100, 0u32..100, 0u32..100)
            .prop_map(|(major, minor, patch)| format!("{}.{}.{}", major, minor, patch))
    }

    /// Strategy to generate Git URLs
    fn git_url_strategy() -> impl Strategy<Value = String> {
        package_name_strategy().prop_map(|name| format!("https://github.com/studio/{}", name))
    }

    /// Strategy to generate checksums
    fn checksum_strategy() -> impl Strategy<Value = Option<String>> {
        prop_oneof![
            Just(None),
            "[a-f0-9]{64}".prop_map(|s| Some(format!("sha256:{}", s))),
        ]
    }

    /// Strategy to generate LockedDependency (Url or Path)
    fn locked_dependency_strategy() -> impl Strategy<Value = LockedDependency> {
        prop_oneof![
            // URL dependency
            (
                version_string_strategy(),
                git_url_strategy(),
                checksum_strategy()
            )
                .prop_map(|(version, url, checksum)| {
                    LockedDependency::from_url(version, url, checksum)
                }),
            // Path dependency
            (
                version_string_strategy(),
                "[a-z/]{1,20}",
                checksum_strategy()
            )
                .prop_map(|(version, path, checksum)| {
                    LockedDependency::from_path(version, format!("../{}", path), checksum)
                }),
        ]
    }

    /// Strategy to generate LockedPythonDependency
    fn locked_python_dependency_strategy() -> impl Strategy<Value = LockedPythonDependency> {
        (
            version_string_strategy(),
            prop::option::of("[a-f0-9]{64}"),
            prop::option::of(Just("https://pypi.org/simple".to_string())),
            prop::option::of(Just("sys_platform == 'linux'".to_string())),
        )
            .prop_map(|(version, checksum, source, markers)| {
                let mut dep = LockedPythonDependency::new(version);
                if let Some(cs) = checksum {
                    dep = dep.with_checksum(cs);
                }
                if let Some(src) = source {
                    dep = dep.with_source(src);
                }
                if let Some(mrk) = markers {
                    dep = dep.with_markers(mrk);
                }
                dep
            })
    }

    /// Strategy to generate LockFile with HPM and Python dependencies
    fn lock_file_strategy() -> impl Strategy<Value = LockFile> {
        (
            package_name_strategy(),
            version_string_strategy(),
            prop::collection::btree_map(
                package_name_strategy(),
                locked_dependency_strategy(),
                0..5,
            ),
            prop::collection::btree_map(
                package_name_strategy(),
                locked_python_dependency_strategy(),
                0..3,
            ),
        )
            .prop_map(|(name, version, deps, py_deps)| {
                let mut lock = LockFile::new(name, version);
                // Clear metadata for consistent comparison (timestamps vary)
                lock.metadata = None;
                for (dep_name, dep) in deps {
                    lock.add_dependency(dep_name, dep);
                }
                for (py_name, py_dep) in py_deps {
                    lock.add_python_dependency(py_name, py_dep);
                }
                lock
            })
    }

    proptest! {
        /// Test LockFile TOML serialization roundtrip (covers creation, add_dependency, to_toml)
        #[test]
        fn prop_lock_file_toml_roundtrip(lock in lock_file_strategy()) {
            let toml_str = lock.to_toml().expect("Should serialize to TOML");
            let parsed: LockFile = toml::from_str(&toml_str).expect("Should parse TOML");

            // Verify is_empty consistency (check before moving values)
            prop_assert_eq!(lock.is_empty(), parsed.is_empty());

            // Verify version field preserved
            prop_assert_eq!(lock.version, parsed.version);

            // Verify package info preserved (use references to avoid moves)
            prop_assert_eq!(&lock.package.name, &parsed.package.name);
            prop_assert_eq!(&lock.package.version, &parsed.package.version);

            // Verify HPM dependencies preserved
            prop_assert_eq!(lock.dependencies.len(), parsed.dependencies.len());
            for (name, dep) in &lock.dependencies {
                let parsed_dep = parsed.dependencies.get(name);
                prop_assert!(parsed_dep.is_some(), "Missing dependency: {}", name);
                let parsed_dep = parsed_dep.unwrap();
                prop_assert_eq!(&dep.version, &parsed_dep.version);
                prop_assert_eq!(&dep.checksum, &parsed_dep.checksum);
                prop_assert_eq!(dep.is_url(), parsed_dep.is_url());
                prop_assert_eq!(dep.is_path(), parsed_dep.is_path());
            }

            // Verify Python dependencies preserved
            prop_assert_eq!(lock.python_dependencies.len(), parsed.python_dependencies.len());
            for (name, dep) in &lock.python_dependencies {
                let parsed_dep = parsed.python_dependencies.get(name);
                prop_assert!(parsed_dep.is_some(), "Missing Python dependency: {}", name);
                let parsed_dep = parsed_dep.unwrap();
                prop_assert_eq!(&dep.version, &parsed_dep.version);
                prop_assert_eq!(&dep.source, &parsed_dep.source);
            }
        }

        /// Test LockFile save/load roundtrip preserves all data
        #[test]
        fn prop_lock_file_save_load_roundtrip(lock in lock_file_strategy()) {
            let temp_dir = TempDir::new().expect("Should create temp dir");
            let lock_path = temp_dir.path().join("hpm.lock");

            lock.save(&lock_path).expect("Should save lock file");
            let loaded = LockFile::load(&lock_path).expect("Should load lock file");

            // Verify is_empty consistency (check before moving values)
            prop_assert_eq!(lock.is_empty(), loaded.is_empty());

            // Verify all data preserved (use references)
            prop_assert_eq!(&lock.package.name, &loaded.package.name);
            prop_assert_eq!(&lock.package.version, &loaded.package.version);
            prop_assert_eq!(lock.dependencies.len(), loaded.dependencies.len());
            prop_assert_eq!(lock.python_dependencies.len(), loaded.python_dependencies.len());

            for (name, dep) in &lock.dependencies {
                let loaded_dep = loaded.dependencies.get(name);
                prop_assert!(loaded_dep.is_some(), "Missing dependency after load: {}", name);
                let loaded_dep = loaded_dep.unwrap();
                prop_assert_eq!(&dep.version, &loaded_dep.version);
                prop_assert_eq!(&dep.checksum, &loaded_dep.checksum);
            }
        }

        /// Test has_changes correctly detects identical lock files (reflexive property)
        #[test]
        fn prop_has_changes_reflexive(lock in lock_file_strategy()) {
            prop_assert!(!lock.has_changes(&lock));
        }

        /// Test has_changes detects added dependencies
        #[test]
        fn prop_has_changes_detects_additions(
            lock in lock_file_strategy(),
            new_dep_name in package_name_strategy(),
            new_dep in locked_dependency_strategy()
        ) {
            let mut modified = lock.clone();
            modified.add_dependency(new_dep_name.clone(), new_dep);

            if !lock.dependencies.contains_key(&new_dep_name) {
                prop_assert!(lock.has_changes(&modified));
            }
        }

        /// Test LockedDependency source type detection
        #[test]
        fn prop_locked_dependency_source_types(dep in locked_dependency_strategy()) {
            // Exactly one of is_url or is_path should be true
            prop_assert!(dep.is_url() != dep.is_path());
        }
    }
}
