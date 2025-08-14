# HPM Project-Aware Cleanup System Design

## Overview

This document outlines the design for HPM's project-aware cleanup system, enabling intelligent removal of orphaned packages while preserving transitive dependencies needed by active projects.

## Problem Statement

Currently, HPM's global storage system (`~/.hpm/packages/`) can accumulate orphaned packages:
- Packages installed for projects that no longer exist
- Old package versions no longer needed
- Transitive dependencies that become unreferenced

Without project awareness, HPM cannot safely determine which packages can be removed.

## Solution Architecture

### 1. Project Discovery System

**Configuration-Based Discovery:**
```toml
[projects]
explicit_paths = ["/path/to/important/project"]           # Direct project paths
search_roots = ["/Users/artist/houdini-projects"]         # Directories to scan
max_search_depth = 3                                      # Search depth limit
ignore_patterns = [".git", "backup", "archive"]           # Skip patterns
```

**Discovery Algorithm:**
- Scan explicit project paths for `hpm.toml` files
- Recursively search root directories up to max depth
- Skip directories matching ignore patterns
- Return list of active HPM-managed projects

### 2. Dependency Graph Construction

**Graph Structure:**
```rust
pub struct DependencyGraph {
    nodes: HashMap<PackageId, PackageNode>,
    edges: HashMap<PackageId, HashSet<PackageId>>,
}

pub struct PackageNode {
    id: PackageId,
    installed_package: Option<InstalledPackage>,
    required_by_projects: Vec<PathBuf>,
    is_root: bool,  // Directly required by a project
}
```

**Construction Process:**
1. **Root Collection**: Extract dependencies from all discovered project manifests
2. **Transitive Resolution**: For each dependency, recursively resolve its dependencies  
3. **DAG Building**: Create directed graph of package relationships
4. **Cycle Detection**: Detect and handle circular dependencies

### 3. Mark-and-Sweep Cleanup

**Marking Phase:**
```rust
impl DependencyGraph {
    pub fn mark_reachable(&self, roots: &[PackageId]) -> HashSet<PackageId> {
        let mut reachable = HashSet::new();
        let mut stack = roots.to_vec();
        
        while let Some(package_id) = stack.pop() {
            if reachable.insert(package_id.clone()) {
                // Add all dependencies to stack
                if let Some(deps) = self.edges.get(&package_id) {
                    stack.extend(deps.iter().cloned());
                }
            }
        }
        
        reachable
    }
}
```

**Cleanup Phase:**
```rust
impl StorageManager {
    pub async fn cleanup_unused(&self, needed_packages: HashSet<PackageId>) -> Result<Vec<String>, StorageError> {
        let installed = self.list_installed()?;
        let mut removed = Vec::new();
        
        for package in installed {
            let package_id = PackageId::from(&package);
            if !needed_packages.contains(&package_id) {
                self.remove_package(&package.name, &package.version).await?;
                removed.push(package.identifier());
            }
        }
        
        Ok(removed)
    }
}
```

## 4. Command Interface

### Command Variations
```bash
hpm clean                    # Interactive cleanup with confirmation
hpm clean --dry-run          # Show what would be cleaned
hpm clean --force            # Remove without confirmation  
hpm clean --keep-versions 3  # Keep N most recent versions
hpm clean package-name       # Clean specific packages only
```

### Safety Features
- **Dry Run Mode**: Preview cleanup actions without execution
- **Interactive Mode**: Confirm removal of each package group
- **Selective Cleanup**: Target specific packages or version ranges
- **Rollback Logging**: Log cleanup actions for potential recovery

## 5. Project Package Integration

### Project Package Concept
Projects can both consume and produce packages:

```
my-project/
├── hpm.toml                 # Dependencies + project package definition
├── scenes/                  # Houdini work files (not packaged)
├── render/                  # Outputs (not packaged)
└── src/                     # Package source code
    ├── otls/               # Reusable HDAs  
    ├── python/             # Python tools
    └── scripts/            # Shelf scripts
```

**Project Manifest Extension:**
```toml
[package]  # Optional: if this project produces a package
name = "my-project-tools" 
version = "1.0.0"
description = "Tools developed for this project"

[dependencies]
utility-nodes = "^2.1.0"
material-library = "1.5.0"

[package-source]  # Where to find package source code
path = "src/"
```

## Implementation Phases

### Phase 1: Configuration Extension
- Add `ProjectsConfig` to configuration system
- Implement project path validation
- Add configuration loading and defaults

### Phase 2: Project Discovery Service
- Implement project scanning logic
- Add directory traversal with depth limits
- Implement ignore pattern matching
- Create project manifest parsing

### Phase 3: Dependency Graph System
- Implement `DependencyGraph` data structure
- Add dependency resolution algorithms
- Implement cycle detection and handling
- Add graph traversal for reachability analysis

### Phase 4: Cleanup Integration
- Enhance `StorageManager::cleanup_unused()` with real logic
- Implement mark-and-sweep cleanup algorithm
- Add safety checks and validation
- Implement cleanup logging and recovery

### Phase 5: CLI Command
- Add `hpm clean` command with subcommands
- Implement dry-run and interactive modes
- Add progress reporting and user feedback
- Integration testing with complex project scenarios

## Benefits

### For Developers
- **Disk Space Management**: Automatic cleanup of unused packages
- **Project Isolation**: Clean separation between project dependencies
- **Development Efficiency**: No manual package management needed

### For Studios
- **Storage Optimization**: Automated cleanup across shared environments
- **Project Discovery**: Automatic detection of HPM-managed projects
- **Dependency Visibility**: Clear understanding of package usage across projects

### For System Administration
- **Resource Management**: Predictable disk usage patterns
- **Maintenance Automation**: Scheduled cleanup operations
- **Dependency Auditing**: Track package usage across the organization

## Edge Cases and Considerations

### Concurrent Access
- Handle multiple HPM instances running cleanup simultaneously
- Lock global storage during cleanup operations
- Safe handling of packages being installed during cleanup

### Version Management  
- Cleanup strategies for multiple package versions
- Preserve pinned versions specified in lock files
- Handle version conflicts in dependency resolution

### Error Recovery
- Graceful handling of missing or corrupted packages
- Recovery from interrupted cleanup operations
- Validation of dependency graph consistency

### Performance
- Efficient project discovery for large directory structures
- Optimized dependency resolution for complex graphs
- Incremental cleanup for large package inventories

This design provides the foundation for intelligent, project-aware package management while maintaining safety and performance.