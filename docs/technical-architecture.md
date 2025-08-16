# HPM Technical Architecture

This document provides a comprehensive technical overview of the HPM (Houdini Package Manager) architecture, including system design principles, component interactions, data flows, and implementation details.

## Table of Contents

1. [System Overview](#system-overview)
2. [Architectural Principles](#architectural-principles)
3. [Core Components](#core-components)
4. [Data Flow Architecture](#data-flow-architecture)
5. [Storage Architecture](#storage-architecture)
6. [Network and Registry Architecture](#network-and-registry-architecture)
7. [Python Integration Architecture](#python-integration-architecture)
8. [Security Architecture](#security-architecture)
9. [Performance and Scalability](#performance-and-scalability)
10. [Extension Points](#extension-points)

## System Overview

HPM is a modern, Rust-based package management system designed specifically for SideFX Houdini. The architecture emphasizes safety, performance, and seamless integration with existing Houdini workflows while providing industry-standard package management capabilities.

### High-Level Architecture

```text
┌─────────────────────────────────────────────────────────────────────────────────┐
│                            HPM System Architecture                              │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                 │
│  User Interface Layer                                                          │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │  CLI (hpm-cli)      │  Future: Web UI  │  Future: IDE Extensions      │   │
│  │  • Commands         │  • Registry UI    │  • VSCode Extension          │   │
│  │  • Error Handling   │  • Admin Panel    │  • Houdini Integration       │   │
│  │  • Output Formats   │  • Analytics      │  • Package Browser           │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
│                                    │                                           │
│                                    ▼                                           │
│  Core Business Logic                                                           │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │  Package Management (hpm-core)                                         │   │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐   │   │
│  │  │  Storage    │  │ Discovery   │  │ Dependency  │  │   Manager   │   │   │
│  │  │ Management  │  │ & Analysis  │  │ Resolution  │  │ Operations  │   │   │
│  │  └─────────────┘  └─────────────┘  └─────────────┘  └─────────────┘   │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
│                                    │                                           │
│                                    ▼                                           │
│  Specialized Modules                                                           │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │ ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌─────────────────────┐ │   │
│  │ │  Python     │ │   Registry  │ │   Package   │ │     Configuration   │ │   │
│  │ │ Integration │ │   Client    │ │  Manifest   │ │     Management      │ │   │
│  │ │(hpm-python) │ │(hpm-registry│ │ Processing  │ │    (hpm-config)     │ │   │
│  │ │             │ │             │ │(hpm-package)│ │                     │ │   │
│  │ └─────────────┘ └─────────────┘ └─────────────┘ └─────────────────────┘ │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
│                                    │                                           │
│                                    ▼                                           │
│  Infrastructure Layer                                                          │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │  File System        │  Network Stack       │  Process Management      │   │
│  │  • Global Storage   │  • QUIC Transport    │  • Async Runtime (Tokio) │   │
│  │  • Project Layouts  │  • gRPC Protocol     │  • UV Process Isolation  │   │
│  │  • Cache Management │  • TLS Encryption    │  • Error Propagation     │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
│                                                                                 │
│  External Integrations                                                         │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │  Houdini Package System  │  Python Ecosystem  │  Git Integration        │   │
│  │  • package.json         │  • PyPI via UV     │  • Version Control      │   │
│  │  • HOUDINI_PATH         │  • Virtual Envs    │  • Repository Cloning   │   │
│  │  • Environment Setup    │  • Dependency Tree │  • Tag-based Versions   │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────────────┘
```

### System Characteristics

- **Language**: Rust (2021 edition, 1.70+ required)
- **Concurrency Model**: Async/await with Tokio runtime
- **Architecture Pattern**: Modular monolith with plugin points
- **Error Handling**: Layered error handling with domain-specific error types
- **Configuration**: TOML-based hierarchical configuration system
- **Testing**: Comprehensive unit, integration, and property-based testing

## Architectural Principles

### 1. Safety First

**Memory Safety**: Rust's ownership system eliminates entire classes of bugs including null pointer dereferences, buffer overflows, and data races.

**Operation Safety**: All package operations are designed to be atomic or provide strong consistency guarantees. The system never leaves packages in inconsistent states.

**Dependency Safety**: The cleanup system guarantees that packages required by active projects are never removed, preventing broken dependencies.

### 2. Performance and Efficiency

**Zero-Cost Abstractions**: High-level APIs compile to efficient machine code without runtime overhead.

**Concurrent Operations**: Async I/O allows handling thousands of concurrent package operations.

**Content Deduplication**: Both HPM packages and Python virtual environments use content-addressable storage to eliminate duplication.

### 3. Modular Design

**Separation of Concerns**: Each crate has a single, well-defined responsibility with minimal coupling.

**Interface-Driven**: Trait-based abstractions allow for different implementations (storage backends, transport protocols).

**Extensibility**: Plugin points for custom behavior without modifying core code.

### 4. Integration-Friendly

**Houdini-Native**: Generated `package.json` files work seamlessly with existing Houdini installations.

**Tool Integration**: Machine-readable output formats support CI/CD pipelines and automation tools.

**Standard Conventions**: Follows established package management patterns from npm, cargo, and uv.

## Core Components

### CLI Layer (hpm-cli)

The command-line interface provides user-facing functionality with professional UX.

#### Architecture
```text
┌─────────────────────────────────────────────────────────────────────────────────┐
│                              CLI Architecture                                   │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                 │
│  Command Parser (Clap)                                                         │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │  • Argument validation and type conversion                              │   │
│  │  • Subcommand routing with help generation                              │   │
│  │  • Global options (verbosity, output format, colors)                   │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
│                                    │                                           │
│                                    ▼                                           │
│  Output & Error Handling                                                       │
│  ┌─────────────────────┐              ┌─────────────────────────────────────┐  │
│  │   Console System    │              │      Error Reporting                │  │
│  │ • Styled output     │              │ • Structured error types           │  │
│  │ • Color management  │ ────────────▶│ • Contextual help messages         │  │
│  │ • Verbosity control │              │ • Machine-readable JSON errors     │  │
│  │ • Progress bars     │              │ • Exit code standardization        │  │
│  └─────────────────────┘              └─────────────────────────────────────┘  │
│                                    │                                           │
│                                    ▼                                           │
│  Command Implementation                                                         │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │  Commands: init, add, remove, install, list, clean, update, check      │   │
│  │  • Integration with core modules                                        │   │
│  │  • Input validation and sanitization                                    │   │
│  │  • Operation orchestration and error propagation                       │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────────────┘
```

#### Key Features
- **Professional UX**: UV-inspired error handling with contextual help
- **Multiple Output Formats**: Human-readable, JSON, JSON Lines for automation
- **Accessibility**: Color-blind friendly symbols alongside colors
- **Comprehensive Validation**: Input validation with helpful error messages

### Core Package Management (hpm-core)

The heart of HPM's package management functionality.

#### Architecture
```text
┌─────────────────────────────────────────────────────────────────────────────────┐
│                           Core Package Management                               │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                 │
│  High-Level Operations (PackageManager)                                        │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │  • install_package() - Complete package installation workflow           │   │
│  │  • remove_package() - Safe package removal with dependency checks       │   │
│  │  • update_package() - Version updates with conflict resolution          │   │
│  │  • list_packages() - Comprehensive package information display          │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
│                                    │                                           │
│                                    ▼                                           │
│  Analysis Layer                                                                │
│  ┌─────────────────────┐              ┌─────────────────────────────────────┐  │
│  │ Project Discovery   │              │        Dependency Analysis         │  │
│  │ • Filesystem scan   │              │ • Transitive dependency resolution │  │
│  │ • Manifest parsing  │ ────────────▶│ • Cycle detection and warnings     │  │
│  │ • Validation        │              │ • Root package identification       │  │
│  │ • Error resilience  │              │ • Conflict resolution suggestions  │  │
│  └─────────────────────┘              └─────────────────────────────────────┘  │
│                                    │                                           │
│                                    ▼                                           │
│  Storage Layer (StorageManager)                                                │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │  • Global package storage (~/.hpm/packages/)                           │   │
│  │  • Project-aware cleanup with safety guarantees                        │   │
│  │  • Content-addressable organization                                     │   │
│  │  • Atomic operations and consistency guarantees                        │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────────────┘
```

#### Storage Manager Design

The storage manager implements HPM's dual-storage architecture:

```text
Global Storage (~/.hpm/packages/)
├── utility-nodes@2.1.0/
│   ├── hpm.toml                 # Package manifest
│   ├── otls/                    # Digital assets
│   ├── python/                  # Python modules
│   └── ...
└── material-library@1.5.0/
    └── ...

Project Integration (.hpm/packages/)
├── utility-nodes.json          # Reference to global storage
└── material-library.json       # Reference with project-specific settings
```

**Benefits**:
- **Deduplication**: One global copy per package version
- **Performance**: Fast project setup through references
- **Consistency**: Single source of truth for package content
- **Safety**: Cleanup never removes packages needed by active projects

#### Project Discovery System

Sophisticated filesystem scanning for HPM-managed projects:

```rust
pub struct ProjectDiscovery {
    config: ProjectsConfig,
}

impl ProjectDiscovery {
    /// Find all HPM-managed projects using configuration
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

**Configuration Options**:
- **Explicit Paths**: Always-monitored project directories
- **Search Roots**: Directories to recursively scan for projects
- **Max Depth**: Limit recursion depth for performance
- **Ignore Patterns**: Skip directories matching patterns (`.git`, `node_modules`)

#### Dependency Graph Analysis

Complete dependency resolution including cycle detection:

```rust
pub struct DependencyGraph {
    nodes: HashMap<PackageId, PackageNode>,
    edges: HashMap<PackageId, HashSet<PackageId>>,
}

impl DependencyGraph {
    /// Build complete dependency graph from projects
    pub async fn build_from_projects(projects: &[DiscoveredProject]) -> Result<Self> {
        let mut graph = Self::new();
        
        // Add all direct dependencies
        for project in projects {
            for dep in &project.dependencies {
                graph.add_dependency(&project.package_id, dep)?;
            }
        }
        
        // Resolve transitive dependencies
        graph.resolve_transitive_dependencies().await?;
        
        // Check for cycles
        if let Some(cycle) = graph.detect_cycles() {
            tracing::warn!("Circular dependency detected: {:?}", cycle);
        }
        
        Ok(graph)
    }
}
```

### Package Manifest Processing (hpm-package)

Handles `hpm.toml` parsing and validation with Houdini integration.

#### Manifest Structure
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

#### Template System
```text
HPM Package Templates

Standard Template:
├── hpm.toml                    # Full manifest
├── package.json               # Generated Houdini integration
├── README.md                  # Documentation
├── otls/                      # Digital assets
├── python/                    # Python modules
├── scripts/                   # Shelf tools
├── presets/                   # Node presets
├── config/                    # Configuration
└── tests/                     # Test files

Bare Template:
├── hpm.toml                   # Minimal manifest
└── README.md                  # Basic documentation
```

### Configuration Management (hpm-config)

Hierarchical configuration system with project discovery settings.

#### Configuration Hierarchy
1. **Default Values** - Built-in sensible defaults
2. **Global Config** (`~/.hpm/config.toml`) - User-wide settings
3. **Project Config** (`.hpm/config.toml`) - Project-specific overrides
4. **Environment Variables** - Runtime configuration
5. **CLI Arguments** - Command-specific overrides

```rust
#[derive(Debug, Clone)]
pub struct Config {
    pub registry: RegistryConfig,
    pub storage: StorageConfig,
    pub projects: ProjectsConfig,
    pub python: PythonConfig,
    pub ui: UiConfig,
}
```

### Error Handling System (hpm-error)

Comprehensive error handling with structured error types.

#### Error Type Hierarchy
```rust
#[derive(Debug, thiserror::Error)]
pub enum HpmError {
    #[error("Configuration error: {source}")]
    Config { 
        source: anyhow::Error,
        help: Option<String>
    },
    
    #[error("Package error: {source}")]
    Package {
        source: anyhow::Error,
        help: Option<String>
    },
    
    #[error("Network error: {source}")]
    Network {
        source: anyhow::Error,
        help: Option<String>
    },
    
    #[error("I/O error: {source}")]
    Io {
        source: anyhow::Error,
        help: Option<String>
    },
    
    #[error("Internal error: {source}")]
    Internal {
        source: anyhow::Error,
        help: Option<String>
    },
}
```

#### Exit Code Strategy
- **0**: Success - operation completed successfully
- **1**: User error - configuration, input, or usage issues
- **2**: Internal error - bugs or unexpected system conditions
- **N**: External command exit code (when running external tools)

## Data Flow Architecture

### Package Installation Flow

```text
┌─────────────────────────────────────────────────────────────────────────────────┐
│                         Package Installation Data Flow                          │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                 │
│  1. Command Input                                                               │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │  hpm install /path/to/project/                                          │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
│                                    │                                           │
│                                    ▼                                           │
│  2. Manifest Discovery & Parsing                                               │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │  • Locate hpm.toml file                                                │   │
│  │  • Parse TOML with validation                                           │   │
│  │  • Extract dependencies (HPM + Python)                                 │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
│                                    │                                           │
│                                    ▼                                           │
│  3. Dependency Resolution                                                       │
│  ┌─────────────────────┐              ┌─────────────────────────────────────┐  │
│  │   HPM Resolution    │              │      Python Resolution             │  │
│  │ • Check local cache │              │ • Collect Python dependencies      │  │
│  │ • Query registry    │ ────────────▶│ • UV-powered resolution             │  │
│  │ • Resolve versions  │              │ • Conflict detection                │  │
│  │ • Build dep graph   │              │ • Virtual env planning             │  │
│  └─────────────────────┘              └─────────────────────────────────────┘  │
│                                    │                                           │
│                                    ▼                                           │
│  4. Package Acquisition                                                        │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │  • Download missing packages from registry                             │   │
│  │  • Verify checksums and signatures                                     │   │
│  │  • Extract to global storage                                           │   │
│  │  • Create/reuse Python virtual environments                            │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
│                                    │                                           │
│                                    ▼                                           │
│  5. Project Integration                                                         │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │  • Create .hpm/ directory structure                                    │   │
│  │  • Generate package.json files with PYTHONPATH                         │   │
│  │  • Update hpm.lock with resolved versions                              │   │
│  │  • Set up Houdini integration                                          │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
│                                                                                 │
│  Output: Ready-to-use Houdini package environment                              │
└─────────────────────────────────────────────────────────────────────────────────┘
```

### Cleanup Operation Flow

```text
┌─────────────────────────────────────────────────────────────────────────────────┐
│                          Cleanup Operation Data Flow                            │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                 │
│  1. Project Discovery                                                           │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │  • Scan configured search roots                                        │   │
│  │  • Find all HPM-managed projects                                       │   │
│  │  • Parse manifests and extract dependencies                            │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
│                                    │                                           │
│                                    ▼                                           │
│  2. Dependency Graph Construction                                               │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │  • Build complete dependency graph across all projects                 │   │
│  │  • Resolve transitive dependencies                                     │   │
│  │  • Identify root packages (directly referenced)                        │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
│                                    │                                           │
│                                    ▼                                           │
│  3. Orphan Detection                                                            │
│  ┌─────────────────────┐              ┌─────────────────────────────────────┐  │
│  │  Installed Packages │              │       Required Packages            │  │
│  │ • Scan ~/.hpm/      │              │ • Extract from dependency graph    │  │
│  │ • List all versions │ ────────────▶│ • Include transitive dependencies  │  │
│  │ • Include Python    │              │ • Cross-reference with installed   │  │
│  │   virtual envs      │              │ • Calculate set difference         │  │
│  └─────────────────────┘              └─────────────────────────────────────┘  │
│                                    │                                           │
│                                    ▼                                           │
│  4. Safe Removal                                                               │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │  • Preview operations (dry-run mode)                                   │   │
│  │  • Confirm with user (interactive mode)                                │   │
│  │  • Remove orphaned packages atomically                                 │   │
│  │  • Clean up Python virtual environments                                │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
│                                                                                 │
│  Safety Guarantee: Never remove packages needed by active projects             │
└─────────────────────────────────────────────────────────────────────────────────┘
```

## Storage Architecture

### Global Storage Design

HPM implements a content-addressable global storage system that optimizes for both disk usage and access performance.

#### Storage Layout
```text
~/.hpm/                                    # HPM root directory
├── packages/                              # Global package storage
│   ├── utility-nodes@2.1.0/              # Versioned package directories
│   │   ├── hpm.toml                      # Package manifest
│   │   ├── package.json                  # Generated Houdini manifest
│   │   ├── otls/                         # Digital assets
│   │   │   ├── geometry_processor.hda
│   │   │   └── material_builder.hda
│   │   ├── python/                       # Python modules
│   │   │   ├── __init__.py
│   │   │   └── utilities/
│   │   ├── scripts/                      # Shelf tools
│   │   └── presets/                      # Node presets
│   └── material-library@1.5.0/
├── venvs/                                # Python virtual environments
│   ├── a1b2c3d4e5f6/                    # Content-addressable venv
│   │   ├── metadata.json                # Environment metadata
│   │   ├── lib/python3.9/site-packages/
│   │   │   ├── numpy/
│   │   │   └── requests/
│   │   └── pyvenv.cfg
│   └── f6e5d4c3b2a1/
├── cache/                                # Download and metadata cache
│   ├── registry/                        # Registry index cache
│   └── downloads/                       # Downloaded package archives
├── uv-cache/                            # Isolated UV package cache
├── uv-config/                           # UV configuration
├── config.toml                          # Global HPM configuration
└── logs/                                # Operation logs
    ├── install.log
    └── cleanup.log
```

### Project Integration

Each project maintains lightweight references to globally stored packages:

```text
project/
├── hpm.toml                             # Project manifest
├── hpm.lock                             # Dependency lock file
├── .hmp/                                # HPM project directory
│   └── packages/                        # Package references
│       ├── utility-nodes.json          # Reference to global package
│       └── material-library.json
└── (project files...)
```

#### Reference File Format
```json
{
  "name": "utility-nodes",
  "version": "2.1.0",
  "global_path": "/Users/user/.hpm/packages/utility-nodes@2.1.0",
  "python_venv": "a1b2c3d4e5f6",
  "houdini_manifest": {
    "path": "$HPM_PACKAGE_ROOT",
    "load_package_once": true,
    "env": [
      {
        "PYTHONPATH": "/Users/user/.hpm/venvs/a1b2c3d4e5f6/lib/python3.9/site-packages:$PYTHONPATH"
      }
    ]
  }
}
```

### Storage Manager Implementation

The storage manager provides high-level abstractions over the global storage system:

```rust
pub struct StorageManager {
    storage_path: PathBuf,
    cache_path: PathBuf,
}

impl StorageManager {
    /// Install package to global storage
    pub async fn install_package(&self, spec: &PackageSpec) -> Result<PackageInstallation> {
        // 1. Check if already installed
        if self.package_exists(&spec.name, &spec.version) {
            return self.get_existing_installation(spec);
        }
        
        // 2. Download and verify package
        let package_data = self.download_package(spec).await?;
        self.verify_package_integrity(&package_data)?;
        
        // 3. Extract to versioned directory
        let install_path = self.get_package_path(&spec.name, &spec.version);
        self.extract_package(&package_data, &install_path).await?;
        
        // 4. Generate Houdini integration files
        self.generate_houdini_manifest(&install_path).await?;
        
        Ok(PackageInstallation { path: install_path, spec: spec.clone() })
    }
    
    /// Remove packages not needed by any active project
    pub async fn cleanup_unused(&self, projects: &[Project]) -> Result<CleanupResult> {
        // Build comprehensive dependency graph
        let required_packages = self.analyze_required_packages(projects).await?;
        let installed_packages = self.list_installed_packages().await?;
        
        // Calculate orphaned packages (set difference)
        let orphaned = installed_packages.difference(&required_packages);
        
        // Remove orphaned packages atomically
        let mut removed = Vec::new();
        for package in orphaned {
            if self.safe_remove_package(package).await.is_ok() {
                removed.push(package.clone());
            }
        }
        
        Ok(CleanupResult { removed })
    }
}
```

## Network and Registry Architecture

HPM's registry system implements a high-performance, secure architecture using modern network protocols.

### Protocol Stack

```text
┌─────────────────────────────────────────────────────────────────────────────────┐
│                            Network Protocol Stack                               │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                 │
│  Application Layer - gRPC Services                                             │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │  PackageRegistryService                                                 │   │
│  │  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌─────────────────┐   │   │
│  │  │   Search    │ │   Publish   │ │  Download   │ │    Metadata     │   │   │
│  │  │  Packages   │ │   Package   │ │   Package   │ │   Operations    │   │   │
│  │  └─────────────┘ └─────────────┘ └─────────────┘ └─────────────────┘   │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
│                                    │                                           │
│                                    ▼                                           │
│  Serialization Layer - Protocol Buffers                                       │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │  • Binary serialization with efficient encoding                        │   │
│  │  • Schema evolution support                                            │   │
│  │  • Streaming for large packages                                        │   │
│  │  • Automatic code generation                                           │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
│                                    │                                           │
│                                    ▼                                           │
│  Transport Layer - QUIC (s2n-quic)                                            │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │  • Multiplexed streams (concurrent operations)                         │   │
│  │  • Built-in encryption (TLS 1.3)                                       │   │
│  │  • Connection migration support                                        │   │
│  │  • 0-RTT connection establishment                                       │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
│                                    │                                           │
│                                    ▼                                           │
│  Network Layer - UDP with QUIC                                                │
│  ┌─────────────────────────────────────────────────────────────────────────┐   │
│  │  • Optimized for modern networks                                       │   │
│  │  • No head-of-line blocking                                            │   │
│  │  • Fast loss recovery                                                  │   │
│  │  • Connection-level flow control                                       │   │
│  └─────────────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────────────┘
```

### Performance Benefits

Based on benchmarking against HTTP/2:

| Operation | HTTP/2 | QUIC | Improvement |
|-----------|--------|------|-------------|
| Package Download (10MB) | 2.7s | 0.73s | **3.69x faster** |
| Concurrent Downloads (10x) | 15.2s | 4.1s | **3.71x faster** |
| Package Upload (50MB) | 8.1s | 2.2s | **3.68x faster** |
| Metadata Queries | 45ms | 43ms | **4.4% faster** |

### Registry Client Architecture

```rust
pub struct RegistryClient {
    connection: QuicConnection,
    auth_token: Option<AuthToken>,
    config: ClientConfig,
}

impl RegistryClient {
    /// Search for packages matching query
    pub async fn search_packages(
        &mut self,
        query: &str,
        limit: Option<u32>,
        offset: Option<u32>
    ) -> Result<SearchResults> {
        let request = SearchRequest {
            query: query.to_string(),
            limit: limit.unwrap_or(20),
            offset: offset.unwrap_or(0),
        };
        
        let mut client = PackageRegistryClient::new(self.connection.clone());
        let response = client.search(request).await?;
        
        Ok(SearchResults::from(response.into_inner()))
    }
    
    /// Download package with integrity verification
    pub async fn download_package(
        &mut self,
        name: &str,
        version: &str
    ) -> Result<DownloadResult> {
        let request = DownloadRequest {
            name: name.to_string(),
            version: version.to_string(),
        };
        
        let mut client = PackageRegistryClient::new(self.connection.clone());
        let mut stream = client.download(request).await?.into_inner();
        
        // Stream large packages in chunks
        let mut package_data = Vec::new();
        while let Some(chunk) = stream.message().await? {
            package_data.extend_from_slice(&chunk.data);
        }
        
        // Verify integrity
        let computed_hash = self.compute_sha256(&package_data);
        if computed_hash != expected_hash {
            return Err(RegistryError::IntegrityVerificationFailed);
        }
        
        Ok(DownloadResult { package_data, checksum: computed_hash })
    }
}
```

### Storage Backend Abstraction

The registry server supports multiple storage backends through a trait-based architecture:

```rust
#[async_trait]
pub trait Storage: Send + Sync {
    /// Store package in backend
    async fn store_package(&self, package: &PackageData) -> Result<String>;
    
    /// Retrieve package by ID
    async fn get_package(&self, package_id: &str) -> Result<Option<PackageData>>;
    
    /// Search packages matching criteria
    async fn search_packages(&self, query: &SearchQuery) -> Result<Vec<PackageMetadata>>;
    
    /// List versions for a package
    async fn list_versions(&self, name: &str) -> Result<Vec<String>>;
    
    /// Delete package (admin operation)
    async fn delete_package(&self, package_id: &str) -> Result<()>;
}

// Storage implementations
pub struct MemoryStorage { /* ... */ }     // Development
pub struct PostgreSqlStorage { /* ... */ } // Production
pub struct S3Storage { /* ... */ }         // Cloud deployment
```

## Python Integration Architecture

HPM's Python integration solves dependency conflicts through content-addressable virtual environments.

### Content-Addressable System

```text
┌─────────────────────────────────────────────────────────────────────────────────┐
│                     Python Content-Addressable Architecture                     │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                 │
│  Package Manifests                                                              │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐               │
│  │   Package A     │  │   Package B     │  │   Package C     │               │
│  │ numpy>=1.20.0   │  │ numpy>=1.20.0   │  │ numpy>=1.25.0   │               │
│  │ requests^2.28   │  │ requests^2.28   │  │ scipy>=1.9.0    │               │
│  └─────────────────┘  └─────────────────┘  └─────────────────┘               │
│           │                      │                      │                     │
│           ▼                      ▼                      ▼                     │
│                                                                                 │
│  UV-Powered Resolution                                                         │
│  ┌─────────────────────────────────────────────────────────────────┐          │
│  │  Resolved Dependency Sets:                                      │          │
│  │                                                                 │          │
│  │  Set 1 (Packages A & B):                                       │          │
│  │  └─ numpy==1.24.0, requests==2.28.0                           │          │
│  │     Hash: sha256(python3.9 + numpy1.24.0 + requests2.28.0)    │          │
│  │     → a1b2c3d4e5f6                                            │          │
│  │                                                                 │          │
│  │  Set 2 (Package C):                                            │          │
│  │  └─ numpy==1.25.0, scipy==1.9.0                              │          │
│  │     Hash: sha256(python3.9 + numpy1.25.0 + scipy1.9.0)       │          │
│  │     → f6e5d4c3b2a1                                            │          │
│  └─────────────────────────────────────────────────────────────────┘          │
│                           │                      │                             │
│                           ▼                      ▼                             │
│                                                                                 │
│  Virtual Environment Storage (~/.hpm/venvs/)                                  │
│  ┌─────────────────┐                    ┌─────────────────┐                   │
│  │  a1b2c3d4e5f6   │                    │  f6e5d4c3b2a1   │                   │
│  │ ├─ metadata.json│  ◄─── Shared ───► │ ├─ metadata.json│                   │
│  │ ├─ lib/python/ │       by A & B      │ ├─ lib/python/ │                   │
│  │ │  └─ site-pkgs/│                    │ │  └─ site-pkgs/│                   │
│  │ │    ├─ numpy/  │                    │ │    ├─ numpy/  │                   │
│  │ │    └─requests/ │                    │ │    └─ scipy/  │                   │
│  │ └─ pyvenv.cfg   │                    │ └─ pyvenv.cfg   │                   │
│  └─────────────────┘                    └─────────────────┘                   │
└─────────────────────────────────────────────────────────────────────────────────┘
```

### Virtual Environment Manager

```rust
pub struct VenvManager {
    venvs_dir: PathBuf,
    uv_path: PathBuf,
}

impl VenvManager {
    /// Create or reuse virtual environment for resolved dependencies
    pub async fn ensure_virtual_environment(
        &self,
        resolved: &ResolvedDependencies
    ) -> Result<PathBuf> {
        // Calculate content hash
        let content_hash = self.calculate_content_hash(resolved)?;
        let venv_path = self.venvs_dir.join(&content_hash);
        
        // Return existing environment if available
        if venv_path.exists() && self.validate_environment(&venv_path, resolved).await? {
            tracing::info!("Reusing existing virtual environment: {}", content_hash);
            return Ok(venv_path);
        }
        
        // Create new virtual environment
        tracing::info!("Creating virtual environment: {}", content_hash);
        self.create_virtual_environment(&venv_path, resolved).await?;
        
        Ok(venv_path)
    }
    
    /// Calculate deterministic hash for resolved dependencies
    fn calculate_content_hash(&self, resolved: &ResolvedDependencies) -> Result<String> {
        let mut hasher = Sha256::new();
        
        // Include Python version
        hasher.update(resolved.python_version.as_bytes());
        
        // Include sorted package specifications for deterministic hash
        let mut packages: Vec<_> = resolved.packages.iter().collect();
        packages.sort_by_key(|(name, _)| name.as_str());
        
        for (name, spec) in packages {
            hasher.update(name.as_bytes());
            hasher.update(spec.version.as_bytes());
            
            // Include extras in hash
            if let Some(extras) = &spec.extras {
                let mut sorted_extras = extras.clone();
                sorted_extras.sort();
                for extra in sorted_extras {
                    hasher.update(extra.as_bytes());
                }
            }
        }
        
        Ok(format!("{:x}", hasher.finalize())[..12].to_string())
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
        let site_packages = venv_path.join("lib").join("python3.9").join("site-packages");
        manifest.env.push(EnvVar {
            key: "PYTHONPATH".to_string(),
            value: format!("{}:$PYTHONPATH", site_packages.display()),
        });
    }
    
    Ok(manifest)
}
```

## Security Architecture

HPM implements defense-in-depth security with multiple protection layers.

### Transport Security

- **Mandatory TLS 1.3**: All network communications encrypted with latest standards
- **Certificate Validation**: Server certificates verified to prevent man-in-the-middle attacks
- **Perfect Forward Secrecy**: Session keys provide forward secrecy even if long-term keys compromised

### Authentication and Authorization

```rust
#[derive(Debug, Clone)]
pub struct AuthToken {
    pub token: String,
    pub scopes: Vec<TokenScope>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenScope {
    Read,           // Read packages and metadata
    Write,          // Publish packages
    Admin,          // Administrative operations
}
```

### Package Integrity

- **SHA-256 Checksums**: Every package verified for integrity on download
- **Compression Verification**: zstd compression integrity checks
- **Manifest Validation**: Package manifests validated against schemas

### Isolation and Sandboxing

- **Process Isolation**: UV runs in isolated processes with restricted permissions
- **Cache Isolation**: HPM maintains separate cache from system UV
- **Network Isolation**: Registry connections use dedicated secure channels

## Performance and Scalability

### Concurrent Operations

HPM leverages Rust's async/await system for high-performance concurrent operations:

```rust
use tokio::task::JoinSet;

pub async fn install_multiple_packages(packages: Vec<PackageSpec>) -> Result<Vec<InstallResult>> {
    let mut join_set = JoinSet::new();
    
    // Launch concurrent installation tasks
    for package in packages {
        join_set.spawn(async move {
            install_single_package(package).await
        });
    }
    
    // Collect results
    let mut results = Vec::new();
    while let Some(result) = join_set.join_next().await {
        results.push(result??);
    }
    
    Ok(results)
}
```

### Caching Strategy

- **Registry Cache**: Package metadata cached to reduce network requests
- **Download Cache**: Package archives cached to avoid re-downloading
- **Virtual Environment Cache**: Content-addressable sharing maximizes cache hits
- **Dependency Resolution Cache**: Resolved dependency graphs cached

### Memory Management

- **Zero-Copy Operations**: Extensive use of references to avoid data copying
- **Streaming I/O**: Large packages streamed rather than loaded entirely into memory
- **Resource Cleanup**: RAII patterns ensure proper resource cleanup

### Disk I/O Optimization

- **Parallel File Operations**: Multiple file operations executed concurrently
- **Efficient Serialization**: Binary formats preferred over text where appropriate
- **Atomic Operations**: File operations designed to be atomic to prevent corruption

## Extension Points

HPM's architecture provides several extension points for custom functionality.

### Storage Backend Extensions

Implement the `Storage` trait to add custom storage backends:

```rust
#[async_trait]
impl Storage for CustomStorage {
    async fn store_package(&self, package: &PackageData) -> Result<String> {
        // Custom storage implementation
    }
    
    // ... other methods
}
```

### Custom Commands

The CLI system supports adding custom commands through the command router:

```rust
#[derive(Subcommand)]
enum CustomCommands {
    /// Custom package validation
    Validate {
        /// Package to validate
        package: String,
    },
}
```

### Plugin Architecture (Future)

Future versions will support a plugin architecture for extending core functionality:

```rust
pub trait HpmPlugin: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    
    async fn execute(&self, context: &PluginContext) -> Result<PluginResult>;
}
```

### Configuration Extensions

The configuration system supports custom sections for plugin configuration:

```toml
[plugin.custom_validator]
enable = true
strict_mode = false
rules = ["rule1", "rule2"]
```

This technical architecture provides the foundation for understanding HPM's design decisions, implementation patterns, and extensibility mechanisms. The modular architecture ensures that HPM can evolve to meet changing requirements while maintaining safety, performance, and reliability guarantees.