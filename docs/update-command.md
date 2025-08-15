# HPM Update Command Documentation

## Overview

The `hpm update` command provides intelligent package dependency updates with efficient dependency resolution and Python virtual environment management. It uses UV-inspired algorithms to ensure optimal performance while maintaining compatibility and consistency across complex dependency graphs.

## Basic Usage

### Update All Packages
```bash
hpm update
```
Updates all packages in the current project to their latest compatible versions.

### Update Specific Packages
```bash
hpm update numpy geometry-tools material-library
```
Updates only the specified packages, maintaining consistency with other dependencies.

### Preview Updates
```bash
hpm update --dry-run
```
Shows what updates would be applied without making any changes.

## Command Options

### Package Selection

| Option | Description | Example |
|--------|-------------|---------|
| `[PACKAGES...]` | Specify packages to update | `hpm update numpy scipy` |
| `--package PATH` | Path to project or manifest | `hpm update --package /path/to/project/` |

### Update Control

| Option | Description | Default |
|--------|-------------|---------|
| `--dry-run` | Preview changes without applying | `false` |
| `--yes` | Skip confirmation prompts | `false` |

### Output Format

| Option | Description | Use Case |
|--------|-------------|----------|
| `--output human` | Human-readable output | Interactive use |
| `--output json` | Pretty-printed JSON | Scripts, debugging |
| `--output json-lines` | Line-delimited JSON | Streaming, logs |
| `--output json-compact` | Compact JSON | Bandwidth efficiency |

## Advanced Usage

### Project-Specific Updates

```bash
# Update dependencies in specific project directory
hpm update --package /path/to/houdini/project/

# Update using custom manifest file
hpm update --package /path/to/custom-manifest.toml

# Update current directory (explicit)
hpm update --package ./
```

### Automation and Scripting

```bash
# Automated updates for CI/CD
hpm update --yes --output json-compact

# Check for updates without applying
hpm update --dry-run --output json | jq '.updates[].name'

# Streaming updates for real-time monitoring  
hpm update --output json-lines | while read line; do
  echo "$line" | jq -r '.message'
done
```

### Selective Package Updates

```bash
# Update only Python dependencies
hpm update numpy scipy matplotlib requests

# Update only HPM packages  
hpm update geometry-tools mesh-utilities material-library

# Update packages matching pattern (using shell expansion)
hpm update *-tools *-utils
```

## Output Formats

### Human-Readable Output

Default format for interactive terminal usage:

```
Updating dependencies for my-houdini-project

The following packages will be updated:
  -> numpy 1.20.0 -> 1.24.0 (Python)
  -> geometry-tools 2.1.0 -> 2.2.0 (HPM)
  -> material-library 1.5.0 -> 1.6.0 (HPM)

Proceed with updates? [y/N] y

Resolving updated Python dependencies...
Python virtual environment updated successfully
✓ Updated numpy==1.24.0
✓ Updated geometry-tools
✓ Updated material-library

Successfully updated 3 packages
```

### JSON Output

Structured format for automation and integration:

```json
{
  "success": true,
  "message": "3 packages updated",
  "updated": [
    "numpy==1.24.0",
    "geometry-tools",
    "material-library"
  ],
  "metadata": {
    "resolution_time_ms": 1250,
    "python_env_updated": true,
    "packages_analyzed": 15
  }
}
```

### JSON Lines Output

Streaming format for real-time processing:

```jsonl
{"type": "progress", "message": "Analyzing dependencies", "step": 1, "total": 5}
{"type": "progress", "message": "Querying registry for versions", "step": 2, "total": 5}  
{"type": "update", "package": "numpy", "from": "1.20.0", "to": "1.24.0", "type": "python"}
{"type": "update", "package": "geometry-tools", "from": "2.1.0", "to": "2.2.0", "type": "hpm"}
{"type": "result", "success": true, "updated": 3, "elapsed_ms": 1250}
```

## Dependency Resolution

### Algorithm

HPM uses a PubGrub-inspired dependency resolution algorithm that provides:

