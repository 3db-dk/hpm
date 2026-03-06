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
use anyhow::Result;
use console::style;
use std::path::PathBuf;
use tracing::info;

/// Run security audit on a package and its dependencies
pub async fn audit_packages(manifest_path: Option<PathBuf>) -> Result<()> {
    info!("Running security audit");

    let manifest_path = determine_manifest_path(manifest_path)?;
    let manifest = load_manifest(&manifest_path)?;
    let project_dir = manifest_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Could not determine project directory"))?;

    println!("\n{}", style("HPM Security Audit").bold().cyan());
    println!("{}\n", "=".repeat(40));

    let mut warnings: Vec<String> = Vec::new();
    let mut passed: Vec<&str> = Vec::new();

    // Check 1: HTTP URLs
    if let Some(deps) = &manifest.dependencies {
        let http_deps: Vec<_> = deps
            .iter()
            .filter_map(|(name, spec)| {
                if let hpm_package::DependencySpec::Url { url, .. } = spec {
                    if url.starts_with("http://") && !url.starts_with("https://") {
                        return Some(name.clone());
                    }
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
    } else {
        passed.push("No dependencies to check");
    }

    // Check 2: Lock file exists
    let lock_path = project_dir.join("hpm.lock");
    if lock_path.exists() {
        passed.push("Lock file exists (hpm.lock)");

        // Load lock file for further checks
        match hpm_core::LockFile::load(&lock_path) {
            Ok(lock) => {
                // Check 3: Lock file staleness
                if let Some(ref metadata) = lock.metadata {
                    if let Some(days) = metadata.days_since_generated() {
                        if days > 90 {
                            warnings.push(format!("Lock file is {} days old", days));
                        } else {
                            passed.push("Lock file is recent");
                        }
                    }
                }

                // Check 4: Checksum verification
                let config = hpm_config::Config::load().unwrap_or_default();
                match lock.verify_checksums(&config.storage.packages_dir) {
                    Ok(()) => passed.push("Package checksums verified"),
                    Err(e) => warnings.push(format!("Checksum verification failed: {}", e)),
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
        println!("  {} {}", style("PASS").green().bold(), msg);
    }
    for msg in &warnings {
        println!("  {} {}", style("WARN").yellow().bold(), msg);
    }

    println!();
    if warnings.is_empty() {
        println!("{}", style("No security issues found.").green().bold());
    } else {
        println!(
            "{}",
            style(format!("Found {} warning(s).", warnings.len()))
                .yellow()
                .bold()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::test_fixtures::{write_test_manifest, TestManifestOpts};
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
        let result = audit_packages(Some(manifest_path)).await;

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
        let result = audit_packages(Some(manifest_path)).await;

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
        let result = audit_packages(Some(manifest_path)).await;

        assert!(result.is_ok());
    }
}
