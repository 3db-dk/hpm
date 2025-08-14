# HPM Python Dependency Management Design

## Executive Summary

HPM requires sophisticated Python dependency management to handle conflicting Python package requirements across multiple Houdini packages. This design document outlines a comprehensive solution using virtual environment isolation, dependency resolution through bundled UV, and seamless Houdini integration.

## Problem Statement

When a Houdini project uses multiple HPM packages with Python dependencies, conflicts can arise:

- Package A requires `numpy>=1.20, <1.21`
- Package B requires `numpy>=1.25`
- Traditional approach fails: Houdini's Python context cannot handle multiple versions simultaneously

Current limitations:
- No dependency resolution mechanism for Python packages
- No isolation between packages' Python dependencies  
- No cleanup system for Python virtual environments
- Manual PYTHONPATH management required

## Research Findings

### Houdini Python Integration

Houdini's Python package loading mechanism:
- Automatically adds `$HOUDINI_PATH/pythonX.Xlibs` directories to Python path
- Uses `PYTHONPATH` environment variable for additional module locations
- Package.json files can set environment variables via `env` array
- Python scripts run in shared context across all loaded packages

### Modern Python Dependency Management (2024)

Key trends and capabilities:
- **Virtual environments** remain the primary isolation mechanism
- **UV package manager**: Rust-based, 80x faster than traditional tools, comprehensive project management
- **Shared dependency storage**: PNPM-style approach reduces disk usage
- **Lock files**: Ensure reproducible builds across environments
- **Content-addressable storage**: Efficient package deduplication

### Virtual Environment Management Patterns

Leading package managers use:
- Hash-based environment identification for sharing compatible environments
- Global storage with project-specific linking
- Automatic cleanup of orphaned environments
- Performance optimization through Rust implementations

## Proposed Architecture

### Core Components

#### 1. HPM Python Crate (`crates/hpm-python/`)

New dedicated crate for Python dependency management:

```
crates/hpm-python/src/
├── lib.rs              # Public API and module exports
├── resolver.rs         # Dependency resolution using bundled UV
├── venv.rs            # Virtual environment creation and management
├── integration.rs      # Houdini package.json generation
├── cleanup.rs         # Virtual environment cleanup logic
├── bundled.rs         # Bundled UV binary management
└── types.rs           # Python-specific data structures
```

#### 2. Storage Architecture

Extended global storage structure:

```
~/.hpm/
├── packages/                    # Existing package storage
│   ├── package-a@1.0.0/
│   └── package-b@2.0.0/
├── venvs/                      # Virtual environment storage
│   ├── {hash1}/               # Shared venv for compatible dependency sets
│   │   ├── metadata.json     # Tracks which packages use this venv
│   │   ├── pyvenv.cfg        # Standard Python venv configuration
│   │   ├── bin/              # Python executables
│   │   └── lib/              # Installed Python packages
│   └── {hash2}/
├── tools/
│   └── uv                     # Bundled UV binary
├── uv-cache/                   # UV package cache (isolated from system UV)
├── uv-config/                  # UV configuration (isolated from system UV)
└── cache/                     # Existing cache directory
```

### UV Isolation Strategy

**Critical Requirement**: HPM's UV usage must be completely isolated from any existing system UV installation to prevent interference with user's existing Python workflows.

**Isolation Mechanisms**:

1. **Dedicated UV Binary**: HPM bundles its own UV binary in `~/.hpm/tools/uv`
2. **Isolated Cache**: All UV cache operations directed to `~/.hpm/uv-cache/`
3. **Isolated Configuration**: UV configuration stored in `~/.hpm/uv-config/`
4. **Environment Variables**: HPM controls all UV-related environment variables during execution

**Environment Variable Override**:
```rust
// Set UV environment variables for complete isolation
env::set_var("UV_CACHE_DIR", hpm_dir.join("uv-cache"));
env::set_var("UV_CONFIG_FILE", hpm_dir.join("uv-config/uv.toml"));
env::set_var("UV_NO_SYNC", "1");  // Prevent UV from syncing with system
```

**Benefits**:
- No interference with existing UV installations
- Predictable behavior regardless of system state
- Complete control over UV operations and storage
- User's existing Python workflows remain unaffected

### Virtual Environment Sharing Strategy

#### Environment Identification

Virtual environments are identified by SHA-256 hash of resolved dependency set:

