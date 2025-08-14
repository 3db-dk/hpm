# HPM Python Dependencies User Guide

## Overview

HPM provides comprehensive Python dependency management for Houdini packages, solving the common problem of conflicting Python package requirements across multiple packages. This guide explains how to define, manage, and troubleshoot Python dependencies in your HPM packages.

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
hpm add my-geometry-tools
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

HPM optimizes disk usage and installation time through intelligent virtual environment sharing:

### How It Works

1. **Dependency Resolution**: HPM resolves all your Python dependencies to exact versions
2. **Hash Generation**: A unique hash is created from the resolved dependency set
3. **Environment Sharing**: Packages with identical resolved dependencies share the same virtual environment

### Example

```bash
# Package A needs: numpy==1.24.0, scipy==1.10.1 → Hash: abc123
# Package B needs: numpy==1.24.0, scipy==1.10.1 → Hash: abc123 (same!)
# Package C needs: numpy==1.25.0, scipy==1.10.1 → Hash: def456 (different)

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
# Install a single package
hpm add geometry-tools

# Install multiple packages (HPM resolves all Python dependencies together)
hpm add geometry-tools visualization-tools mesh-processing

# Install with specific version
hpm add "geometry-tools@2.1.0"
```

### Viewing Python Dependencies

```bash
# List installed packages and their Python environments
hpm list

# Show detailed package information including Python dependencies
hpm info geometry-tools

# Show dependency tree including Python packages
hpm list --tree
```

### Updating Packages

```bash
# Update a package (may create new virtual environment if dependencies changed)
hpm update geometry-tools

# Update all packages
hpm update

# Update Python dependencies only (within version constraints)
hpm update --python-only
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

# Interactive cleanup with confirmation prompts
hpm clean --comprehensive
```

### Cleanup Output Example

```bash
$ hpm clean --python-only --dry-run

Analyzing Python virtual environments for cleanup (dry run)...
Found 3 orphaned virtual environments that would be removed:
  - /Users/user/.hpm/venvs/abc123def (145 MB, created 30 days ago)
  - /Users/user/.hpm/venvs/def456ghi (89 MB, created 15 days ago)
  - /Users/user/.hpm/venvs/ghi789jkl (234 MB, created 7 days ago)
Would free approximately: 468 MB

Run 'hpm clean --python-only' to remove these virtual environments
Run 'hpm clean --python-only --yes' to remove without confirmation
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
# Check UV status
RUST_LOG=debug hpm clean --python-only --dry-run

# Manually clear cache and retry
rm -rf ~/.hpm/uv-cache/
hpm add your-package
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
RUST_LOG=debug hpm add your-package
RUST_LOG=debug hpm clean --comprehensive --dry-run
```

### Getting Help

1. **Check Installation**: Ensure your package's `hpm.toml` Python dependencies are correct
2. **Verify Environment**: Check that virtual environments were created in `~/.hpm/venvs/`
3. **Review Logs**: Use `RUST_LOG=debug` for detailed operation information
4. **Test Isolation**: Create a minimal test package to isolate the issue

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

## Advanced Topics

### Custom Python Versions

While HPM automatically maps Houdini versions to Python versions, you can influence this:

```toml
[houdini]
min_version = "20.0"       # Primary version requirement
max_version = "20.5"       # Optional: prevents installation on newer versions

# The min_version determines Python version:
# 20.0 → Python 3.9
# 20.5 → Python 3.10
```

### Development Dependencies

```toml
[python_dependencies]
# Production dependencies
numpy = ">=1.20.0"
scipy = ">=1.7.0"

# Development/testing dependencies (not installed in production)
pytest = { version = ">=6.0.0", optional = true }
black = { version = ">=22.0.0", optional = true }
mypy = { version = ">=0.950", optional = true }
```

### Integration with CI/CD

```bash
# In CI/CD pipeline
hpm clean --comprehensive --yes    # Clean before tests
hpm add your-package              # Install with dependencies
hpm list                          # Verify installation

# Test Python imports
houdini -c "import numpy; print('Success')"
```

This user guide covers the essential aspects of using HPM's Python dependency management. For technical implementation details, see `python-dependency-management.md`.