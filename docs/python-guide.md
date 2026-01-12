# HPM Python Dependencies Guide

This guide covers Python dependency management for Houdini packages, including usage, configuration, and technical details.

## Table of Contents

- [Overview](#overview)
- [Quick Start](#quick-start)
- [Python Dependency Specifications](#python-dependency-specifications)
- [Houdini Version Mapping](#houdini-version-mapping)
- [Virtual Environment Sharing](#virtual-environment-sharing)
- [Package Management](#package-management)
- [Cleanup and Maintenance](#cleanup-and-maintenance)
- [Troubleshooting](#troubleshooting)
- [Best Practices](#best-practices)
- [Technical Architecture](#technical-architecture)

## Overview

HPM provides comprehensive Python dependency management for Houdini packages, solving the common problem of conflicting Python package requirements across multiple packages. Key features:

- **Automatic dependency resolution** using UV (80x faster than pip)
- **Virtual environment isolation** prevents conflicts between packages
- **Content-addressable sharing** optimizes disk usage
- **Seamless Houdini integration** via automatic PYTHONPATH injection

## Quick Start

### 1. Define Python Dependencies

Add Python dependencies to your `hpm.toml` file:

```toml
[package]
name = "my-geometry-tools"
version = "1.0.0"
description = "Advanced geometry processing tools"

[houdini]
min_version = "20.0"  # Automatically maps to Python 3.9

[python_dependencies]
numpy = ">=1.20.0"
scipy = ">=1.7.0"
matplotlib = "^3.5.0"
```

### 2. Install Your Package

When you install a package with Python dependencies, HPM automatically:

1. Resolves all Python dependencies to exact versions
2. Creates or reuses a virtual environment based on the resolved dependencies
3. Installs all Python packages into the virtual environment
4. Generates the appropriate Houdini `package.json` with PYTHONPATH integration

```bash
hpm install
```

### 3. Use Python Packages in Houdini

Once installed, your Python packages are automatically available in Houdini's Python context:

```python
import hou
import numpy as np
import scipy.spatial

# Your Python dependencies are ready to use
points = np.array([[0, 0, 0], [1, 1, 1], [2, 2, 2]])
tree = scipy.spatial.KDTree(points)
```

## Python Dependency Specifications

### Simple Version Requirements

```toml
[python_dependencies]
numpy = ">=1.20.0"           # Minimum version
requests = "^2.25.0"         # Compatible version (>= 2.25.0, < 3.0.0)
matplotlib = "~=3.5.0"       # Approximately equal (>= 3.5.0, < 3.6.0)
pillow = "==8.4.0"           # Exact version
```

### Advanced Dependency Specifications

```toml
[python_dependencies]
# Package with extras (additional features)
requests = { version = ">=2.25.0", extras = ["security", "socks"] }

# Optional dependencies
plotly = { version = ">=5.0.0", optional = true }

# Detailed specification
scikit-learn = {
    version = "^1.0.0",
    extras = ["tests"],
    optional = false
}
```

### Version Specifier Reference

| Specifier | Meaning | Example |
|-----------|---------|---------|
| `>=1.0.0` | Minimum version | `>=1.20.0` |
| `^1.0.0`  | Compatible release | `^2.25.0` (allows 2.25.x, 2.26.x, but not 3.x) |
| `~=1.0.0` | Approximately equal | `~=3.5.0` (allows 3.5.x, but not 3.6.x) |
| `==1.0.0` | Exact version | `==8.4.0` |
| `!=1.0.0` | Exclude version | `!=2.24.0` |
| `>1.0.0`  | Greater than | `>1.19.0` |
| `<2.0.0`  | Less than | `<3.0.0` |

## Houdini Version Mapping

HPM automatically maps Houdini versions to compatible Python versions:

| Houdini Version | Python Version | Common Use Cases |
|----------------|----------------|------------------|
| 19.0 - 19.5    | Python 3.7     | Legacy workflows, older packages |
| 20.0           | Python 3.9     | Current production, most packages |
| 20.5           | Python 3.10    | Modern features, type hints |
| 21.x           | Python 3.11    | Latest features, performance improvements |

The Python version is determined by the `min_version` in your `[houdini]` section:

```toml
[houdini]
min_version = "20.0"  # Uses Python 3.9
max_version = "20.5"  # Optional compatibility upper bound
```

## Virtual Environment Sharing

HPM optimizes disk usage and installation time through intelligent virtual environment sharing.

### How It Works

1. **Dependency Resolution**: HPM resolves all your Python dependencies to exact versions
2. **Hash Generation**: A unique hash is created from the resolved dependency set
3. **Environment Sharing**: Packages with identical resolved dependencies share the same virtual environment

### Example

```bash
# Package A needs: numpy==1.24.0, scipy==1.10.1 -> Hash: abc123
# Package B needs: numpy==1.24.0, scipy==1.10.1 -> Hash: abc123 (same!)
# Package C needs: numpy==1.25.0, scipy==1.10.1 -> Hash: def456 (different)

# Result:
# - Packages A and B share virtual environment abc123
# - Package C gets its own virtual environment def456
```

### Benefits

- **Disk Space**: Significant savings when multiple packages have similar dependencies
- **Installation Speed**: Skip installation if compatible environment already exists
- **Consistency**: All packages using the same dependencies get identical package versions

## Package Management

### Installing Packages with Python Dependencies

```bash
# Install all dependencies from hpm.toml
hpm install

# Install with optional dependencies
hpm install --all-optional
```

### Viewing Python Dependencies

```bash
# List installed packages and their Python environments
hpm list

# Show dependency tree including Python packages
hpm list --tree
```

### Updating Packages

```bash
# Update all packages (may create new virtual environment if dependencies changed)
hpm update

# Preview updates
hpm update --dry-run
```

## Cleanup and Maintenance

### Understanding Python Cleanup

HPM's cleanup system intelligently manages Python virtual environments:

- **Orphaned Environments**: Virtual environments no longer used by any installed packages
- **Active Environments**: Virtual environments used by one or more installed packages
- **Safety First**: HPM never removes environments needed by active packages

### Cleanup Commands

```bash
# Preview what would be cleaned (recommended first step)
hpm clean --python-only --dry-run

# Clean only orphaned Python virtual environments
hpm clean --python-only

# Comprehensive cleanup (both packages and Python environments)
hpm clean --comprehensive --dry-run
hpm clean --comprehensive
```

### Cleanup Output Example

```bash
$ hpm clean --python-only --dry-run

Analyzing Python virtual environments for cleanup (dry run)...
Found 3 orphaned virtual environments that would be removed:
  - ~/.hpm/venvs/abc123def (145 MB, created 30 days ago)
  - ~/.hpm/venvs/def456ghi (89 MB, created 15 days ago)
  - ~/.hpm/venvs/ghi789jkl (234 MB, created 7 days ago)
Would free approximately: 468 MB

Run 'hpm clean --python-only' to remove these virtual environments
```

## Troubleshooting

### Common Issues and Solutions

#### 1. Dependency Conflicts

**Problem**: HPM reports conflicting Python package versions.

```
Error: Conflicting versions for package numpy:
  - geometry-tools requires numpy>=1.20,<1.21
  - mesh-tools requires numpy>=1.25
```

**Solutions**:
- Update one of the packages to use compatible version ranges
- Use optional dependencies where possible
- Split conflicting packages into separate projects

#### 2. Missing Python Packages in Houdini

**Problem**: Python packages are not available in Houdini despite successful installation.

**Diagnostic Steps**:
```python
# In Houdini Python console:
import sys
print(sys.path)
# Check if HPM virtual environment path is included
```

**Solutions**:
- Restart Houdini after package installation
- Check that `package.json` files were generated correctly
- Verify HOUDINI_PACKAGE_PATH includes your project

#### 3. Virtual Environment Creation Failures

**Problem**: HPM cannot create virtual environments.

**Common Causes**:
- Python interpreter not found for specified version
- Network connectivity issues preventing package downloads
- Insufficient disk space
- Permission issues in `~/.hpm/` directory

**Solutions**:
```bash
# Check with debug logging
RUST_LOG=debug hpm clean --python-only --dry-run

# Manually clear cache and retry
rm -rf ~/.hpm/uv-cache/
hpm install
```

#### 4. Large Virtual Environment Sizes

**Problem**: Virtual environments consuming excessive disk space.

**Investigation**:
```bash
# See virtual environment sizes
hpm clean --python-only --dry-run

# List all virtual environments
ls -la ~/.hpm/venvs/
```

**Solutions**:
- Regular cleanup of orphaned environments
- Review dependency specifications for unnecessary packages
- Use optional dependencies where appropriate

### Debug Mode

Enable detailed logging to troubleshoot issues:

```bash
RUST_LOG=debug hpm install
RUST_LOG=debug hpm clean --comprehensive --dry-run
```

## Best Practices

### 1. Version Constraint Guidelines

```toml
[python_dependencies]
# Good: Allows compatible updates
numpy = "^1.20.0"          # Allows 1.20.x, 1.21.x, etc. but not 2.x
requests = ">=2.25.0"      # Minimum version with flexibility

# Avoid: Too restrictive
numpy = "==1.20.5"         # Blocks all updates, prevents sharing

# Avoid: Too permissive
requests = "*"             # Could install incompatible versions
```

### 2. Managing Optional Dependencies

```toml
[python_dependencies]
# Core dependencies - always installed
numpy = ">=1.20.0"
scipy = ">=1.7.0"

# Optional features - only installed if explicitly requested
plotly = { version = ">=5.0.0", optional = true }
jupyter = { version = ">=1.0.0", optional = true }
```

### 3. Project Organization

- **Single Project**: Place packages with similar Python requirements in the same project
- **Separate Projects**: Isolate packages with conflicting requirements
- **Regular Cleanup**: Run `hpm clean --comprehensive` periodically

### 4. Development Workflow

1. **Define Dependencies**: Start with broad version ranges in `hpm.toml`
2. **Test Installation**: Install and test in clean environment
3. **Refine Constraints**: Narrow version ranges if compatibility issues arise
4. **Document Requirements**: Note any special Python package requirements in README

### 5. Performance Optimization

- **Leverage Sharing**: Design packages to use common dependency versions when possible
- **Monitor Size**: Regular cleanup prevents excessive disk usage
- **Cache Awareness**: Let HPM manage UV cache automatically

## Technical Architecture

### Storage Structure

HPM stores Python-related data in a structured directory:

```
~/.hpm/
├── packages/                    # HPM package storage
├── venvs/                       # Virtual environment storage
│   ├── {hash1}/                 # Shared venv for compatible dependency sets
│   │   ├── metadata.json        # Tracks which packages use this venv
│   │   ├── pyvenv.cfg           # Standard Python venv configuration
│   │   ├── bin/                 # Python executables
│   │   └── lib/                 # Installed Python packages
│   └── {hash2}/
├── tools/
│   └── uv                       # Bundled UV binary
├── uv-cache/                    # UV package cache (isolated from system UV)
└── uv-config/                   # UV configuration (isolated from system UV)
```

### UV Isolation

HPM bundles its own UV binary and maintains complete isolation from any existing system UV installation:

- **Dedicated UV Binary**: HPM bundles its own UV in `~/.hpm/tools/uv`
- **Isolated Cache**: All UV cache operations use `~/.hpm/uv-cache/`
- **Isolated Configuration**: UV configuration stored in `~/.hpm/uv-config/`
- **No System Interference**: HPM's UV operations never affect your existing Python workflows

### Environment Identification

Virtual environments are identified by SHA-256 hash of resolved dependency set:

```rust
pub struct ResolvedDependencySet {
    pub packages: BTreeMap<String, String>,  // name -> exact_version
    pub python_version: String,              // Python version requirement
}
```

The hash ensures:
- Packages with identical resolved dependencies share environments
- Any change in dependencies creates a new environment
- Deterministic environment creation across systems

### Houdini Integration

HPM generates Houdini `package.json` files with PYTHONPATH injection:

```json
{
    "path": "$HPM_PACKAGE_ROOT",
    "env": [
        {
            "PYTHONPATH": "/path/to/venv/lib/python3.9/site-packages:$PYTHONPATH"
        }
    ]
}
```

Cross-platform path handling:
- **Unix/macOS**: `{venv}/lib/python3.x/site-packages:$PYTHONPATH`
- **Windows**: `{venv}\Lib\site-packages;%PYTHONPATH%`

### Performance Characteristics

| Operation | Performance | Notes |
|-----------|-------------|-------|
| Environment Reuse | ~50ms | When hash matches existing |
| New Environment Creation | 5-15s | Includes Python + packages |
| Environment Cleanup | ~100ms | Removing unused environments |
| UV Dependency Resolution | 1-3s | Depends on package count |

### Expected Benefits

- **Shared environments**: Reduce disk usage by up to 80% for compatible packages
- **UV speed**: 80x faster dependency resolution compared to pip
- **Caching**: Global package cache reduces download times
- **Parallel operations**: Concurrent environment creation when possible

## Resources

- [UV Documentation](https://docs.astral.sh/uv/) - The underlying Python package manager
- [PEP 440](https://peps.python.org/pep-0440/) - Version specifiers for Python packages
- [Houdini Package System](https://www.sidefx.com/docs/houdini/ref/plugins.html) - SideFX documentation
