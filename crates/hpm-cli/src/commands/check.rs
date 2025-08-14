use anyhow::{Context, Result};
use hpm_package::PackageManifest;
use std::fs;
use std::path::Path;
use tracing::{error, info, warn};

#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub info_messages: Vec<String>,
}

impl ValidationResult {
    pub fn new() -> Self {
        Self {
            is_valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
            info_messages: Vec::new(),
        }
    }

    pub fn add_error(&mut self, message: String) {
        self.errors.push(message);
        self.is_valid = false;
    }

    pub fn add_warning(&mut self, message: String) {
        self.warnings.push(message);
    }

    pub fn add_info(&mut self, message: String) {
        self.info_messages.push(message);
    }

    #[allow(dead_code)]
    pub fn merge(&mut self, other: ValidationResult) {
        self.errors.extend(other.errors);
        self.warnings.extend(other.warnings);
        self.info_messages.extend(other.info_messages);
        if !other.is_valid {
            self.is_valid = false;
        }
    }
}

pub async fn check_package() -> Result<()> {
    let current_dir = std::env::current_dir().context("Failed to get current directory")?;
    let mut result = ValidationResult::new();

    info!("Checking HPM package configuration...");

    // Check for hpm.toml existence and parse it
    let manifest_path = current_dir.join("hpm.toml");
    let manifest = match validate_manifest_file(&manifest_path, &mut result).await {
        Ok(Some(manifest)) => manifest,
        Ok(None) => return display_results(result),
        Err(e) => {
            result.add_error(format!("Failed to validate manifest: {}", e));
            return display_results(result);
        }
    };

    // Validate manifest structure
    validate_manifest_content(&manifest, &mut result);

    // Validate project structure
    validate_project_structure(&current_dir, &manifest, &mut result).await;

    // Validate Houdini package compatibility
    validate_houdini_compatibility(&manifest, &mut result);

    // Check for best practices
    validate_best_practices(&current_dir, &manifest, &mut result).await;

    display_results(result)
}

async fn validate_manifest_file(
    manifest_path: &Path,
    result: &mut ValidationResult,
) -> Result<Option<PackageManifest>> {
    if !manifest_path.exists() {
        result.add_error("No hpm.toml found in current directory".to_string());
        return Ok(None);
    }

    result.add_info("[OK] hpm.toml found".to_string());

    let content = fs::read_to_string(manifest_path).context("Failed to read hpm.toml")?;

    match toml::from_str::<PackageManifest>(&content) {
        Ok(manifest) => {
            result.add_info("[OK] hpm.toml has valid TOML syntax".to_string());
            Ok(Some(manifest))
        }
        Err(e) => {
            result.add_error(format!("Invalid TOML syntax in hpm.toml: {}", e));
            Ok(None)
        }
    }
}

fn validate_manifest_content(manifest: &PackageManifest, result: &mut ValidationResult) {
    match manifest.validate() {
        Ok(_) => {
            result.add_info("[OK] Package manifest validation passed".to_string());
        }
        Err(e) => {
            result.add_error(format!("Manifest validation failed: {}", e));
        }
    }

    // Additional validations beyond basic manifest validation
    if manifest.package.description.is_none() {
        result.add_warning(
            "Package description is missing - consider adding one for better discoverability"
                .to_string(),
        );
    }

    if manifest.package.authors.is_none() || manifest.package.authors.as_ref().unwrap().is_empty() {
        result.add_warning(
            "Package authors are missing - consider adding author information".to_string(),
        );
    }

    if manifest.package.keywords.is_none() || manifest.package.keywords.as_ref().unwrap().is_empty()
    {
        result.add_warning(
            "Package keywords are missing - consider adding keywords for better discoverability"
                .to_string(),
        );
    }

    if manifest.houdini.is_none() {
        result.add_warning(
            "Houdini configuration is missing - consider specifying min/max version constraints"
                .to_string(),
        );
    }
}

async fn validate_project_structure(
    project_dir: &Path,
    manifest: &PackageManifest,
    result: &mut ValidationResult,
) {
    let expected_dirs = ["otls", "python", "scripts", "presets", "config"];
    let mut found_dirs = Vec::new();

    for dir_name in &expected_dirs {
        let dir_path = project_dir.join(dir_name);
        if dir_path.exists() && dir_path.is_dir() {
            found_dirs.push(*dir_name);
            result.add_info(format!("[OK] Found {} directory", dir_name));
        }
    }

    if found_dirs.is_empty() {
        result.add_warning(
            "No standard Houdini directories found (otls, python, scripts, presets, config)"
                .to_string(),
        );
    }

    // Check for README
    let readme_files = ["README.md", "README.txt", "README"];
    let mut found_readme = false;
    for readme in &readme_files {
        if project_dir.join(readme).exists() {
            found_readme = true;
            result.add_info(format!("[OK] Found {}", readme));
            break;
        }
    }

    if !found_readme {
        if let Some(ref readme_path) = manifest.package.readme {
            if !project_dir.join(readme_path).exists() {
                result.add_error(format!(
                    "README file specified in manifest not found: {}",
                    readme_path
                ));
            } else {
                result.add_info(format!("[OK] Found specified README: {}", readme_path));
            }
        } else {
            result.add_warning("No README file found - consider adding documentation".to_string());
        }
    }

    // Validate otls directory if it exists
    if found_dirs.contains(&"otls") {
        validate_otls_directory(project_dir, result).await;
    }
}