- **Optimal Solutions**: Finds the best possible version combination
- **Conflict Resolution**: Intelligently resolves version conflicts  
- **Performance**: Optimized for large dependency graphs
- **Deterministic Results**: Consistent results across runs

### Version Constraints

The resolver understands semantic versioning constraints:

| Constraint | Syntax | Meaning | Example |
|------------|---------|---------|---------|
| Exact | `==1.2.3` | Exact version match | `numpy==1.24.0` |
| Compatible | `^1.2.3` | Compatible range | `^2.1.0` allows `2.1.x`, `2.2.x` |
| Tilde | `~1.2.3` | Patch-level changes | `~1.2.0` allows `1.2.x` |
| Greater/Equal | `>=1.2.3` | Minimum version | `>=1.20.0` |
| Range | `>=1.0,<2.0` | Version range | Complex constraints |

### Conflict Resolution

When version conflicts occur, the resolver:

1. **Analyzes Constraints**: Identifies conflicting requirements
2. **Finds Alternatives**: Looks for compatible version combinations
3. **Reports Conflicts**: Provides detailed error information if no solution exists
4. **Suggests Fixes**: Recommends specific actions to resolve conflicts

## Python Environment Management

### Content-Addressable Environments

HPM uses content-addressable virtual environments for Python dependencies:

- **Hash-Based**: Each unique set of resolved dependencies gets a unique hash
- **Environment Sharing**: Packages with identical dependencies share environments  
- **Automatic Migration**: Updates create new environments only when dependencies change
- **Intelligent Cleanup**: Removes orphaned environments while preserving shared ones

### Environment Lifecycle

```text
┌─────────────────┐    ┌──────────────────┐    ┌─────────────────────┐
│ Dependency      │    │ Hash             │    │ Environment         │
│ Resolution      │───▶│ Calculation      │───▶│ Creation/Reuse      │
│ (via UV)        │    │ (SHA-256)        │    │                     │
└─────────────────┘    └──────────────────┘    └─────────────────────┘
           │                       │                        │
           ▼                       ▼                        ▼
┌─────────────────┐    ┌──────────────────┐    ┌─────────────────────┐
│ Package         │    │ Environment      │    │ Old Environment     │
│ Linking         │    │ Activation       │    │ Cleanup             │
└─────────────────┘    └──────────────────┘    └─────────────────────┘
```

### Environment Sharing Example

```bash
# Package A and B have identical Python dependencies
# They share the same virtual environment

Project A: numpy==1.24.0, requests==2.28.0 → venv: a1b2c3d4
Project B: numpy==1.24.0, requests==2.28.0 → venv: a1b2c3d4 (shared)
Project C: numpy==1.23.0, requests==2.28.0 → venv: e5f6g7h8 (different)
```

## Error Handling

### Version Conflicts

```bash
$ hpm update
error: Version conflict detected
  package: geometry-tools
  conflicting requirements:
    material-library requires geometry-tools ^2.0.0  
    mesh-utilities requires geometry-tools ~1.5.0
  help: Update mesh-utilities to version 2.x or use compatible geometry-tools version
  suggestion: Try 'hpm update mesh-utilities' first
```

### Network Errors

```bash  
$ hpm update
error: Registry connection failed
  registry: https://packages.houdini.org
  operation: fetch package versions for 'geometry-tools'
  cause: Connection timeout after 30s
  help: Check network connectivity or try again later
```

### Python Environment Errors

```bash
$ hpm update numpy
error: Python virtual environment creation failed
  python_version: 3.9
  packages: numpy==1.24.0, scipy==1.10.0
  cause: Package installation failed - numpy compilation error
  help: Check Python development headers are installed
  suggestion: sudo apt-get install python3-dev (Ubuntu/Debian)
```

## Performance Characteristics

### Dependency Resolution Performance

