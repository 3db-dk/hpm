//! Dependency graph for HPM packages.
//!
//! Backed by [`petgraph::graph::DiGraph`]: node data is `PackageNode`,
//! edges are unlabelled (`()`) and run from a package to one of its
//! dependencies. A `PackageId -> NodeIndex` side map provides O(1)
//! lookup from the public key type.
//!
//! Reachability (used by `hpm clean` orphan detection) and cycle
//! detection (used to warn on cyclic dependency declarations) reuse
//! `petgraph::visit::Bfs` and `petgraph::algo::tarjan_scc` rather
//! than hand-rolled DFS.
use crate::discovery::DiscoveredProject;
use crate::storage::{InstalledPackage, StorageManager};
use hpm_package::DependencySpec;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::Bfs;
use std::collections::{HashMap, HashSet};
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
        Self::new(
            package.manifest.package.slug().to_string(),
            package.version.clone(),
        )
    }
}

#[derive(Debug, Clone)]
pub struct PackageNode {
    pub id: PackageId,
    pub installed_package: Option<InstalledPackage>,
    pub required_by_projects: Vec<PathBuf>,
    pub is_root: bool,
}

#[derive(Debug, Default)]
pub struct DependencyGraph {
    graph: DiGraph<PackageNode, ()>,
    index_of: HashMap<PackageId, NodeIndex>,
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a node, or no-op if a node with the same `PackageId` already
    /// exists. Existing nodes are *not* replaced — the graph's node data is
    /// considered immutable from outside; updates happen via mutable access
    /// in `node_mut`.
    pub fn add_node(&mut self, node: PackageNode) {
        if self.index_of.contains_key(&node.id) {
            return;
        }
        let id = node.id.clone();
        let idx = self.graph.add_node(node);
        self.index_of.insert(id, idx);
    }

    /// Add a directed edge `package -> dependency`. Both endpoints must
    /// already be present as nodes; missing endpoints are silently skipped,
    /// preserving the pre-petgraph behaviour where `add_dependency` couldn't
    /// fail.
    pub fn add_dependency(&mut self, package: &PackageId, dependency: &PackageId) {
        let (Some(&from), Some(&to)) = (self.index_of.get(package), self.index_of.get(dependency))
        else {
            return;
        };
        if !self.graph.contains_edge(from, to) {
            self.graph.add_edge(from, to, ());
        }
    }

    /// Mark a node as a root, recording the project that pulls it in.
    /// Used internally by [`DependencyResolver`] when the same package
    /// surfaces as both a transitive dep and a direct project dep.
    pub fn node_mut(&mut self, id: &PackageId) -> Option<&mut PackageNode> {
        self.index_of.get(id).map(|&idx| &mut self.graph[idx])
    }

    /// BFS from each root, returning every `PackageId` reachable along
    /// dependency edges. The roots themselves are included.
    pub fn mark_reachable_from_roots(&self, roots: &[PackageId]) -> HashSet<PackageId> {
        let mut reachable = HashSet::new();
        for root in roots {
            let Some(&start) = self.index_of.get(root) else {
                continue;
            };
            let mut bfs = Bfs::new(&self.graph, start);
            while let Some(idx) = bfs.next(&self.graph) {
                reachable.insert(self.graph[idx].id.clone());
            }
        }
        reachable
    }

    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    pub fn nodes(&self) -> impl Iterator<Item = &PackageNode> {
        self.graph.node_weights()
    }

