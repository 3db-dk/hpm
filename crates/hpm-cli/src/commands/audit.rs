//! HPM Security Audit Command
//!
//! This module implements the `hpm audit` command for running security audits
//! on HPM packages and their dependencies.
//!
//! ## Security Checks Performed
//!
//! 1. **HTTP URLs**: Warns about insecure HTTP Git URLs (should use HTTPS)
//! 2. **Lock File Presence**: Verifies hpm.lock exists for reproducible builds
//! 3. **Lock File Staleness**: Warns if lock file is older than 90 days
//! 4. **Checksum Verification**: Validates package checksums against lock file
//! 5. **Installed Files**: Compares each locked package's files with the
//!    record its install wrote (missing, truncated or changed files)
//!
//! ## Usage
//!
//! ```bash
//! # Run audit on current project
//! hpm audit
//!
//! # Run audit on specific project
//! hpm audit --manifest /path/to/project/
//! ```

use super::manifest_utils::{determine_manifest_path, load_manifest};
use crate::console::Console;
use anyhow::Result;
use console::style;
use hpm_config::Config;
use std::path::PathBuf;
use tracing::info;

/// Run security audit on a package and its dependencies
pub async fn audit_packages(
    config: &Config,
    manifest_path: Option<PathBuf>,
    console: &mut Console,
) -> Result<()> {
    info!("Running security audit");

    let manifest_path = determine_manifest_path(manifest_path)?;
    let manifest = load_manifest(&manifest_path)?;
    let project_dir = manifest_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Could not determine project directory"))?;

    console.stdout(format!("\n{}", style("HPM Security Audit").bold().cyan()));
    console.stdout(format!("{}\n", "=".repeat(40)));

    let mut warnings: Vec<String> = Vec::new();
    let mut passed: Vec<&str> = Vec::new();

    // Check 1: HTTP URLs
    if manifest.dependencies.is_empty() {
        passed.push("No dependencies to check");
    } else {
        let http_deps: Vec<_> = manifest
            .dependencies
            .iter()
            .filter_map(|(name, spec)| {
                if let hpm_package::DependencySpec::Url { url, .. } = spec
                    && url.starts_with("http://")
                    && !url.starts_with("https://")
                {
                    return Some(name.clone());
                }
                None
            })
            .collect();

        if http_deps.is_empty() {
            passed.push("All URLs use HTTPS");
        } else {
            for name in http_deps {
                warnings.push(format!("{}: Uses insecure HTTP URL", name));
            }
        }
    }

    // Check 2: Lock file exists
    let lock_path = project_dir.join("hpm.lock");
    if lock_path.exists() {
        passed.push("Lock file exists (hpm.lock)");

        // Load lock file for further checks
        match hpm_core::LockFile::load(&lock_path) {
            Ok(lock) => {
                // Check 3: Lock file staleness
                if let Some(ref metadata) = lock.metadata
                    && let Some(days) = metadata.days_since_generated()
                {
                    if days > 90 {
                        warnings.push(format!("Lock file is {} days old", days));
                    } else {
                        passed.push("Lock file is recent");
                    }
                }

                // Check 4: Checksum verification
                match lock.verify_checksums(&config.storage.packages_dir) {
                    Ok(()) => passed.push("Package checksums verified"),
                    Err(e) => warnings.push(format!("Checksum verification failed: {e}")),
                }

                // Check 5: Installed files against their install records
                let (checked, unrecorded, problems) =
                    audit_installed_files(&lock, &config.storage.packages_dir);
                if problems.is_empty() && checked > 0 {
                    passed.push("Installed files match their install records");
                }
                warnings.extend(problems);
                if unrecorded > 0 {
                    info!(
                        "{unrecorded} installed package(s) predate install records and were not checked"
                    );
                }
            }
            Err(e) => {
                warnings.push(format!("Failed to load lock file: {}", e));
            }
        }
    } else {
        warnings.push("No lock file found - run 'hpm install' for reproducible builds".into());
    }

    // Print results
    for msg in &passed {
        console.stdout(format!("  {} {}", style("PASS").green().bold(), msg));
    }
    for msg in &warnings {
        console.stdout(format!("  {} {}", style("WARN").yellow().bold(), msg));
    }

    console.stdout("");
    if warnings.is_empty() {
        console.stdout(
            style("No security issues found.")
                .green()
                .bold()
                .to_string(),
        );
    } else {
        console.stdout(
            style(format!("Found {} warning(s).", warnings.len()))
                .yellow()
                .bold()
                .to_string(),
        );
    }

    Ok(())
}