async fn validate_otls_directory(project_dir: &Path, result: &mut ValidationResult) {
    let otls_path = project_dir.join("otls");

    match fs::read_dir(&otls_path) {
        Ok(entries) => {
            let mut has_assets = false;
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(extension) = path.extension() {
                    if extension == "hda" || extension == "otl" {
                        has_assets = true;
                        result.add_info(format!(
                            "[OK] Found Houdini asset: {}",
                            path.file_name().unwrap().to_string_lossy()
                        ));
                    }
                }
            }

            if !has_assets {
                result.add_warning(
                    "otls directory exists but contains no .hda or .otl files".to_string(),
                );
            }
        }
        Err(e) => {
            result.add_warning(format!("Could not read otls directory: {}", e));
        }
    }
}

fn validate_houdini_compatibility(manifest: &PackageManifest, result: &mut ValidationResult) {
    let houdini_package = manifest.generate_houdini_package();

    // Validate generated package.json structure
    match serde_json::to_string_pretty(&houdini_package) {
        Ok(json) => {
            result.add_info("[OK] Generated Houdini package.json is valid".to_string());

            // Validate JSON can be parsed back
            match serde_json::from_str::<serde_json::Value>(&json) {
                Ok(_) => {
                    result.add_info(
                        "[OK] Generated package.json can be parsed by Houdini".to_string(),
                    );
                }
                Err(e) => {
                    result.add_error(format!("Generated package.json is invalid: {}", e));
                }
            }
        }
        Err(e) => {
            result.add_error(format!("Failed to generate Houdini package.json: {}", e));
        }
    }

    // Validate version constraints
    if let Some(ref houdini_config) = manifest.houdini {
        if let Some(ref min_version) = houdini_config.min_version {
            if !is_valid_houdini_version(min_version) {
                result.add_warning(format!(
                    "Minimum Houdini version '{}' format may not be recognized by Houdini",
                    min_version
                ));
            } else {
                result.add_info(format!("[OK] Minimum Houdini version: {}", min_version));
            }
        }

        if let Some(ref max_version) = houdini_config.max_version {
            if !is_valid_houdini_version(max_version) {
                result.add_warning(format!(
                    "Maximum Houdini version '{}' format may not be recognized by Houdini",
                    max_version
                ));
            } else {
                result.add_info(format!("[OK] Maximum Houdini version: {}", max_version));
            }
        }

        if let (Some(ref min), Some(ref max)) =
            (&houdini_config.min_version, &houdini_config.max_version)
        {
            if compare_versions(min, max).unwrap_or(0) > 0 {
                result.add_error(
                    "Minimum Houdini version is greater than maximum version".to_string(),
                );
            }
        }
    }
}

async fn validate_best_practices(
    project_dir: &Path,
    manifest: &PackageManifest,
    result: &mut ValidationResult,
) {
    // Check for license file
    let license_files = ["LICENSE", "LICENSE.md", "LICENSE.txt", "COPYING"];
    let mut found_license = false;
    for license_file in &license_files {
        if project_dir.join(license_file).exists() {
            found_license = true;
            result.add_info(format!("[OK] Found license file: {}", license_file));
            break;
        }
    }

    if !found_license && manifest.package.license.is_some() {
        result.add_warning("License specified in manifest but no LICENSE file found".to_string());
    }

    // Check for version control
    if project_dir.join(".git").exists() {
        result.add_info("[OK] Git repository initialized".to_string());

        // Check for .gitignore
        if !project_dir.join(".gitignore").exists() {
            result.add_warning("No .gitignore file found - consider adding one".to_string());
        }
    } else {
        result.add_info("No version control detected - consider initializing git".to_string());
    }

    // Check for scripts
    if let Some(ref scripts) = manifest.scripts {
        result.add_info(format!("[OK] Package defines {} script(s)", scripts.len()));

        for (script_name, script_cmd) in scripts {
            if script_cmd.trim().is_empty() {
                result.add_warning(format!("Script '{}' has empty command", script_name));
            }
        }
    }

    // Check package size considerations
    check_package_size(project_dir, result).await;
}

