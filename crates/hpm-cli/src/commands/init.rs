use anyhow::{Context, Result};
use hpm_package::{PackageManifest, PackageTemplate};
use owo_colors::{OwoColorize, Style};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{info, warn};

pub struct InitOptions {
    pub name_or_path: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    pub version: String,
    pub license: String,
    pub houdini_min: Option<String>,
    pub houdini_max: Option<String>,
    pub bare: bool,
    pub vcs: String,
    /// Base directory where the package should be created. If None, uses current working directory.
    pub base_dir: Option<PathBuf>,
}

pub async fn init_package(options: InitOptions) -> Result<String> {
    let (target_dir, package_name, should_create_dir) = determine_init_strategy(&options)?;

    // Check for existing hpm.toml
    let hpm_toml_path = target_dir.join("hpm.toml");
    if hpm_toml_path.exists() {
        return Err(anyhow::anyhow!(
            "hpm.toml already exists in '{}'. Package already initialized.",
            target_dir.display()
        ));
    }

    // If directory exists and we're not creating it, validate Houdini package structure
    if target_dir.exists()
        && !should_create_dir
        && !is_valid_houdini_package_structure(&target_dir)?
    {
        return Err(anyhow::anyhow!(
            "Directory '{}' does not have a valid Houdini package structure.",
            target_dir.display()
        ));
    }

    if options.bare {
        info!("Creating new minimal Houdini package: {}", package_name);
    } else {
        info!("Creating new Houdini package: {}", package_name);
    }

    // Determine author from git config if not provided
    let author = match options.author {
        Some(author) => Some(vec![author]),
        None => get_git_author().await.map(|a| vec![a]),
    };

    // Create package manifest
    let mut manifest = PackageManifest::new(
        package_name.clone(),
        options.version,
        options.description,
        author,
        Some(options.license),
    );

    // Update Houdini configuration
    if let Some(houdini_config) = &mut manifest.houdini {
        if let Some(min_version) = options.houdini_min {
            houdini_config.min_version = Some(min_version);
        }
        if let Some(max_version) = options.houdini_max {
            houdini_config.max_version = Some(max_version);
        }
    }

    // Validate manifest
    manifest
        .validate()
        .map_err(|e| anyhow::anyhow!("Package manifest validation failed: {}", e))?;

    // Create package template
    let template = PackageTemplate::new(&package_name, &manifest, options.bare);

    // Create directory only if needed
    if should_create_dir {
        fs::create_dir_all(&target_dir)
            .with_context(|| format!("Failed to create directory '{}'", package_name))?;
    }

    // Create package structure
    template
        .create_structure(&target_dir)
        .context("Failed to create package structure")?;

    info!("Package structure created successfully");

    // Initialize version control
    if options.vcs == "git" {
        init_git_repository(&target_dir).await?;
        info!("Initialized git repository");
    }

    // Print success message with styling
    if options.bare {
        println!(
            "{}",
            format!(
                "Successfully created minimal Houdini package '{}'",
                package_name
            )
            .style(Style::new().green())
        );
    } else {
        println!(
            "{}",
            format!("Successfully created Houdini package '{}'", package_name)
                .style(Style::new().green())
        );
    }

    println!("\n{}", "Package structure:".style(Style::new().bold()));
    print_directory_tree(&target_dir, 0)?;

    if !options.bare {
        println!("\n{}", "Next steps:".style(Style::new().bold()));
        println!("  {} {}", "cd".style(Style::new().cyan()), package_name);
        println!(
            "  {} {}",
            "hpm add".style(Style::new().cyan()),
            "# Add dependencies".style(Style::new().bright_black())
        );
    }

    Ok(package_name)
}

