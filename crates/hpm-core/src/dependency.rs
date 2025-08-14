use crate::discovery::DiscoveredProject;
use crate::storage::{InstalledPackage, StorageManager};
use hpm_package::DependencySpec;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use tracing::{debug, info, warn};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PackageId {
    pub name: String,
    pub version: String,
}

impl PackageId {
    pub fn new(name: String, version: String) -> Self {
        Self { name, version }
    }

    pub fn identifier(&self) -> String {
        format!("{}@{}", self.name, self.version)
    }
}

impl From<&InstalledPackage> for PackageId {
    fn from(package: &InstalledPackage) -> Self {
        Self::new(package.name.clone(), package.version.clone())
    }
}

#[derive(Debug, Clone)]
pub struct PackageNode {
    pub id: PackageId,
    pub installed_package: Option<InstalledPackage>,
    pub required_by_projects: Vec<PathBuf>,
    pub is_root: bool,
}

#[derive(Debug)]
pub struct DependencyGraph {
    nodes: HashMap<PackageId, PackageNode>,
    edges: HashMap<PackageId, HashSet<PackageId>>, // package -> its dependencies
    reverse_edges: HashMap<PackageId, HashSet<PackageId>>, // package -> packages that depend on it
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: HashMap::new(),
            reverse_edges: HashMap::new(),
        }
    }

    pub fn add_node(&mut self, node: PackageNode) {
        let id = node.id.clone();
        self.nodes.insert(id.clone(), node);
        self.edges.entry(id.clone()).or_default();
        self.reverse_edges.entry(id).or_default();
    }

    pub fn add_dependency(&mut self, package: &PackageId, dependency: &PackageId) {
        self.edges
            .entry(package.clone())
            .or_default()
            .insert(dependency.clone());

        self.reverse_edges
            .entry(dependency.clone())
            .or_default()
            .insert(package.clone());
    }

    pub fn mark_reachable_from_roots(&self, roots: &[PackageId]) -> HashSet<PackageId> {
        let mut reachable = HashSet::new();
        let mut queue = VecDeque::from(roots.to_vec());

        while let Some(package_id) = queue.pop_front() {
            if reachable.insert(package_id.clone()) {
                // Add all dependencies to the queue
                if let Some(dependencies) = self.edges.get(&package_id) {
                    for dep in dependencies {
                        if !reachable.contains(dep) {
                            queue.push_back(dep.clone());
                        }
                    }
                }
            }
        }

        reachable
    }

    pub fn get_orphaned_packages(&self, needed_packages: &HashSet<PackageId>) -> Vec<PackageId> {
        self.nodes
            .keys()
            .filter(|id| !needed_packages.contains(id))
            .cloned()
            .collect()
    }

    pub fn get_package_dependents(&self, package: &PackageId) -> Vec<PackageId> {
        self.reverse_edges
            .get(package)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect()
    }

    pub fn nodes(&self) -> &HashMap<PackageId, PackageNode> {
        &self.nodes
    }

    pub fn has_cycles(&self) -> Vec<Vec<PackageId>> {
        let mut cycles = Vec::new();
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();
        let mut path = Vec::new();

        for package_id in self.nodes.keys() {
            if !visited.contains(package_id) {
                self.dfs_cycle_detection(
                    package_id,
                    &mut visited,
                    &mut rec_stack,
                    &mut path,
                    &mut cycles,
                );
            }
        }

        cycles
    }

    fn dfs_cycle_detection(
        &self,
        package_id: &PackageId,
        visited: &mut HashSet<PackageId>,
        rec_stack: &mut HashSet<PackageId>,
        path: &mut Vec<PackageId>,
        cycles: &mut Vec<Vec<PackageId>>,
    ) {
        visited.insert(package_id.clone());
        rec_stack.insert(package_id.clone());
        path.push(package_id.clone());

        if let Some(dependencies) = self.edges.get(package_id) {
            for dep in dependencies {
                if !visited.contains(dep) {
                    self.dfs_cycle_detection(dep, visited, rec_stack, path, cycles);
                } else if rec_stack.contains(dep) {
                    // Found a cycle - extract it from the path
                    if let Some(cycle_start) = path.iter().position(|p| p == dep) {
                        let cycle = path[cycle_start..].to_vec();
                        cycles.push(cycle);
                    }
                }
            }
        }

        rec_stack.remove(package_id);
        path.pop();
    }
}

#[derive(Debug)]
pub struct DependencyResolver {
    storage_manager: std::sync::Arc<StorageManager>,
}

impl DependencyResolver {
    pub fn new(storage_manager: std::sync::Arc<StorageManager>) -> Self {
        Self { storage_manager }
    }