async fn check_package_size(project_dir: &Path, result: &mut ValidationResult) {
    let mut total_size = 0u64;
    let mut large_files = Vec::new();

    if let Ok(entries) = walkdir::WalkDir::new(project_dir)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
    {
        for entry in entries {
            if entry.file_type().is_file() {
                if let Ok(metadata) = entry.metadata() {
                    let size = metadata.len();
                    total_size += size;

                    // Flag files larger than 10MB
                    if size > 10 * 1024 * 1024 {
                        large_files.push((entry.path().to_path_buf(), size));
                    }
                }
            }
        }
    }

    if total_size > 100 * 1024 * 1024 {
        result.add_warning(format!(
            "Package size is large ({:.1} MB) - consider if all files are necessary",
            total_size as f64 / (1024.0 * 1024.0)
        ));
    }

    for (path, size) in large_files {
        let relative_path = path.strip_prefix(project_dir).unwrap_or(&path);
        result.add_warning(format!(
            "Large file detected: {} ({:.1} MB)",
            relative_path.display(),
            size as f64 / (1024.0 * 1024.0)
        ));
    }
}

fn is_valid_houdini_version(version: &str) -> bool {
    // Houdini uses major.minor format primarily
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() < 2 || parts.len() > 3 {
        return false;
    }

    parts.iter().all(|part| part.parse::<u32>().is_ok())
}

fn compare_versions(v1: &str, v2: &str) -> Option<i32> {
    let parts1: Vec<u32> = v1.split('.').filter_map(|s| s.parse().ok()).collect();
    let parts2: Vec<u32> = v2.split('.').filter_map(|s| s.parse().ok()).collect();

    if parts1.is_empty() || parts2.is_empty() {
        return None;
    }

    for i in 0..parts1.len().max(parts2.len()) {
        let p1 = parts1.get(i).unwrap_or(&0);
        let p2 = parts2.get(i).unwrap_or(&0);

        if p1 > p2 {
            return Some(1);
        } else if p1 < p2 {
            return Some(-1);
        }
    }

    Some(0)
}

fn display_results(result: ValidationResult) -> Result<()> {
    println!();

    // Display info messages
    for info in &result.info_messages {
        info!("{}", info);
    }

    // Display warnings
    if !result.warnings.is_empty() {
        println!();
        for warning in &result.warnings {
            warn!("[WARN] {}", warning);
        }
    }

    // Display errors
    if !result.errors.is_empty() {
        println!();
        for error in &result.errors {
            error!("[ERROR] {}", error);
        }
    }

    println!();

    if result.is_valid {
        info!("Package validation completed successfully!");
        if !result.warnings.is_empty() {
            info!(
                "   {} warning(s) found - consider addressing them",
                result.warnings.len()
            );
        }
    } else {
        error!(
            "[ERROR] Package validation failed with {} error(s)",
            result.errors.len()
        );
        std::process::exit(1);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio;

    #[tokio::test]
    async fn test_validation_result_creation() {
        let mut result = ValidationResult::new();
        assert!(result.is_valid);
        assert!(result.errors.is_empty());
        assert!(result.warnings.is_empty());
        assert!(result.info_messages.is_empty());

        result.add_error("Test error".to_string());
        assert!(!result.is_valid);
        assert_eq!(result.errors.len(), 1);

        result.add_warning("Test warning".to_string());
        assert_eq!(result.warnings.len(), 1);

        result.add_info("Test info".to_string());
        assert_eq!(result.info_messages.len(), 1);
    }

    #[tokio::test]
    async fn test_is_valid_houdini_version() {
        assert!(is_valid_houdini_version("20.0"));
        assert!(is_valid_houdini_version("19.5"));
        assert!(is_valid_houdini_version("20.0.123"));

        assert!(!is_valid_houdini_version("invalid"));
        assert!(!is_valid_houdini_version(""));
        assert!(!is_valid_houdini_version("20"));
        assert!(!is_valid_houdini_version("20.0.0.1"));
    }

    #[tokio::test]
    async fn test_compare_versions() {
        assert_eq!(compare_versions("20.0", "19.5"), Some(1));
        assert_eq!(compare_versions("19.5", "20.0"), Some(-1));
        assert_eq!(compare_versions("20.0", "20.0"), Some(0));
        assert_eq!(compare_versions("20.0.1", "20.0"), Some(1));

        assert_eq!(compare_versions("invalid", "20.0"), None);
        assert_eq!(compare_versions("20.0", "invalid"), None);
    }

    #[tokio::test]
    async fn test_validate_manifest_file_missing() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_path = temp_dir.path().join("hpm.toml");
        let mut result = ValidationResult::new();

        let manifest = validate_manifest_file(&manifest_path, &mut result)
            .await
            .unwrap();

        assert!(manifest.is_none());
        assert!(!result.is_valid);
        assert!(!result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_validate_manifest_file_valid() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_path = temp_dir.path().join("hpm.toml");

        let manifest_content = r#"
[package]
name = "test-package"
version = "1.0.0"
description = "A test package"

[houdini]
min_version = "19.5"
"#;

        std::fs::write(&manifest_path, manifest_content).unwrap();

        let mut result = ValidationResult::new();
        let manifest = validate_manifest_file(&manifest_path, &mut result)
            .await
            .unwrap();

        assert!(manifest.is_some());
        assert!(result.is_valid);
        assert_eq!(result.info_messages.len(), 2); // Found + valid syntax
    }
}