fn determine_init_strategy(options: &InitOptions) -> Result<(PathBuf, String, bool)> {
    let base_working_dir = match &options.base_dir {
        Some(dir) => dir.clone(),
        None => env::current_dir().context("Failed to get current directory")?,
    };

    match &options.name_or_path {
        None => {
            // Case 1: `hpm init` - initialize in current directory
            let dir_name = base_working_dir
                .file_name()
                .context("Failed to get directory name")?
                .to_string_lossy()
                .to_string();
            let package_name = convert_to_kebab_case(&dir_name);
            Ok((base_working_dir, package_name, false))
        }
        Some(name_or_path) => {
            let path = Path::new(name_or_path);

            if path.is_absolute() || name_or_path.contains('/') || name_or_path.contains('\\') {
                // Case 3: `hpm init <directory_path>` - absolute or relative path
                let target_dir = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    base_working_dir.join(path)
                };

                let package_name = target_dir
                    .file_name()
                    .context("Failed to get directory name from path")?
                    .to_string_lossy()
                    .to_string();
                let package_name = convert_to_kebab_case(&package_name);

                let dir_exists = target_dir.exists();
                Ok((target_dir, package_name, !dir_exists))
            } else {
                // Case 2: `hpm init <package_name>` - create directory with this name
                let package_name = convert_to_kebab_case(name_or_path);
                let target_dir = base_working_dir.join(&package_name);

                if target_dir.exists() {
                    return Err(anyhow::anyhow!(
                        "Directory '{}' already exists. Choose a different name or remove the existing directory.",
                        package_name
                    ));
                }

                Ok((target_dir, package_name, true))
            }
        }
    }
}

fn is_valid_houdini_package_structure(dir: &Path) -> Result<bool> {
    // Check if directory is empty or contains typical Houdini package structure
    let entries: Vec<_> = fs::read_dir(dir)
        .context("Failed to read directory")?
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to collect directory entries")?;

    if entries.is_empty() {
        return Ok(true); // Empty directory is valid
    }

    // Check for typical Houdini package directories/files
    let valid_dirs = [
        "otls",
        "python",
        "scripts",
        "presets",
        "config",
        "tests",
        "dso",
        "python_panels",
        "viewer_states",
    ];
    let valid_files = ["package.json", "README.md", ".gitignore", "OPlibraries"];

    for entry in &entries {
        let file_name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden files except for .gitignore
        if file_name.starts_with('.') && file_name != ".gitignore" {
            continue;
        }

        let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
        let is_valid = if is_dir {
            valid_dirs.contains(&file_name.as_str())
        } else {
            valid_files.contains(&file_name.as_str())
        };

        if !is_valid {
            // Allow any files that look like Houdini-related files
            if file_name.ends_with(".hda")
                || file_name.ends_with(".otl")
                || file_name.ends_with(".hip")
            {
                continue;
            }
            return Ok(false);
        }
    }

    Ok(true)
}

fn convert_to_kebab_case(name: &str) -> String {
    let mut result = String::new();
    let mut prev_was_uppercase = false;
    let mut prev_was_separator = false;

    for c in name.chars() {
        if c.is_uppercase() {
            // Only add hyphen if previous char wasn't uppercase and we have content
            if !result.is_empty() && !prev_was_uppercase && !prev_was_separator {
                result.push('-');
            }
            result.push(c.to_lowercase().next().unwrap());
            prev_was_uppercase = true;
            prev_was_separator = false;
        } else if c == '_' {
            if !result.is_empty() && !prev_was_separator {
                result.push('-');
                prev_was_separator = true;
            }
            prev_was_uppercase = false;
        } else {
            result.push(c);
            prev_was_uppercase = false;
            prev_was_separator = false;
        }
    }

    // Remove duplicate hyphens and trim leading/trailing hyphens
    result
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase()
}

async fn get_git_author() -> Option<String> {
    let name_output = Command::new("git")
        .args(["config", "user.name"])
        .output()
        .ok()?;

    let email_output = Command::new("git")
        .args(["config", "user.email"])
        .output()
        .ok()?;

    if name_output.status.success() && email_output.status.success() {
        let name = String::from_utf8(name_output.stdout)
            .ok()?
            .trim()
            .to_string();
        let email = String::from_utf8(email_output.stdout)
            .ok()?
            .trim()
            .to_string();

        if !name.is_empty() && !email.is_empty() {
            Some(format!("{} <{}>", name, email))
        } else if !name.is_empty() {
            Some(name)
        } else {
            None
        }
    } else {
        None
    }
}

async fn init_git_repository(dir: &Path) -> Result<()> {
    let output = Command::new("git")
        .args(["init"])
        .current_dir(dir)
        .output()
        .context("Failed to execute git init")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!("Git initialization failed: {}", stderr);
        // Don't fail the entire init process if git fails
    }

    Ok(())
}