    pub async fn build_dependency_graph(
        &self,
        projects: &[DiscoveredProject],
    ) -> Result<DependencyGraph, DependencyError> {
        let mut graph = DependencyGraph::new();
        let installed_packages = self
            .storage_manager
            .list_installed()
            .map_err(|e| DependencyError::StorageRead(e.to_string()))?;

        info!("Building dependency graph from {} projects", projects.len());

        // Create a map of installed packages for quick lookup
        let installed_map: HashMap<String, &InstalledPackage> = installed_packages
            .iter()
            .map(|pkg| (pkg.name.clone(), pkg))
            .collect();

        // Process each project
        for project in projects {
            self.process_project_dependencies(
                project,
                &installed_map,
                &mut graph,
                &mut HashSet::new(), // visited set for cycle detection
            )?;
        }

        // Detect and warn about cycles
        let cycles = graph.has_cycles();
        if !cycles.is_empty() {
            warn!("Detected {} dependency cycles", cycles.len());
            for (i, cycle) in cycles.iter().enumerate() {
                let cycle_str = cycle
                    .iter()
                    .map(|id| id.identifier())
                    .collect::<Vec<_>>()
                    .join(" -> ");
                warn!("Cycle {}: {}", i + 1, cycle_str);
            }
        }

        info!("Built dependency graph with {} packages", graph.nodes.len());
        Ok(graph)
    }

    fn process_project_dependencies(
        &self,
        project: &DiscoveredProject,
        installed_map: &HashMap<String, &InstalledPackage>,
        graph: &mut DependencyGraph,
        visited: &mut HashSet<String>,
    ) -> Result<(), DependencyError> {
        if let Some(dependencies) = &project.manifest.dependencies {
            for (dep_name, dep_spec) in dependencies {
                // Skip if we've already processed this dependency in this traversal
                if visited.contains(dep_name) {
                    continue;
                }
                visited.insert(dep_name.clone());

                if let Some(installed_package) = installed_map.get(dep_name) {
                    let package_id = PackageId::from(*installed_package);

                    // Add or update the package node
                    if let Some(existing_node) = graph.nodes.get_mut(&package_id) {
                        // Add this project to the list of projects requiring this package
                        if !existing_node.required_by_projects.contains(&project.path) {
                            existing_node
                                .required_by_projects
                                .push(project.path.clone());
                        }
                        existing_node.is_root = true;
                    } else {
                        // Create new package node
                        let node = PackageNode {
                            id: package_id.clone(),
                            installed_package: Some((*installed_package).clone()),
                            required_by_projects: vec![project.path.clone()],
                            is_root: true,
                        };
                        graph.add_node(node);
                    }

                    // Recursively process this package's dependencies
                    self.process_package_dependencies(
                        installed_package,
                        installed_map,
                        graph,
                        visited,
                        &package_id,
                    )?;
                } else {
                    // Package is required but not installed - could be an issue
                    debug!(
                        "Project {} requires {} but it's not installed",
                        project.path.display(),
                        dep_name
                    );

                    // Create a placeholder node for missing package
                    let version = self.extract_version_from_spec(dep_spec);
                    let package_id = PackageId::new(dep_name.clone(), version);

                    let node = PackageNode {
                        id: package_id,
                        installed_package: None,
                        required_by_projects: vec![project.path.clone()],
                        is_root: true,
                    };
                    graph.add_node(node);
                }
            }
        }

        Ok(())
    }

    fn process_package_dependencies(
        &self,
        package: &InstalledPackage,
        installed_map: &HashMap<String, &InstalledPackage>,
        graph: &mut DependencyGraph,
        visited: &mut HashSet<String>,
        parent_id: &PackageId,
    ) -> Result<(), DependencyError> {
        if let Some(dependencies) = &package.manifest.dependencies {
            for (dep_name, dep_spec) in dependencies {
                // Skip if we've already processed this dependency
                if visited.contains(dep_name) {
                    continue;
                }

                if let Some(dep_package) = installed_map.get(dep_name) {
                    let dep_id = PackageId::from(*dep_package);

                    // Add dependency edge
                    graph.add_dependency(parent_id, &dep_id);

                    // Add or update dependency node
                    if !graph.nodes.contains_key(&dep_id) {
                        let node = PackageNode {
                            id: dep_id.clone(),
                            installed_package: Some((*dep_package).clone()),
                            required_by_projects: vec![],
                            is_root: false,
                        };
                        graph.add_node(node);
                    }

                    // Recursively process transitive dependencies
                    visited.insert(dep_name.clone());
                    self.process_package_dependencies(
                        dep_package,
                        installed_map,
                        graph,
                        visited,
                        &dep_id,
                    )?;
                    visited.remove(dep_name);
                } else {
                    // Missing transitive dependency
                    debug!(
                        "Package {} requires {} but it's not installed",
                        package.identifier(),
                        dep_name
                    );

                    let version = self.extract_version_from_spec(dep_spec);
                    let dep_id = PackageId::new(dep_name.clone(), version);

                    graph.add_dependency(parent_id, &dep_id);

                    if !graph.nodes.contains_key(&dep_id) {
                        let node = PackageNode {
                            id: dep_id,
                            installed_package: None,
                            required_by_projects: vec![],
                            is_root: false,
                        };
                        graph.add_node(node);
                    }
                }
            }
        }

        Ok(())
    }

