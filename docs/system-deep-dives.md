# HPM System Deep Dives

This document provides comprehensive deep dives into HPM's core systems, explaining the algorithms, data structures, and implementation details that power the package manager. These technical deep dives are intended for developers who want to understand, extend, or contribute to HPM's core functionality.

## Table of Contents

1. [Dependency Resolution System](#dependency-resolution-system)
2. [Project-Aware Cleanup System](#project-aware-cleanup-system)
3. [Python Integration Architecture](#python-integration-architecture)
4. [Registry and Network Architecture](#registry-and-network-architecture)
5. [Storage Management System](#storage-management-system)
6. [Configuration Management](#configuration-management)
7. [Error Handling and Recovery](#error-handling-and-recovery)
8. [Performance Optimization](#performance-optimization)

## Dependency Resolution System

HPM's dependency resolution system is inspired by PubGrub (used by Dart's pub) but adapted for the unique challenges of Houdini package management, including Python dependency integration and complex version constraints.

### Theoretical Foundation

#### PubGrub Algorithm Adaptation

The core resolution algorithm follows these principles:

1. **Incremental Solving**: Build solutions incrementally, backtracking when conflicts arise
2. **Conflict Learning**: Remember conflicts to avoid repeating failed resolution paths
3. **Version Selection**: Choose versions that satisfy all constraints with minimal conflicts
4. **Transitive Resolution**: Recursively resolve dependencies of dependencies

#### Mathematical Model

The dependency resolution problem can be modeled as a **Constraint Satisfaction Problem (CSP)**:

- **Variables**: Each package name represents a variable
- **Domains**: Each variable's domain is the set of available versions
- **Constraints**: Version requirements define relationships between variables

```text
Given:
  - P = {p₁, p₂, ..., pₙ} (set of packages)
  - V(pᵢ) = {v₁, v₂, ..., vₖ} (available versions for package pᵢ)
  - C = {c₁, c₂, ..., cₘ} (set of version constraints)

Find:
  - Assignment A: P → V such that ∀cᵢ ∈ C, cᵢ(A) = true
  - Minimize conflicts while maximizing version freshness
```

### Implementation Architecture

#### Core Data Structures

```rust
/// Central dependency resolution engine
pub struct DependencyResolver {
    /// Package registry client for version lookups
    registry_client: Arc<RegistryClient>,
    
    /// Cache for resolved package metadata
    package_cache: HashMap<PackageId, PackageMetadata>,
    
    /// Conflict learning database
    conflict_db: ConflictDatabase,
    
    /// Resolution configuration
    config: ResolverConfig,
}

/// Represents a specific package version constraint
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct VersionConstraint {
    /// Target package name
    pub package: String,
    
    /// Version requirement (e.g., "^1.2.0", ">=2.0.0")
    pub requirement: VersionRequirement,
    
    /// Source of this constraint (root, dependency, etc.)
    pub source: ConstraintSource,
}

/// Resolution state during incremental solving
#[derive(Debug, Clone)]
pub struct ResolutionState {
    /// Current partial solution
    assignments: HashMap<String, Version>,
    
    /// Remaining constraints to satisfy
    pending_constraints: Vec<VersionConstraint>,
    
    /// Conflicts discovered during resolution
    conflicts: Vec<DependencyConflict>,
    
    /// Resolution stack for backtracking
    decision_stack: Vec<DecisionPoint>,
}

/// Represents a decision point for backtracking
#[derive(Debug, Clone)]
pub struct DecisionPoint {
    /// Package being decided
    package: String,
    
    /// Chosen version
    version: Version,
    
    /// Alternative versions available
    alternatives: Vec<Version>,
    
    /// State before this decision
    previous_state: Box<ResolutionState>,
}
```

#### Resolution Algorithm Implementation

```rust
impl DependencyResolver {
    /// Main resolution entry point
    pub async fn resolve(
        &mut self,
        root_requirements: &[DependencyRequirement]
    ) -> Result<ResolutionResult, ResolutionError> {
        // Initialize resolution state
        let mut state = ResolutionState::new(root_requirements);
        
        // Main resolution loop with conflict learning
        loop {
            match self.attempt_resolution(&mut state).await {
                Ok(solution) => return Ok(solution),
                
                Err(ResolutionError::Conflict { conflict }) => {
                    // Learn from conflict to avoid repeating
                    self.learn_conflict(&conflict);
                    
                    // Attempt backtracking
                    if let Some(backtrack_point) = self.find_backtrack_point(&state, &conflict) {
                        state = self.backtrack_to(state, backtrack_point);
                    } else {
                        // No solution exists
                        return Err(ResolutionError::NoSolution { 
                            conflicts: state.conflicts 
                        });
                    }
                }
                
                Err(other_error) => return Err(other_error),
            }
        }
    }
    
    /// Attempt to complete resolution with current state
    async fn attempt_resolution(
        &mut self, 
        state: &mut ResolutionState
    ) -> Result<ResolutionResult, ResolutionError> {
        while !state.pending_constraints.is_empty() {
            // Select next constraint to resolve
            let constraint = self.select_constraint(state)?;
            
            // Find compatible versions
            let compatible_versions = self.find_compatible_versions(&constraint).await?;
            
            if compatible_versions.is_empty() {
                // Conflict detected - no compatible versions
                let conflict = self.analyze_conflict(state, &constraint).await;
                return Err(ResolutionError::Conflict { conflict });
            }
            
            // Choose best version (prefer latest stable)
            let chosen_version = self.select_best_version(&compatible_versions, &constraint)?;
            
            // Make assignment and update state
            self.make_assignment(state, &constraint.package, chosen_version).await?;
        }
        
        // All constraints satisfied
        self.build_solution(state)
    }
    
    /// Select next constraint to resolve (heuristic-based)
    fn select_constraint(&self, state: &ResolutionState) -> Result<VersionConstraint, ResolutionError> {
        // Priority heuristics:
        // 1. Root requirements (highest priority)
        // 2. Constraints with fewer compatible versions (fail-fast)
        // 3. Constraints from already-assigned packages
        
        let mut scored_constraints: Vec<_> = state.pending_constraints
            .iter()
            .map(|constraint| {
                let score = self.calculate_constraint_priority(constraint, state);
                (score, constraint.clone())
            })
            .collect();
        
        // Sort by priority (higher score = higher priority)
        scored_constraints.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        
        scored_constraints
            .first()
            .map(|(_, constraint)| constraint.clone())
            .ok_or(ResolutionError::InternalError {
                message: "No constraints to resolve".to_string()
            })
    }
    
    /// Make version assignment and propagate constraints
    async fn make_assignment(
        &mut self,
        state: &mut ResolutionState,
        package: &str,
        version: Version
    ) -> Result<(), ResolutionError> {
        // Record assignment
        state.assignments.insert(package.to_string(), version.clone());
        
        // Create decision point for backtracking
        let decision_point = DecisionPoint {
            package: package.to_string(),
            version: version.clone(),
            alternatives: vec![], // TODO: Store alternatives
            previous_state: Box::new(state.clone()),
        };
        state.decision_stack.push(decision_point);
        
        // Fetch package metadata to get dependencies
        let package_id = PackageId::new(package.to_string(), version.clone());
        let metadata = self.get_package_metadata(&package_id).await?;
        
        // Add new constraints from dependencies
        for dependency in &metadata.dependencies {
            let new_constraint = VersionConstraint {
                package: dependency.name.clone(),
                requirement: dependency.version_requirement.clone(),
                source: ConstraintSource::Dependency {
                    package: package.to_string(),
                    version: version.clone(),
                },
            };
            
            state.pending_constraints.push(new_constraint);
        }
        
        // Remove satisfied constraints
        state.pending_constraints.retain(|constraint| {
            if constraint.package == package {
                // Check if assignment satisfies constraint
                !constraint.requirement.satisfies(&version)
            } else {
                true
            }
        });
        
        Ok(())
    }
    
    /// Analyze conflict to understand root cause
    async fn analyze_conflict(
        &mut self,
        state: &ResolutionState,
        failing_constraint: &VersionConstraint
    ) -> DependencyConflict {
        // Find all constraints affecting the same package
        let conflicting_constraints: Vec<_> = state.pending_constraints
            .iter()
            .filter(|c| c.package == failing_constraint.package)
            .cloned()
            .collect();
        
        // Analyze version ranges to find intersection
        let version_analysis = self.analyze_version_ranges(&conflicting_constraints);
        
        // Generate resolution suggestions
        let suggestions = self.generate_resolution_suggestions(&conflicting_constraints);
        
        DependencyConflict {
            package_name: failing_constraint.package.clone(),
            conflicting_requirements: conflicting_constraints
                .into_iter()
                .map(|c| ConflictingRequirement {
                    requirement: c.requirement,
                    source: c.source,
                    optional: false, // TODO: Track optionality
                })
                .collect(),
            resolution_suggestions: suggestions,
        }
    }
}
```

#### Version Constraint Satisfaction

```rust
/// Advanced version requirement handling
#[derive(Debug, Clone, PartialEq)]
pub enum VersionRequirement {
    /// Exact version (=1.2.3)
    Exact(Version),
    
    /// Caret requirement (^1.2.3)
    Caret(Version),
    
    /// Tilde requirement (~1.2.3)
    Tilde(Version),
    
    /// Range requirement (>=1.0.0, <2.0.0)
    Range { 
        min: Option<Version>, 
        max: Option<Version> 
    },
    
    /// Union of requirements (>=1.0.0 || >=2.0.0)
    Union(Vec<VersionRequirement>),
    
    /// Intersection of requirements (>=1.0.0, <2.0.0)
    Intersection(Vec<VersionRequirement>),
}

impl VersionRequirement {
    /// Check if version satisfies requirement
    pub fn satisfies(&self, version: &Version) -> bool {
        match self {
            VersionRequirement::Exact(exact) => version == exact,
            
            VersionRequirement::Caret(base) => {
                // ^1.2.3 allows >=1.2.3, <2.0.0
                version >= base && version.major() == base.major()
            }
            
            VersionRequirement::Tilde(base) => {
                // ~1.2.3 allows >=1.2.3, <1.3.0
                version >= base 
                    && version.major() == base.major()
                    && version.minor() == base.minor()
            }
            
            VersionRequirement::Range { min, max } => {
                let min_ok = min.as_ref().map(|m| version >= m).unwrap_or(true);
                let max_ok = max.as_ref().map(|m| version < m).unwrap_or(true);
                min_ok && max_ok
            }
            
            VersionRequirement::Union(requirements) => {
                requirements.iter().any(|req| req.satisfies(version))
            }
            
            VersionRequirement::Intersection(requirements) => {
                requirements.iter().all(|req| req.satisfies(version))
            }
        }
    }
    
    /// Find intersection of two version requirements
    pub fn intersect(&self, other: &Self) -> Option<Self> {
        use VersionRequirement::*;
        
        match (self, other) {
            // Exact version intersections
            (Exact(a), Exact(b)) if a == b => Some(Exact(a.clone())),
            (Exact(a), other) | (other, Exact(a)) => {
                if other.satisfies(a) {
                    Some(Exact(a.clone()))
                } else {
                    None
                }
            }
            
            // Range intersections
            (Range { min: min1, max: max1 }, Range { min: min2, max: max2 }) => {
                let new_min = match (min1, min2) {
                    (Some(a), Some(b)) => Some(std::cmp::max(a, b).clone()),
                    (Some(a), None) | (None, Some(a)) => Some(a.clone()),
                    (None, None) => None,
                };
                
                let new_max = match (max1, max2) {
                    (Some(a), Some(b)) => Some(std::cmp::min(a, b).clone()),
                    (Some(a), None) | (None, Some(a)) => Some(a.clone()),
                    (None, None) => None,
                };
                
                // Check if range is valid
                if let (Some(min), Some(max)) = (&new_min, &new_max) {
                    if min >= max {
                        return None; // Empty intersection
                    }
                }
                
                Some(Range { min: new_min, max: new_max })
            }
            
            // Complex cases - convert to intersection
            _ => Some(Intersection(vec![self.clone(), other.clone()])),
        }
    }
}
```

### Conflict Learning and Backtracking

#### Conflict Database

```rust
/// Database for learning from resolution conflicts
#[derive(Debug, Default)]
pub struct ConflictDatabase {
    /// Learned conflicts mapped by root cause
    learned_conflicts: HashMap<ConflictSignature, LearnedConflict>,
    
    /// Statistics for conflict analysis
    statistics: ConflictStatistics,
}

/// Signature identifying a specific type of conflict
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct ConflictSignature {
    /// Packages involved in conflict
    packages: BTreeSet<String>,
    
    /// Types of constraints that conflict
    constraint_types: BTreeSet<ConstraintType>,
}

/// Information learned from resolving conflicts
#[derive(Debug, Clone)]
pub struct LearnedConflict {
    /// Assignments that lead to this conflict
    problematic_assignments: Vec<(String, Version)>,
    
    /// Known resolution strategies
    resolution_strategies: Vec<ResolutionStrategy>,
    
    /// Success rate of strategies
    strategy_success_rates: HashMap<ResolutionStrategy, f64>,
}

impl ConflictDatabase {
    /// Learn from a resolution conflict
    pub fn learn_conflict(&mut self, conflict: &DependencyConflict) {
        let signature = self.extract_signature(conflict);
        
        let learned = self.learned_conflicts
            .entry(signature)
            .or_insert_with(|| LearnedConflict::new());
        
        // Update learned information
        learned.record_conflict_instance(conflict);
        
        // Update statistics
        self.statistics.total_conflicts += 1;
    }
    
    /// Suggest resolution strategy based on learned conflicts
    pub fn suggest_resolution(&self, conflict: &DependencyConflict) -> Option<ResolutionStrategy> {
        let signature = self.extract_signature(conflict);
        
        self.learned_conflicts
            .get(&signature)
            .and_then(|learned| learned.best_strategy())
    }
    
    /// Extract conflict signature for indexing
    fn extract_signature(&self, conflict: &DependencyConflict) -> ConflictSignature {
        let packages = conflict.conflicting_requirements
            .iter()
            .map(|req| match &req.source {
                ConstraintSource::Root => "ROOT".to_string(),
                ConstraintSource::Dependency { package, .. } => package.clone(),
            })
            .collect();
        
        let constraint_types = conflict.conflicting_requirements
            .iter()
            .map(|req| classify_constraint_type(&req.requirement))
            .collect();
        
        ConflictSignature { packages, constraint_types }
    }
}
```

### Performance Optimizations

#### Caching Strategy

```rust
/// Multi-level caching for resolution performance
pub struct ResolutionCache {
    /// Package metadata cache (persistent)
    metadata_cache: LruCache<PackageId, PackageMetadata>,
    
    /// Version listing cache (time-based expiry)
    version_cache: TimedCache<String, Vec<Version>>,
    
    /// Partial resolution cache (session-based)
    resolution_cache: HashMap<ResolutionCacheKey, PartialResolution>,
}

/// Key for caching partial resolution results
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
struct ResolutionCacheKey {
    /// Root requirements (sorted for consistency)
    requirements: BTreeSet<DependencyRequirement>,
    
    /// Resolver configuration hash
    config_hash: u64,
}

impl ResolutionCache {
    /// Check if partial resolution is cached
    pub fn get_cached_resolution(&self, key: &ResolutionCacheKey) -> Option<&PartialResolution> {
        self.resolution_cache.get(key)
    }
    
    /// Cache partial resolution result
    pub fn cache_resolution(&mut self, key: ResolutionCacheKey, resolution: PartialResolution) {
        // Limit cache size to prevent memory issues
        if self.resolution_cache.len() >= 1000 {
            self.evict_oldest_resolutions();
        }
        
        self.resolution_cache.insert(key, resolution);
    }
    
    /// Evict oldest cached resolutions
    fn evict_oldest_resolutions(&mut self) {
        // Simple LRU eviction - could be more sophisticated
        let keys_to_remove: Vec<_> = self.resolution_cache
            .keys()
            .take(200) // Remove oldest 200 entries
            .cloned()
            .collect();
        
        for key in keys_to_remove {
            self.resolution_cache.remove(&key);
        }
    }
}
```

#### Parallel Resolution

```rust
impl DependencyResolver {
    /// Resolve multiple requirement sets in parallel
    pub async fn resolve_parallel(
        &mut self,
        requirement_sets: Vec<Vec<DependencyRequirement>>
    ) -> Vec<Result<ResolutionResult, ResolutionError>> {
        // Create semaphore to limit concurrent resolutions
        let semaphore = Arc::new(Semaphore::new(self.config.max_parallel_resolutions));
        
        let tasks: Vec<_> = requirement_sets
            .into_iter()
            .map(|requirements| {
                let resolver = self.clone(); // Clone resolver state
                let semaphore = semaphore.clone();
                
                tokio::spawn(async move {
                    let _permit = semaphore.acquire().await.unwrap();
                    resolver.resolve(&requirements).await
                })
            })
            .collect();
        
        // Wait for all resolutions to complete
        let results = try_join_all(tasks).await
            .map_err(|e| ResolutionError::ParallelExecution { 
                error: e.to_string() 
            })?;
        
        Ok(results)
    }
}
```

## Project-Aware Cleanup System

HPM's cleanup system implements sophisticated algorithms to safely identify and remove orphaned packages while preserving dependencies needed by active projects.

### Problem Statement and Approach

#### The Challenge

Traditional package managers often struggle with safe cleanup because they lack global visibility into package usage across multiple projects. HPM solves this by:

1. **Global Project Discovery**: Scanning configured paths to find all HPM-managed projects
2. **Dependency Graph Analysis**: Building complete dependency graphs including transitive dependencies
3. **Set-Based Orphan Detection**: Using mathematical set operations to identify truly orphaned packages
4. **Safety Guarantees**: Never removing packages that are transitively required

#### Mathematical Foundation

The cleanup problem can be formulated as:

```text
Given:
  - I = {i₁, i₂, ..., iₙ} (set of installed packages)
  - P = {p₁, p₂, ..., pₘ} (set of active projects)
  - D: P → 2^I (dependency function mapping projects to required packages)
  - T: I → 2^I (transitive dependency function)

Find:
  - O = I \ ⋃(p∈P) T(D(p)) (orphaned packages)

Where T(D(p)) is the transitive closure of dependencies for project p
```

### Implementation Architecture

#### Core Data Structures

```rust
/// Central cleanup coordination system
pub struct CleanupManager {
    /// Project discovery system
    discovery: ProjectDiscovery,
    
    /// Storage manager for package operations
    storage: StorageManager,
    
    /// Python environment cleanup
    python_cleaner: PythonCleanupAnalyzer,
    
    /// Configuration
    config: CleanupConfig,
}

/// Represents the complete dependency graph across all projects
#[derive(Debug)]
pub struct GlobalDependencyGraph {
    /// All packages in the graph
    packages: HashMap<PackageId, PackageNode>,
    
    /// Direct dependency edges
    edges: HashMap<PackageId, HashSet<PackageId>>,
    
    /// Reverse dependency edges (dependents)
    reverse_edges: HashMap<PackageId, HashSet<PackageId>>,
    
    /// Root packages (directly required by projects)
    roots: HashSet<PackageId>,
}

/// Node information in dependency graph
#[derive(Debug, Clone)]
pub struct PackageNode {
    /// Package identification
    pub id: PackageId,
    
    /// Installation status
    pub status: InstallationStatus,
    
    /// Projects that directly require this package
    pub direct_dependents: HashSet<String>,
    
    /// Installation metadata
    pub metadata: InstallationMetadata,
}

/// Cleanup analysis result
#[derive(Debug)]
pub struct CleanupAnalysis {
    /// Packages that can be safely removed
    pub orphaned_packages: Vec<OrphanedPackage>,
    
    /// Python environments that can be cleaned
    pub orphaned_python_envs: Vec<OrphanedVirtualEnv>,
    
    /// Total disk space that would be freed
    pub total_space_freed: u64,
    
    /// Analysis metadata
    pub analysis_metadata: AnalysisMetadata,
}
```

#### Project Discovery Algorithm

```rust
impl ProjectDiscovery {
    /// Comprehensive project discovery with caching and validation
    pub async fn discover_all_projects(&self) -> Result<Vec<DiscoveredProject>, DiscoveryError> {
        let mut discovered_projects = Vec::new();
        let mut visited_paths = HashSet::new();
        
        // Phase 1: Process explicit project paths
        for explicit_path in &self.config.explicit_paths {
            if let Some(project) = self.discover_single_project(explicit_path).await? {
                discovered_projects.push(project);
                visited_paths.insert(explicit_path.canonicalize()?);
            }
        }
        
        // Phase 2: Recursive search in search roots
        for search_root in &self.config.search_roots {
            let found_projects = self.recursive_search(
                search_root, 
                0, // Initial depth
                &mut visited_paths
            ).await?;
            
            discovered_projects.extend(found_projects);
        }
        
        // Phase 3: Validation and deduplication
        self.validate_and_deduplicate(discovered_projects).await
    }
    
    /// Recursive project search with depth limiting
    async fn recursive_search(
        &self,
        root_path: &Path,
        current_depth: usize,
        visited_paths: &mut HashSet<PathBuf>
    ) -> Result<Vec<DiscoveredProject>, DiscoveryError> {
        // Check depth limit
        if current_depth >= self.config.max_search_depth {
            return Ok(vec![]);
        }
        
        let mut found_projects = Vec::new();
        
        // Read directory contents
        let entries = match fs::read_dir(root_path).await {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                tracing::warn!("Permission denied accessing {}: {}", root_path.display(), e);
                return Ok(vec![]);
            }
            Err(e) => return Err(DiscoveryError::Io(e)),
        };
        
        let mut dir_entries = Vec::new();
        let mut entries = entries;
        while let Some(entry) = entries.next_entry().await? {
            dir_entries.push(entry);
        }
        
        // Process entries in parallel for better performance
        let search_tasks: Vec<_> = dir_entries
            .into_iter()
            .filter_map(|entry| {
                let path = entry.path();
                let canonical_path = path.canonicalize().ok()?;
                
                // Skip if already visited (handles symlinks)
                if visited_paths.contains(&canonical_path) {
                    return None;
                }
                
                // Skip ignored patterns
                if self.should_ignore(&path) {
                    return None;
                }
                
                // Mark as visited
                visited_paths.insert(canonical_path);
                
                Some(path)
            })
            .map(|path| {
                let discovery = self.clone();
                let mut visited_paths = visited_paths.clone();
                
                async move {
                    // Check if this directory contains an HPM project
                    if let Some(project) = discovery.discover_single_project(&path).await? {
                        return Ok(vec![project]);
                    }
                    
                    // If it's a directory, recurse
                    if path.is_dir() {
                        return discovery.recursive_search(&path, current_depth + 1, &mut visited_paths).await;
                    }
                    
                    Ok(vec![])
                }
            })
            .collect();
        
        // Execute searches concurrently
        let search_results: Result<Vec<_>, DiscoveryError> = try_join_all(search_tasks).await;
        
        // Flatten results
        for result in search_results? {
            found_projects.extend(result);
        }
        
        Ok(found_projects)
    }
    
    /// Check if path should be ignored based on patterns
    fn should_ignore(&self, path: &Path) -> bool {
        if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
            for pattern in &self.config.ignore_patterns {
                if glob_match(pattern, filename) {
                    return true;
                }
            }
        }
        false
    }
}
```

#### Dependency Graph Construction

```rust
impl GlobalDependencyGraph {
    /// Build complete dependency graph from discovered projects
    pub async fn build_from_projects(
        projects: &[DiscoveredProject],
        storage: &StorageManager
    ) -> Result<Self, DependencyError> {
        let mut graph = Self::new();
        
        // Phase 1: Add all direct dependencies as roots
        for project in projects {
            for dependency in &project.dependencies {
                let package_id = PackageId {
                    name: dependency.name.clone(),
                    version: dependency.resolved_version.clone(),
                };
                
                // Mark as root package
                graph.roots.insert(package_id.clone());
                
                // Add package node if not exists
                graph.ensure_package_node(package_id, project, storage).await?;
            }
        }
        
        // Phase 2: Resolve transitive dependencies iteratively
        let mut packages_to_process: VecDeque<_> = graph.roots.iter().cloned().collect();
        let mut processed = HashSet::new();
        
        while let Some(package_id) = packages_to_process.pop_front() {
            if processed.contains(&package_id) {
                continue;
            }
            processed.insert(package_id.clone());
            
            // Get package metadata to find dependencies
            let package_metadata = storage.get_package_metadata(&package_id).await?;
            
            // Process each dependency
            for dependency in &package_metadata.dependencies {
                let dep_package_id = PackageId {
                    name: dependency.name.clone(),
                    version: dependency.resolved_version.clone(),
                };
                
                // Add dependency edge
                graph.add_edge(package_id.clone(), dep_package_id.clone());
                
                // Ensure dependency node exists
                if !graph.packages.contains_key(&dep_package_id) {
                    graph.ensure_package_node(dep_package_id.clone(), project, storage).await?;
                    packages_to_process.push_back(dep_package_id);
                }
            }
        }
        
        // Phase 3: Build reverse edges for efficient traversal
        graph.build_reverse_edges();
        
        Ok(graph)
    }
    
    /// Calculate transitive closure of dependencies for root packages
    pub fn calculate_reachable_packages(&self) -> HashSet<PackageId> {
        let mut reachable = HashSet::new();
        let mut stack: Vec<_> = self.roots.iter().cloned().collect();
        
        // Depth-first traversal to find all reachable packages
        while let Some(package_id) = stack.pop() {
            if reachable.contains(&package_id) {
                continue;
            }
            
            reachable.insert(package_id.clone());
            
            // Add dependencies to stack
            if let Some(dependencies) = self.edges.get(&package_id) {
                for dep_id in dependencies {
                    if !reachable.contains(dep_id) {
                        stack.push(dep_id.clone());
                    }
                }
            }
        }
        
        reachable
    }
    
    /// Identify orphaned packages using set difference
    pub fn identify_orphaned_packages(
        &self,
        installed_packages: &HashSet<PackageId>
    ) -> Vec<PackageId> {
        let reachable = self.calculate_reachable_packages();
        
        // Orphaned = Installed - Reachable
        installed_packages
            .difference(&reachable)
            .cloned()
            .collect()
    }
    
    /// Detect cycles in dependency graph (diagnostic)
    pub fn detect_cycles(&self) -> Vec<Vec<PackageId>> {
        let mut cycles = Vec::new();
        let mut visited = HashSet::new();
        let mut recursion_stack = HashSet::new();
        let mut current_path = Vec::new();
        
        for package_id in self.packages.keys() {
            if !visited.contains(package_id) {
                self.dfs_cycle_detection(
                    package_id,
                    &mut visited,
                    &mut recursion_stack,
                    &mut current_path,
                    &mut cycles
                );
            }
        }
        
        cycles
    }
    
    /// Depth-first search for cycle detection
    fn dfs_cycle_detection(
        &self,
        package_id: &PackageId,
        visited: &mut HashSet<PackageId>,
        recursion_stack: &mut HashSet<PackageId>,
        current_path: &mut Vec<PackageId>,
        cycles: &mut Vec<Vec<PackageId>>
    ) {
        visited.insert(package_id.clone());
        recursion_stack.insert(package_id.clone());
        current_path.push(package_id.clone());
        
        if let Some(dependencies) = self.edges.get(package_id) {
            for dep_id in dependencies {
                if recursion_stack.contains(dep_id) {
                    // Cycle detected - extract cycle from current path
                    if let Some(cycle_start) = current_path.iter().position(|id| id == dep_id) {
                        let cycle = current_path[cycle_start..].to_vec();
                        cycles.push(cycle);
                    }
                } else if !visited.contains(dep_id) {
                    self.dfs_cycle_detection(dep_id, visited, recursion_stack, current_path, cycles);
                }
            }
        }
        
        recursion_stack.remove(package_id);
        current_path.pop();
    }
}
```

#### Cleanup Safety Verification

```rust
impl CleanupManager {
    /// Verify cleanup safety with multiple validation layers
    pub async fn verify_cleanup_safety(
        &self,
        cleanup_candidates: &[PackageId]
    ) -> Result<SafetyAnalysis, CleanupError> {
        let mut analysis = SafetyAnalysis::new();
        
        // Layer 1: Immediate dependency check
        for package_id in cleanup_candidates {
            let immediate_dependents = self.find_immediate_dependents(package_id).await?;
            if !immediate_dependents.is_empty() {
                analysis.add_safety_violation(SafetyViolation::HasDependents {
                    package: package_id.clone(),
                    dependents: immediate_dependents,
                });
            }
        }
        
        // Layer 2: Transitive dependency verification
        let dependency_graph = self.build_dependency_graph().await?;
        let reachable_packages = dependency_graph.calculate_reachable_packages();
        
        for package_id in cleanup_candidates {
            if reachable_packages.contains(package_id) {
                analysis.add_safety_violation(SafetyViolation::TransitivelyRequired {
                    package: package_id.clone(),
                    required_by: self.find_dependency_chain(package_id, &dependency_graph),
                });
            }
        }
        
        // Layer 3: Cross-reference with active projects
        let active_projects = self.discovery.discover_all_projects().await?;
        for package_id in cleanup_candidates {
            let requiring_projects = self.find_projects_requiring_package(package_id, &active_projects);
            if !requiring_projects.is_empty() {
                analysis.add_safety_violation(SafetyViolation::RequiredByActiveProject {
                    package: package_id.clone(),
                    projects: requiring_projects,
                });
            }
        }
        
        // Layer 4: Python virtual environment check
        for package_id in cleanup_candidates {
            if let Some(python_usage) = self.check_python_environment_usage(package_id).await? {
                analysis.add_safety_violation(SafetyViolation::PythonEnvironmentInUse {
                    package: package_id.clone(),
                    usage_info: python_usage,
                });
            }
        }
        
        Ok(analysis)
    }
    
    /// Find dependency chain showing why package is required
    fn find_dependency_chain(
        &self,
        target_package: &PackageId,
        graph: &GlobalDependencyGraph
    ) -> Vec<DependencyChain> {
        let mut chains = Vec::new();
        
        // Find all root packages that transitively depend on target
        for root_package in &graph.roots {
            if let Some(chain) = self.find_path_to_package(root_package, target_package, graph) {
                chains.push(DependencyChain {
                    root_package: root_package.clone(),
                    intermediate_packages: chain,
                    target_package: target_package.clone(),
                });
            }
        }
        
        chains
    }
    
    /// Find path from source to target in dependency graph
    fn find_path_to_package(
        &self,
        source: &PackageId,
        target: &PackageId,
        graph: &GlobalDependencyGraph
    ) -> Option<Vec<PackageId>> {
        let mut queue = VecDeque::new();
        let mut visited = HashSet::new();
        let mut parent_map = HashMap::new();
        
        queue.push_back(source.clone());
        visited.insert(source.clone());
        
        while let Some(current) = queue.pop_front() {
            if current == *target {
                // Reconstruct path
                let mut path = Vec::new();
                let mut node = target.clone();
                
                while let Some(parent) = parent_map.get(&node) {
                    path.push(parent.clone());
                    node = parent.clone();
                }
                
                path.reverse();
                return Some(path);
            }
            
            if let Some(dependencies) = graph.edges.get(&current) {
                for dep in dependencies {
                    if !visited.contains(dep) {
                        visited.insert(dep.clone());
                        parent_map.insert(dep.clone(), current.clone());
                        queue.push_back(dep.clone());
                    }
                }
            }
        }
        
        None
    }
}
```

### Performance Optimizations

#### Incremental Discovery

```rust
/// Incremental project discovery with change detection
pub struct IncrementalDiscovery {
    /// Last discovery results with timestamps
    cached_results: HashMap<PathBuf, (DiscoveredProject, SystemTime)>,
    
    /// File system watcher for change detection
    watcher: RecommendedWatcher,
    
    /// Change notifications receiver
    changes_rx: mpsc::Receiver<DebouncedEvent>,
}

impl IncrementalDiscovery {
    /// Perform incremental discovery, only re-scanning changed directories
    pub async fn discover_incremental(&mut self) -> Result<Vec<DiscoveredProject>, DiscoveryError> {
        // Process file system changes
        let mut changed_paths = HashSet::new();
        while let Ok(event) = self.changes_rx.try_recv() {
            match event {
                DebouncedEvent::Create(path) |
                DebouncedEvent::Write(path) |
                DebouncedEvent::Remove(path) => {
                    // If hpm.toml changed, mark directory for re-scan
                    if path.file_name() == Some(OsStr::new("hpm.toml")) {
                        if let Some(parent) = path.parent() {
                            changed_paths.insert(parent.to_path_buf());
                        }
                    }
                }
                _ => {}
            }
        }
        
        // Re-scan changed directories
        for changed_path in changed_paths {
            if let Some(project) = self.discover_single_project(&changed_path).await? {
                let modified_time = fs::metadata(&changed_path)
                    .await?
                    .modified()?;
                self.cached_results.insert(changed_path, (project, modified_time));
            } else {
                // Project removed
                self.cached_results.remove(&changed_path);
            }
        }
        
        // Return all current projects
        Ok(self.cached_results.values().map(|(project, _)| project.clone()).collect())
    }
}
```

#### Parallel Cleanup Analysis

```rust
impl CleanupManager {
    /// Analyze cleanup candidates in parallel
    pub async fn analyze_cleanup_parallel(
        &self,
        candidates: Vec<PackageId>
    ) -> Result<CleanupAnalysis, CleanupError> {
        // Split candidates into batches for parallel processing
        let batch_size = 50;
        let batches: Vec<_> = candidates.chunks(batch_size).map(|chunk| chunk.to_vec()).collect();
        
        // Create semaphore to limit concurrent analysis tasks
        let semaphore = Arc::new(Semaphore::new(10));
        
        // Analyze batches in parallel
        let analysis_tasks: Vec<_> = batches
            .into_iter()
            .map(|batch| {
                let manager = self.clone();
                let semaphore = semaphore.clone();
                
                tokio::spawn(async move {
                    let _permit = semaphore.acquire().await.unwrap();
                    manager.analyze_batch(batch).await
                })
            })
            .collect();
        
        // Wait for all analysis to complete
        let batch_results: Result<Vec<_>, _> = try_join_all(analysis_tasks).await;
        let batch_analyses = batch_results.map_err(|e| CleanupError::ParallelAnalysis {
            error: e.to_string()
        })?;
        
        // Merge batch results
        let mut merged_analysis = CleanupAnalysis::new();
        for batch_analysis in batch_analyses {
            let batch_analysis = batch_analysis?;
            merged_analysis.merge(batch_analysis);
        }
        
        Ok(merged_analysis)
    }
}
```

## Python Integration Architecture

HPM's Python integration system addresses the complex challenge of managing Python dependencies across multiple Houdini packages while avoiding conflicts and optimizing disk usage.

### Content-Addressable Virtual Environment System

#### Theoretical Foundation

The core insight is that Python virtual environments can be shared when they contain identical resolved dependencies. This is achieved through content-addressable storage:

```text
Given:
  - P = {p₁, p₂, ..., pₙ} (set of packages with Python dependencies)
  - D(pᵢ) = {(pkg, version), ...} (resolved Python dependencies for package pᵢ)
  - H: D → String (hash function mapping dependency sets to identifiers)

Properties:
  - If D(pᵢ) = D(pⱼ), then H(D(pᵢ)) = H(D(pⱼ))
  - Packages pᵢ and pⱼ can share the same virtual environment
  - Storage requirement = O(|unique(H(D(p)) for p in P)|)
```

#### Hash Algorithm Implementation

```rust
/// Content-addressable hash calculation for Python dependencies
pub struct PythonEnvironmentHasher {
    /// Hasher implementation (SHA-256)
    hasher: Sha256,
}

impl PythonEnvironmentHasher {
    /// Calculate deterministic hash for resolved dependencies
    pub fn calculate_hash(&self, resolved: &ResolvedDependencies) -> String {
        let mut hasher = Sha256::new();
        
        // Include Python version in hash
        hasher.update(resolved.python_version.as_bytes());
        hasher.update(b"\n");
        
        // Sort packages by name for deterministic ordering
        let mut sorted_packages: Vec<_> = resolved.packages.iter().collect();
        sorted_packages.sort_by_key(|(name, _)| name.as_str());
        
        // Hash each package specification
        for (name, package) in sorted_packages {
            hasher.update(name.as_bytes());
            hasher.update(b"==");
            hasher.update(package.version.as_bytes());
            
            // Include extras in hash (sorted for determinism)
            if let Some(extras) = &package.extras {
                let mut sorted_extras = extras.clone();
                sorted_extras.sort();
                hasher.update(b"[");
                for (i, extra) in sorted_extras.iter().enumerate() {
                    if i > 0 {
                        hasher.update(b",");
                    }
                    hasher.update(extra.as_bytes());
                }
                hasher.update(b"]");
            }
            
            // Include package source information
            match &package.source {
                PackageSource::Registry { .. } => {
                    // Registry packages identified by name+version only
                }
                PackageSource::Git { url, reference } => {
                    hasher.update(b"@git:");
                    hasher.update(url.as_bytes());
                    hasher.update(b"#");
                    hasher.update(reference.as_bytes());
                }
                PackageSource::Local { path } => {
                    hasher.update(b"@local:");
                    hasher.update(path.to_string_lossy().as_bytes());
                }
            }
            
            hasher.update(b"\n");
        }
        
        // Return first 12 characters of hex digest for readability
        format!("{:x}", hasher.finalize())[..12].to_string()
    }
    
    /// Verify hash matches resolved dependencies
    pub fn verify_hash(&self, hash: &str, resolved: &ResolvedDependencies) -> bool {
        self.calculate_hash(resolved) == hash
    }
}
```

#### Virtual Environment Management

```rust
/// Manages content-addressable virtual environments
pub struct VirtualEnvironmentManager {
    /// Root directory for virtual environments
    venvs_dir: PathBuf,
    
    /// UV binary path for operations
    uv_path: PathBuf,
    
    /// Environment metadata cache
    metadata_cache: LruCache<String, VenvMetadata>,
    
    /// Hash calculator
    hasher: PythonEnvironmentHasher,
}

impl VirtualEnvironmentManager {
    /// Ensure virtual environment exists for resolved dependencies
    pub async fn ensure_environment(
        &mut self,
        resolved: &ResolvedDependencies
    ) -> Result<PathBuf, PythonError> {
        // Calculate content hash
        let content_hash = self.hasher.calculate_hash(resolved);
        let venv_path = self.venvs_dir.join(&content_hash);
        
        // Check if environment already exists and is valid
        if venv_path.exists() {
            match self.validate_existing_environment(&venv_path, resolved).await {
                Ok(true) => {
                    tracing::info!("Reusing existing virtual environment: {}", content_hash);
                    self.update_last_used(&venv_path).await?;
                    return Ok(venv_path);
                }
                Ok(false) => {
                    tracing::warn!("Virtual environment {} is corrupted, recreating", content_hash);
                    fs::remove_dir_all(&venv_path).await?;
                }
                Err(e) => {
                    tracing::warn!("Failed to validate environment {}: {}", content_hash, e);
                    fs::remove_dir_all(&venv_path).await?;
                }
            }
        }
        
        // Create new virtual environment
        tracing::info!("Creating new virtual environment: {}", content_hash);
        self.create_virtual_environment(&venv_path, resolved).await?;
        
        Ok(venv_path)
    }
    
    /// Create new virtual environment with UV
    async fn create_virtual_environment(
        &self,
        venv_path: &Path,
        resolved: &ResolvedDependencies
    ) -> Result<(), PythonError> {
        // Step 1: Create virtual environment
        let create_result = self.run_uv_command(&[
            "venv",
            venv_path.to_str().ok_or(PythonError::InvalidPath)?,
            "--python",
            &resolved.python_version,
            "--seed",  // Include pip, setuptools, wheel
        ]).await?;
        
        if !create_result.success() {
            return Err(PythonError::VenvCreationFailed {
                path: venv_path.to_path_buf(),
                error: create_result.stderr,
            });
        }
        
        // Step 2: Install packages using UV
        self.install_packages_in_venv(venv_path, resolved).await?;
        
        // Step 3: Create metadata file
        let metadata = VenvMetadata {
            hpm_version: env!("CARGO_PKG_VERSION").to_string(),
            content_hash: self.hasher.calculate_hash(resolved),
            python_version: resolved.python_version.clone(),
            resolved_packages: resolved.packages.clone(),
            creation_time: Utc::now(),
            last_used: Utc::now(),
            source_manifests: vec![], // TODO: Track source manifests
        };
        
        let metadata_path = venv_path.join("hpm_metadata.json");
        let metadata_json = serde_json::to_string_pretty(&metadata)?;
        fs::write(&metadata_path, metadata_json).await?;
        
        tracing::info!("Virtual environment created successfully: {}", venv_path.display());
        Ok(())
    }
    
    /// Install packages in virtual environment using UV
    async fn install_packages_in_venv(
        &self,
        venv_path: &Path,
        resolved: &ResolvedDependencies
    ) -> Result<(), PythonError> {
        // Generate requirements file with exact versions
        let requirements = self.generate_requirements_file(resolved)?;
        let requirements_path = venv_path.join("requirements.txt");
        fs::write(&requirements_path, requirements).await?;
        
        // Install using UV
        let install_result = self.run_uv_command(&[
            "pip",
            "install",
            "--python",
            venv_path.join("bin/python").to_str().unwrap(),
            "--requirement",
            requirements_path.to_str().unwrap(),
            "--no-deps", // We already resolved dependencies
        ]).await?;
        
        if !install_result.success() {
            return Err(PythonError::PackageInstallationFailed {
                error: install_result.stderr,
            });
        }
        
        // Clean up requirements file
        fs::remove_file(&requirements_path).await?;
        
        Ok(())
    }
    
    /// Generate requirements.txt content from resolved dependencies
    fn generate_requirements_file(&self, resolved: &ResolvedDependencies) -> Result<String, PythonError> {
        let mut requirements = String::new();
        
        // Sort packages for deterministic output
        let mut packages: Vec<_> = resolved.packages.iter().collect();
        packages.sort_by_key(|(name, _)| name.as_str());
        
        for (name, package) in packages {
            // Write package specification
            requirements.push_str(name);
            requirements.push_str("==");
            requirements.push_str(&package.version);
            
            // Add extras if present
            if let Some(extras) = &package.extras {
                if !extras.is_empty() {
                    requirements.push('[');
                    requirements.push_str(&extras.join(","));
                    requirements.push(']');
                }
            }
            
            // Add source specifier for non-registry packages
            match &package.source {
                PackageSource::Git { url, reference } => {
                    requirements.push_str(" @ git+");
                    requirements.push_str(url);
                    requirements.push('@');
                    requirements.push_str(reference);
                }
                PackageSource::Local { path } => {
                    requirements.push_str(" @ file://");
                    requirements.push_str(&path.to_string_lossy());
                }
                PackageSource::Registry { .. } => {
                    // Registry packages don't need source specifier
                }
            }
            
            requirements.push('\n');
        }
        
        Ok(requirements)
    }
    
    /// Validate existing virtual environment
    async fn validate_existing_environment(
        &self,
        venv_path: &Path,
        expected: &ResolvedDependencies
    ) -> Result<bool, PythonError> {
        // Check if metadata file exists
        let metadata_path = venv_path.join("hpm_metadata.json");
        if !metadata_path.exists() {
            return Ok(false);
        }
        
        // Load and validate metadata
        let metadata_content = fs::read_to_string(&metadata_path).await?;
        let metadata: VenvMetadata = serde_json::from_str(&metadata_content)?;
        
        // Verify content hash
        let expected_hash = self.hasher.calculate_hash(expected);
        if metadata.content_hash != expected_hash {
            return Ok(false);
        }
        
        // Verify Python version
        if metadata.python_version != expected.python_version {
            return Ok(false);
        }
        
        // Verify package count (quick check)
        if metadata.resolved_packages.len() != expected.packages.len() {
            return Ok(false);
        }
        
        // Check if Python executable exists
        let python_exe = venv_path.join("bin").join("python");
        if !python_exe.exists() {
            return Ok(false);
        }
        
        // Optional: Verify installed packages by running pip list
        // This is more thorough but slower
        if self.should_deep_validate() {
            return self.deep_validate_environment(venv_path, expected).await;
        }
        
        Ok(true)
    }
}
```

### UV Integration and Isolation

#### Bundled UV Management

```rust
/// Manages bundled UV binary with complete isolation
pub struct BundledUvManager {
    /// Path to UV binary
    uv_binary: PathBuf,
    
    /// UV cache directory (isolated)
    cache_dir: PathBuf,
    
    /// UV configuration directory (isolated)
    config_dir: PathBuf,
    
    /// Environment variables for UV isolation
    isolation_env: HashMap<String, String>,
}

impl BundledUvManager {
    /// Initialize UV with complete isolation
    pub async fn initialize() -> Result<Self, PythonError> {
        let hpm_dir = dirs::home_dir()
            .ok_or(PythonError::HomeDirectoryNotFound)?
            .join(".hpm");
        
        let cache_dir = hpm_dir.join("uv-cache");
        let config_dir = hpm_dir.join("uv-config");
        
        // Ensure directories exist
        fs::create_dir_all(&cache_dir).await?;
        fs::create_dir_all(&config_dir).await?;
        
        // Extract or locate UV binary
        let uv_binary = Self::ensure_uv_binary().await?;
        
        // Set up isolation environment variables
        let isolation_env = HashMap::from([
            ("UV_CACHE_DIR".to_string(), cache_dir.to_string_lossy().to_string()),
            ("UV_CONFIG_FILE".to_string(), config_dir.join("uv.toml").to_string_lossy().to_string()),
            ("UV_NO_PROGRESS".to_string(), "1".to_string()), // Disable progress bars
            ("UV_QUIET".to_string(), "1".to_string()), // Reduce output
            // Prevent UV from using system configuration
            ("UV_SYSTEM_PYTHON".to_string(), "0".to_string()),
            ("UV_BREAK_SYSTEM_PACKAGES".to_string(), "0".to_string()),
        ]);
        
        // Create UV configuration file for additional isolation
        let uv_config = r#"
[pip]
# HPM-managed UV configuration
no-build-isolation = false
compile-bytecode = true
require-hashes = false

[cache]
# Use HPM-managed cache directory
dir = "~/.hpm/uv-cache"
"#;
        
        let config_file = config_dir.join("uv.toml");
        fs::write(&config_file, uv_config).await?;
        
        Ok(Self {
            uv_binary,
            cache_dir,
            config_dir,
            isolation_env,
        })
    }
    
    /// Execute UV command with isolation
    pub async fn execute_command(&self, args: &[&str]) -> Result<CommandResult, PythonError> {
        let mut command = Command::new(&self.uv_binary);
        command.args(args);
        
        // Set isolation environment
        for (key, value) in &self.isolation_env {
            command.env(key, value);
        }
        
        // Clear system Python-related environment variables
        command.env_remove("PYTHONPATH");
        command.env_remove("PYTHONHOME");
        command.env_remove("VIRTUAL_ENV");
        
        // Execute with timeout
        let timeout = Duration::from_secs(300); // 5 minutes default timeout
        let output = timeout_after(timeout, command.output())
            .await
            .map_err(|_| PythonError::UvTimeout { timeout })?
            .map_err(|e| PythonError::UvExecutionFailed { 
                error: e.to_string() 
            })?;
        
        Ok(CommandResult {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
            success: output.status.success(),
        })
    }
    
    /// Ensure UV binary is available (extract from embedded binary)
    async fn ensure_uv_binary() -> Result<PathBuf, PythonError> {
        let hpm_bin_dir = dirs::home_dir()
            .ok_or(PythonError::HomeDirectoryNotFound)?
            .join(".hpm")
            .join("bin");
        
        fs::create_dir_all(&hpm_bin_dir).await?;
        
        let uv_binary_path = hpm_bin_dir.join("uv");
        
        // Check if UV binary already exists and is current version
        if uv_binary_path.exists() {
            if let Ok(version_output) = Command::new(&uv_binary_path)
                .args(&["--version"])
                .output()
                .await
            {
                let version_str = String::from_utf8_lossy(&version_output.stdout);
                if version_str.contains(EXPECTED_UV_VERSION) {
                    return Ok(uv_binary_path);
                }
            }
        }
        
        // Extract UV binary from embedded data
        tracing::info!("Extracting UV binary...");
        let uv_binary_data = include_bytes!("../../embedded/uv");
        fs::write(&uv_binary_path, uv_binary_data).await?;
        
        // Make executable (Unix only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&uv_binary_path).await?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&uv_binary_path, perms).await?;
        }
        
        tracing::info!("UV binary extracted to: {}", uv_binary_path.display());
        Ok(uv_binary_path)
    }
}
```

### Dependency Resolution with UV

#### Advanced Dependency Collection

```rust
/// Collect and analyze Python dependencies from HPM packages
pub struct PythonDependencyCollector {
    /// Houdini version mapping
    version_mapper: HoudiniVersionMapper,
    
    /// Conflict analyzer
    conflict_analyzer: ConflictAnalyzer,
}

impl PythonDependencyCollector {
    /// Collect Python dependencies from multiple package manifests
    pub async fn collect_dependencies(
        &self,
        packages: &[PackageManifest]
    ) -> Result<PythonDependencies, PythonError> {
        let mut dependencies = PythonDependencies::default();
        let mut conflicts = Vec::new();
        
        // Phase 1: Collect all Python dependencies
        for package in packages {
            if let Some(python_deps) = &package.python_dependencies {
                for (dep_name, dep_spec) in python_deps {
                    self.process_python_dependency(
                        &mut dependencies,
                        dep_name,
                        dep_spec,
                        &package.package.name,
                        &mut conflicts
                    )?;
                }
            }
            
            // Set Houdini version for Python version mapping
            if let Some(houdini_compat) = &package.houdini {
                if let Some(min_version) = &houdini_compat.min_version {
                    dependencies.houdini_version = Some(min_version.clone());
                }
            }
        }
        
        // Phase 2: Detect and analyze conflicts
        dependencies.conflicts = self.conflict_analyzer.analyze_conflicts(&dependencies)?;
        
        Ok(dependencies)
    }
    
    /// Process single Python dependency with conflict detection
    fn process_python_dependency(
        &self,
        dependencies: &mut PythonDependencies,
        dep_name: &str,
        dep_spec: &PythonDependencySpec,
        source_package: &str,
        conflicts: &mut Vec<DependencyConflict>
    ) -> Result<(), PythonError> {
        let dependency_key = dep_name.to_string();
        
        match dependencies.requirements.entry(dependency_key) {
            Entry::Vacant(entry) => {
                // First occurrence of this dependency
                let mut spec = dep_spec.clone();
                spec.source_packages.push(source_package.to_string());
                entry.insert(spec);
            }
            Entry::Occupied(mut entry) => {
                // Dependency already exists - check for conflicts
                let existing_spec = entry.get_mut();
                existing_spec.source_packages.push(source_package.to_string());
                
                // Check version requirement compatibility
                if !self.are_version_specs_compatible(&existing_spec.version_spec, &dep_spec.version_spec) {
                    conflicts.push(DependencyConflict {
                        package_name: dep_name.to_string(),
                        conflicting_requirements: vec![
                            ConflictingRequirement {
                                requirement: existing_spec.version_spec.clone(),
                                source_package: existing_spec.source_packages[0].clone(),
                            },
                            ConflictingRequirement {
                                requirement: dep_spec.version_spec.clone(),
                                source_package: source_package.to_string(),
                            },
                        ],
                        resolution_suggestions: self.generate_resolution_suggestions(dep_name, existing_spec, dep_spec),
                    });
                } else {
                    // Merge compatible requirements
                    existing_spec.version_spec = self.merge_version_specs(
                        &existing_spec.version_spec,
                        &dep_spec.version_spec
                    )?;
                }
                
                // Merge extras
                if let (Some(existing_extras), Some(new_extras)) = (&mut existing_spec.extras, &dep_spec.extras) {
                    for extra in new_extras {
                        if !existing_extras.contains(extra) {
                            existing_extras.push(extra.clone());
                        }
                    }
                } else if existing_spec.extras.is_none() && dep_spec.extras.is_some() {
                    existing_spec.extras = dep_spec.extras.clone();
                }
                
                // Handle optionality (dependency is optional only if ALL sources mark it optional)
                existing_spec.optional = existing_spec.optional && dep_spec.optional;
            }
        }
        
        Ok(())
    }
    
    /// Check if two version specifications are compatible
    fn are_version_specs_compatible(&self, spec1: &str, spec2: &str) -> bool {
        // Parse version requirements
        let req1 = match VersionReq::parse(spec1) {
            Ok(req) => req,
            Err(_) => return false,
        };
        
        let req2 = match VersionReq::parse(spec2) {
            Ok(req) => req,
            Err(_) => return false,
        };
        
        // Check if there exists any version that satisfies both requirements
        // This is a simplified check - could be more sophisticated
        self.has_compatible_version_range(&req1, &req2)
    }
    
    /// Generate resolution suggestions for conflicting dependencies
    fn generate_resolution_suggestions(
        &self,
        package_name: &str,
        spec1: &PythonDependencySpec,
        spec2: &PythonDependencySpec
    ) -> Vec<String> {
        let mut suggestions = Vec::new();
        
        // Suggest updating to compatible version range
        if let Ok(merged_spec) = self.suggest_compatible_version_spec(&spec1.version_spec, &spec2.version_spec) {
            suggestions.push(format!(
                "Update {} requirement to '{}' to satisfy both packages",
                package_name, merged_spec
            ));
        }
        
        // Suggest making one dependency optional
        suggestions.push(format!(
            "Make {} optional in one of the conflicting packages",
            package_name
        ));
        
        // Suggest alternative packages if known
        if let Some(alternatives) = self.get_package_alternatives(package_name) {
            suggestions.push(format!(
                "Consider using alternative packages: {}",
                alternatives.join(", ")
            ));
        }
        
        suggestions
    }
}
```

#### UV-Powered Resolution

```rust
/// Python dependency resolution using UV
pub struct UvDependencyResolver {
    /// UV manager for command execution
    uv_manager: Arc<BundledUvManager>,
    
    /// Resolution cache
    resolution_cache: LruCache<ResolutionCacheKey, ResolvedDependencies>,
    
    /// Python version mapper
    version_mapper: HoudiniVersionMapper,
}

impl UvDependencyResolver {
    /// Resolve Python dependencies using UV
    pub async fn resolve_dependencies(
        &mut self,
        collected: &PythonDependencies
    ) -> Result<ResolvedDependencies, PythonError> {
        // Check cache first
        let cache_key = self.build_cache_key(collected);
        if let Some(cached_result) = self.resolution_cache.get(&cache_key) {
            tracing::info!("Using cached Python dependency resolution");
            return Ok(cached_result.clone());
        }
        
        // Map Houdini version to Python version
        let python_version = self.determine_python_version(collected)?;
        
        tracing::info!("Resolving Python dependencies with UV (Python {})", python_version);
        
        // Create temporary requirements file
        let temp_dir = tempdir()?;
        let requirements_file = temp_dir.path().join("requirements.in");
        self.write_requirements_file(&requirements_file, collected).await?;
        
        // Run UV resolution
        let resolution_result = self.run_uv_resolution(&requirements_file, &python_version).await?;
        
        // Parse resolution result
        let resolved_dependencies = self.parse_uv_output(resolution_result, python_version).await?;
        
        // Cache result
        self.resolution_cache.put(cache_key, resolved_dependencies.clone());
        
        Ok(resolved_dependencies)
    }
    
    /// Run UV dependency resolution
    async fn run_uv_resolution(
        &self,
        requirements_file: &Path,
        python_version: &str
    ) -> Result<String, PythonError> {
        let temp_dir = requirements_file.parent().unwrap();
        let output_file = temp_dir.join("requirements.txt");
        
        // Execute UV resolution command
        let args = [
            "pip",
            "compile",
            requirements_file.to_str().unwrap(),
            "--output-file",
            output_file.to_str().unwrap(),
            "--python-version",
            python_version,
            "--resolver",
            "uv", // Use UV's resolver
            "--generate-hashes", // Include package hashes for security
            "--annotation-style",
            "line", // Clear annotation format
        ];
        
        let result = self.uv_manager.execute_command(&args).await?;
        
        if !result.success {
            return Err(PythonError::ResolutionFailed {
                error: format!("UV resolution failed: {}", result.stderr),
            });
        }
        
        // Read resolved requirements
        let resolved_content = fs::read_to_string(&output_file).await?;
        Ok(resolved_content)
    }
    
    /// Parse UV resolution output into structured data
    async fn parse_uv_output(
        &self,
        uv_output: String,
        python_version: String
    ) -> Result<ResolvedDependencies, PythonError> {
        let mut packages = BTreeMap::new();
        
        for line in uv_output.lines() {
            let line = line.trim();
            
            // Skip comments and empty lines
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            
            // Parse package specification
            if let Some(package) = self.parse_package_line(line)? {
                packages.insert(package.name.clone(), package);
            }
        }
        
        let resolved = ResolvedDependencies {
            python_version,
            packages,
            resolution_time: Utc::now(),
            content_hash: String::new(), // Will be calculated later
        };
        
        Ok(resolved)
    }
    
    /// Parse single package line from UV output
    fn parse_package_line(&self, line: &str) -> Result<Option<ResolvedPackage>, PythonError> {
        // Handle different package specification formats:
        // numpy==1.24.0 --hash=sha256:...
        // requests[security]==2.28.0
        // git+https://github.com/user/package.git@v1.0.0#egg=package
        
        // Split on whitespace to separate package spec from hashes/options
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            return Ok(None);
        }
        
        let package_spec = parts[0];
        
        // Parse package name, version, and extras
        let (name, version, extras, source) = self.parse_package_specification(package_spec)?;
        
        Ok(Some(ResolvedPackage {
            name,
            version,
            extras,
            source,
            dependencies: vec![], // TODO: Parse dependencies from UV output
        }))
    }
    
    /// Parse package specification (name[extras]==version)
    fn parse_package_specification(
        &self,
        spec: &str
    ) -> Result<(String, String, Option<Vec<String>>, PackageSource), PythonError> {
        // Handle git URLs
        if spec.starts_with("git+") {
            return self.parse_git_specification(spec);
        }
        
        // Handle local paths
        if spec.starts_with("file://") || spec.starts_with("/") || spec.starts_with("./") {
            return self.parse_local_specification(spec);
        }
        
        // Handle registry packages: name[extras]==version
        let (name_with_extras, version) = if let Some(version_pos) = spec.find("==") {
            let name_part = &spec[..version_pos];
            let version_part = &spec[version_pos + 2..];
            (name_part, version_part)
        } else {
            return Err(PythonError::InvalidPackageSpecification {
                spec: spec.to_string(),
            });
        };
        
        // Parse name and extras
        let (name, extras) = if let Some(bracket_pos) = name_with_extras.find('[') {
            let name = &name_with_extras[..bracket_pos];
            let extras_str = &name_with_extras[bracket_pos + 1..];
            let extras_str = extras_str.strip_suffix(']')
                .ok_or_else(|| PythonError::InvalidPackageSpecification {
                    spec: spec.to_string(),
                })?;
            
            let extras: Vec<String> = extras_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            
            (name.to_string(), if extras.is_empty() { None } else { Some(extras) })
        } else {
            (name_with_extras.to_string(), None)
        };
        
        Ok((name, version.to_string(), extras, PackageSource::Registry {
            url: "https://pypi.org".to_string(),
        }))
    }
}
```

### Houdini Integration

#### Package.json Generation

```rust
/// Generate Houdini package.json with Python environment integration
pub struct HoudiniIntegrationGenerator {
    /// Python environment manager
    venv_manager: Arc<VirtualEnvironmentManager>,
}

impl HoudiniIntegrationGenerator {
    /// Generate complete Houdini package.json for HPM package
    pub async fn generate_package_json(
        &self,
        package_manifest: &PackageManifest,
        python_venv_hash: Option<&str>
    ) -> Result<HoudiniPackageJson, PythonError> {
        let mut package_json = HoudiniPackageJson {
            path: "$HPM_PACKAGE_ROOT".to_string(),
            load_package_once: Some(true),
            env: vec![],
            hpm_managed: Some(true),
            hpm_package: Some(package_manifest.package.name.clone()),
            hpm_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        };
        
        // Add Python environment integration if present
        if let Some(venv_hash) = python_venv_hash {
            let venv_path = self.venv_manager.get_venv_path(venv_hash);
            self.add_python_environment_integration(&mut package_json, &venv_path)?;
        }
        
        // Add custom environment variables from manifest
        if let Some(env_vars) = self.extract_env_vars_from_manifest(package_manifest) {
            package_json.env.extend(env_vars);
        }
        
        Ok(package_json)
    }
    
    /// Add Python virtual environment integration to package.json
    fn add_python_environment_integration(
        &self,
        package_json: &mut HoudiniPackageJson,
        venv_path: &Path
    ) -> Result<(), PythonError> {
        // Add Python executable to PATH
        let venv_bin = venv_path.join("bin");
        package_json.env.push(EnvironmentVariable {
            key: "PATH".to_string(),
            value: format!("{}:$PATH", venv_bin.display()),
        });
        
        // Add site-packages to PYTHONPATH
        let site_packages = self.find_site_packages_dir(venv_path)?;
        package_json.env.push(EnvironmentVariable {
            key: "PYTHONPATH".to_string(),
            value: format!("{}:$PYTHONPATH", site_packages.display()),
        });
        
        // Set virtual environment variable
        package_json.env.push(EnvironmentVariable {
            key: "VIRTUAL_ENV".to_string(),
            value: venv_path.display().to_string(),
        });
        
        // Disable pip version check (reduces noise)
        package_json.env.push(EnvironmentVariable {
            key: "PIP_DISABLE_PIP_VERSION_CHECK".to_string(),
            value: "1".to_string(),
        });
        
        Ok(())
    }
    
    /// Find site-packages directory in virtual environment
    fn find_site_packages_dir(&self, venv_path: &Path) -> Result<PathBuf, PythonError> {
        let lib_dir = venv_path.join("lib");
        
        // Find Python version directory (e.g., python3.9, python3.10)
        let entries = fs::read_dir(&lib_dir)
            .map_err(|_| PythonError::VenvCorrupted { 
                path: venv_path.to_path_buf() 
            })?;
        
        for entry in entries {
            let entry = entry.map_err(|_| PythonError::VenvCorrupted {
                path: venv_path.to_path_buf()
            })?;
            
            let dir_name = entry.file_name();
            if let Some(name_str) = dir_name.to_str() {
                if name_str.starts_with("python") {
                    let site_packages = entry.path().join("site-packages");
                    if site_packages.exists() {
                        return Ok(site_packages);
                    }
                }
            }
        }
        
        Err(PythonError::VenvCorrupted {
            path: venv_path.to_path_buf()
        })
    }
}
```

This completes the comprehensive system deep dives documentation for HPM's core systems. The documentation covers the theoretical foundations, implementation details, performance optimizations, and practical considerations for each major system component.