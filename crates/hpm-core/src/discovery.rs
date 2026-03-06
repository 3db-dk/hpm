use hpm_config::ProjectsConfig;
use hpm_package::PackageManifest;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

#[derive(Debug, Clone)]
pub struct DiscoveredProject {
    pub path: PathBuf,
    pub manifest: PackageManifest,
}

#[derive(Debug)]
pub struct ProjectDiscovery {
    config: ProjectsConfig,
}

impl ProjectDiscovery {
    pub fn new(config: ProjectsConfig) -> Self {
        Self { config }
    }

    pub fn find_projects(&self) -> Result<Vec<DiscoveredProject>, DiscoveryError> {
        let mut projects = Vec::new();
        let mut seen_paths = HashSet::new();

        info!("Starting project discovery");

        // Process explicit project paths
        for path in &self.config.explicit_paths {
            if seen_paths.insert(path.clone()) {
                if let Some(project) = self.check_project_path(path)? {
                    projects.push(project);
                }
            }
        }

        // Process search root directories
        for root in &self.config.search_roots {
            let discovered = self.scan_directory(root, 0, &mut seen_paths)?;
            projects.extend(discovered);
        }

        info!("Found {} HPM-managed projects", projects.len());
        Ok(projects)
    }

    fn check_project_path(&self, path: &Path) -> Result<Option<DiscoveredProject>, DiscoveryError> {
        let manifest_path = path.join("hpm.toml");

        if !manifest_path.exists() {
            debug!("No hpm.toml found at {}", path.display());
            return Ok(None);
        }

        debug!("Found HPM project at {}", path.display());

        let manifest_content = std::fs::read_to_string(&manifest_path)
            .map_err(|e| DiscoveryError::ManifestRead(manifest_path.clone(), e.to_string()))?;

        let manifest: PackageManifest = toml::from_str(&manifest_content)
            .map_err(|e| DiscoveryError::ManifestParse(manifest_path.clone(), e.to_string()))?;

        Ok(Some(DiscoveredProject {
            path: path.to_path_buf(),
            manifest,
        }))
    }

    fn scan_directory(
        &self,
        dir: &PathBuf,
        depth: usize,
        seen_paths: &mut HashSet<PathBuf>,
    ) -> Result<Vec<DiscoveredProject>, DiscoveryError> {
        let mut projects = Vec::new();

        if depth >= self.config.max_search_depth {
            debug!(
                "Max search depth {} reached at {}",
                self.config.max_search_depth,
                dir.display()
            );
            return Ok(projects);
        }

        if !dir.exists() || !dir.is_dir() {
            debug!(
                "Directory does not exist or is not a directory: {}",
                dir.display()
            );
            return Ok(projects);
        }

        let entries = std::fs::read_dir(dir)
            .map_err(|e| DiscoveryError::DirectoryRead(dir.clone(), e.to_string()))?;

        for entry in entries.flatten() {
            let entry_path = entry.path();

            if !entry_path.is_dir() {
                continue;
            }

            // Get directory name for ignore pattern check
            let dir_name = entry_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");

            if self.config.should_ignore(dir_name) {
                debug!("Ignoring directory: {}", entry_path.display());
                continue;
            }

            // Skip if we've already seen this path
            if seen_paths.contains(&entry_path) {
                continue;
            }
            seen_paths.insert(entry_path.clone());

            // Check if this directory is a project
            if let Some(project) = self.check_project_path(&entry_path)? {
                projects.push(project);
            } else if depth + 1 < self.config.max_search_depth {
                // Recursively search subdirectories
                let sub_projects = self.scan_directory(&entry_path, depth + 1, seen_paths)?;
                projects.extend(sub_projects);
            }
        }

        Ok(projects)
    }

    pub fn discover_project_dependencies(&self, projects: &[DiscoveredProject]) -> Vec<String> {
        let mut dependencies = HashSet::new();

        for project in projects {
            if let Some(deps) = &project.manifest.dependencies {
                for dep_name in deps.keys() {
                    dependencies.insert(dep_name.clone());
                }
            }
        }

        dependencies.into_iter().collect()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error("Failed to read directory {0}: {1}")]
    DirectoryRead(PathBuf, String),

    #[error("Failed to read manifest {0}: {1}")]
    ManifestRead(PathBuf, String),

