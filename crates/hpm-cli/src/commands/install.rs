use anyhow::{Context, Result};
use hpm_package::PackageManifest;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Install dependencies from hpm.toml manifest
///
/// This function reads the hpm.toml file from the specified path (or current directory),
/// resolves all dependencies (both HPM and Python), and ensures they are installed
/// and configured in the .hpm directory structure.
pub async fn install_dependencies(manifest_path: Option<PathBuf>) -> Result<()> {
    info!("Starting dependency installation");

    // Determine manifest path
    let manifest_path = determine_manifest_path(manifest_path)?;
    info!("Using manifest: {}", manifest_path.display());

    // Load and validate manifest
    let manifest = load_manifest(&manifest_path)
        .with_context(|| format!("Failed to load manifest from {}", manifest_path.display()))?;

    info!(
        "Installing dependencies for package: {} v{}",
        manifest.package.name, manifest.package.version
    );

    // Create .hpm directory structure
    let project_dir = manifest_path
        .parent()
        .context("Manifest file has no parent directory")?;
    let hpm_dir = project_dir.join(".hpm");

    setup_hpm_directory(&hpm_dir)
        .await
        .context("Failed to setup .hpm directory")?;

    // Install HPM dependencies
    if let Some(dependencies) = &manifest.dependencies {
        if !dependencies.is_empty() {
            info!("Installing {} HPM dependencies", dependencies.len());
            install_hpm_dependencies(dependencies, &hpm_dir)
                .await
                .context("Failed to install HPM dependencies")?;
        } else {
            info!("No HPM dependencies to install");
        }
    } else {
        info!("No HPM dependencies specified");
    }

    // Install Python dependencies
    if let Some(python_deps) = &manifest.python_dependencies {
        if !python_deps.is_empty() {
            info!("Installing {} Python dependencies", python_deps.len());
            install_python_dependencies(&manifest, &hpm_dir)
                .await
                .context("Failed to install Python dependencies")?;
        } else {
            info!("No Python dependencies to install");
        }
    } else {
        info!("No Python dependencies specified");
    }

    // Generate or update lock file
    generate_lock_file(&manifest, project_dir)
        .await
        .context("Failed to generate lock file")?;

    info!("Dependency installation completed successfully");
    Ok(())
}

/// Determine the path to the hpm.toml manifest file
fn determine_manifest_path(provided_path: Option<PathBuf>) -> Result<PathBuf> {
    match provided_path {
        Some(path) => {
            if path.is_file() {
                Ok(path)
            } else if path.is_dir() {
                let manifest_in_dir = path.join("hpm.toml");
                if manifest_in_dir.exists() {
                    Ok(manifest_in_dir)
                } else {
                    anyhow::bail!("No hpm.toml found in directory: {}", path.display());
                }
            } else {
                anyhow::bail!(
                    "Provided path does not exist or is not accessible: {}",
                    path.display()
                );
            }
        }
        None => {
            let current_dir = std::env::current_dir().context("Failed to get current directory")?;
            let manifest_path = current_dir.join("hpm.toml");

            if manifest_path.exists() {
                Ok(manifest_path)
            } else {
                anyhow::bail!(
                    "No hpm.toml found in current directory: {}. Use --manifest to specify a path.",
                    current_dir.display()
                );
            }
        }
    }
}

/// Load and parse the package manifest
fn load_manifest(manifest_path: &Path) -> Result<PackageManifest> {
    let content = std::fs::read_to_string(manifest_path)
        .with_context(|| format!("Failed to read manifest file: {}", manifest_path.display()))?;

    let manifest: PackageManifest = toml::from_str(&content)
        .with_context(|| format!("Failed to parse manifest file: {}", manifest_path.display()))?;

    // Validate manifest
    manifest
        .validate()
        .map_err(|e| anyhow::anyhow!("Manifest validation failed: {}", e))
        .with_context(|| format!("Manifest validation failed: {}", manifest_path.display()))?;

    Ok(manifest)
}