    /// Strongly connected components of size > 1 (and self-loops) are cycles.
    /// Returned cycles list `PackageId`s in the order petgraph emits them
    /// inside each SCC; we don't promise the cycle is rotated to any
    /// particular starting node.
    pub fn has_cycles(&self) -> Vec<Vec<PackageId>> {
        petgraph::algo::tarjan_scc(&self.graph)
            .into_iter()
            .filter_map(|component| {
                let is_cycle = component.len() > 1
                    || component
                        .first()
                        .is_some_and(|&n| self.graph.contains_edge(n, n));
                if !is_cycle {
                    return None;
                }
                Some(
                    component
                        .into_iter()
                        .map(|idx| self.graph[idx].id.clone())
                        .collect(),
                )
            })
            .collect()
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
            .map_err(DependencyError::from)?;

        info!("Building dependency graph from {} projects", projects.len());

        // Create a map of installed packages for quick lookup
        // Use &str keys to avoid cloning package names
        let installed_map: HashMap<&str, &InstalledPackage> = installed_packages
            .iter()
            .map(|pkg| (pkg.manifest.package.slug(), pkg))
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

        info!(
            "Built dependency graph with {} packages",
            graph.node_count()
        );
        Ok(graph)
    }

    fn process_project_dependencies<'a>(
        &self,
        project: &'a DiscoveredProject,
        installed_map: &HashMap<&str, &'a InstalledPackage>,
        graph: &mut DependencyGraph,
        visited: &mut HashSet<&'a str>,
    ) -> Result<(), DependencyError> {
        for (dep_name, dep_spec) in &project.manifest.dependencies {
            if visited.contains(dep_name.as_str()) {
                continue;
            }
            visited.insert(dep_name.as_str());

            let (package_id, installed_package) =
                self.resolve_dependency(dep_name, dep_spec, installed_map);

            // For project dependencies, mark as root and track requiring project
            self.ensure_node(
                graph,
                package_id.clone(),
                installed_package.cloned(),
                Some(&project.path),
                true, // is_root
            );

            if installed_package.is_none() {
                debug!(
                    "Project {} requires {} but it's not installed",
                    project.path.display(),
                    dep_name
                );
                continue;
            }

            // Recursively process transitive dependencies
            self.process_transitive_dependencies(
                installed_package.unwrap(),
                installed_map,
                graph,
                visited,
                &package_id,
            )?;
        }

        Ok(())
    }

    fn process_transitive_dependencies<'a>(
        &self,
        package: &'a InstalledPackage,
        installed_map: &HashMap<&str, &'a InstalledPackage>,
        graph: &mut DependencyGraph,
        visited: &mut HashSet<&'a str>,
        parent_id: &PackageId,
    ) -> Result<(), DependencyError> {
        for (dep_name, dep_spec) in &package.manifest.dependencies {
            if visited.contains(dep_name.as_str()) {
                continue;
            }

            let (dep_id, dep_package) = self.resolve_dependency(dep_name, dep_spec, installed_map);

            // Add node if not exists (transitive deps are not roots). Node
            // must exist before add_dependency, which silently skips edges
            // with missing endpoints.
            self.ensure_node(
                graph,
                dep_id.clone(),
                dep_package.cloned(),
                None,  // no project path for transitive deps
                false, // not a root
            );
            graph.add_dependency(parent_id, &dep_id);

            if let Some(dep_pkg) = dep_package {
                // Recursively process with cycle prevention
                visited.insert(dep_name.as_str());
                self.process_transitive_dependencies(
                    dep_pkg,
                    installed_map,
                    graph,
                    visited,
                    &dep_id,
                )?;
                visited.remove(dep_name.as_str());
            } else {
                debug!(
                    "Package {} requires {} but it's not installed",
                    package.identifier(),
                    dep_name
                );
            }
        }

        Ok(())
    }

    /// Resolve a dependency name to its PackageId and optional installed package.
    fn resolve_dependency<'a>(
        &self,
        dep_name: &str,
        dep_spec: &DependencySpec,
        installed_map: &HashMap<&str, &'a InstalledPackage>,
    ) -> (PackageId, Option<&'a InstalledPackage>) {
        if let Some(installed) = installed_map.get(dep_name) {
            (PackageId::from(*installed), Some(*installed))
        } else {
            let version = self.extract_version_from_spec(dep_spec);
            (PackageId::new(dep_name.to_string(), version), None)
        }
    }

    /// Ensure a node exists in the graph, updating it if it already exists.
    fn ensure_node(
        &self,
        graph: &mut DependencyGraph,
        id: PackageId,
        installed_package: Option<InstalledPackage>,
        project_path: Option<&PathBuf>,
        is_root: bool,
    ) {
        if let Some(existing) = graph.node_mut(&id) {
            if is_root {
                existing.is_root = true;
            }
            if let Some(path) = project_path
                && !existing.required_by_projects.contains(path)
            {
                existing.required_by_projects.push(path.clone());
            }
        } else {
            graph.add_node(PackageNode {
                id: id.clone(),
                installed_package,
                required_by_projects: project_path.map(|p| vec![p.clone()]).unwrap_or_default(),
                is_root,
            });
        }
    }

    fn extract_version_from_spec(&self, spec: &DependencySpec) -> String {
        match spec {
            DependencySpec::Url { version, .. } | DependencySpec::Registry { version, .. } => {
                version.clone()
            }
            DependencySpec::Path { .. } => "local".to_string(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DependencyError {
    #[error(transparent)]
    Storage(#[from] Box<crate::storage::StorageError>),
}

impl From<crate::storage::StorageError> for DependencyError {
    fn from(err: crate::storage::StorageError) -> Self {
        Self::Storage(Box::new(err))
    }
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

    fn make_node(name: &str, version: &str, is_root: bool) -> PackageNode {
        PackageNode {
            id: PackageId::new(name.to_string(), version.to_string()),
            installed_package: None,
            required_by_projects: vec![],
            is_root,
        }
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

        graph.add_node(make_node("package1", "1.0.0", true));
        graph.add_node(make_node("package2", "1.0.0", false));
        graph.add_dependency(&pkg1, &pkg2);

        assert_eq!(graph.node_count(), 2);

        let reachable = graph.mark_reachable_from_roots(std::slice::from_ref(&pkg1));
        assert_eq!(reachable.len(), 2);
        assert!(reachable.contains(&pkg1));
        assert!(reachable.contains(&pkg2));
    }

    #[test]
    fn dependency_graph_cycle_detection() {
        let mut graph = DependencyGraph::new();
        let pkg1 = PackageId::new("package1".to_string(), "1.0.0".to_string());
        let pkg2 = PackageId::new("package2".to_string(), "1.0.0".to_string());

        graph.add_node(make_node("package1", "1.0.0", false));
        graph.add_node(make_node("package2", "1.0.0", false));

        // Create cycle: pkg1 -> pkg2 -> pkg1
        graph.add_dependency(&pkg1, &pkg2);
        graph.add_dependency(&pkg2, &pkg1);

        let cycles = graph.has_cycles();
        // tarjan_scc reports the cycle as a single SCC of size 2.
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].len(), 2);
    }

    #[test]
    fn dependency_graph_acyclic_chain_has_no_cycle() {
        // A -> B -> C is one SCC per node but no cycles.
        let mut graph = DependencyGraph::new();
        let a = PackageId::new("a".to_string(), "1.0.0".to_string());
        let b = PackageId::new("b".to_string(), "1.0.0".to_string());
        let c = PackageId::new("c".to_string(), "1.0.0".to_string());
        graph.add_node(make_node("a", "1.0.0", true));
        graph.add_node(make_node("b", "1.0.0", false));
        graph.add_node(make_node("c", "1.0.0", false));
        graph.add_dependency(&a, &b);
        graph.add_dependency(&b, &c);
        assert!(graph.has_cycles().is_empty());
    }

    #[tokio::test]
    async fn dependency_resolver_empty_projects() {
        let storage_manager = create_test_storage_manager();
        let resolver = DependencyResolver::new(storage_manager);

        let projects = vec![];
        let graph = resolver.build_dependency_graph(&projects).await.unwrap();

        assert_eq!(graph.node_count(), 0);
    }
}
