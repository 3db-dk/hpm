# HPM Package Manifest Specification

## Overview

HPM uses a `hpm.toml` manifest file to define package metadata, dependencies, and build configuration. This file works alongside Houdini's native `package.json` to provide modern package management features while maintaining compatibility with Houdini's package system.

## File Structure

HPM packages have the following structure:

```
my-houdini-package/
├── hpm.toml           # HPM package manifest (required)
├── package.json       # Generated Houdini package file
├── README.md          # Package documentation
├── LICENSE            # License file
├── otls/             # Digital assets
│   ├── my_node.hda
│   └── utility.otl
├── python/           # Python modules
│   ├── __init__.py
│   ├── my_tool.py
│   └── utils/
├── scripts/          # Shelf tools and scripts
│   └── my_shelf_tool.py
├── presets/          # Node presets
│   └── my_node.preset
├── config/           # Configuration files
│   └── env.json
└── tests/            # Package tests
    └── test_my_node.py
```

## hpm.toml Specification

### `[package]` Section (Required)

Basic package information following Cargo/PyProject conventions:

```toml
[package]
name = "my-houdini-tool"           # Package name (required)
version = "1.0.0"                  # Semantic version (required)
description = "A useful Houdini tool"  # Short description
authors = ["Name <email@domain.com>"]   # List of authors
license = "MIT"                    # License identifier
readme = "README.md"               # Path to readme file
homepage = "https://example.com"   # Project homepage
repository = "https://github.com/user/repo"  # Source repository
documentation = "https://docs.example.com"   # Documentation URL
keywords = ["houdini", "modeling", "vfx"]    # Search keywords
categories = ["digital-assets", "modeling"]   # Package categories
```

### `[houdini]` Section (Required)

Houdini-specific configuration:

```toml
[houdini]
min_version = "19.5"               # Minimum Houdini version
max_version = "21.0"               # Maximum Houdini version  
contexts = ["sop", "lop", "cop"]   # Supported Houdini contexts
build_requires = ["cmake", "gcc"]  # Build-time requirements
```

### `[dependencies]` Section

Package dependencies with version constraints:

```toml
[dependencies]
utility-nodes = "^2.1.0"          # Compatible with 2.x
math-lib = "~1.5.0"               # Compatible with 1.5.x
geometry-tools = { version = "1.0", optional = true }
custom-package = { path = "../local-package" }
git-package = { git = "https://github.com/user/repo.git", tag = "v1.0" }
```

### `[dev-dependencies]` Section

Development and testing dependencies:

```toml
[dev-dependencies]
test-framework = "0.1.0"
benchmark-tools = "2.0"
```

### `[[assets]]` Section

Define digital assets and their properties:

```toml
[[assets]]
name = "my_custom_sop"             # Asset identifier
path = "otls/my_custom_sop.hda"    # Path to asset file
type = "hda"                       # Asset type: hda, otl, python, script
contexts = ["sop"]                 # Houdini contexts where asset applies
description = "Custom SOP node"    # Asset description
version = "1.0"                    # Asset version (optional)

[[assets]]
name = "utility_functions"
path = "python/utility_functions.py"
type = "python"
contexts = ["*"]                   # All contexts
```

### `[build]` Section

Build configuration and scripts:

```toml
[build]
build-script = "build.py"         # Custom build script
exclude = ["tests/", "*.tmp"]     # Files to exclude from package
include = ["otls/", "python/"]    # Files to explicitly include
```

### `[scripts]` Section

Custom commands and workflows:

```toml
[scripts]
test = "python -m pytest tests/"
lint = "ruff check python/"
build-docs = "sphinx-build docs/ docs/_build"
```

### `[tool.hpm]` Section

HPM-specific configuration:

```toml
[tool.hpm]
registry = "https://registry.houdini-packages.org"  # Custom registry
cache-dir = "~/.hpm/cache"        # Custom cache directory
install-location = "user"         # user, site, or custom path

[tool.hpm.package-json]
# Custom package.json generation settings
template = "custom_template.json"
extra-env = { "CUSTOM_VAR" = "$HPM_PACKAGE_ROOT/bin" }
```

## Version Constraints

HPM uses Semantic Versioning with Cargo-style version constraints:

- `"1.0.0"` - Exact version
- `"^1.0.0"` - Compatible with 1.x (>= 1.0.0, < 2.0.0)
- `"~1.0.0"` - Compatible with 1.0.x (>= 1.0.0, < 1.1.0)
- `">=1.0.0"` - Greater than or equal
- `"1.0.0 - 2.0.0"` - Range (inclusive)
- `"*"` - Any version

## Asset Types

HPM supports the following asset types:

### `hda` - Houdini Digital Assets
```toml
[[assets]]
name = "my_node"
path = "otls/my_node.hda"
type = "hda"
contexts = ["sop", "lop"]
```

### `python` - Python Modules
```toml
[[assets]]
name = "my_module"
path = "python/my_module.py"
type = "python"
contexts = ["*"]
```

### `script` - Shelf Tools and Scripts
```toml
[[assets]]
name = "my_tool"
path = "scripts/my_tool.py"
type = "script"
contexts = ["*"]
```

### `preset` - Node Presets
```toml
[[assets]]
name = "my_preset"
path = "presets/my_node.preset"
type = "preset"
contexts = ["sop"]
```

### `config` - Configuration Files
```toml
[[assets]]
name = "environment"
path = "config/env.json"
type = "config"
contexts = ["*"]
```

## Generated package.json

HPM automatically generates a Houdini-compatible `package.json` from the `hpm.toml`. Example transformation:

**hpm.toml:**
```toml
[package]
name = "my-tool"
version = "1.0.0"

[houdini]
min_version = "20.0"

[[assets]]
name = "my_sop"
path = "otls/my_sop.hda"
type = "hda"
contexts = ["sop"]
```

**Generated package.json:**
```json
{
    "hpath": "$HPM_PACKAGE_ROOT/otls",
    "env": [
        {"HPM_PACKAGE_ROOT": "$PACKAGE_PATH"},
        {"HPM_PACKAGE_NAME": "my-tool"},
        {"HPM_PACKAGE_VERSION": "1.0.0"}
    ],
    "enable": "houdini_version >= '20.0'",
    "package_name": "my-tool",
    "package_version": "1.0.0"
}
```

## Best Practices

### Naming Conventions
- Package names: lowercase with hyphens (`my-houdini-tool`)
- Asset names: lowercase with underscores (`my_custom_node`)
- Follow semantic versioning (major.minor.patch)

### File Organization
- Keep related assets in appropriate directories
- Use consistent naming across assets
- Include comprehensive documentation
- Add tests for complex functionality

### Dependencies
- Specify minimum required versions
- Use version ranges for compatibility
- Keep dependency tree minimal
- Document external requirements

### Metadata
- Provide clear descriptions and keywords
- Include comprehensive author information
- Specify appropriate license
- Link to documentation and repository

This specification provides the foundation for HPM's package management system while maintaining compatibility with Houdini's existing package infrastructure.