/// Setup the .hpm directory structure
async fn setup_hpm_directory(hpm_dir: &Path) -> Result<()> {
    info!("Setting up .hpm directory: {}", hpm_dir.display());

    // Create main .hpm directory
    tokio::fs::create_dir_all(hpm_dir)
        .await
        .with_context(|| format!("Failed to create .hpm directory: {}", hpm_dir.display()))?;

    // Create subdirectories
    let packages_dir = hpm_dir.join("packages");
    tokio::fs::create_dir_all(&packages_dir)
        .await
        .with_context(|| {
            format!(
                "Failed to create packages directory: {}",
                packages_dir.display()
            )
        })?;

    info!(".hpm directory structure created");
    Ok(())
}

/// Install HPM package dependencies
async fn install_hpm_dependencies(
    dependencies: &std::collections::HashMap<String, hpm_package::DependencySpec>,
    _hpm_dir: &Path,
) -> Result<()> {
    info!("Installing HPM dependencies...");

    for (name, spec) in dependencies {
        info!("Processing dependency: {}", name);

        match spec {
            hpm_package::DependencySpec::Simple(version) => {
                info!("  Version spec: {}", version);
            }
            hpm_package::DependencySpec::Detailed {
                version,
                git,
                tag,
                branch,
                optional,
                registry,
            } => {
                if let Some(v) = version {
                    info!("  Version: {}", v);
                }
                if let Some(g) = git {
                    info!("  Git: {}", g);
                }
                if let Some(t) = tag {
                    info!("  Tag: {}", t);
                }
                if let Some(b) = branch {
                    info!("  Branch: {}", b);
                }
                if let Some(r) = registry {
                    info!("  Registry: {}", r);
                }
                if optional.unwrap_or(false) {
                    info!("  Optional dependency");
                }
            }
        }

        // TODO: Implement actual package installation logic
        // This would involve:
        // 1. Resolve version constraints
        // 2. Download package from registry
        // 3. Extract to global storage (~/.hpm/packages/)
        // 4. Create symlink/reference in project .hpm/packages/
        warn!("HPM package installation not yet implemented for: {}", name);
    }

    Ok(())
}

/// Install Python dependencies using the hpm-python crate
async fn install_python_dependencies(manifest: &PackageManifest, _hpm_dir: &Path) -> Result<()> {
    info!("Installing Python dependencies...");

    // Initialize Python dependency management
    hpm_python::initialize()
        .await
        .context("Failed to initialize Python dependency management")?;

    // Collect Python dependencies from the manifest
    let python_deps = hpm_python::collect_python_dependencies(std::slice::from_ref(manifest))
        .await
        .context("Failed to collect Python dependencies")?;

    if python_deps.dependencies.is_empty() {
        info!("No Python dependencies to process");
        return Ok(());
    }

    info!(
        "Found {} Python dependencies",
        python_deps.dependencies.len()
    );

    // Resolve dependencies to exact versions
    let resolved_deps = hpm_python::resolve_dependencies(&python_deps)
        .await
        .context("Failed to resolve Python dependencies")?;

    info!("Resolved {} Python packages", resolved_deps.packages.len());

    // Ensure virtual environment exists
    let venv_manager = hpm_python::VenvManager::new();
    let venv_path = venv_manager
        .ensure_virtual_environment(&resolved_deps)
        .await
        .context("Failed to create virtual environment")?;

    info!("Python virtual environment ready: {}", venv_path.display());

    // Generate Houdini integration files
    generate_houdini_integration(manifest, &venv_path)
        .await
        .context("Failed to generate Houdini integration")?;

    Ok(())
}

/// Generate Houdini package.json integration file
async fn generate_houdini_integration(manifest: &PackageManifest, venv_path: &Path) -> Result<()> {
    info!("Generating Houdini integration files");

    // Create base Houdini package configuration
    let mut houdini_pkg = manifest.generate_houdini_package();

    // Add Python virtual environment to PYTHONPATH
    if let Some(ref mut env) = houdini_pkg.env {
        let python_site_packages = venv_path.join("lib").join("python").join("site-packages");

        let mut python_env = std::collections::HashMap::new();
        python_env.insert(
            "PYTHONPATH".to_string(),
            hpm_package::HoudiniEnvValue::Simple(format!(
                "{}:$PYTHONPATH",
                python_site_packages.display()
            )),
        );
        env.push(python_env);
    }

    // Add HPM metadata
    let mut hpm_metadata = std::collections::HashMap::new();
    hpm_metadata.insert(
        "HPM_PACKAGE_NAME".to_string(),
        hpm_package::HoudiniEnvValue::Simple(manifest.package.name.clone()),
    );
    hpm_metadata.insert(
        "HPM_PACKAGE_VERSION".to_string(),
        hpm_package::HoudiniEnvValue::Simple(manifest.package.version.clone()),
    );

    if let Some(ref mut env) = houdini_pkg.env {
        env.push(hpm_metadata);
    } else {
        houdini_pkg.env = Some(vec![hpm_metadata]);
    }

    info!("Houdini integration configuration generated");
    Ok(())
}