    #[error("Failed to parse manifest {0}: {1}")]
    ManifestParse(PathBuf, String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use hpm_config::ProjectsConfig;
    use tempfile::TempDir;

    #[test]
    fn project_discovery_empty_config() {
        let config = ProjectsConfig::default();
        let discovery = ProjectDiscovery::new(config);
        let projects = discovery.find_projects().unwrap();
        assert_eq!(projects.len(), 0);
    }

    #[test]
    fn project_discovery_explicit_path() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("test-project");
        std::fs::create_dir_all(&project_dir).unwrap();

        let manifest_content = r#"
[package]
name = "test-project"
version = "1.0.0"
description = "A test project"

[dependencies]
utility-nodes = { url = "https://example.com/packages/utility-nodes/1.0.0/utility-nodes-1.0.0.zip", version = "1.0.0" }
"#;
        std::fs::write(project_dir.join("hpm.toml"), manifest_content).unwrap();

        let mut config = ProjectsConfig::default();
        config.add_explicit_path(project_dir.clone());

        let discovery = ProjectDiscovery::new(config);
        let projects = discovery.find_projects().unwrap();

        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].manifest.package.name, "test-project");
        assert_eq!(projects[0].path, project_dir);
    }

    #[test]
    fn project_discovery_search_root() {
        let temp_dir = TempDir::new().unwrap();
        let projects_root = temp_dir.path().join("projects");
        std::fs::create_dir_all(&projects_root).unwrap();

        // Create multiple projects
        for i in 1..=3 {
            let project_dir = projects_root.join(format!("project-{}", i));
            std::fs::create_dir_all(&project_dir).unwrap();

            let manifest_content = format!(
                r#"
[package]
name = "project-{}"
version = "1.0.0"
description = "Test project {}"
"#,
                i, i
            );
            std::fs::write(project_dir.join("hpm.toml"), manifest_content).unwrap();
        }

        // Create ignored directory
        let ignored_dir = projects_root.join(".git");
        std::fs::create_dir_all(&ignored_dir).unwrap();
        std::fs::write(
            ignored_dir.join("hpm.toml"),
            "[package]\nname=\"ignored\"\nversion=\"1.0.0\"",
        )
        .unwrap();

        let mut config = ProjectsConfig::default();
        config.add_search_root(projects_root);

        let discovery = ProjectDiscovery::new(config);
        let projects = discovery.find_projects().unwrap();

        assert_eq!(projects.len(), 3);

        let project_names: Vec<_> = projects
            .iter()
            .map(|p| p.manifest.package.name.clone())
            .collect();
        assert!(project_names.contains(&"project-1".to_string()));
        assert!(project_names.contains(&"project-2".to_string()));
        assert!(project_names.contains(&"project-3".to_string()));
    }

    #[test]
    fn project_discovery_max_depth() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().join("root");

        // Create nested project beyond max depth
        let deep_project = root
            .join("level1")
            .join("level2")
            .join("level3")
            .join("deep-project");
        std::fs::create_dir_all(&deep_project).unwrap();
        std::fs::write(
            deep_project.join("hpm.toml"),
            "[package]\nname=\"deep\"\nversion=\"1.0.0\"",
        )
        .unwrap();

        // Create shallow project within max depth
        let shallow_project = root.join("shallow-project");
        std::fs::create_dir_all(&shallow_project).unwrap();
        std::fs::write(
            shallow_project.join("hpm.toml"),
            "[package]\nname=\"shallow\"\nversion=\"1.0.0\"",
        )
        .unwrap();

        let mut config = ProjectsConfig::default();
        config.add_search_root(root);
        config.max_search_depth = 2; // Should find shallow but not deep

        let discovery = ProjectDiscovery::new(config);
        let projects = discovery.find_projects().unwrap();

        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].manifest.package.name, "shallow");
    }

    #[test]
    fn discover_project_dependencies() {
        let temp_dir = TempDir::new().unwrap();

        let manifest1 = hpm_package::PackageManifest::new(
            "project-1".to_string(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );

        let mut manifest2 = hpm_package::PackageManifest::new(
            "project-2".to_string(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );

        // Add dependencies to project-2
        let mut deps = indexmap::IndexMap::new();
        deps.insert(
            "utility-nodes".to_string(),
            hpm_package::DependencySpec::Url {
                url: "https://example.com/packages/utility-nodes/1.0.0/utility-nodes-1.0.0.zip"
                    .to_string(),
                version: "1.0.0".to_string(),
                optional: false,
            },
        );
        deps.insert(
            "material-lib".to_string(),
            hpm_package::DependencySpec::Path {
                path: "../material-lib".to_string(),
                optional: false,
            },
        );
        manifest2.dependencies = Some(deps);

        let projects = vec![
            DiscoveredProject {
                path: temp_dir.path().join("project-1"),
                manifest: manifest1,
            },
            DiscoveredProject {
                path: temp_dir.path().join("project-2"),
                manifest: manifest2,
            },
        ];

        let config = ProjectsConfig::default();
        let discovery = ProjectDiscovery::new(config);
        let deps = discovery.discover_project_dependencies(&projects);

        assert_eq!(deps.len(), 2);
        assert!(deps.contains(&"utility-nodes".to_string()));
        assert!(deps.contains(&"material-lib".to_string()));
    }
}