```rust
pub struct ResolvedDependencySet {
    pub packages: BTreeMap<String, String>,  // name -> exact_version
    pub python_version: String,              // Python version requirement
}

impl ResolvedDependencySet {
    pub fn hash(&self) -> String {
        // Create deterministic hash from sorted dependencies and Python version
        let mut hasher = Sha256::new();
        hasher.update(format!("python:{}", self.python_version));
        for (name, version) in &self.packages {
            hasher.update(format!("{}:{}", name, version));
        }
        format!("{:x}", hasher.finalize())[..16].to_string()
    }
}
```

#### Sharing Logic

Packages with compatible dependency sets automatically share virtual environments:

1. HPM collects all Python dependencies from packages in a project
2. UV resolves dependencies to exact versions
3. Hash identifies existing compatible environment or creates new one
4. Multiple packages can reference the same environment

Example:
- Package A: `numpy==1.24.0, scipy==1.10.0` → `venv_abc123`
- Package B: `numpy==1.24.0, matplotlib==3.7.0` → `venv_def456`  
- Package C: `numpy==1.24.0, scipy==1.10.0` → `venv_abc123` (shared with A)

### Dependency Resolution Workflow

#### 1. Collection Phase
```rust
pub async fn collect_python_dependencies(
    packages: &[PackageManifest]
) -> Result<HashMap<String, VersionSpec>> {
    let mut all_deps = HashMap::new();
    
    for package in packages {
        if let Some(python_deps) = &package.python_dependencies {
            for (name, spec) in python_deps {
                // Merge version specifications, detecting conflicts
                merge_version_spec(&mut all_deps, name, spec)?;
            }
        }
    }
    
    Ok(all_deps)
}
```

#### 2. Resolution Phase
```rust
pub async fn resolve_dependencies(
    dependencies: &HashMap<String, VersionSpec>,
    python_version: &str
) -> Result<ResolvedDependencySet> {
    let uv_path = ensure_uv_binary().await?;
    
    // Create temporary requirements file
    let req_file = create_requirements_file(dependencies)?;
    
    // Run UV to resolve dependencies
    let output = Command::new(uv_path)
        .args(["pip", "compile", req_file.path()])
        .args(["--python-version", python_version])
        .output()
        .await?;
        
    parse_resolved_dependencies(&output.stdout)
}
```

#### 3. Environment Creation
```rust
pub async fn ensure_virtual_environment(
    resolved_deps: &ResolvedDependencySet
) -> Result<PathBuf> {
    let hash = resolved_deps.hash();
    let venv_path = get_venv_path(&hash);
    
    if !venv_path.exists() {
        create_virtual_environment(&venv_path, resolved_deps).await?;
    }
    
    update_venv_metadata(&venv_path, resolved_deps).await?;
    Ok(venv_path)
}

async fn create_virtual_environment(
    venv_path: &Path,
    resolved_deps: &ResolvedDependencySet
) -> Result<()> {
    let uv_path = ensure_uv_binary().await?;
    
    // Create virtual environment
    Command::new(uv_path)
        .args(["venv", venv_path])
        .args(["--python", &resolved_deps.python_version])
        .spawn()?
        .wait()
        .await?;
    
    // Install resolved dependencies
    let req_file = create_resolved_requirements_file(resolved_deps)?;
    Command::new(uv_path)
        .args(["pip", "install", "-r", req_file.path()])
        .args(["--target", &venv_path.join("lib/python/site-packages")])
        .spawn()?
        .wait()
        .await?;
    
    Ok(())
}
```

### Houdini Integration

#### Package.json Generation Enhancement

Existing package.json generation extended to include Python environment:

```rust
pub fn generate_houdini_package_json(
    manifest: &PackageManifest,
    venv_path: Option<&Path>
) -> Result<Value> {
    let mut package_json = json!({
        "path": "$HPM_PACKAGE_ROOT"
    });
    
    if let Some(venv) = venv_path {
        let python_path = get_python_site_packages_path(venv)?;
        package_json["env"] = json!([
            {
                "PYTHONPATH": format!(
                    "{}{}$PYTHONPATH", 
                    python_path.display(),
                    get_path_separator()
                )
            }
        ]);
    }
    
    Ok(package_json)
}

#[cfg(target_os = "windows")]
fn get_path_separator() -> &'static str { ";" }

#[cfg(not(target_os = "windows"))]
fn get_path_separator() -> &'static str { ":" }
```

#### Cross-Platform Path Handling

Platform-specific PYTHONPATH generation:

- **Unix/macOS**: `{venv}/lib/python3.x/site-packages:$PYTHONPATH`
- **Windows**: `{venv}\Lib\site-packages;%PYTHONPATH%`

### Bundled UV Management

#### Binary Distribution Strategy

UV binaries bundled as embedded resources:

```rust
// Embed UV binaries at compile time
#[cfg(target_os = "windows")]
static UV_BINARY: &[u8] = include_bytes!("../resources/uv-x86_64-pc-windows-msvc.exe");

#[cfg(target_os = "macos")]  
static UV_BINARY: &[u8] = include_bytes!("../resources/uv-x86_64-apple-darwin");

#[cfg(target_os = "linux")]
static UV_BINARY: &[u8] = include_bytes!("../resources/uv-x86_64-unknown-linux-gnu");

pub async fn ensure_uv_binary() -> Result<PathBuf> {
    let hpm_dir = get_hpm_dir();
    let uv_path = hpm_dir.join("tools/uv");
    
    if !uv_path.exists() {
        extract_uv_binary(&uv_path).await?;
        set_executable_permissions(&uv_path)?;
        setup_uv_isolation(&hpm_dir).await?;
    }
    
    Ok(uv_path)
}

async fn extract_uv_binary(target_path: &Path) -> Result<()> {
    fs::create_dir_all(target_path.parent().unwrap()).await?;
    fs::write(target_path, UV_BINARY).await?;
    Ok(())
}

async fn setup_uv_isolation(hpm_dir: &Path) -> Result<()> {
    // Create isolated UV directories
    fs::create_dir_all(hpm_dir.join("uv-cache")).await?;
    fs::create_dir_all(hpm_dir.join("uv-config")).await?;
    
    // Create isolated UV configuration
    let uv_config = r#"
[cache]
dir = "$HPM_DIR/uv-cache"

[global]
# Prevent UV from interfering with system installations
no-cache-dir = false
no-python-downloads = false
"#;
    
    fs::write(
        hpm_dir.join("uv-config/uv.toml"),
        uv_config.replace("$HPM_DIR", &hpm_dir.to_string_lossy())
    ).await?;
    
    Ok(())
}

pub fn setup_uv_environment(hpm_dir: &Path) {
    // Set UV environment variables for complete isolation
    env::set_var("UV_CACHE_DIR", hpm_dir.join("uv-cache"));
    env::set_var("UV_CONFIG_FILE", hpm_dir.join("uv-config/uv.toml"));
    env::set_var("UV_NO_SYNC", "1");
    env::set_var("UV_SYSTEM_PYTHON", "0"); // Prevent system Python detection
}
```

#### Version Management

UV binary updates handled through HPM releases:
- New UV versions included in HPM binary releases
- Automatic extraction on first use or version mismatch
- **No fallback to system UV**: Complete isolation maintained
- Version compatibility verified during extraction

### Cleanup System Integration

#### Virtual Environment Tracking

Extension of existing cleanup system to handle Python environments:

```rust
pub struct PythonCleanupAnalyzer {
    venv_manager: VirtualEnvironmentManager,
}

impl PythonCleanupAnalyzer {
    pub async fn analyze_orphaned_venvs(
        &self,
        active_packages: &[PackageInfo]
    ) -> Result<Vec<OrphanedVenv>> {
        let mut active_venvs = HashSet::new();
        
        // Collect venvs referenced by active packages
        for package in active_packages {
            if let Some(venv_hash) = self.get_package_venv_hash(package).await? {
                active_venvs.insert(venv_hash);
            }
        }
        
        // Find orphaned venvs
        let all_venvs = self.list_all_venvs().await?;
        let orphaned = all_venvs
            .into_iter()
            .filter(|venv| !active_venvs.contains(&venv.hash))
            .collect();
            
        Ok(orphaned)
    }
    
    pub async fn cleanup_orphaned_venvs(
        &self,
        orphaned_venvs: &[OrphanedVenv],
        dry_run: bool
    ) -> Result<CleanupResult> {
        let mut result = CleanupResult::default();
        
        for venv in orphaned_venvs {
            if dry_run {
                result.would_remove.push(venv.path.clone());
            } else {
                self.remove_venv(&venv.path).await?;
                result.removed.push(venv.path.clone());
            }
            
            result.space_freed += venv.size;
        }
        
        Ok(result)
    }
}
```

#### CLI Integration

Extended `hpm clean` command:

```bash
# Clean packages and Python environments
hpm clean --dry-run                    # Preview all cleanup
hpm clean --python-only --dry-run      # Preview Python cleanup only
hpm clean --yes                        # Clean everything
RUST_LOG=debug hpm clean --dry-run     # Debug cleanup analysis
```

### Error Handling and Edge Cases

#### Dependency Resolution Failures