/// Generate or update the hpm.lock file
async fn generate_lock_file(manifest: &PackageManifest, project_dir: &Path) -> Result<()> {
    info!("Generating lock file");

    let lock_file_path = project_dir.join("hpm.lock");

    // Create a basic lock file structure
    // TODO: Implement proper lock file with resolved versions and checksums
    let lock_content = format!(
        "# HPM Lock File\n# Generated by hpm install\n\n[package]\nname = \"{}\"\nversion = \"{}\"\n",
        manifest.package.name,
        manifest.package.version
    );

    tokio::fs::write(&lock_file_path, lock_content)
        .await
        .with_context(|| format!("Failed to write lock file: {}", lock_file_path.display()))?;

    info!("Lock file generated: {}", lock_file_path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use tempfile::TempDir;

    /// Create a test hpm.toml file with basic package info and dependencies
    fn create_test_manifest(path: &Path, include_python_deps: bool) -> Result<()> {
        let mut manifest_content = String::from(
            r#"[package]
name = "test-package"
version = "1.0.0"
description = "A test package for HPM install"
authors = ["Test Author <test@example.com>"]
license = "MIT"

[houdini]
min_version = "20.0"

[dependencies]
utility-nodes = "^2.1.0"
material-library = { version = "1.5", optional = true }
"#,
        );

        if include_python_deps {
            manifest_content.push_str(
                r#"
[python_dependencies]
numpy = ">=1.20.0"
requests = { version = ">=2.25.0", extras = ["security"] }
matplotlib = { version = "^3.5.0", optional = true }
"#,
            );
        }

        std::fs::write(path.join("hpm.toml"), manifest_content)?;
        Ok(())
    }

    #[test]
    fn test_determine_manifest_path_current_directory() {
        let temp_dir = TempDir::new().unwrap();
        let original_dir = env::current_dir().unwrap();

        // Create test manifest
        create_test_manifest(temp_dir.path(), false).unwrap();

        // Change to temp directory
        env::set_current_dir(temp_dir.path()).unwrap();

        let result = determine_manifest_path(None);

        // Restore directory first
        env::set_current_dir(original_dir).unwrap();

        assert!(result.is_ok());
        let manifest_path = result.unwrap();
        assert!(manifest_path.ends_with("hpm.toml"));
    }

    #[test]
    fn test_determine_manifest_path_explicit_file() {
        let temp_dir = TempDir::new().unwrap();
        create_test_manifest(temp_dir.path(), false).unwrap();

        let manifest_path = temp_dir.path().join("hpm.toml");
        let result = determine_manifest_path(Some(manifest_path.clone()));

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), manifest_path);
    }

    #[test]
    fn test_determine_manifest_path_explicit_directory() {
        let temp_dir = TempDir::new().unwrap();
        create_test_manifest(temp_dir.path(), false).unwrap();

        let result = determine_manifest_path(Some(temp_dir.path().to_path_buf()));

        assert!(result.is_ok());
        assert!(result.unwrap().ends_with("hpm.toml"));
    }

    #[test]
    fn test_determine_manifest_path_no_manifest() {
        let temp_dir = TempDir::new().unwrap();
        let original_dir = env::current_dir().unwrap();

        // Change to temp directory without creating a manifest
        env::set_current_dir(temp_dir.path()).unwrap();

        let result = determine_manifest_path(None);

        // Restore directory first
        env::set_current_dir(original_dir).unwrap();

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("No hpm.toml found"));
    }

    #[test]
    fn test_load_manifest_valid() {
        let temp_dir = TempDir::new().unwrap();
        create_test_manifest(temp_dir.path(), true).unwrap();

        let manifest_path = temp_dir.path().join("hpm.toml");
        let result = load_manifest(&manifest_path);

        assert!(result.is_ok());
        let manifest = result.unwrap();
        assert_eq!(manifest.package.name, "test-package");
        assert_eq!(manifest.package.version, "1.0.0");
        assert!(manifest.dependencies.is_some());
        assert!(manifest.python_dependencies.is_some());
        assert_eq!(manifest.dependencies.as_ref().unwrap().len(), 2);
        assert_eq!(manifest.python_dependencies.as_ref().unwrap().len(), 3);
    }

    #[test]
    fn test_load_manifest_invalid_toml() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_path = temp_dir.path().join("hpm.toml");

        // Create invalid TOML
        std::fs::write(&manifest_path, "invalid toml content [[[").unwrap();

        let result = load_manifest(&manifest_path);
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("Failed to parse manifest file"));
    }

    #[test]
    fn test_load_manifest_validation_failure() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_path = temp_dir.path().join("hpm.toml");

        // Create manifest with invalid package name
        let invalid_content = r#"[package]
