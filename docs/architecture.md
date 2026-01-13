# HPM Architecture

This document provides a comprehensive technical overview of HPM (Houdini Package Manager), covering system design, algorithms, and implementation details.

## Table of Contents

1. [System Overview](#system-overview)
2. [Architectural Principles](#architectural-principles)
3. [Core Components](#core-components)
4. [Dependency Resolution](#dependency-resolution)
5. [Project-Aware Cleanup System](#project-aware-cleanup-system)
6. [Storage Architecture](#storage-architecture)
7. [Python Integration](#python-integration)
8. [Security & Performance](#security--performance)

---

## System Overview

HPM is a Rust-based package management system designed for SideFX Houdini. The architecture emphasizes safety, performance, and seamless integration with existing Houdini workflows.

### High-Level Architecture

```text
┌─────────────────────────────────────────────────────────────────────────────┐
│                          HPM System Architecture                            │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  User Interface Layer                                                       │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  CLI (hpm-cli)                                                      │   │
│  │  • Commands: init, add, remove, install, list, clean, update, check │   │
│  │  • Output Formats: Human-readable, JSON, JSON Lines                 │   │
│  │  • Shell Completions: bash, zsh, fish, powershell, elvish           │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                    │                                       │
│                                    ▼                                       │
│  Core Package Management (hpm-core)                                        │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌────────────┐  │   │
│  │  │  Storage    │  │ Discovery   │  │ Dependency  │  │  Manager   │  │   │
│  │  │ Management  │  │ & Analysis  │  │ Resolution  │  │ Operations │  │   │
│  │  └─────────────┘  └─────────────┘  └─────────────┘  └────────────┘  │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                    │                                       │
│                                    ▼                                       │
│  Specialized Modules                                                       │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │ ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌─────────────────┐ │   │
│  │ │  Python     │ │   Package   │ │   Config    │ │     Error       │ │   │
│  │ │ Integration │ │  Manifest   │ │ Management  │ │    Handling     │ │   │
│  │ │(hpm-python) │ │(hpm-package)│ │(hpm-config) │ │  (hpm-error)    │ │   │
│  │ └─────────────┘ └─────────────┘ └─────────────┘ └─────────────────┘ │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                    │                                       │
│                                    ▼                                       │
│  External Integrations                                                     │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  Houdini Package System  │  Python Ecosystem  │  Git Integration    │   │
│  │  • package.json         │  • PyPI via UV     │  • Repository Clone │   │
│  │  • HOUDINI_PATH         │  • Virtual Envs    │  • Commit Pinning   │   │
│  │  • Environment Setup    │  • Dependency Tree │  • Path References  │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────────┘
```

### System Characteristics

- **Language**: Rust (2021 edition, 1.70+ required)
- **Concurrency Model**: Async/await with Tokio runtime
- **Architecture Pattern**: Modular monolith with plugin points
- **Configuration**: TOML-based hierarchical configuration system
- **Testing**: Unit, integration, and property-based testing

---

## Architectural Principles

### 1. Safety First

**Memory Safety**: Rust's ownership system eliminates null pointer dereferences, buffer overflows, and data races.

**Operation Safety**: All package operations are atomic or provide strong consistency guarantees. The system never leaves packages in inconsistent states.

**Dependency Safety**: The cleanup system guarantees that packages required by active projects are never removed.

### 2. Performance and Efficiency

**Zero-Cost Abstractions**: High-level APIs compile to efficient machine code without runtime overhead.

**Concurrent Operations**: Async I/O handles concurrent package operations efficiently.

**Content Deduplication**: Both HPM packages and Python virtual environments use content-addressable storage to eliminate duplication.

### 3. Modular Design

**Separation of Concerns**: Each crate has a single, well-defined responsibility with minimal coupling.

**Interface-Driven**: Trait-based abstractions allow for different implementations.

**Extensibility**: Plugin points for custom behavior without modifying core code.

### 4. Integration-Friendly

**Houdini-Native**: Generated `package.json` files work seamlessly with Houdini.

**Tool Integration**: Machine-readable output formats support CI/CD pipelines.

**Standard Conventions**: Follows established patterns from npm, cargo, and uv.

---

## Core Components

### CLI Layer (hpm-cli)

The command-line interface provides user-facing functionality.

```text
┌─────────────────────────────────────────────────────────────────────────────┐
│                              CLI Architecture                               │
├─────────────────────────────────────────────────────────────────────────────┤
│  Command Parser (Clap)                                                      │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  • Argument validation and type conversion                          │   │
│  │  • Subcommand routing with help generation                          │   │
│  │  • Global options (verbosity, output format, colors)               │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                    │                                       │
│                                    ▼                                       │
│  Output & Error Handling                                                   │
│  ┌─────────────────────┐              ┌───────────────────────────────┐   │
│  │   Console System    │              │      Error Reporting          │   │
│  │ • Styled output     │              │ • Structured error types     │   │
│  │ • Color management  │ ────────────▶│ • Contextual help messages   │   │
│  │ • Progress bars     │              │ • Machine-readable errors    │   │
│  └─────────────────────┘              └───────────────────────────────┘   │
│                                    │                                       │
│                                    ▼                                       │
│  Command Implementation                                                    │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │  init, add, remove, install, list, clean, update, check, completions│   │
│  └─────────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Package Manifest Processing (hpm-package)

Handles `hpm.toml` parsing and validation.

```rust
#[derive(Debug, Deserialize, Serialize)]
pub struct PackageManifest {
    pub package: PackageMetadata,
    pub houdini: Option<HoudiniCompatibility>,
    pub dependencies: BTreeMap<String, DependencySpec>,
    pub dev_dependencies: Option<BTreeMap<String, DependencySpec>>,
    pub python_dependencies: Option<BTreeMap<String, PythonDependencySpec>>,
    pub scripts: Option<BTreeMap<String, String>>,
}
```

### Configuration Management (hpm-config)

Hierarchical configuration system:

1. **Default Values** - Built-in sensible defaults
2. **Global Config** (`~/.hpm/config.toml`) - User-wide settings
3. **Project Config** (`.hpm/config.toml`) - Project-specific overrides
4. **Environment Variables** - Runtime configuration
5. **CLI Arguments** - Command-specific overrides

### Error Handling System (hpm-error)

Exit code strategy:
- **0**: Success
- **1**: User error (configuration, input, usage)
- **2**: Internal error (bugs, unexpected conditions)
- **N**: External command exit code

---

## Dependency Resolution

HPM's dependency resolution is inspired by PubGrub but adapted for Houdini package management.

### Theoretical Foundation

The dependency resolution problem is modeled as a **Constraint Satisfaction Problem (CSP)**:

- **Variables**: Each package name
- **Domains**: Available versions for each package
- **Constraints**: Version requirements

```text
Given:
  - P = {p₁, p₂, ..., pₙ} (set of packages)
  - V(pᵢ) = {v₁, v₂, ..., vₖ} (available versions)
  - C = {c₁, c₂, ..., cₘ} (version constraints)

Find:
  - Assignment A: P → V such that ∀cᵢ ∈ C, cᵢ(A) = true
```

### Core Data Structures

```rust
/// Central dependency resolution engine
pub struct DependencyResolver {
    package_cache: HashMap<PackageId, PackageMetadata>,
    conflict_db: ConflictDatabase,
    config: ResolverConfig,
}

/// Resolution state during incremental solving
pub struct ResolutionState {
    assignments: HashMap<String, Version>,
    pending_constraints: Vec<VersionConstraint>,
    conflicts: Vec<DependencyConflict>,
    decision_stack: Vec<DecisionPoint>,
}
```

### Resolution Algorithm

1. **Incremental Solving**: Build solutions incrementally, backtracking on conflicts
2. **Conflict Learning**: Remember conflicts to avoid repeating failed paths
3. **Version Selection**: Choose versions satisfying all constraints with minimal conflicts
4. **Transitive Resolution**: Recursively resolve dependencies of dependencies

### Version Constraints

```rust
pub enum VersionRequirement {
    Exact(Version),           // =1.2.3
    Caret(Version),           // ^1.2.3 (>=1.2.3, <2.0.0)
    Tilde(Version),           // ~1.2.3 (>=1.2.3, <1.3.0)
    Range { min, max },       // >=1.0.0, <2.0.0
    Union(Vec<...>),          // >=1.0.0 || >=2.0.0
    Intersection(Vec<...>),   // >=1.0.0, <2.0.0
}
```

---

## Project-Aware Cleanup System

HPM's cleanup system safely identifies and removes orphaned packages while preserving dependencies needed by active projects.

### Mathematical Foundation

```text
Given:
  - I = {i₁, i₂, ..., iₙ} (installed packages)
  - P = {p₁, p₂, ..., pₘ} (active projects)
  - D: P → 2^I (dependency function)
  - T: I → 2^I (transitive dependency function)

Find:
  - O = I \ ⋃(p∈P) T(D(p)) (orphaned packages)
```

### Algorithm

1. **Global Project Discovery**: Scan configured paths to find all HPM-managed projects
2. **Dependency Graph Construction**: Build complete graphs including transitive dependencies
3. **Set-Based Orphan Detection**: Calculate `Installed - Reachable`
4. **Safety Verification**: Multi-layer validation before removal

### Project Discovery

```rust
pub struct ProjectDiscovery {
    config: ProjectsConfig,
}

impl ProjectDiscovery {
    pub fn find_projects(&self) -> Result<Vec<DiscoveredProject>> {
        let mut projects = Vec::new();

        // Add explicit project paths
        for path in &self.config.explicit_paths {
            if let Some(project) = self.discover_project(path)? {
                projects.push(project);
            }
        }

        // Search configured root directories
        for root in &self.config.search_roots {
            self.search_recursive(root, 0, &mut projects)?;
        }

        Ok(projects)
    }
}
```

### Dependency Graph

```rust
pub struct GlobalDependencyGraph {
    packages: HashMap<PackageId, PackageNode>,
    edges: HashMap<PackageId, HashSet<PackageId>>,
    reverse_edges: HashMap<PackageId, HashSet<PackageId>>,
    roots: HashSet<PackageId>,
}

impl GlobalDependencyGraph {
    /// Calculate transitive closure of dependencies
    pub fn calculate_reachable_packages(&self) -> HashSet<PackageId> {
        let mut reachable = HashSet::new();
        let mut stack: Vec<_> = self.roots.iter().cloned().collect();

        while let Some(package_id) = stack.pop() {
            if reachable.insert(package_id.clone()) {
                if let Some(deps) = self.edges.get(&package_id) {
                    stack.extend(deps.iter().cloned());
                }
            }
        }

        reachable
    }

    /// Orphaned = Installed - Reachable
    pub fn identify_orphaned_packages(&self, installed: &HashSet<PackageId>) -> Vec<PackageId> {
        let reachable = self.calculate_reachable_packages();
        installed.difference(&reachable).cloned().collect()
    }
}
```

---

## Storage Architecture

### Global Storage Design

HPM implements a content-addressable global storage system that optimizes for disk usage and access performance.

```text
~/.hpm/                                    # HPM root directory
├── packages/                              # Global package storage
│   ├── utility-nodes@2.1.0/              # Versioned packages
│   │   ├── hpm.toml                      # Package manifest
│   │   ├── package.json                  # Generated Houdini manifest
│   │   ├── otls/                         # Digital assets
│   │   ├── python/                       # Python modules
│   │   ├── scripts/                      # Shelf tools
│   │   └── presets/                      # Node presets
│   └── material-library@1.5.0/
├── venvs/                                # Python virtual environments
│   ├── a1b2c3d4e5f6/                    # Content-addressable venv
│   │   ├── metadata.json                # Environment metadata
│   │   ├── lib/python3.x/site-packages/
│   │   └── pyvenv.cfg
│   └── f6e5d4c3b2a1/
├── cache/                                # Download cache
├── uv-cache/                            # Isolated UV package cache
├── config.toml                          # Global HPM configuration
└── logs/                                # Operation logs
```

### Project Integration

Each project maintains lightweight references to globally stored packages:

```text
project/
├── hpm.toml                             # Project manifest
├── hpm.lock                             # Dependency lock file
└── .hpm/                                # HPM project directory
    └── packages/                        # Package references
        ├── utility-nodes.json          # Reference to global package
        └── material-library.json
```

**Benefits**:
- **Deduplication**: One global copy per package version
- **Performance**: Fast project setup through references
- **Consistency**: Single source of truth
- **Safety**: Cleanup never removes packages needed by active projects

---

## Python Integration

HPM's Python integration solves dependency conflicts through content-addressable virtual environments.

### Content-Addressable System

```text
┌─────────────────────────────────────────────────────────────────────────────┐
│                 Python Content-Addressable Architecture                     │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Package Manifests                                                          │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐             │
│  │   Package A     │  │   Package B     │  │   Package C     │             │
│  │ numpy>=1.20.0   │  │ numpy>=1.20.0   │  │ numpy>=1.25.0   │             │
│  │ requests^2.28   │  │ requests^2.28   │  │ scipy>=1.9.0    │             │
│  └─────────────────┘  └─────────────────┘  └─────────────────┘             │
│           │                    │                    │                       │
│           ▼                    ▼                    ▼                       │
│                                                                             │
│  UV-Powered Resolution                                                      │
│  ┌─────────────────────────────────────────────────────────────────┐       │
│  │  Resolved Dependency Sets:                                      │       │
│  │                                                                 │       │
│  │  Set 1 (Packages A & B - identical deps):                      │       │
│  │  └─ numpy==1.24.0, requests==2.28.0                           │       │
│  │     Hash: sha256(...) → a1b2c3d4e5f6                          │       │
│  │                                                                 │       │
│  │  Set 2 (Package C - different deps):                           │       │
│  │  └─ numpy==1.25.0, scipy==1.9.0                               │       │
│  │     Hash: sha256(...) → f6e5d4c3b2a1                          │       │
│  └─────────────────────────────────────────────────────────────────┘       │
│                           │                    │                           │
│                           ▼                    ▼                           │
│                                                                             │
│  Virtual Environment Storage (~/.hpm/venvs/)                               │
│  ┌─────────────────┐                  ┌─────────────────┐                  │
│  │  a1b2c3d4e5f6   │ ◄── Shared ──► │  f6e5d4c3b2a1   │                  │
│  │ (A & B share)   │     by A & B    │ (C uses)        │                  │
│  └─────────────────┘                  └─────────────────┘                  │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Hash Algorithm

```rust
pub fn calculate_content_hash(resolved: &ResolvedDependencies) -> String {
    let mut hasher = Sha256::new();

    // Include Python version
    hasher.update(resolved.python_version.as_bytes());

    // Include sorted package specifications (deterministic ordering)
    let mut packages: Vec<_> = resolved.packages.iter().collect();
    packages.sort_by_key(|(name, _)| name.as_str());

    for (name, spec) in packages {
        hasher.update(name.as_bytes());
        hasher.update(spec.version.as_bytes());

        if let Some(extras) = &spec.extras {
            let mut sorted_extras = extras.clone();
            sorted_extras.sort();
            for extra in sorted_extras {
                hasher.update(extra.as_bytes());
            }
        }
    }

    // Return first 12 characters for readability
    format!("{:x}", hasher.finalize())[..12].to_string()
}
```

### Virtual Environment Management

```rust
pub struct VenvManager {
    venvs_dir: PathBuf,
    uv_path: PathBuf,
}

impl VenvManager {
    pub async fn ensure_virtual_environment(
        &self,
        resolved: &ResolvedDependencies
    ) -> Result<PathBuf> {
        let content_hash = calculate_content_hash(resolved);
        let venv_path = self.venvs_dir.join(&content_hash);

        // Return existing environment if valid
        if venv_path.exists() && self.validate_environment(&venv_path, resolved).await? {
            tracing::info!("Reusing virtual environment: {}", content_hash);
            return Ok(venv_path);
        }

        // Create new virtual environment
        tracing::info!("Creating virtual environment: {}", content_hash);
        self.create_virtual_environment(&venv_path, resolved).await?;

        Ok(venv_path)
    }
}
```

### Houdini Integration

Generated `package.json` files provide seamless Houdini integration:

```rust
pub fn generate_houdini_manifest(
    package_name: &str,
    package_path: &Path,
    python_venv: Option<&Path>
) -> Result<HoudiniManifest> {
    let mut manifest = HoudiniManifest {
        path: "$HPM_PACKAGE_ROOT".to_string(),
        load_package_once: Some(true),
        env: Vec::new(),
        hpm_managed: Some(true),
        hpm_package: Some(package_name.to_string()),
    };

    // Add Python virtual environment to PYTHONPATH
    if let Some(venv_path) = python_venv {
        let site_packages = venv_path.join("lib/pythonX.X/site-packages");
        manifest.env.push(EnvVar {
            key: "PYTHONPATH".to_string(),
            value: format!("{}:$PYTHONPATH", site_packages.display()),
        });
    }

    Ok(manifest)
}
```

---

## Security & Performance

### Security Architecture

**Transport Security**:
- Mandatory TLS 1.3 for network communications
- Certificate validation to prevent MITM attacks
- Perfect forward secrecy

**Package Integrity**:
- SHA-256 checksums for all packages
- Manifest validation against schemas
- Git commit pinning for reproducibility

**Isolation**:
- Process isolation for UV execution
- Cache isolation from system tools
- Environment variable sandboxing

### Performance Optimizations

**Concurrent Operations**:
```rust
pub async fn install_multiple_packages(packages: Vec<PackageSpec>) -> Result<Vec<InstallResult>> {
    let mut join_set = JoinSet::new();

    for package in packages {
        join_set.spawn(async move {
            install_single_package(package).await
        });
    }

    let mut results = Vec::new();
    while let Some(result) = join_set.join_next().await {
        results.push(result??);
    }

    Ok(results)
}
```

**Caching Strategy**:
- Package metadata cache to reduce network requests
- Download cache to avoid re-downloading
- Virtual environment sharing maximizes cache hits
- Resolved dependency graph caching

**Memory Management**:
- Zero-copy operations via references
- Streaming I/O for large packages
- RAII patterns for resource cleanup

**Disk I/O Optimization**:
- Parallel file operations
- Binary formats where appropriate
- Atomic operations to prevent corruption

---

## Extension Points

### Custom Commands

```rust
#[derive(Subcommand)]
enum CustomCommands {
    /// Custom package validation
    Validate {
        package: String,
    },
}
```

### Configuration Extensions

```toml
[plugin.custom_validator]
enable = true
strict_mode = false
rules = ["rule1", "rule2"]
```

---

This architecture provides the foundation for understanding HPM's design decisions, implementation patterns, and extensibility mechanisms.