| Scenario | Package Count | Resolution Time | Memory Usage |
|----------|---------------|-----------------|--------------|
| Small Project | 10-20 packages | <100ms | ~5MB |
| Medium Project | 50-100 packages | <500ms | ~15MB |
| Large Project | 200+ packages | <2s | ~50MB |
| Complex Conflicts | Variable | <10s | ~100MB |

### Virtual Environment Performance  

| Operation | Time | Notes |
|-----------|------|-------|
| Environment Reuse | ~50ms | When hash matches existing |
| New Environment | ~5-15s | Python + package installation |
| Environment Cleanup | ~100ms | Removing unused environments |
| Dependency Resolution | ~1-3s | Via UV, depends on package count |

## Integration Examples

### CI/CD Pipeline

```yaml
# GitHub Actions example
- name: Update HPM dependencies
  run: |
    hpm update --dry-run --output json > update-preview.json
    if jq -e '.updates | length > 0' update-preview.json; then
      hpm update --yes --output json-lines | tee update-results.jsonl
      echo "Dependencies updated successfully"
    else
      echo "No updates available"  
    fi
```

### Automated Monitoring

```bash
#!/bin/bash
# Check for updates daily

UPDATE_CHECK=$(hpm update --dry-run --output json)
UPDATE_COUNT=$(echo "$UPDATE_CHECK" | jq '.updates | length')

if [ "$UPDATE_COUNT" -gt 0 ]; then
    echo "Updates available for $UPDATE_COUNT packages"
    echo "$UPDATE_CHECK" | jq -r '.updates[].name' | mail -s "HPM Updates Available" admin@studio.com
fi
```

### Custom Tooling Integration

```python
#!/usr/bin/env python3
import subprocess
import json

def check_hpm_updates():
    """Check for HPM package updates"""
    result = subprocess.run([
        'hpm', 'update', '--dry-run', '--output', 'json'
    ], capture_output=True, text=True)
    
    if result.returncode != 0:
        raise Exception(f"HPM update check failed: {result.stderr}")
    
    data = json.loads(result.stdout)
    return data.get('updates', [])

def apply_hpm_updates():
    """Apply HPM package updates"""
    result = subprocess.run([
        'hpm', 'update', '--yes', '--output', 'json'
    ], capture_output=True, text=True)
    
    if result.returncode != 0:
        raise Exception(f"HPM update failed: {result.stderr}")
    
    data = json.loads(result.stdout)
    return data.get('updated', [])

# Usage
updates = check_hpm_updates()
if updates:
    print(f"Found {len(updates)} updates")
    updated = apply_hpm_updates()
    print(f"Successfully updated {len(updated)} packages")
```

## Troubleshooting

### Common Issues

#### 1. No Updates Found
```bash
$ hpm update
All packages are up to date
```
**Cause**: All packages are already at their latest compatible versions.
**Solution**: Check if you want to update to incompatible versions or relax constraints.

#### 2. Version Constraint Conflicts
**Cause**: Multiple packages require incompatible versions of the same dependency.
**Solution**: Update packages to compatible versions or adjust version constraints in `hpm.toml`.

#### 3. Network/Registry Issues
**Cause**: Cannot connect to package registry or fetch package information.
**Solution**: Check network connectivity, registry configuration, or try again later.

#### 4. Python Environment Issues
**Cause**: Python virtual environment creation or package installation fails.
**Solution**: Check Python installation, development headers, and disk space.

### Debug Information

Enable verbose logging for detailed information:

```bash
# Enable debug logging
export RUST_LOG=hpm=debug
hpm update

# Enable trace logging (very verbose)
export RUST_LOG=hpm=trace  
hpm update geometry-tools
```

### Configuration

Update behavior can be configured in `~/.hpm/config.toml`:

```toml
[update]
# Prefer latest versions vs. minimal updates
prefer_latest = true

# Allow prerelease versions
allow_prereleases = false

# Maximum time to spend on dependency resolution
resolution_timeout_secs = 300

# Confirmation prompts
interactive = true

[python]
# Python environment cleanup behavior
cleanup_orphaned_envs = true

# Python version selection
preferred_python_version = "3.9"
```