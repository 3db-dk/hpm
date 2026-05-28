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
}

pub async fn check_package(directory: Option<std::path::PathBuf>) -> Result<()> {
    let current_dir = match directory {
        Some(dir) => dir,
        None => std::env::current_dir().context("Failed to get current directory")?,
    };
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

    if manifest.compat.as_ref().is_none_or(|c| c.houdini.is_none()) {
        result.add_warning(
            "[compat].houdini is missing - consider declaring a Houdini version range".to_string(),
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

    // Validate [compat].houdini parses as a Cargo-style range. Manifest
    // validate() already rejects malformed ranges, so this branch only
    // surfaces an info line on the happy path.
    if let Some(compat) = &manifest.compat
        && let Some(req) = &compat.houdini
    {
        match hpm_package::compile_houdini_req(req) {
            Ok(expr) => {
                result.add_info(format!(
                    "[OK] Houdini compatibility: {} (compiles to `{}`)",
                    req, expr
                ));
            }
            Err(e) => {
                result.add_error(format!("[compat].houdini '{}': {}", req, e));
            }
        }

        // Native-binary packages that leave their Houdini range
        // unbounded above are a footgun: DSOs compiled against one
        // Houdini major typically won't load in the next, so the
        // package will install cleanly on a newer Houdini and then
        // crash at load. Surface it as a warning so the author can
        // either narrow the range or confirm they really mean to ship
        // platform-agnostic content.
        if !compat.platforms.is_empty() && !hpm_package::houdini_req_has_upper_bound(req) {
            result.add_warning(format!(
                "[compat].platforms is declared but [compat].houdini = \"{}\" \
                 has no upper bound. Native binaries compiled against one \
                 Houdini major typically won't load in the next. Consider \
                 \"^21\" (Houdini 21.x only) or an explicit range like \
                 \">=20.5, <22\".",
                req
            ));
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
        result.add_info(format!(
            "[OK] Package defines {} script(s)",
            scripts.commands.len()
        ));

        let host_os = hpm_package::Platform::current().and_then(|p| p.os_key().map(str::to_string));
        for (name, entry) in &scripts.commands {
            let resolved = entry.resolve_cmd(host_os.as_deref());
            match resolved {
                Some(cmd) if cmd.trim().is_empty() => {
                    result.add_warning(format!("Script '{}' has empty command", name));
                }
                Some(_) => {}
                None => {
                    // No variant matches the host — still legitimate (the
                    // script just isn't available on this OS) but worth a
                    // hint so authors notice typos in their `when` axes.
                    result.add_info(format!(
                        "Script '{}' has no command for host OS — only matches other platforms",
                        name
                    ));
                }
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
        return Err(anyhow::anyhow!(
            "Package validation failed with {} error(s)",
            result.errors.len()
        ));
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
path = "studio/test-package"
name = "test-package"
version = "1.0.0"
description = "A test package"

[compat]
houdini = ">=20.5"
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

    #[tokio::test]
    async fn warns_when_platforms_declared_with_unbounded_houdini() {
        // A package declaring [compat].platforms but leaving the houdini
        // range unbounded above ships DSOs that will likely fail to load
        // on the next Houdini major. `hpm check` must flag this.
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path();
        let manifest_path = project_dir.join("hpm.toml");
        std::fs::write(
            &manifest_path,
            r#"
[package]
path = "studio/needs-bound"
name = "Needs Bound"
version = "1.0.0"

[compat]
houdini = ">=21"
platforms = ["linux-x86_64"]
"#,
        )
        .unwrap();

        let mut result = ValidationResult::new();
        let manifest = validate_manifest_file(&manifest_path, &mut result)
            .await
            .unwrap()
            .expect("manifest parses");
        super::validate_houdini_compatibility(&manifest, &mut result);

        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("no upper bound") && w.contains("Native binaries")),
            "expected upper-bound warning, got: {:?}",
            result.warnings
        );
    }

    #[tokio::test]
    async fn no_warning_when_platforms_declared_with_bounded_houdini() {
        // Same shape but with a bounded houdini range — no warning.
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path();
        let manifest_path = project_dir.join("hpm.toml");
        std::fs::write(
            &manifest_path,
            r#"
[package]
path = "studio/bounded"
name = "Bounded"
version = "1.0.0"

[compat]
houdini = "^21"
platforms = ["linux-x86_64"]
"#,
        )
        .unwrap();

        let mut result = ValidationResult::new();
        let manifest = validate_manifest_file(&manifest_path, &mut result)
            .await
            .unwrap()
            .expect("manifest parses");
        super::validate_houdini_compatibility(&manifest, &mut result);

        assert!(
            !result.warnings.iter().any(|w| w.contains("no upper bound")),
            "expected no upper-bound warning, got: {:?}",
            result.warnings
        );
    }

    #[tokio::test]
    async fn no_warning_when_no_platforms_declared() {
        // Pure-data / pure-Python package — unbounded houdini range is
        // fine, no warning.
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path();
        let manifest_path = project_dir.join("hpm.toml");
        std::fs::write(
            &manifest_path,
            r#"
[package]
path = "studio/pure"
name = "Pure"
version = "1.0.0"

[compat]
houdini = ">=21"
"#,
        )
        .unwrap();

        let mut result = ValidationResult::new();
        let manifest = validate_manifest_file(&manifest_path, &mut result)
            .await
            .unwrap()
            .expect("manifest parses");
        super::validate_houdini_compatibility(&manifest, &mut result);

        assert!(
            !result.warnings.iter().any(|w| w.contains("no upper bound")),
            "expected no warning for pure-data package, got: {:?}",
            result.warnings
        );
    }
}
