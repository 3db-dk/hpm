# HPM CLI Design Specification

## Overview

HPM (Houdini Package Manager) provides modern package management capabilities for SideFX Houdini, following industry-standard patterns from tools like npm, uv, and cargo. This document outlines the comprehensive CLI design and functionality.

## Core Commands

### `hpm init`

Initialize a new Houdini package with standard structure and configuration.

#### Syntax
```bash
hpm init [OPTIONS] [NAME]
```

#### Options
- `--name <NAME>` - Package name (required if not specified as positional argument)
- `--description <DESC>` - Package description
- `--author <AUTHOR>` - Package author
- `--version <VERSION>` - Initial version (default: "0.1.0")
- `--license <LICENSE>` - License identifier (default: "MIT")
- `--houdini-min <VERSION>` - Minimum Houdini version
- `--houdini-max <VERSION>` - Maximum Houdini version
- `--bare` - Create minimal package structure (only hpm.toml)
- `--vcs <TYPE>` - Initialize version control (git, none)

#### Examples
```bash
# Basic package initialization
hpm init my-houdini-tools

# Package with description and author
hpm init geometry-utils --description "Geometry utility functions for Houdini" --author "John Doe <john@example.com>"

# Minimal package structure
hpm init --bare my-minimal-package

# Full specification with Houdini version constraints
hpm init advanced-tools --description "Advanced Houdini tools" --author "Developer <dev@example.com>" --houdini-min 19.5 --houdini-max 20.5
```

#### Generated Structure

**Standard Houdini Package (default):**
```
my-houdini-tools/
├── hpm.toml              # HPM package manifest
├── package.json          # Generated Houdini package file
├── README.md             # Package documentation
├── .gitignore           # Version control ignore file
├── otls/                # Digital assets (.hda, .otl files)
│   └── .gitkeep
├── python/              # Python modules and libraries
│   └── __init__.py
├── scripts/             # Shelf tools and scripts
│   └── .gitkeep
├── presets/             # Node parameter presets
│   └── .gitkeep
├── config/              # Configuration files
│   └── .gitkeep
└── tests/               # Test files
    └── .gitkeep
```

**Minimal Package (`--bare`):**
```
my-minimal-package/
└── hpm.toml             # Only the HPM manifest
```

## Package Manifest (hpm.toml)

The `hpm.toml` file serves as the primary package descriptor, similar to `pyproject.toml` or `package.json`.

### Structure

```toml
[package]
name = "my-houdini-tool"
version = "1.0.0"
description = "Custom Houdini digital assets and tools"
authors = ["Author Name <email@example.com>"]
license = "MIT"
readme = "README.md"
homepage = "https://github.com/author/my-houdini-tool"
repository = "https://github.com/author/my-houdini-tool"
documentation = "https://docs.example.com/my-houdini-tool"
keywords = ["houdini", "modeling", "vfx"]
categories = ["digital-assets", "tools"]

[houdini]
min_version = "19.5"
max_version = "20.5"

[dependencies]
utility-nodes = "^2.1.0"
material-library = { version = "1.5", optional = true }
geo-tools = { git = "https://github.com/example/geo-tools", tag = "v1.0" }

[scripts]
build = "python scripts/build.py"
test = "python -m pytest tests/"
docs = "python scripts/generate_docs.py"
```

### Field Definitions

#### `[package]` Section
- `name` - Package name (kebab-case recommended)
- `version` - Semantic version string
- `description` - Brief package description
- `authors` - Array of author strings with optional email
- `license` - SPDX license identifier
- `readme` - Path to README file
- `homepage` - Package homepage URL
- `repository` - Source repository URL
- `documentation` - Documentation URL
- `keywords` - Array of descriptive keywords
- `categories` - Array of package categories

#### `[houdini]` Section
- `min_version` - Minimum supported Houdini version
- `max_version` - Maximum supported Houdini version (optional)

