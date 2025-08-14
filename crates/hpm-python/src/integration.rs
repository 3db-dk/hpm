//! Houdini integration for Python dependencies

use crate::venv::VenvManager;
use anyhow::Result;
use serde_json::{json, Value};
use std::path::Path;

/// Generate Houdini package.json with Python environment integration
pub fn generate_houdini_package_json(
    package_name: &str,
    venv_path: Option<&Path>,
) -> Result<Value> {
    let mut package_json = json!({
        "path": "$HPM_PACKAGE_ROOT"
    });

    if let Some(venv) = venv_path {
        let venv_manager = VenvManager::new();
        let python_path = venv_manager.get_python_site_packages_path(venv);

        let python_path_str = format!(
            "{}{}$PYTHONPATH",
            python_path.display(),
            get_path_separator()
        );

        package_json["env"] = json!([
            {
                "PYTHONPATH": python_path_str
            }
        ]);
    }

    // Add package metadata
    package_json["hpm_managed"] = json!(true);
    package_json["hpm_package"] = json!(package_name);

    Ok(package_json)
}

/// Get the appropriate path separator for the current platform
#[cfg(target_os = "windows")]
fn get_path_separator() -> &'static str {
    ";"
}

#[cfg(not(target_os = "windows"))]
fn get_path_separator() -> &'static str {
    ":"
}

/// Update package.json file with Python environment
pub async fn update_package_json_with_python(
    package_json_path: &Path,
    package_name: &str,
    venv_path: Option<&Path>,
) -> Result<()> {
    use tokio::fs;

    let updated_json = generate_houdini_package_json(package_name, venv_path)?;
    let json_content = serde_json::to_string_pretty(&updated_json)?;

    // Ensure parent directory exists
    if let Some(parent) = package_json_path.parent() {
        fs::create_dir_all(parent).await?;
    }

    fs::write(package_json_path, json_content).await?;
    Ok(())
}

/// Extract Python environment path from package.json
pub async fn extract_python_env_from_package_json(
    package_json_path: &Path,
) -> Result<Option<String>> {
    use tokio::fs;

    if !package_json_path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(package_json_path).await?;
    let json: Value = serde_json::from_str(&content)?;

    if let Some(env_array) = json.get("env").and_then(|e| e.as_array()) {
        for env_item in env_array {
            if let Some(pythonpath) = env_item.get("PYTHONPATH").and_then(|p| p.as_str()) {
                // Extract the venv path from PYTHONPATH (before the separator)
                let separator = get_path_separator();
                if let Some(venv_path) = pythonpath.split(separator).next() {
                    return Ok(Some(venv_path.to_string()));
                }
            }
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn test_generate_package_json_without_python() {
        let result = generate_houdini_package_json("test-package", None).unwrap();

        assert_eq!(result["path"], "$HPM_PACKAGE_ROOT");
        assert_eq!(result["hpm_managed"], true);
        assert_eq!(result["hpm_package"], "test-package");
        assert!(result.get("env").is_none());
    }

    #[test]
    fn test_generate_package_json_with_python() {
        let venv_path = PathBuf::from("/home/user/.hpm/venvs/abc123");
        let result = generate_houdini_package_json("test-package", Some(&venv_path)).unwrap();

        assert_eq!(result["path"], "$HPM_PACKAGE_ROOT");
        assert_eq!(result["hpm_managed"], true);
        assert_eq!(result["hpm_package"], "test-package");

        let env_array = result["env"].as_array().unwrap();
        assert_eq!(env_array.len(), 1);

        let pythonpath = env_array[0]["PYTHONPATH"].as_str().unwrap();
        assert!(pythonpath.contains("site-packages"));
        assert!(pythonpath.ends_with("$PYTHONPATH"));
    }

    #[tokio::test]
    async fn test_update_package_json_with_python() {
        let temp_dir = TempDir::new().unwrap();
        let package_json_path = temp_dir.path().join("package.json");
        let venv_path = PathBuf::from("/test/venv");

        update_package_json_with_python(&package_json_path, "test-package", Some(&venv_path))
            .await
            .unwrap();

        assert!(package_json_path.exists());

        let content = tokio::fs::read_to_string(&package_json_path).await.unwrap();
        let json: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(json["hpm_package"], "test-package");
    }
}