name = ""
version = "1.0.0"
"#;
        std::fs::write(&manifest_path, invalid_content).unwrap();

        let result = load_manifest(&manifest_path);
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("Manifest validation failed"));
    }

    #[tokio::test]
    async fn test_setup_hpm_directory() {
        let temp_dir = TempDir::new().unwrap();
        let hpm_dir = temp_dir.path().join(".hpm");

        let result = setup_hpm_directory(&hpm_dir).await;

        assert!(result.is_ok());
        assert!(hpm_dir.exists());
        assert!(hpm_dir.is_dir());
        assert!(hpm_dir.join("packages").exists());
        assert!(hpm_dir.join("packages").is_dir());
    }

    #[tokio::test]
    async fn test_install_dependencies_basic_manifest() {
        let temp_dir = TempDir::new().unwrap();
        let original_dir = env::current_dir().unwrap();

        // Create test manifest without Python dependencies to avoid Python system requirements
        let manifest_content = r#"[package]
name = "test-install-package"
version = "1.0.0"
description = "Test package for install command"
authors = ["Test Author <test@example.com>"]
license = "MIT"

[houdini]
min_version = "20.0"

[dependencies]
utility-nodes = "^2.1.0"
"#;
        std::fs::write(temp_dir.path().join("hpm.toml"), manifest_content).unwrap();

        env::set_current_dir(temp_dir.path()).unwrap();

        // Install dependencies (should not fail even though we don't have real packages)
        let result = install_dependencies(None).await;

        env::set_current_dir(original_dir).unwrap();

        // The function should complete successfully even though actual package installation is not implemented
        // This tests the manifest parsing and directory setup logic
        assert!(result.is_ok());

        // Verify directory structure was created
        let hpm_dir = temp_dir.path().join(".hpm");
        assert!(hpm_dir.exists());
        assert!(hpm_dir.join("packages").exists());

        // Verify lock file was created
        let lock_file = temp_dir.path().join("hpm.lock");
        assert!(lock_file.exists());

        let lock_content = std::fs::read_to_string(lock_file).unwrap();
        assert!(lock_content.contains("test-install-package"));
        assert!(lock_content.contains("1.0.0"));
    }

    #[tokio::test]
    async fn test_install_dependencies_explicit_manifest_path() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_path = temp_dir.path().join("custom-manifest.toml");

        let manifest_content = r#"[package]
name = "custom-path-package"
version = "2.0.0"
description = "Test custom manifest path"

[dependencies]
test-dep = "1.0.0"
"#;
        std::fs::write(&manifest_path, manifest_content).unwrap();

        let result = install_dependencies(Some(manifest_path)).await;

        assert!(result.is_ok());

        // Verify directory structure was created relative to manifest location
        let hpm_dir = temp_dir.path().join(".hpm");
        assert!(hpm_dir.exists());

        // Verify lock file was created in the same directory as the manifest
        let lock_file = temp_dir.path().join("hpm.lock");
        assert!(lock_file.exists());
    }

    #[test]
    fn test_install_dependencies_nonexistent_manifest() {
        let temp_dir = TempDir::new().unwrap();
        let nonexistent_path = temp_dir.path().join("nonexistent.toml");

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(install_dependencies(Some(nonexistent_path)));

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("does not exist"));
    }
}
