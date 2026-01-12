# HPM User Guide

HPM (Houdini Package Manager) is a modern, Rust-based package management system for SideFX Houdini that provides industry-standard dependency management capabilities equivalent to npm for Node.js, uv for Python, or cargo for Rust.

## Table of Contents

1. [Installation](#installation)
2. [Getting Started](#getting-started)
3. [Package Management](#package-management)
4. [Command Reference](#command-reference)
5. [Configuration](#configuration)
6. [Python Dependencies](#python-dependencies)
7. [Package Structure](#package-structure)
8. [Troubleshooting](#troubleshooting)
9. [Advanced Usage](#advanced-usage)

## Installation

### Prerequisites

- **Rust 1.70 or later** - Required for building HPM from source
- **SideFX Houdini 19.5+** - The target application for package management
- **Git** (optional) - For version control integration during package initialization

### Build from Source

Currently, HPM is available as source code and must be built locally:

```bash
# Clone the repository
git clone https://github.com/hpm-org/hpm.git
cd hpm

# Build the release version
cargo build --release

# The hpm binary will be available at target/release/hpm
# Optionally, add it to your PATH
export PATH="$PWD/target/release:$PATH"
```

### Verification

Verify your installation:

```bash
hpm --version
# Output: hpm 0.1.0
```

## Getting Started

### Your First Package

Create your first Houdini package with HPM:

```bash
# Create a new package with standard structure
hpm init my-first-package --description "My first Houdini package"

# Navigate to the package directory
cd my-first-package

# Examine the generated structure
ls -la
```

This creates a complete Houdini package structure:

```
my-first-package/
├── hpm.toml           # Package manifest
├── package.json       # Generated Houdini package file  
├── README.md          # Package documentation
├── otls/             # Digital assets directory
├── python/           # Python modules directory
│   └── __init__.py   
├── scripts/          # Shelf tools and scripts
├── presets/          # Node presets
├── config/           # Configuration files
└── tests/            # Test files
```

### Basic Workflow

The typical HPM workflow follows these steps:

1. **Initialize** - Create or work with an HPM package
2. **Add Dependencies** - Include packages your project needs
3. **Install** - Download and set up all dependencies
4. **Develop** - Build your Houdini tools and assets
5. **Maintain** - Update dependencies and clean up unused packages

```bash
# 1. Initialize (if creating new package)
hpm init advanced-geometry-tools

# 2. Add dependencies
hpm add utility-nodes --git https://github.com/studio/utility-nodes --commit abc1234
hpm add material-library --git https://github.com/studio/materials --commit def5678 --optional

# 3. Install all dependencies
hpm install

# 4. Check package health
hpm check

# 5. List current dependencies
hpm list

# 6. Clean up unused packages periodically
hpm clean --dry-run  # Preview what will be cleaned
hpm clean            # Perform cleanup
```

## Package Management

### Package Initialization

HPM supports two package templates:

#### Standard Package (Default)
Creates a complete Houdini package structure with all standard directories:

```bash
hpm init my-package --description "Custom Houdini tools"
```

#### Bare Package
Creates only the essential `hpm.toml` manifest for custom layouts:

```bash
hpm init --bare minimal-package
```

#### Advanced Initialization Options

```bash
# Full customization
hpm init advanced-tools \
  --description "Advanced geometry manipulation tools" \
  --author "Artist Name <artist@studio.com>" \
  --license Apache-2.0 \
  --version 0.2.0 \
  --houdini-min 20.0 \
  --houdini-max 21.0 \
  --vcs git
```

### Adding Dependencies

HPM uses Git-based dependencies with commit pinning for security (commit hashes cannot be changed, unlike tags):

```bash
# Add from Git repository (primary method)
hpm add utility-nodes --git https://github.com/studio/utility-nodes --commit abc1234

# Add multiple packages at once (from same repo)
hpm add pkg1 pkg2 --git https://github.com/studio/tools --commit def5678

# Add local path dependency
hpm add local-tools --path ../my-local-tools

# Add optional dependencies
hpm add visualization-tools --git https://github.com/studio/viz --commit 789xyz --optional

# Add to specific project
hpm add node-library --git https://github.com/studio/nodes --commit abc123 --package /path/to/project/
```

### Removing Dependencies

Remove dependencies from your project manifest:

```bash
# Remove from current project
hpm remove old-package

# Remove from specific project  
hpm remove unused-library --package /path/to/project/

# Remove from specific manifest
hpm remove legacy-tools --package /path/to/project/hpm.toml
```

**Important**: The `remove` command only removes the dependency from your `hpm.toml` manifest. The actual downloaded package files remain available for reuse by other projects. Use `hpm clean` to remove unused package files.

### Installing Dependencies

Install all dependencies specified in your `hpm.toml`:

```bash
# Install from current directory
hpm install

# Install from specific manifest
hpm install --manifest /path/to/project/hpm.toml
hpm install -m ../other-project/hpm.toml

# Install from directory containing hpm.toml
hpm install --manifest /path/to/project/
```

The install command:
- Resolves HPM package dependencies
- Creates content-addressable Python virtual environments
- Sets up project structure in `.hpm/` directory
- Generates `hpm.lock` with resolved dependency information
- Configures Houdini integration via generated `package.json` files

### Updating Packages

Keep your dependencies current with the update system:

```bash
# Preview available updates
hpm update --dry-run

# Update all packages to latest compatible versions
hpm update

# Update specific packages only
hpm update numpy geometry-tools material-library

# Update specific project
hpm update --package /path/to/project/

# Automated updates (for scripts/CI)
hpm update --yes

# Machine-readable output for automation
hpm update --dry-run --output json
hpm update --yes --output json-lines
```

### Viewing Package Information

List and inspect your project dependencies:

```bash
# List dependencies from current project
hpm list

# List dependencies from specific project
hpm list --package /path/to/project/
hpm list --package /path/to/project/hpm.toml
```

Example output:
```
Package: geometry-tools v1.2.0
Description: Advanced geometry manipulation tools for Houdini
Houdini compatibility: min: 20.0, max: 21.0

HPM Dependencies:
  utility-nodes ^2.1.0
  material-library 1.5 (optional)
  mesh-utils git: https://github.com/example/mesh-utils (tag: v1.0)

Python Dependencies:
  numpy >=1.20.0
  matplotlib ^3.5.0 (optional)
  requests >=2.25.0 [security,socks]
```

### Package Validation

Verify your package configuration and compatibility:

```bash
# Check current package
hpm check

# Check specific project
hpm check --package /path/to/project/
```

The check command validates:
- Manifest syntax and required fields
- Houdini version compatibility
- Dependency constraint validity
- Package structure integrity

### System Maintenance

HPM includes intelligent cleanup systems to manage disk usage:

```bash
# Preview cleanup operations (recommended first step)
hpm clean --dry-run

# Interactive cleanup with confirmation
hpm clean

# Automated cleanup for scripts
hpm clean --yes

# Clean only Python virtual environments
hpm clean --python-only --dry-run
hpm clean --python-only

# Comprehensive cleanup (packages + Python environments)
hpm clean --comprehensive --dry-run
hpm clean --comprehensive --yes
```

The cleanup system:
- **Never removes packages needed by active projects**
- **Preserves transitive dependencies automatically**
- **Detects orphaned packages through project scanning**
- **Warns when no projects found (prevents removing all packages)**
- **Supports Python virtual environment cleanup**

## Command Reference

### Global Options

All HPM commands support these global options:

| Option | Description |
|--------|-------------|
| `-v, --verbose` | Increase verbosity (use multiple times for more detail) |
| `-q, --quiet` | Suppress output except for errors |
| `--color <WHEN>` | Control color output: `auto`, `always`, `never` |
| `--output <FORMAT>` | Set output format: `human`, `json`, `json-lines`, `json-compact` |
| `-C, --directory <DIR>` | Run command in specified directory |

### Command Overview

| Command | Purpose | Status |
|---------|---------|--------|
| [`init`](#init) | Initialize new HPM package | ✅ Fully Implemented |
| [`add`](#add) | Add package dependencies | ✅ Fully Implemented |
| [`remove`](#remove) | Remove package dependency | ✅ Fully Implemented |
| [`install`](#install) | Install dependencies from hpm.toml | ✅ Fully Implemented |
| [`update`](#update) | Update packages to latest versions | ✅ Fully Implemented |
| [`list`](#list) | Display package information | ✅ Fully Implemented |
| [`check`](#check) | Validate package configuration | ✅ Fully Implemented |
| [`clean`](#clean) | Clean orphaned packages | ✅ Fully Implemented |
| [`completions`](#completions) | Generate shell completions | ✅ Fully Implemented |

### init

Initialize a new HPM package with standardized structure.

```bash
hpm init [OPTIONS] [NAME]
```

**Arguments:**
- `NAME` - Package name (optional, defaults to current directory name)

**Options:**
- `--description <DESC>` - Package description
- `--author <AUTHOR>` - Package author (format: "Name <email@example.com>")
- `--version <VERSION>` - Initial version (default: "0.1.0")
- `--license <LICENSE>` - License identifier (default: "MIT")
- `--houdini-min <VERSION>` - Minimum Houdini version
- `--houdini-max <VERSION>` - Maximum Houdini version
- `--bare` - Create minimal package structure (only hpm.toml)
- `--vcs <VCS>` - Initialize version control: `git`, `none` (default: "git")

**Examples:**
```bash
# Basic package creation
hpm init my-houdini-tools

# Minimal package
hpm init --bare simple-package

# Full customization
hpm init advanced-tools \
  --description "Professional geometry tools" \
  --author "Studio Artist <artist@studio.com>" \
  --license Apache-2.0 \
  --houdini-min 20.0 \
  --houdini-max 21.0
```

### add

Add package dependencies to your project's hpm.toml manifest.

```bash
hpm add [OPTIONS] <PACKAGE>...
```

**Arguments:**
- `PACKAGE...` - One or more package names to add

**Options:**
- `--git <URL>` - Git repository URL (required for Git dependencies)
- `--commit <HASH>` - Git commit hash (required for Git dependencies)
- `--path <PATH>` - Local path to package (for path dependencies)
- `-p, --package <PATH>` - Path to directory containing hpm.toml or direct path to hpm.toml file
- `--optional` - Mark dependency as optional

**Examples:**
```bash
# Add from Git repository
hpm add utility-nodes --git https://github.com/studio/utility-nodes --commit abc1234

# Add multiple packages at once
hpm add pkg1 pkg2 --git https://github.com/studio/tools --commit def5678

# Add local path dependency
hpm add local-tools --path ../my-local-tools

# Add optional dependency
hpm add material-library --git https://github.com/studio/materials --commit 789xyz --optional

# Add to specific project
hpm add mesh-utils --git https://github.com/studio/mesh --commit abc123 --package /path/to/project/
```

**Note:** HPM uses Git-based dependencies with commit pinning for security. Commit hashes cannot be changed (unlike tags), ensuring reproducible builds.

### remove

Remove a package dependency from your project's hpm.toml manifest.

```bash
hpm remove [OPTIONS] <PACKAGE>
```

**Arguments:**
- `PACKAGE` - Name of the package to remove

**Options:**
- `-p, --package <PATH>` - Path to directory containing hpm.toml or direct path to hpm.toml file

**Examples:**
```bash
# Remove from current project
hpm remove old-package

# Remove from specific project
hpm remove unused-library --package /path/to/project/
```

### install

Install all dependencies specified in hpm.toml manifest.

```bash
hpm install [OPTIONS]
```

**Options:**
- `-m, --manifest <PATH>` - Path to hpm.toml file or directory containing it

**Examples:**
```bash
# Install from current directory
hpm install

# Install from specific manifest
hpm install --manifest /path/to/project/hpm.toml
hpm install -m ../other-project/
```

### update

Update packages to their latest compatible versions.

```bash
hpm update [OPTIONS] [PACKAGES]...
```

**Arguments:**
- `PACKAGES` - Only update these specific packages (optional)

**Options:**
- `-p, --package <PATH>` - Path to directory containing hpm.toml or direct path to hpm.toml file
- `--dry-run` - Preview changes without applying them
- `-y, --yes` - Skip confirmation prompts

**Examples:**
```bash
# Preview all available updates
hpm update --dry-run

# Update all packages
hpm update

# Update specific packages
hpm update numpy geometry-tools

# Update with automation
hpm update --yes --output json
```

### list

Display comprehensive package information and dependencies.

```bash
hpm list [OPTIONS]
```

**Options:**
- `-p, --package <PATH>` - Path to directory containing hpm.toml or direct path to hpm.toml file
- `--tree` - Display dependencies as a visual tree

**Examples:**
```bash
# List current project dependencies
hpm list

# List as visual tree
hpm list --tree

# List specific project dependencies
hpm list --package /path/to/project/
```

**Tree output example:**
```
my-package v1.0.0
├── geometry-tools (repo@abc1234)
├── utility-nodes (repo@def5678) [optional]
└── local-tools (path: ../local-tools)

Python dependencies:
├── numpy >=1.20.0
└── requests >=2.25.0
```

### check

Validate package configuration and Houdini compatibility.

```bash
hpm check [OPTIONS]
```

**Examples:**
```bash
# Check current package
hpm check

# Check with verbose output
hpm check --verbose
```

### clean

Clean orphaned packages and Python virtual environments.

```bash
hpm clean [OPTIONS]
```

**Options:**
- `--dry-run` - Preview cleanup operations without removing anything
- `--yes` - Skip confirmation prompts for automation
- `--python-only` - Clean only Python virtual environments
- `--comprehensive` - Clean both packages and Python environments

**Examples:**
```bash
# Preview cleanup (recommended first step)
hpm clean --dry-run

# Interactive cleanup
hpm clean

# Comprehensive automated cleanup
hpm clean --comprehensive --yes

# Clean only Python environments
hpm clean --python-only --dry-run
```

### completions

Generate shell completion scripts.

```bash
hpm completions <SHELL>
```

**Arguments:**
- `SHELL` - Target shell: `bash`, `zsh`, `fish`, `powershell`, `elvish`

**Examples:**
```bash
# Bash - add to ~/.bashrc
eval "$(hpm completions bash)"

# Zsh - add to ~/.zshrc
eval "$(hpm completions zsh)"

# Fish - add to ~/.config/fish/config.fish
hpm completions fish | source

# PowerShell - add to $PROFILE
hpm completions powershell | Out-String | Invoke-Expression
```

## Configuration

### Package Manifest (hpm.toml)

The `hpm.toml` file is the heart of every HPM package, containing all metadata and dependency information:

```toml
[package]
name = "my-houdini-tool"
version = "1.0.0"
description = "Custom Houdini digital assets and tools"
authors = ["Your Name <email@example.com>"]
license = "MIT"
readme = "README.md"
keywords = ["houdini", "geometry", "tools"]

[houdini]
min_version = "19.5"
max_version = "21.0"

# HPM package dependencies (Git-based with commit pinning)
[dependencies]
utility-nodes = { git = "https://github.com/studio/utility-nodes", commit = "abc1234" }
material-library = { git = "https://github.com/studio/materials", commit = "def5678", optional = true }
local-tools = { path = "../local-tools" }

# Python dependencies with Houdini integration
[python_dependencies]
numpy = ">=1.20.0"
requests = { version = ">=2.25.0", extras = ["security", "socks"] }
matplotlib = { version = "^3.5.0", optional = true }

# Package scripts for automation
[scripts]
build = "python scripts/build.py"
test = "python -m pytest tests/"
format = "python -m black python/"
```

#### Package Section

The `[package]` section contains core package metadata:

| Field | Required | Description |
|-------|----------|-------------|
| `name` | Yes | Package name (must be unique) |
| `version` | Yes | Package version (semantic versioning) |
| `description` | No | Brief package description |
| `authors` | No | List of package authors |
| `license` | No | License identifier (e.g., "MIT", "Apache-2.0") |
| `readme` | No | Path to README file |
| `keywords` | No | List of keywords for discovery |

#### Houdini Section

The `[houdini]` section specifies Houdini compatibility:

| Field | Description |
|-------|-------------|
| `min_version` | Minimum supported Houdini version |
| `max_version` | Maximum supported Houdini version |

#### Dependencies

HPM uses Git-based dependencies with commit pinning for security and reproducibility:

##### Git Dependencies
```toml
[dependencies]
# Basic Git dependency with commit hash
utility-nodes = { git = "https://github.com/studio/utility-nodes", commit = "abc1234def5678" }

# Optional Git dependency
material-library = { git = "https://github.com/studio/materials", commit = "def5678", optional = true }
```

##### Path Dependencies
```toml
[dependencies]
# Local path dependency (for development)
local-tools = { path = "../my-local-tools" }
dev-utilities = { path = "./packages/dev-utils", optional = true }
```

**Note:** HPM requires commit hashes (not tags or branches) for Git dependencies. This ensures reproducible builds since commit hashes are immutable.

### Global Configuration (~/.hpm/config.toml)

Global HPM settings stored in your home directory:

```toml
[install]
# Package installation paths
package_dir = "packages"
cache_dir = "cache"

[projects]
# Project discovery configuration
search_roots = [
  "/Users/username/houdini-projects",
  "/shared/studio-projects"
]
max_search_depth = 3
ignore_patterns = [".git", "node_modules", "*.tmp", ".DS_Store"]

# Explicit project paths (always monitored)
explicit_paths = [
  "/important/project1",
  "/critical/project2"
]

[python]
# Python environment configuration
cache_dir = "python-cache"
max_environments = 50
cleanup_threshold_days = 30

[ui]
# User interface preferences
color = "auto"  # auto, always, never
progress_bars = true
confirm_destructive = true
```

## Python Dependencies

HPM provides comprehensive Python dependency management for Houdini packages, addressing the challenge of conflicting Python dependencies across multiple packages.

### Key Features

- **Content-Addressable Virtual Environments** - Packages with identical resolved dependencies share virtual environments
- **UV-Powered Resolution** - High-performance dependency resolution using bundled UV
- **Houdini Integration** - Seamless PYTHONPATH injection via generated package.json files
- **Intelligent Cleanup** - Automatic orphaned virtual environment detection and removal
- **Conflict Detection** - Automatic detection and reporting of dependency conflicts

### Specifying Python Dependencies

Add Python dependencies to your `hpm.toml`:

```toml
[python_dependencies]
# Basic version constraints
numpy = ">=1.20.0"
requests = "^2.28.0"

# Dependencies with extras
matplotlib = { version = "^3.5.0", extras = ["tk"] }

# Optional dependencies
seaborn = { version = ">=0.11.0", optional = true }

# Complex specifications
scientific-tools = {
  version = ">=1.0.0",
  extras = ["analysis", "visualization"],
  optional = false
}
```

### Houdini Version Mapping

HPM automatically maps Houdini versions to compatible Python versions:

| Houdini Version | Python Version | Notes |
|----------------|----------------|-------|
| 19.0 - 19.5 | Python 3.7 | Legacy support |
| 20.0 | Python 3.9 | Current stable |
| 20.5 | Python 3.10 | Enhanced performance |
| 21.x | Python 3.11 | Latest features |

### Virtual Environment Sharing

Multiple packages with identical resolved dependencies automatically share virtual environments:

```bash
# Example: Two packages with same resolved dependencies
Package A: numpy==1.24.0, requests==2.28.0 → venv: a1b2c3d4e5f6
Package B: numpy==1.24.0, requests==2.28.0 → venv: a1b2c3d4e5f6 (shared)
Package C: numpy==1.25.0, requests==2.28.0 → venv: f6e5d4c3b2a1 (different)
```

### Generated Houdini Integration

HPM automatically generates `package.json` files with Python environment integration:

```json
{
  "path": "$HPM_PACKAGE_ROOT",
  "env": [
    {
      "PYTHONPATH": "/Users/user/.hpm/venvs/a1b2c3d4e5f6/lib/python/site-packages:$PYTHONPATH"
    }
  ],
  "hpm_managed": true,
  "hpm_package": "geometry-tools"
}
```

### Python Environment Cleanup

HPM's cleanup system extends to Python virtual environments:

```bash
# Preview Python environment cleanup
hpm clean --python-only --dry-run

# Clean orphaned Python environments
hpm clean --python-only

# Comprehensive cleanup (packages + Python)
hpm clean --comprehensive --dry-run
hpm clean --comprehensive
```

### Troubleshooting Python Issues

Common Python dependency scenarios and solutions:

#### Conflicting Versions
```bash
# HPM will detect and report conflicts
$ hpm install
Error: Package error: Conflicting Python dependencies detected
  numpy: package-a requires ">=1.20.0", package-b requires ">=1.25.0"
  help: Update package-a to use numpy ">=1.25.0" or make one dependency optional
```

#### Missing Python Version
If Houdini version mapping fails, HPM automatically falls back to Python 3.9 with a warning.

#### Virtual Environment Corruption
HPM automatically detects and recreates corrupted virtual environments during installation.

#### Network Issues
UV dependency resolution failures are reported with full context and suggested solutions.

## Package Structure

HPM creates standardized Houdini package structures that integrate seamlessly with Houdini's package loading system.

### Standard Package Template

The standard template creates a complete Houdini package structure:

```
my-package/
├── hpm.toml              # HPM package manifest
├── package.json          # Generated Houdini package file  
├── README.md             # Package documentation
├── .gitignore           # Git ignore file (if using Git)
├── otls/                # Digital assets directory
│   └── .gitkeep         # Ensure directory exists in Git
├── python/              # Python modules directory
│   └── __init__.py      # Python package initialization
├── scripts/             # Shelf tools and scripts directory
│   └── .gitkeep
├── presets/             # Node presets directory
│   └── .gitkeep
├── config/              # Configuration files directory
│   └── .gitkeep
└── tests/               # Test files directory
    └── .gitkeep
```

### Bare Package Template

The bare template creates only the essential `hpm.toml` for custom layouts:

```
minimal-package/
├── hpm.toml             # HPM package manifest only
└── README.md            # Basic documentation
```

### Project Integration Structure

When you run `hpm install`, HPM creates a project integration structure:

```
your-project/
├── hpm.toml             # Project manifest
├── hpm.lock             # Dependency lock file (generated)
├── .hpm/                # HPM project directory
│   └── packages/        # Package installation references
│       ├── utility-nodes.json
│       └── material-library.json
└── (your project files...)
```

### Global Storage Structure

HPM manages a global storage system for efficient package sharing:

```
~/.hpm/
├── packages/                      # Versioned package storage
│   ├── utility-nodes@2.1.0/      # Individual package installations
│   └── material-library@1.5.0/
├── venvs/                         # Python virtual environments
│   ├── a1b2c3d4e5f6/             # Content-addressable environments
│   │   ├── metadata.json         # Environment metadata
│   │   └── lib/python/site-packages/
│   └── f6e5d4c3b2a1/
├── cache/                         # Download cache and metadata
├── uv-cache/                      # Isolated UV package cache
├── config.toml                   # Global configuration
└── logs/                         # Operation logs
```

### Directory Purposes

| Directory | Purpose | Contents |
|-----------|---------|----------|
| `otls/` | Digital Assets | Houdini Digital Assets (.hda, .otl files) |
| `python/` | Python Code | Python modules and packages for Houdini |
| `scripts/` | Tools & Automation | Shelf tools, event handlers, automation scripts |
| `presets/` | Node Presets | Parameter presets and node configurations |
| `config/` | Configuration | Environment variables, pipeline configuration |
| `tests/` | Testing | Unit tests, integration tests, test assets |

### Generated Files

HPM automatically generates and manages these files:

#### package.json (Houdini Integration)
```json
{
  "path": "$HPM_PACKAGE_ROOT",
  "load_package_once": true,
  "env": [
    {
      "PYTHONPATH": "/Users/user/.hpm/venvs/a1b2c3d4e5f6/lib/python/site-packages:$PYTHONPATH"
    }
  ],
  "hpm_managed": true,
  "hpm_package": "my-package",
  "hpm_version": "1.0.0"
}
```

#### hpm.lock (Dependency Lock File)
```toml
# This file is automatically generated by HPM
# Do not modify manually

[[package]]
name = "utility-nodes"
git = "https://github.com/studio/utility-nodes"
commit = "abc1234def5678901234567890abcdef12345678"

[[package]]
name = "numpy"
version = "1.24.0"
source = "python"
python_version = "3.9"
```

## Troubleshooting

### Common Issues and Solutions

#### Package Not Found
```bash
Error: Package error: Package 'nonexistent-package' not found
```

**Solution**: Verify the package name and ensure the Git repository URL and commit hash are correct. HPM uses Git-based dependencies, so the package must be available in the specified Git repository.

#### Directory Already Exists
```bash
Error: Package error: Directory 'my-package' already exists
help: Choose a different name or remove the existing directory
```

**Solution**: Use a different package name or remove the existing directory if safe to do so.

#### Permission Denied
```bash
Error: I/O error: Permission denied (os error 13)
help: Check file permissions and ensure you have write access to the target directory
```

**Solution**: Ensure you have write permissions to the target directory or run with appropriate permissions.

#### Network Connection Issues
```bash
Error: Network error: Connection timeout
help: Check your internet connection and proxy settings
```

**Solution**: Verify internet connectivity and check if you're behind a corporate firewall that requires proxy configuration.

#### Python Environment Issues
```bash
Error: Python dependency resolution failed: Could not find compatible Python version
help: Ensure Houdini version is correctly specified in hpm.toml
```

**Solution**: Check your `houdini.min_version` setting in `hpm.toml` and ensure it's valid.

### Debug Mode

Enable debug logging for detailed troubleshooting:

```bash
# Enable debug logging for all HPM operations
RUST_LOG=debug hpm <command>

# Enable debug logging for specific modules
RUST_LOG=hpm_core=debug hpm clean --dry-run
RUST_LOG=hpm_python=debug hpm install
```

### Configuration Issues

#### Invalid hpm.toml Syntax
HPM validates your `hpm.toml` file and provides detailed error messages:

```bash
$ hpm check
Error: Config error: Invalid TOML syntax at line 5, column 12
help: Check the syntax around line 5 in your hpm.toml file
```

#### Dependency Conflicts
```bash
$ hpm install  
Error: Package error: Dependency conflict detected
  utility-nodes requires geometry-tools "^1.0.0"
  material-library requires geometry-tools "^2.0.0"
help: Update one of the packages to use a compatible version range
```

### Performance Issues

#### Slow Dependency Resolution
```bash
# Use parallel downloads
hpm install --verbose  # Shows download progress

# Clear cache if corrupted
rm -rf ~/.hpm/cache
hpm install
```

#### Large Virtual Environment Cache
```bash
# Clean up old Python environments
hpm clean --python-only --dry-run
hpm clean --python-only

# Check disk usage
du -sh ~/.hpm/venvs/
```

### Recovery Operations

#### Reset HPM Configuration
```bash
# Backup existing configuration
cp ~/.hpm/config.toml ~/.hpm/config.toml.backup

# Remove all HPM data (nuclear option)
rm -rf ~/.hpm/

# Reinstall packages
hpm install
```

#### Fix Corrupted Project State
```bash
# Remove generated files and reinstall
rm -rf .hpm/ hpm.lock
hpm install
```

### Getting Help

#### Built-in Help
```bash
# General help
hpm --help

# Command-specific help
hpm init --help
hpm clean --help

# Version information
hpm --version
```

#### Verbose Output
```bash
# Increase verbosity for more information
hpm --verbose install
hpm -vv clean --dry-run  # Very verbose
```

#### Support Channels

- **Issues**: [GitHub Issues](https://github.com/hpm-org/hpm/issues) for bug reports
- **Discussions**: [GitHub Discussions](https://github.com/hpm-org/hpm/discussions) for questions
- **Documentation**: [CLAUDE.md](https://github.com/hpm-org/hpm/blob/main/CLAUDE.md) for development guidelines

## Advanced Usage

### Output Formats

HPM supports multiple output formats for different use cases:

#### Human-Readable (Default)
```bash
hpm install
# ✓ Package 'geometry-tools' installed successfully
# ⚠ Warning: Optional dependency 'visualization-tools' not found
```

#### JSON Output
```bash
hpm --output json install
# {
#   "success": true,
#   "command": "install",
#   "message": "3 packages installed",
#   "elapsed_ms": 1250
# }
```

#### JSON Lines (Streaming)
```bash
hpm --output json-lines update --dry-run
# {"type":"update","package":"numpy","from":"1.23.0","to":"1.24.0"}  
# {"type":"update","package":"requests","from":"2.27.0","to":"2.28.0"}
```

#### JSON Compact
```bash
hpm --output json-compact list
# {"success":true,"packages":[{"name":"numpy","version":"1.24.0"}]}
```

### Automation and CI/CD

HPM is designed for automation with machine-readable output and non-interactive modes:

```bash
#!/bin/bash
# Example CI/CD script

set -e  # Exit on any error

# Install dependencies non-interactively
hpm install --quiet

# Check for updates (without applying)
UPDATES=$(hpm update --dry-run --output json --quiet)
echo "Available updates: $UPDATES"

# Clean up old packages automatically
hpm clean --yes --quiet

# Validate package configuration
hpm check --quiet
```

### Custom Workflows

#### Development Setup Script
```bash
#!/bin/bash
# dev-setup.sh - Set up development environment

echo "Setting up Houdini development environment..."

# Initialize package if not exists
if [ ! -f "hpm.toml" ]; then
    hpm init $(basename "$PWD") --description "Development package"
fi

# Install dependencies
hpm install

# Add common development dependencies
hpm add test-assets --git https://github.com/studio/test-assets --commit abc123
hpm add debug-tools --git https://github.com/studio/debug --commit def456 --optional

echo "Development environment ready!"
```

#### Package Maintenance Script
```bash
#!/bin/bash
# maintenance.sh - Regular package maintenance

echo "Performing HPM maintenance..."

# Check for security updates
hpm update --dry-run --output json > updates.json

# Clean up orphaned packages
CLEANED=$(hpm clean --dry-run --output json)
echo "Cleanup analysis: $CLEANED"

# Validate all configurations
hpm check --verbose

echo "Maintenance complete!"
```

### Environment Integration

#### Shell Configuration
Add to your `.bashrc` or `.zshrc`:

```bash
# HPM environment setup
export PATH="/path/to/hpm/target/release:$PATH"

# Enable HPM shell completions
eval "$(hpm completions bash)"  # or zsh, fish, powershell

# HPM aliases for common operations
alias hpm-update="hpm update --dry-run && hpm update"
alias hpm-clean="hpm clean --dry-run && hpm clean"
alias hpm-check="hpm check && hpm list"
```

#### Studio Integration
For studio environments, consider centralizing configuration:

```bash
# Studio-wide HPM configuration
export HPM_CONFIG="/shared/studio/hpm-config.toml"
export HPM_CACHE="/shared/cache/hpm"

# Project templates
export HPM_TEMPLATE_DIR="/shared/templates/hpm"
```

This comprehensive user guide provides everything needed to effectively use HPM for Houdini package management. For technical details about HPM's architecture and development, see the Developer Documentation.