#### `[dependencies]` Section
- Package dependencies with version constraints
- Supports semantic versioning (`^1.0`, `~1.2.3`, `>=1.0.0`)
- Git dependencies with repository URLs and tags/branches
- Optional dependencies with `optional = true`

#### `[scripts]` Section
- Custom scripts for common tasks
- Executable via `hpm run <script-name>` (future feature)

## Implementation Status

The following commands are currently implemented and functional:

### ✅ Fully Implemented Commands
- `hpm init` - Initialize new HPM packages with templates
- `hpm install` - Install dependencies from hpm.toml manifest
- `hpm check` - Validate package configuration and Houdini compatibility
- `hpm clean` - Project-aware cleanup with Python virtual environment support

### 🚧 Placeholder Commands (Not Yet Implemented)
- `hpm add` - Add packages and dependencies
- `hpm remove` - Remove installed packages
- `hpm update` - Update packages to latest versions
- `hpm list` - List installed packages
- `hpm search` - Search for packages in registry
- `hpm publish` - Publish packages to registry
- `hpm info` - Show detailed package information

## Additional CLI Commands

### Package Management

#### `hpm add [PACKAGE]` 🚧
**Status: Placeholder implementation**

Add packages and dependencies.

```bash
# Add from hpm.toml
hpm add

# Add specific package
hpm add utility-nodes

# Add with version constraint
hpm add "utility-nodes>=2.0"

# Add from git
hpm add git+https://github.com/author/package.git

# Add with options
hpm add --dev  # Include dev dependencies
hpm add --no-deps  # Skip dependencies
hpm add --force  # Force reinstall
```

#### `hpm remove <PACKAGE>` 🚧
**Status: Placeholder implementation**

Remove installed packages.

```bash
hpm remove utility-nodes
hpm remove --all  # Remove all packages
```

#### `hpm update [PACKAGE]` 🚧
**Status: Placeholder implementation**

Update packages to latest versions.

```bash
hpm update  # Update all packages
hpm update utility-nodes  # Update specific package
```

#### `hpm list` 🚧
**Status: Placeholder implementation**

List installed packages.

```bash
hpm list
hpm list --tree  # Show dependency tree
hpm list --outdated  # Show outdated packages
```

### Registry Operations

#### `hpm search <QUERY>` 🚧
**Status: Placeholder implementation**

Search for packages in registry.

```bash
hpm search "geometry tools"
hpm search --category modeling
hpm search --author "John Doe"
```

#### `hpm info <PACKAGE>` 🚧
**Status: Placeholder implementation**

Show detailed package information.

```bash
hpm info utility-nodes
hpm info utility-nodes --versions  # Show all versions
```

#### `hpm publish` 🚧
**Status: Placeholder implementation**

Publish package to registry.

```bash
hpm publish
hpm publish --dry-run  # Preview publish
hpm publish --allow-dirty  # Publish with uncommitted changes
```

### Development Tools

#### `hpm run <SCRIPT>` 🚧
**Status: Not yet implemented**

Execute package scripts.

```bash
hpm run build
hpm run test
hpm run docs
```

#### `hpm check`
Validate package configuration and dependencies with comprehensive analysis.

```bash
hpm check
```

**Validation Checks:**

1. **Manifest Validation**
   - `hpm.toml` existence and valid TOML syntax
   - Package structure (name, version, semantic versioning)
   - Required and optional field validation
   - Package name format (kebab-case recommended)

2. **Project Structure Analysis**
   - Standard Houdini directories (`otls`, `python`, `scripts`, `presets`, `config`)
   - README file existence and consistency with manifest
   - Digital asset detection in `otls` directory (.hda, .otl files)

3. **Houdini Compatibility**
   - Generated `package.json` structure validation
   - Houdini version constraint format verification
   - Version range logic validation (min <= max)

4. **Best Practices Assessment**
   - License file presence when license specified
   - Version control setup (Git repository, .gitignore)
   - Package size considerations (warnings for large packages/files)
   - Script command validation

**Output Categories:**
- ✅ **Success**: Validation checks passed
- ⚠️ **Warnings**: Recommendations for improvement
- ❌ **Errors**: Critical issues requiring fixes