fn print_directory_tree(dir: &Path, depth: usize) -> Result<()> {
    if depth > 3 {
        // Limit recursion depth
        return Ok(());
    }

    let entries = fs::read_dir(dir)
        .context("Failed to read directory")?
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to collect directory entries")?;

    for (i, entry) in entries.iter().enumerate() {
        let is_last = i == entries.len() - 1;
        let prefix = if depth == 0 {
            if is_last {
                "└── ".to_string()
            } else {
                "├── ".to_string()
            }
        } else {
            let mut p = "    ".repeat(depth);
            p.push_str(if is_last { "└── " } else { "├── " });
            p
        };

        let file_name_os = entry.file_name();
        let file_name = file_name_os.to_string_lossy();

        // Skip hidden files and directories
        if file_name.starts_with('.') && file_name != ".gitignore" {
            continue;
        }

        println!("{}{}", prefix, file_name);

        // Recursively print subdirectories
        if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
            print_directory_tree(&entry.path(), depth + 1)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_convert_to_kebab_case() {
        assert_eq!(convert_to_kebab_case("MyPackage"), "my-package");
        assert_eq!(convert_to_kebab_case("my_package"), "my-package");
        assert_eq!(convert_to_kebab_case("my-package"), "my-package");
        assert_eq!(convert_to_kebab_case("MyProject_Name"), "my-project-name");
    }

    #[tokio::test]
    async fn test_init_package_bare() {
        let temp_dir = TempDir::new().unwrap();

        let options = InitOptions {
            name_or_path: Some("test-bare-package".to_string()),
            description: Some("Test bare package".to_string()),
            author: Some("Test Author <test@example.com>".to_string()),
            version: "1.0.0".to_string(),
            license: "MIT".to_string(),
            houdini_min: Some("19.5".to_string()),
            houdini_max: None,
            bare: true,
            vcs: "none".to_string(),
            base_dir: Some(temp_dir.path().to_path_buf()),
        };

        let result = init_package(options).await;
        if let Err(e) = &result {
            eprintln!("Init package failed: {}", e);
        }

        assert!(result.is_ok());
        let package_path = temp_dir.path().join("test-bare-package");

        // Verify package directory exists
        assert!(package_path.exists());
        assert!(package_path.is_dir());

        // Verify only hpm.toml exists (bare package)
        assert!(package_path.join("hpm.toml").exists());
        assert!(!package_path.join("package.json").exists());
        assert!(!package_path.join("README.md").exists());
        assert!(!package_path.join("python").exists());
        assert!(!package_path.join("otls").exists());

        // Validate hpm.toml content
        let hpm_toml_content = fs::read_to_string(package_path.join("hpm.toml")).unwrap();
        assert!(hpm_toml_content.contains("name = \"test-bare-package\""));
        assert!(hpm_toml_content.contains("version = \"1.0.0\""));
        assert!(hpm_toml_content.contains("description = \"Test bare package\""));
        assert!(hpm_toml_content.contains("Test Author <test@example.com>"));
        assert!(hpm_toml_content.contains("license = \"MIT\""));
        assert!(hpm_toml_content.contains("min_version = \"19.5\""));
    }

    #[tokio::test]
    async fn test_init_package_standard() {
        let temp_dir = TempDir::new().unwrap();

        let options = InitOptions {
            name_or_path: Some("test-standard-package".to_string()),
            description: Some("A comprehensive test package".to_string()),
            author: Some("Test Author <test@example.com>".to_string()),
            version: "2.1.0".to_string(),
            license: "Apache-2.0".to_string(),
            houdini_min: Some("19.5".to_string()),
            houdini_max: Some("21.0".to_string()),
            bare: false,
            vcs: "none".to_string(),
            base_dir: Some(temp_dir.path().to_path_buf()),
        };

        let result = init_package(options).await;
        if let Err(e) = &result {
            eprintln!("Init package failed: {}", e);
        }

        assert!(result.is_ok());
        let package_path = temp_dir.path().join("test-standard-package");

        // Verify package directory exists
        assert!(package_path.exists());
        assert!(package_path.is_dir());

        // Verify all expected files exist
        assert!(package_path.join("hpm.toml").exists());
        assert!(package_path.join("package.json").exists());
        assert!(package_path.join("README.md").exists());
        assert!(package_path.join(".gitignore").exists());
        assert!(package_path.join("python").join("__init__.py").exists());

        // Verify all expected directories exist
        assert!(package_path.join("python").is_dir());
        assert!(package_path.join("otls").is_dir());
        assert!(package_path.join("scripts").is_dir());
        assert!(package_path.join("presets").is_dir());
        assert!(package_path.join("config").is_dir());
        assert!(package_path.join("tests").is_dir());

        // Validate hpm.toml content
        let hpm_toml_content = fs::read_to_string(package_path.join("hpm.toml")).unwrap();
        assert!(hpm_toml_content.contains("name = \"test-standard-package\""));
        assert!(hpm_toml_content.contains("version = \"2.1.0\""));
        assert!(hpm_toml_content.contains("description = \"A comprehensive test package\""));
        assert!(hpm_toml_content.contains("Test Author <test@example.com>"));
        assert!(hpm_toml_content.contains("license = \"Apache-2.0\""));
        assert!(hpm_toml_content.contains("min_version = \"19.5\""));
        assert!(hpm_toml_content.contains("max_version = \"21.0\""));

        // Validate package.json content (Houdini package manifest)
        let package_json_content = fs::read_to_string(package_path.join("package.json")).unwrap();
        let package_json: serde_json::Value = serde_json::from_str(&package_json_content).unwrap();
        assert_eq!(package_json["env"].as_array().unwrap().len(), 2);
        assert!(package_json_content.contains("PYTHONPATH"));
        assert!(package_json_content.contains("HOUDINI_SCRIPT_PATH"));
        assert!(package_json_content.contains("houdini_version >= '19.5'"));
        assert!(package_json["hpath"].as_array().is_some());

        // Validate README.md content
        let readme_content = fs::read_to_string(package_path.join("README.md")).unwrap();
        assert!(readme_content.contains("# test-standard-package"));
        assert!(readme_content.contains("A comprehensive test package"));
        assert!(readme_content.contains("hpm add test-standard-package"));
        assert!(readme_content.contains("Apache-2.0"));

        // Validate .gitignore content
        let gitignore_content = fs::read_to_string(package_path.join(".gitignore")).unwrap();
        assert!(gitignore_content.contains("*.hip.bak"));
        assert!(gitignore_content.contains("__pycache__"));
        assert!(gitignore_content.contains(".DS_Store"));

        // Validate python/__init__.py content
        let python_init_content =
            fs::read_to_string(package_path.join("python").join("__init__.py")).unwrap();
        assert!(python_init_content.contains("Houdini package Python module"));
        assert!(python_init_content.contains("__version__ = \"1.0.0\""));
    }

    #[tokio::test]
    async fn test_init_package_with_minimal_options() {
        let temp_dir = TempDir::new().unwrap();

        let options = InitOptions {
            name_or_path: Some("minimal-pkg".to_string()),
            description: None, // No description
            author: None,      // No author
            version: "0.1.0".to_string(),
            license: "MIT".to_string(),
            houdini_min: None,
            houdini_max: None,
            bare: false,
            vcs: "none".to_string(),
            base_dir: Some(temp_dir.path().to_path_buf()),
        };

        let result = init_package(options).await;

        assert!(result.is_ok());
        let package_path = temp_dir.path().join("minimal-pkg");
        assert!(package_path.exists());

        // Validate hpm.toml handles missing optional fields
        let hpm_toml_content = fs::read_to_string(package_path.join("hpm.toml")).unwrap();
        assert!(hpm_toml_content.contains("name = \"minimal-pkg\""));
        assert!(hpm_toml_content.contains("version = \"0.1.0\""));
        assert!(hpm_toml_content.contains("license = \"MIT\""));
        // Should have default description
        assert!(!hpm_toml_content.contains("description = \"\""));

        // Validate README handles missing description
        let readme_content = fs::read_to_string(package_path.join("README.md")).unwrap();
        assert!(readme_content.contains("# minimal-pkg"));
        assert!(readme_content.contains("A Houdini package")); // Default description
    }

    #[tokio::test]
    async fn test_init_package_with_special_characters() {
        let temp_dir = TempDir::new().unwrap();

        let options = InitOptions {
            name_or_path: Some("special-chars-test".to_string()),
            description: Some("Package with \"quotes\" & special chars!".to_string()),
            author: Some("Author Name <email+test@example.com>".to_string()),
            version: "1.0.0".to_string(),
            license: "MIT".to_string(),
            houdini_min: None,
            houdini_max: None,
            bare: false,
            vcs: "none".to_string(),
            base_dir: Some(temp_dir.path().to_path_buf()),
        };

        let result = init_package(options).await;

        assert!(result.is_ok());
        let package_path = temp_dir.path().join("special-chars-test");

        // Validate TOML escaping handles special characters correctly
        let hpm_toml_content = fs::read_to_string(package_path.join("hpm.toml")).unwrap();
        assert!(hpm_toml_content.contains("special-chars-test"));
        // Should properly escape quotes and special characters in TOML
        assert!(hpm_toml_content.contains("Author Name <email+test@example.com>"));
    }

    #[tokio::test]
    async fn test_init_package_directory_already_exists() {
        let temp_dir = TempDir::new().unwrap();

        // Create directory first
        let package_dir = temp_dir.path().join("existing-pkg");
        fs::create_dir(&package_dir).unwrap();

        let options = InitOptions {
            name_or_path: Some("existing-pkg".to_string()),
            description: Some("Test package".to_string()),
            author: Some("Test Author <test@example.com>".to_string()),
            version: "1.0.0".to_string(),
            license: "MIT".to_string(),
            houdini_min: None,
            houdini_max: None,
            bare: false,
            vcs: "none".to_string(),
            base_dir: Some(temp_dir.path().to_path_buf()),
        };

        let result = init_package(options).await;

        // Should fail because directory already exists
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("already exists"));
    }

    #[tokio::test]
    async fn test_empty_name_handling() {
        let temp_dir = TempDir::new().unwrap();
        // Create a subdirectory with a valid package name for testing
        let test_dir = temp_dir.path().join("my-test-package");
        fs::create_dir(&test_dir).unwrap();

        let options = InitOptions {
            name_or_path: None, // No name provided - should derive from directory name
            description: Some("Test package".to_string()),
            author: Some("Test Author <test@example.com>".to_string()),
            version: "1.0.0".to_string(),
            license: "MIT".to_string(),
            houdini_min: None,
            houdini_max: None,
            bare: false,
            vcs: "none".to_string(),
            base_dir: Some(test_dir),
        };

        let result = init_package(options).await;

        // Should derive name from directory name and succeed
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_init_current_directory() {
        let temp_dir = TempDir::new().unwrap();
        let test_dir = temp_dir.path().join("my-houdini-package");
        fs::create_dir(&test_dir).unwrap();

        let options = InitOptions {
            name_or_path: None, // No name - should init in current directory
            description: Some("Test current directory init".to_string()),
            author: Some("Test Author <test@example.com>".to_string()),
            version: "1.0.0".to_string(),
            license: "MIT".to_string(),
            houdini_min: None,
            houdini_max: None,
            bare: false,
            vcs: "none".to_string(),
            base_dir: Some(test_dir.clone()),
        };

        let result = init_package(options).await;
        assert!(result.is_ok());
        let package_name = result.unwrap();
        assert_eq!(package_name, "my-houdini-package");

        // Verify hpm.toml was created in the current directory
        assert!(test_dir.join("hpm.toml").exists());
        assert!(test_dir.join("README.md").exists());
    }

    #[tokio::test]
    async fn test_init_with_absolute_path() {
        let temp_dir = TempDir::new().unwrap();
        let target_path = temp_dir.path().join("absolute-path-package");

        let options = InitOptions {
            name_or_path: Some(target_path.to_string_lossy().to_string()),
            description: Some("Test absolute path init".to_string()),
            author: Some("Test Author <test@example.com>".to_string()),
            version: "1.0.0".to_string(),
            license: "MIT".to_string(),
            houdini_min: None,
            houdini_max: None,
            bare: false,
            vcs: "none".to_string(),
            base_dir: Some(temp_dir.path().to_path_buf()),
        };

        let result = init_package(options).await;
        assert!(result.is_ok());
        let package_name = result.unwrap();
        assert_eq!(package_name, "absolute-path-package");

        // Verify directory was created
        assert!(target_path.exists());
        assert!(target_path.join("hpm.toml").exists());
    }

    #[tokio::test]
    async fn test_init_with_relative_path() {
        let temp_dir = TempDir::new().unwrap();

        let options = InitOptions {
            name_or_path: Some("subfolder/relative-package".to_string()),
            description: Some("Test relative path init".to_string()),
            author: Some("Test Author <test@example.com>".to_string()),
            version: "1.0.0".to_string(),
            license: "MIT".to_string(),
            houdini_min: None,
            houdini_max: None,
            bare: false,
            vcs: "none".to_string(),
            base_dir: Some(temp_dir.path().to_path_buf()),
        };

        let result = init_package(options).await;
        assert!(result.is_ok());
        let package_name = result.unwrap();
        assert_eq!(package_name, "relative-package");

        // Verify nested directory structure was created
        let target_path = temp_dir.path().join("subfolder").join("relative-package");
        assert!(target_path.exists());
        assert!(target_path.join("hpm.toml").exists());
    }

    #[tokio::test]
    async fn test_init_existing_hpm_toml_error() {
        let temp_dir = TempDir::new().unwrap();

        // Create an existing hpm.toml file
        let hpm_toml_path = temp_dir.path().join("hpm.toml");
        fs::write(&hpm_toml_path, "# existing hpm.toml").unwrap();

        let options = InitOptions {
            name_or_path: None,
            description: Some("Test package".to_string()),
            author: Some("Test Author <test@example.com>".to_string()),
            version: "1.0.0".to_string(),
            license: "MIT".to_string(),
            houdini_min: None,
            houdini_max: None,
            bare: false,
            vcs: "none".to_string(),
            base_dir: Some(temp_dir.path().to_path_buf()),
        };

        let result = init_package(options).await;
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("hpm.toml already exists"));
    }

    #[tokio::test]
    async fn test_init_invalid_houdini_structure_error() {
        let temp_dir = TempDir::new().unwrap();

        // Create some non-Houdini files
        fs::write(temp_dir.path().join("invalid_file.txt"), "invalid").unwrap();
        fs::create_dir(temp_dir.path().join("invalid_dir")).unwrap();

        let options = InitOptions {
            name_or_path: None,
            description: Some("Test package".to_string()),
            author: Some("Test Author <test@example.com>".to_string()),
            version: "1.0.0".to_string(),
            license: "MIT".to_string(),
            houdini_min: None,
            houdini_max: None,
            bare: false,
            vcs: "none".to_string(),
            base_dir: Some(temp_dir.path().to_path_buf()),
        };

        let result = init_package(options).await;
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("does not have a valid Houdini package structure"));
    }

    #[tokio::test]
    async fn test_init_valid_houdini_structure_success() {
        let temp_dir = TempDir::new().unwrap();
        let test_dir = temp_dir.path().join("valid-houdini-package");
        fs::create_dir(&test_dir).unwrap();

        // Create valid Houdini package directories
        fs::create_dir(test_dir.join("otls")).unwrap();
        fs::create_dir(test_dir.join("python")).unwrap();
        fs::write(test_dir.join("package.json"), "{}").unwrap();

        let options = InitOptions {
            name_or_path: None,
            description: Some("Test package".to_string()),
            author: Some("Test Author <test@example.com>".to_string()),
            version: "1.0.0".to_string(),
            license: "MIT".to_string(),
            houdini_min: None,
            houdini_max: None,
            bare: false,
            vcs: "none".to_string(),
            base_dir: Some(test_dir.clone()),
        };

        let result = init_package(options).await;
        if let Err(e) = &result {
            eprintln!("Test error: {}", e);
        }
        assert!(result.is_ok());

        // Verify hpm.toml was created
        assert!(test_dir.join("hpm.toml").exists());
    }

    #[test]
    fn test_special_character_handling() {
        // Test various edge cases for kebab case conversion
        assert_eq!(convert_to_kebab_case("TEST"), "test");
        assert_eq!(convert_to_kebab_case("Test123"), "test123");
        assert_eq!(
            convert_to_kebab_case("Test_With_Numbers123"),
            "test-with-numbers123"
        );
    }
}