    fn extract_version_from_spec(&self, spec: &DependencySpec) -> String {
        match spec {
            DependencySpec::Simple(version) => version.clone(),
            DependencySpec::Detailed {
                version: Some(v), ..
            } => v.clone(),
            DependencySpec::Detailed { version: None, .. } => "unknown".to_string(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DependencyError {
    #[error("Storage read error: {0}")]
    StorageRead(String),

    #[error("Dependency resolution error: {0}")]
    ResolutionError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StorageManager;
    use hpm_config::StorageConfig;
    use tempfile::TempDir;

    fn create_test_storage_manager() -> std::sync::Arc<StorageManager> {
        let temp_dir = TempDir::new().unwrap();
        let storage_config = StorageConfig {
            home_dir: temp_dir.path().to_path_buf(),
            cache_dir: temp_dir.path().join("cache"),
            packages_dir: temp_dir.path().join("packages"),
            registry_cache_dir: temp_dir.path().join("registry"),
        };

        std::sync::Arc::new(StorageManager::new(storage_config).unwrap())
    }

    #[test]
    fn package_id_creation() {
        let package_id = PackageId::new("test-package".to_string(), "1.0.0".to_string());
        assert_eq!(package_id.identifier(), "test-package@1.0.0");
    }

    #[test]
    fn dependency_graph_basic_operations() {
        let mut graph = DependencyGraph::new();

        let pkg1 = PackageId::new("package1".to_string(), "1.0.0".to_string());
        let pkg2 = PackageId::new("package2".to_string(), "1.0.0".to_string());

        let node1 = PackageNode {
            id: pkg1.clone(),
            installed_package: None,
            required_by_projects: vec![],
            is_root: true,
        };

        let node2 = PackageNode {
            id: pkg2.clone(),
            installed_package: None,
            required_by_projects: vec![],
            is_root: false,
        };

        graph.add_node(node1);
        graph.add_node(node2);
        graph.add_dependency(&pkg1, &pkg2);

        assert_eq!(graph.nodes.len(), 2);

        let reachable = graph.mark_reachable_from_roots(std::slice::from_ref(&pkg1));
        assert_eq!(reachable.len(), 2);
        assert!(reachable.contains(&pkg1));
        assert!(reachable.contains(&pkg2));
    }

    #[test]
    fn dependency_graph_orphan_detection() {
        let mut graph = DependencyGraph::new();

        let pkg1 = PackageId::new("needed".to_string(), "1.0.0".to_string());
        let pkg2 = PackageId::new("orphan".to_string(), "1.0.0".to_string());

        let node1 = PackageNode {
            id: pkg1.clone(),
            installed_package: None,
            required_by_projects: vec![],
            is_root: true,
        };

        let node2 = PackageNode {
            id: pkg2.clone(),
            installed_package: None,
            required_by_projects: vec![],
            is_root: false,
        };

        graph.add_node(node1);
        graph.add_node(node2);

        let needed = vec![pkg1].into_iter().collect();
        let orphans = graph.get_orphaned_packages(&needed);

        assert_eq!(orphans.len(), 1);
        assert!(orphans.contains(&pkg2));
    }

    #[test]
    fn dependency_graph_cycle_detection() {
        let mut graph = DependencyGraph::new();

        let pkg1 = PackageId::new("package1".to_string(), "1.0.0".to_string());
        let pkg2 = PackageId::new("package2".to_string(), "1.0.0".to_string());

        // Create nodes
        for pkg in [&pkg1, &pkg2] {
            let node = PackageNode {
                id: pkg.clone(),
                installed_package: None,
                required_by_projects: vec![],
                is_root: false,
            };
            graph.add_node(node);
        }

        // Create cycle: pkg1 -> pkg2 -> pkg1
        graph.add_dependency(&pkg1, &pkg2);
        graph.add_dependency(&pkg2, &pkg1);

        let cycles = graph.has_cycles();
        assert!(!cycles.is_empty());
    }

    #[tokio::test]
    async fn dependency_resolver_empty_projects() {
        let storage_manager = create_test_storage_manager();
        let resolver = DependencyResolver::new(storage_manager);

        let projects = vec![];
        let graph = resolver.build_dependency_graph(&projects).await.unwrap();

        assert_eq!(graph.nodes.len(), 0);
    }
}