/// Compare every locked package's installed files with its install record.
/// Returns `(packages_checked, packages_unrecorded, warnings)`; a package
/// that is not installed is check 4's to report, not this one's.
fn audit_installed_files(
    lock: &hpm_core::LockFile,
    packages_dir: &std::path::Path,
) -> (usize, usize, Vec<String>) {
    use hpm_core::install_manifest::{ProblemKind, Verification, verify};

    let mut checked = 0;
    let mut unrecorded = 0;
    let mut warnings = Vec::new();
    for (name, dep) in &lock.dependencies {
        let dir = hpm_core::storage::layout::cas_dir(packages_dir, name, &dep.version);
        if !dir.is_dir() {
            continue;
        }
        match verify(&dir) {
            Ok(Verification::Unrecorded) => unrecorded += 1,
            Ok(Verification::Checked(report)) => {
                checked += 1;
                for problem in report.problems {
                    let what = match problem.kind {
                        ProblemKind::Missing => "is missing".to_string(),
                        ProblemKind::Truncated { expected, actual } => {
                            format!("is truncated ({actual} of {expected} bytes)")
                        }
                        ProblemKind::Changed => "differs from what was installed".to_string(),
                        ProblemKind::Unreadable { error } => format!("cannot be read ({error})"),
                    };
                    warnings.push(format!("{name}@{}: {} {what}", dep.version, problem.path));
                }
            }
            Err(e) => warnings.push(format!(
                "{name}@{}: install record could not be read: {e}",
                dep.version
            )),
        }
    }
    (checked, unrecorded, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_fixtures::{TestManifestOpts, write_test_manifest};
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_audit_with_https_dependencies() {
        let temp_dir = TempDir::new().unwrap();
        write_test_manifest(
            temp_dir.path(),
            TestManifestOpts {
                include_deps: true,
                ..Default::default()
            },
        )
        .unwrap();

        let lock = hpm_core::LockFile::new("test-package".to_string(), "1.0.0".to_string());
        lock.save(&temp_dir.path().join("hpm.lock")).unwrap();

        let manifest_path = temp_dir.path().join("hpm.toml");
        let config = Config::default();
        let result = audit_packages(&config, Some(manifest_path), &mut Console::new()).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_audit_with_http_dependencies() {
        let temp_dir = TempDir::new().unwrap();
        write_test_manifest(
            temp_dir.path(),
            TestManifestOpts {
                include_deps: true,
                use_http: true,
                ..Default::default()
            },
        )
        .unwrap();

        let lock = hpm_core::LockFile::new("test-package".to_string(), "1.0.0".to_string());
        lock.save(&temp_dir.path().join("hpm.lock")).unwrap();

        let manifest_path = temp_dir.path().join("hpm.toml");
        let config = Config::default();
        let result = audit_packages(&config, Some(manifest_path), &mut Console::new()).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_audit_no_lock_file() {
        let temp_dir = TempDir::new().unwrap();
        write_test_manifest(
            temp_dir.path(),
            TestManifestOpts {
                include_deps: true,
                ..Default::default()
            },
        )
        .unwrap();

        let manifest_path = temp_dir.path().join("hpm.toml");
        let config = Config::default();
        let result = audit_packages(&config, Some(manifest_path), &mut Console::new()).await;

        assert!(result.is_ok());
    }

    /// Check 5 names the damaged file of a locked package, ignores what a
    /// session wrote into it, and leaves a package without a record alone.
    #[test]
    fn installed_files_check_reports_only_damage() {
        let temp_dir = TempDir::new().unwrap();
        let packages = temp_dir.path().join("packages");
        let dir = packages.join("tool@1.0.0");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("tt_prepare.exe"), b"MZ-full-binary").unwrap();
        let record = hpm_core::install_manifest::record_tree(&dir).unwrap();
        hpm_core::install_manifest::write(&dir, &record).unwrap();
        std::fs::create_dir_all(packages.join("old@1.0.0")).unwrap();

        let mut lock = hpm_core::LockFile::new("p".into(), "1.0.0".into());
        for name in ["studio/tool", "studio/old"] {
            lock.add_dependency(
                name.into(),
                hpm_core::lock::LockedDependency {
                    version: "1.0.0".into(),
                    checksum: None,
                    source: hpm_core::lock::LockedSource::url(
                        "https://example.invalid/a.zip",
                        "1.0.0",
                    ),
                },
            );
        }

        std::fs::create_dir_all(dir.join("__pycache__")).unwrap();
        std::fs::write(dir.join("__pycache__/a.pyc"), b"x").unwrap();
        assert_eq!(audit_installed_files(&lock, &packages), (1, 1, vec![]));

        std::fs::write(dir.join("tt_prepare.exe"), b"MZ").unwrap();
        let (checked, unrecorded, warnings) = audit_installed_files(&lock, &packages);
        assert_eq!((checked, unrecorded), (1, 1));
        assert_eq!(
            warnings,
            ["studio/tool@1.0.0: tt_prepare.exe is truncated (2 of 14 bytes)"]
        );
    }
}
