# HPM Package Storage Architecture

## Overview

This document outlines the package storage and management architecture for HPM, drawing insights from uv's approach while adapting to Houdini's unique package loading system.

## Key System Comparisons

### uv Approach
- **Global Storage**: `~/.local/share/uv` and `~/.cache/uv` for global package cache
- **Project Environment**: `.venv` directory in each project for isolated environments  
- **Activation**: Virtual environments must be activated or use `uv run` for automatic activation
- **Linking**: Symlinks and direct file copying into virtual environments
- **Configuration**: `pyproject.toml` and `uv.lock` files manage dependencies

### Houdini Package System
- **Loading Method**: `HOUDINI_PACKAGE_PATH` environment variable points to package directories
- **Package Files**: `.json` files that define environment variables and paths
- **Discovery Locations**: 
  - `$HOUDINI_USER_PREF_DIR/packages`
  - `$HOUDINI_PACKAGE_DIR`
  - Custom paths via `HOUDINI_PACKAGE_PATH`
- **No Activation**: Houdini automatically loads packages at startup
- **Global Nature**: Packages are loaded globally within Houdini session

## HPM Architecture Design

### Global Package Storage (~/.hpm)

```
~/.hpm/
├── packages/                    # Global package storage
│   ├── package-name@1.0.0/     # Versioned package installations
│   │   ├── otls/
│   │   ├── python/
│   │   ├── scripts/
│   │   └── hpm.toml
│   └── another-package@2.1.0/
├── cache/                       # Download cache and metadata
│   ├── downloads/
│   └── metadata/
├── config.toml                  # Global HPM configuration
└── registry/                    # Registry metadata cache
    └── index.json
```

### Project-Specific Package Management

Each project maintains:

```
project/
├── .hpm/
│   ├── packages/               # Project-specific package manifests
│   │   ├── package-name.json   # Houdini package.json linking to ~/.hpm
│   │   └── another-package.json
│   ├── hpm.lock               # Dependency lock file
│   └── config.toml            # Project configuration
├── hpm.toml                   # Project manifest
└── [project files]
```

### Key Architectural Differences

| Aspect | uv | HPM |
|--------|----|----- |
| **Storage Model** | Global cache + project .venv | Global storage + project manifests |
| **Isolation** | Virtual environment activation | Houdini package.json files |
| **Linking Strategy** | Copy/symlink to .venv | JSON manifests pointing to global storage |
| **Activation** | Manual activation or `uv run` | Automatic via HOUDINI_PACKAGE_PATH |
| **Environment** | Python-specific virtualenv | Houdini session environment |

## Implementation Strategy

### Phase 1: Global Storage System
1. **Directory Management**: Create and manage `~/.hpm` structure
2. **Package Installation**: Download and extract packages to versioned directories
3. **Metadata Management**: Track installed packages and their versions
4. **Configuration**: Global settings and registry configuration

### Phase 2: Project Integration
1. **Manifest Generation**: Create Houdini package.json files for each dependency
2. **Path Management**: Configure HOUDINI_PACKAGE_PATH to include project package directory
3. **Dependency Resolution**: Ensure correct versions are linked to projects
4. **Lock File**: Maintain hpm.lock for reproducible environments

### Phase 3: Advanced Features
1. **Version Switching**: Support multiple versions of same package
2. **Environment Isolation**: Per-project package environments
3. **Cleanup**: Remove unused packages and manage disk space
4. **Update Management**: Handle package updates and version conflicts

## Houdini Package.json Generation

For each installed package dependency, HPM generates a Houdini package.json file:

```json
{
    "name": "hpm-package-name",
    "description": "HPM managed package: package-name v1.0.0",
    "hpath": "$HOUDINI_PACKAGE_PATH/../../../.hpm/packages/package-name@1.0.0",
    "env": [
        {
            "PACKAGE_NAME_ROOT": "$HOME/.hpm/packages/package-name@1.0.0"
        }
    ],
    "load_package_once": true,
    "enable": true
}
```

## Benefits of This Architecture

1. **Disk Efficiency**: Single global storage prevents duplicate package installations
2. **Version Management**: Multiple versions can coexist in global storage
3. **Houdini Integration**: Seamless integration with existing Houdini package system
4. **Project Isolation**: Each project can use different package versions
5. **No Environment Activation**: Works automatically within Houdini sessions
6. **Familiar Patterns**: Similar to other package managers but adapted for Houdini

## Environment Variable Strategy

- **HOUDINI_PACKAGE_PATH**: Point to project's `.hpm/packages/` directory
- **HPM_HOME**: Override default `~/.hpm` location if needed
- **HPM_CACHE_DIR**: Custom cache location
- **HOUDINI_USER_PREF_DIR**: Standard Houdini user directory for integration

## Security and Safety Considerations

1. **Path Validation**: Ensure package paths don't escape intended directories
2. **Package Verification**: Verify package integrity before installation
3. **Permission Management**: Appropriate file permissions for shared installations
4. **Cleanup Safety**: Safe removal of packages and directories
5. **Conflict Resolution**: Handle version conflicts gracefully

This architecture provides the foundation for implementing a robust, efficient, and user-friendly package management system for Houdini that leverages the best practices from modern package managers while respecting Houdini's unique requirements.