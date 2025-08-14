use crate::PackageManifest;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Package template for Houdini packages
pub struct PackageTemplate {
    pub directories: Vec<String>,
    pub files: HashMap<String, String>,
    pub bare: bool,
}

impl PackageTemplate {
    /// Create a new Houdini package template
    pub fn new(name: &str, manifest: &PackageManifest, bare: bool) -> Self {
        let mut template = Self {
            directories: Vec::new(),
            files: HashMap::new(),
            bare,
        };

        if bare {
            template.setup_bare_template(name, manifest);
        } else {
            template.setup_standard_template(name, manifest);
        }

        template
    }

    /// Create the package structure on filesystem
    pub fn create_structure<P: AsRef<Path>>(&self, base_path: P) -> Result<(), std::io::Error> {
        let base_path = base_path.as_ref();

        // Create directories
        for dir in &self.directories {
            let dir_path = base_path.join(dir);
            fs::create_dir_all(&dir_path)?;
        }

        // Create files
        for (file_path, content) in &self.files {
            let full_path = base_path.join(file_path);

            // Ensure parent directory exists
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent)?;
            }

            fs::write(full_path, content)?;
        }

        Ok(())
    }

    fn setup_standard_template(&mut self, name: &str, manifest: &PackageManifest) {
        // Standard Houdini package directories
        self.directories.extend([
            "otls".to_string(),
            "python".to_string(),
            "scripts".to_string(),
            "presets".to_string(),
            "config".to_string(),
            "tests".to_string(),
        ]);

        // Files
        self.files.insert(
            "hpm.toml".to_string(),
            self.generate_manifest_toml(manifest),
        );
        self.files.insert(
            "package.json".to_string(),
            self.generate_houdini_package_json(manifest),
        );
        self.files.insert(
            "README.md".to_string(),
            self.generate_readme(name, manifest),
        );
        self.files
            .insert(".gitignore".to_string(), self.generate_gitignore());
        self.files.insert(
            "python/__init__.py".to_string(),
            self.generate_python_init(),
        );
        self.files
            .insert("otls/.gitkeep".to_string(), String::new());
        self.files
            .insert("scripts/.gitkeep".to_string(), String::new());
        self.files
            .insert("presets/.gitkeep".to_string(), String::new());
        self.files
            .insert("config/.gitkeep".to_string(), String::new());
        self.files
            .insert("tests/.gitkeep".to_string(), String::new());
    }

    fn setup_bare_template(&mut self, _name: &str, manifest: &PackageManifest) {
        // Only create the essential files
        self.files.insert(
            "hpm.toml".to_string(),
            self.generate_manifest_toml(manifest),
        );
    }

    fn generate_manifest_toml(&self, manifest: &PackageManifest) -> String {
        toml::to_string_pretty(manifest).unwrap_or_else(|_| {
            // Fallback if serialization fails
            format!(
                r#"[package]
name = "{}"
version = "{}"
description = "{}"
authors = {}
license = "{}"
readme = "README.md"
keywords = ["houdini"]

[houdini]
min_version = "19.5"
"#,
                manifest.package.name,
                manifest.package.version,
                manifest
                    .package
                    .description
                    .as_ref()
                    .unwrap_or(&"".to_string()),
                toml::to_string(&manifest.package.authors).unwrap_or("[]".to_string()),
                manifest
                    .package
                    .license
                    .as_ref()
                    .unwrap_or(&"MIT".to_string())
            )
        })
    }

    fn generate_houdini_package_json(&self, manifest: &PackageManifest) -> String {
        let houdini_pkg = manifest.generate_houdini_package();
        serde_json::to_string_pretty(&houdini_pkg).unwrap_or_else(|_| {
            r#"{
    "hpath": ["$HPM_PACKAGE_ROOT/otls"],
    "env": [
        {"PYTHONPATH": {"method": "prepend", "value": "$HPM_PACKAGE_ROOT/python"}},
        {"HOUDINI_SCRIPT_PATH": {"method": "prepend", "value": "$HPM_PACKAGE_ROOT/scripts"}}
    ],
    "enable": "houdini_version >= '19.5'"
}"#
            .to_string()
        })
    }

    fn generate_readme(&self, name: &str, manifest: &PackageManifest) -> String {
        let default_description = "A Houdini package".to_string();
        let description = manifest
            .package
            .description
            .as_ref()
            .unwrap_or(&default_description);

        format!(
            r#"# {}

{}

## Installation

Install using HPM:

```bash
hpm add {}
```

## Usage

This package provides Houdini assets and tools for enhanced workflow capabilities.

## Development

### Building

```bash
hpm run build
```

### Testing

```bash
hpm run test
```

## License

{}
"#,
            name,
            description,
            name,
            manifest
                .package
                .license
                .as_ref()
                .unwrap_or(&"MIT".to_string())
        )
    }

    fn generate_gitignore(&self) -> String {
        r#"# Python
__pycache__/
*.py[cod]
*$py.class
*.so
.Python
build/
develop-eggs/
dist/
downloads/
eggs/
.eggs/
lib/
lib64/
parts/
sdist/
var/
wheels/
*.egg-info/
.installed.cfg
*.egg
MANIFEST

# Houdini
*.hip.bak
*.hipnc.bak
*.hiplc.bak
backup/
*.log

# HPM
.hpm/
*.hpm-lock

# IDE
.vscode/
.idea/
*.swp
*.swo

# OS
.DS_Store
Thumbs.db
"#
        .to_string()
    }

    fn generate_python_init(&self) -> String {
        r#"""
Houdini package Python module.
"""

__version__ = "1.0.0"
"#
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PackageManifest;
    use tempfile::TempDir;

    #[test]
    fn test_standard_template() {
        let manifest = PackageManifest::new(
            "test-package".to_string(),
            "1.0.0".to_string(),
            Some("Test package".to_string()),
            None,
            Some("MIT".to_string()),
        );

        let template = PackageTemplate::new("test-package", &manifest, false);

        assert!(!template.bare);
        assert!(template.directories.contains(&"otls".to_string()));
        assert!(template.directories.contains(&"python".to_string()));
        assert!(template.directories.contains(&"scripts".to_string()));
        assert!(template.files.contains_key("hpm.toml"));
        assert!(template.files.contains_key("README.md"));
        assert!(template.files.contains_key("package.json"));
    }

    #[test]
    fn test_bare_template() {
        let manifest = PackageManifest::new(
            "test-bare".to_string(),
            "1.0.0".to_string(),
            Some("Test bare package".to_string()),
            None,
            Some("MIT".to_string()),
        );

        let template = PackageTemplate::new("test-bare", &manifest, true);

        assert!(template.bare);
        assert!(template.directories.is_empty());
        assert_eq!(template.files.len(), 1);
        assert!(template.files.contains_key("hpm.toml"));
    }

    #[test]
    fn test_bare_template_filesystem_creation() {
        let manifest = PackageManifest::new(
            "test-bare-pkg".to_string(),
            "1.0.0".to_string(),
            Some("A test bare package".to_string()),
            Some(vec!["Test Author <test@example.com>".to_string()]),
            Some("MIT".to_string()),
        );

        let template = PackageTemplate::new("test-bare-pkg", &manifest, true);
        let temp_dir = TempDir::new().unwrap();

        let result = template.create_structure(&temp_dir);
        assert!(result.is_ok());

        // Check that only hpm.toml was created (bare template)
        let hpm_toml_path = temp_dir.path().join("hpm.toml");
        assert!(hpm_toml_path.exists());
        assert!(hpm_toml_path.is_file());

        // Verify no other files were created
        assert!(!temp_dir.path().join("README.md").exists());
        assert!(!temp_dir.path().join("package.json").exists());
        assert!(!temp_dir.path().join("python").exists());

        // Validate hpm.toml content
        let toml_content = std::fs::read_to_string(hpm_toml_path).unwrap();
        assert!(toml_content.contains("name = \"test-bare-pkg\""));
        assert!(toml_content.contains("version = \"1.0.0\""));
        assert!(toml_content.contains("description = \"A test bare package\""));
        assert!(toml_content.contains("Test Author <test@example.com>"));
        assert!(toml_content.contains("license = \"MIT\""));
    }

    #[test]
    fn test_standard_template_filesystem_creation() {
        let manifest = PackageManifest::new(
            "test-standard-pkg".to_string(),
            "2.0.0".to_string(),
            Some("A comprehensive test package".to_string()),
            Some(vec!["Test Author <test@example.com>".to_string()]),
            Some("Apache-2.0".to_string()),
        );

        let template = PackageTemplate::new("test-standard-pkg", &manifest, false);
        let temp_dir = TempDir::new().unwrap();

        let result = template.create_structure(&temp_dir);
        assert!(result.is_ok());

        // Validate all expected directories were created
        let expected_dirs = ["python", "otls", "scripts", "presets", "config", "tests"];
        for dir in &expected_dirs {
            let dir_path = temp_dir.path().join(dir);
            assert!(dir_path.exists(), "Directory {} should exist", dir);
            assert!(dir_path.is_dir(), "Path {} should be a directory", dir);
        }

        // Validate all expected files were created
        let expected_files = ["hpm.toml", "README.md", "package.json", ".gitignore"];
        for file in &expected_files {
            let file_path = temp_dir.path().join(file);
            assert!(file_path.exists(), "File {} should exist", file);
            assert!(file_path.is_file(), "Path {} should be a file", file);
        }

        // Validate python/__init__.py was created
        let python_init = temp_dir.path().join("python").join("__init__.py");
        assert!(python_init.exists());
        assert!(python_init.is_file());

        // Validate file contents
        validate_generated_file_contents(temp_dir.path(), &manifest);
    }

    fn validate_generated_file_contents(
        package_path: &std::path::Path,
        manifest: &PackageManifest,
    ) {
        // Validate hpm.toml content
        let toml_content = std::fs::read_to_string(package_path.join("hpm.toml")).unwrap();
        assert!(toml_content.contains(&format!("name = \"{}\"", manifest.package.name)));
        assert!(toml_content.contains(&format!("version = \"{}\"", manifest.package.version)));
        if let Some(ref description) = manifest.package.description {
            assert!(toml_content.contains(&format!("description = \"{}\"", description)));
        }
        if let Some(ref license) = manifest.package.license {
            assert!(toml_content.contains(&format!("license = \"{}\"", license)));
        }

        // Validate README.md content
        let readme_content = std::fs::read_to_string(package_path.join("README.md")).unwrap();
        assert!(readme_content.contains(&format!("# {}", manifest.package.name)));
        assert!(readme_content.contains(&format!("hpm add {}", manifest.package.name)));
        if let Some(ref description) = manifest.package.description {
            assert!(readme_content.contains(description));
        }
        if let Some(ref license) = manifest.package.license {
            assert!(readme_content.contains(license));
        }

        // Validate package.json content (Houdini package manifest)
        let package_json_content =
            std::fs::read_to_string(package_path.join("package.json")).unwrap();
        let package_json: serde_json::Value = serde_json::from_str(&package_json_content).unwrap();
        assert!(package_json["env"].is_array());
        assert!(package_json_content.contains("PYTHONPATH"));
        assert!(package_json_content.contains("HOUDINI_SCRIPT_PATH"));
        assert!(package_json["hpath"].is_array());

        // Validate .gitignore content
        let gitignore_content = std::fs::read_to_string(package_path.join(".gitignore")).unwrap();
        assert!(gitignore_content.contains("*.hip.bak"));
        assert!(gitignore_content.contains("__pycache__"));
        assert!(gitignore_content.contains(".DS_Store"));
        assert!(gitignore_content.contains(".hpm/"));

        // Validate python/__init__.py content
        let python_init_content =
            std::fs::read_to_string(package_path.join("python").join("__init__.py")).unwrap();
        assert!(python_init_content.contains("Houdini package Python module"));
        assert!(python_init_content.contains("__version__ = \"1.0.0\""));
    }
}