```rust
#[derive(Debug, thiserror::Error)]
pub enum PythonDependencyError {
    #[error("Conflicting Python dependencies: {conflicts:?}")]
    ConflictingDependencies { conflicts: Vec<DependencyConflict> },
    
    #[error("Python version {required} not compatible with Houdini {houdini_version}")]
    IncompatiblePythonVersion { required: String, houdini_version: String },
    
    #[error("Failed to resolve dependencies: {message}")]
    ResolutionFailed { message: String },
    
    #[error("UV binary not found and extraction failed: {source}")]
    UvNotAvailable { source: Box<dyn std::error::Error> },
}
```

#### Concurrent Access Protection

```rust
use tokio::sync::Mutex;
use std::collections::HashMap;

lazy_static! {
    static ref VENV_LOCKS: Mutex<HashMap<String, Arc<Mutex<()>>>> = 
        Mutex::new(HashMap::new());
}

pub async fn with_venv_lock<F, R>(hash: &str, f: F) -> Result<R>
where
    F: Future<Output = Result<R>>,
{
    let lock = {
        let mut locks = VENV_LOCKS.lock().await;
        locks.entry(hash.to_string())
             .or_insert_with(|| Arc::new(Mutex::new(())))
             .clone()
    };
    
    let _guard = lock.lock().await;
    f.await
}
```

## Implementation Strategy

### Phase 1: Foundation (Milestone 1)
- [ ] Create `crates/hpm-python` crate
- [ ] Implement UV binary bundling and extraction
- [ ] Basic dependency collection from package manifests
- [ ] Virtual environment creation and management

### Phase 2: Core Functionality (Milestone 2)
- [ ] Dependency resolution using UV
- [ ] Virtual environment sharing based on dependency hashes
- [ ] Enhanced package.json generation with PYTHONPATH
- [ ] Basic cleanup integration

### Phase 3: Advanced Features (Milestone 3)  
- [ ] Comprehensive error handling and conflict resolution
- [ ] Cross-platform path handling and testing
- [ ] Performance optimization and caching
- [ ] Advanced cleanup with orphan detection

### Phase 4: Production Readiness (Milestone 4)
- [ ] Comprehensive testing suite including integration tests
- [ ] Documentation and usage guides
- [ ] Performance benchmarking
- [ ] Security audit of bundled binaries

## Testing Strategy

### Unit Tests
- Dependency collection and merging logic
- Virtual environment hash generation
- Package.json generation with PYTHONPATH
- Cleanup analysis for orphaned environments

### Integration Tests  
- End-to-end dependency resolution workflow
- Virtual environment creation and sharing
- Houdini package loading with Python dependencies
- Cleanup system with real filesystem operations

### Cross-Platform Testing
- PYTHONPATH generation on Windows/Unix
- UV binary extraction and execution
- File permission handling
- Path separator handling

## Security Considerations

### Bundled Binary Safety
- UV binaries verified with SHA-256 checksums
- Secure extraction to user-controlled directory
- No elevation of privileges required
- Regular updates through HPM releases

### Virtual Environment Isolation
- Each environment isolated in separate directory
- No shared state between environments  
- Cleanup system respects environment boundaries
- No modification of system Python installation

## Performance Characteristics

### Expected Performance Benefits
- **Shared environments**: Reduce disk usage by up to 80% for compatible packages
- **UV speed**: 80x faster dependency resolution compared to pip
- **Caching**: Global package cache reduces download times
- **Parallel operations**: Concurrent environment creation when possible

### Resource Usage
- **Disk space**: Shared environments minimize duplication
- **Memory**: Lazy loading of Python packages in Houdini
- **Network**: Cached packages reduce bandwidth usage
- **CPU**: Rust implementation provides efficient operations

## Future Enhancements

### Advanced Dependency Resolution
- Support for optional dependencies and extras
- Conflict resolution with user intervention prompts
- Integration with conda packages for scientific computing

### Enhanced Houdini Integration
- Automatic Python module discovery and registration
- Support for Houdini-specific Python package types
- Integration with Houdini's plugin system

### Registry Integration
- Python package metadata in HPM registry
- Dependency security scanning
- Package compatibility verification

## Conclusion

This design provides comprehensive Python dependency management for HPM through:

- **Isolation**: Virtual environments prevent dependency conflicts
- **Efficiency**: Shared environments optimize disk usage  
- **Performance**: UV-based resolution provides fast dependency handling
- **Integration**: Seamless Houdini compatibility through package.json enhancement
- **Maintenance**: Automated cleanup prevents environment proliferation

The architecture leverages modern Python tooling while maintaining compatibility with Houdini's existing package system, providing a robust foundation for complex Python dependency scenarios.