**Exit Codes:**
- `0` - Validation successful (may include warnings)
- `1` - Validation failed with errors

**Example Output:**
```
🔍 Checking HPM package configuration...

✓ hpm.toml found
✓ hpm.toml has valid TOML syntax  
✓ Package manifest validation passed
✓ Found otls directory
✓ Generated Houdini package.json is valid
✓ Minimum Houdini version: 19.5
✓ Git repository initialized

⚠️ otls directory exists but contains no .hda or .otl files
⚠️ License specified in manifest but no LICENSE file found

✅ Package validation completed successfully!
   2 warning(s) found - consider addressing them
```

#### `hpm clean`
Clean orphaned packages and Python virtual environments using project-aware analysis.

**Overview:**
The `hpm clean` command provides intelligent cleanup that safely removes orphaned packages and Python virtual environments while preserving dependencies needed by active projects. It uses dependency graph analysis to ensure no required packages are accidentally removed.

**Syntax:**
```bash
hpm clean [OPTIONS]
```

**Options:**
- `-n, --dry-run` - Perform a dry run without actually removing packages
- `-y, --yes` - Remove packages without asking for confirmation
- `--package <PACKAGE>` - Target specific package patterns
- `--python-only` - Clean only Python virtual environments
- `--comprehensive` - Clean both packages and Python virtual environments

**Examples:**
```bash
# Interactive cleanup with confirmation prompts
hpm clean

# Preview what would be removed without making changes
hpm clean --dry-run

# Automated cleanup without confirmation prompts
hpm clean --yes

# Clean only Python virtual environments
hpm clean --python-only

# Preview comprehensive cleanup (packages + Python)
hpm clean --comprehensive --dry-run

# Automated comprehensive cleanup
hpm clean --comprehensive --yes
```

**How It Works:**

1. **Project Discovery** - Scans configured directories to find HPM-managed projects
2. **Dependency Analysis** - Builds complete dependency graph including transitive dependencies
3. **Orphan Detection** - Identifies packages not needed by any active project
4. **Safe Removal** - Only removes truly orphaned packages while preserving needed dependencies

**Safety Features:**
- Never removes packages required by active projects
- Preserves transitive dependencies automatically
- Warns if no HPM-managed projects are found (prevents removing all packages)
- Provides detailed logging of cleanup operations

**Configuration:**
Project discovery is configured via `~/.hpm/config.toml`:
```toml
[projects]
# Explicit project paths to monitor
explicit_paths = ["/path/to/project1", "/path/to/project2"]

# Root directories to search for HPM projects
search_roots = ["/Users/username/houdini-projects", "/shared/projects"]

# Maximum directory depth for project search
max_search_depth = 3

# Patterns to ignore during project search
ignore_patterns = [".git", "node_modules", "*.tmp"]
```

**Output Examples:**

*Dry Run Mode:*
```
🔍 Analyzing packages for cleanup (dry run)...
📁 Found 2 HPM-managed projects for cleanup analysis
📦 Found 5 installed packages to analyze
✓ Marked 3 packages as needed (including transitive dependencies)
🗑️ Would remove 2 orphaned packages:
  - unused-library@1.0.0
  - old-tools@2.1.0
```

*Interactive Mode:*
```
🔍 Analyzing packages for cleanup...
📦 Found 2 orphaned packages to remove:
  - unused-library@1.0.0
  - old-tools@2.1.0

These packages are not used by any active HPM projects.
Remove these packages? [y/N]: y

✓ Removed unused-library@1.0.0
✓ Removed old-tools@2.1.0
🎉 Cleanup completed: removed 2 orphaned packages
```

