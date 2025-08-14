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

### `[houdini]` Section (Optional)

Houdini-specific configuration:

```toml
[houdini]
min_version = "19.5"               # Minimum Houdini version
max_version = "21.0"               # Maximum Houdini version (optional)
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







### `[scripts]` Section

Custom commands and workflows:

```toml
[scripts]
test = "python -m pytest tests/"
lint = "ruff check python/"
build-docs = "sphinx-build docs/ docs/_build"
```



## Version Constraints

HPM uses Semantic Versioning with Cargo-style version constraints:

- `"1.0.0"` - Exact version
- `"^1.0.0"` - Compatible with 1.x (>= 1.0.0, < 2.0.0)
- `"~1.0.0"` - Compatible with 1.0.x (>= 1.0.0, < 1.1.0)
- `">=1.0.0"` - Greater than or equal
- `"1.0.0 - 2.0.0"` - Range (inclusive)
- `"*"` - Any version



## Generated package.json

HPM automatically generates a Houdini-compatible `package.json` from the `hpm.toml`. Example transformation:

**hpm.toml:**
```toml
[package]
name = "my-tool"
version = "1.0.0"
description = "My Houdini tool"
authors = ["Author <author@example.com>"]
license = "MIT"
readme = "README.md"
keywords = ["houdini"]

[houdini]
min_version = "20.0"
```

**Generated package.json:**
```json
{
    "hpath": ["$HPM_PACKAGE_ROOT/otls"],
    "env": [
        {"PYTHONPATH": {"method": "prepend", "value": "$HPM_PACKAGE_ROOT/python"}},
        {"HOUDINI_SCRIPT_PATH": {"method": "prepend", "value": "$HPM_PACKAGE_ROOT/scripts"}}
    ],
    "enable": "houdini_version >= '20.0'"
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