*Python Virtual Environment Cleanup:*
```
# Python-only cleanup
hpm clean --python-only --dry-run
🔍 Analyzing Python virtual environments for cleanup (dry run)...
Found 3 orphaned virtual environments that would be removed:
  - ~/.hpm/venvs/a1b2c3d4e5f6
  - ~/.hpm/venvs/f6e5d4c3b2a1  
  - ~/.hpm/venvs/9x8y7z6w5v4u
Would free approximately: 250 MB

# Comprehensive cleanup  
hpm clean --comprehensive --yes
🔍 Performing comprehensive cleanup (packages + Python environments)...
Successfully removed 2 orphaned packages:
  - unused-library@1.0.0
  - old-tools@2.1.0
Successfully removed 3 orphaned virtual environments:
  - ~/.hpm/venvs/a1b2c3d4e5f6
  - ~/.hpm/venvs/f6e5d4c3b2a1
  - ~/.hpm/venvs/9x8y7z6w5v4u
Total disk space freed: 280 MB
```

**Exit Codes:**
- `0` - Cleanup completed successfully
- `1` - Cleanup failed due to errors

## Package Templates

### Standard Houdini Package (Default)
- Complete directory structure following Houdini conventions
- Includes directories for all standard Houdini asset types:
  - **otls/**: Digital assets (.hda, .otl files)
  - **python/**: Python modules and libraries
  - **scripts/**: Shelf tools, event handlers, and automation scripts
  - **presets/**: Node parameter presets and configurations
  - **config/**: Environment and pipeline configuration files
  - **tests/**: Test files and validation scripts
- Automatic generation of `package.json` for Houdini integration
- Standard project files (README.md, .gitignore)
- Git repository initialization (unless `--vcs none`)

### Minimal Package (`--bare`)
- Contains only `hpm.toml` manifest file
- For custom package layouts and specialized use cases
- Maximum flexibility for users who want complete control over structure
- Can be extended manually with any directory structure needed

## Integration with Houdini

### Package.json Generation
HPM automatically generates standard Houdini `package.json` files from `hpm.toml`:

```json
{
    "hpath": ["$HPM_PACKAGE_ROOT/otls", "$HPM_PACKAGE_ROOT/python"],
    "env": [
        {"PYTHONPATH": {"method": "prepend", "value": "$HPM_PACKAGE_ROOT/python"}},
        {"HOUDINI_SCRIPT_PATH": {"method": "prepend", "value": "$HPM_PACKAGE_ROOT/scripts"}}
    ],
    "enable": "houdini_version >= '19.5' and houdini_version <= '20.5'"
}
```

### Installation Locations
- User packages: `$HOUDINI_USER_PREF_DIR/packages/hpm/`
- Project packages: `$HOUDINI_PACKAGE_DIR/hpm/`
- System packages: `$HOUDINI_INSTALL_DIR/packages/hpm/`

### Environment Variables
- `HPM_PACKAGE_ROOT` - Root directory of installed package
- `HPM_CONFIG_HOME` - HPM configuration directory
- `HPM_CACHE_DIR` - Package cache directory

## Configuration Management

### Global Configuration (`~/.hpm/config.toml`)
```toml
[registry]
default = "https://packages.houdini.org"
token = "your-auth-token"

[install]
location = "user"  # user, project, system
parallel = 4

[cache]
directory = "~/.hpm/cache"
ttl = 86400  # 24 hours

[log]
level = "info"
```

### Project Configuration (`.hpm/config.toml`)
```toml
[install]
location = "project"

[registry]
url = "https://internal.registry.com"
```

## Error Handling and Validation

### Package Validation
- Semantic version compliance
- Houdini version compatibility
- Asset file existence
- Dependency resolution
- Manifest schema validation

### Error Categories
- Configuration errors
- Network and registry errors
- File system errors
- Dependency resolution errors
- Houdini integration errors

### Recovery Strategies
- Automatic retry for transient failures
- Rollback for failed installations
- Cache invalidation for corrupted data
- Dependency conflict resolution

## Security Features

### Package Integrity
- Cryptographic signature verification
- Checksum validation
- Source verification

### Sandboxing
- Isolated package extraction
- Path validation and sanitization
- Permission validation

### Audit Trail
- Installation logging
- Dependency tracking
- Security vulnerability scanning