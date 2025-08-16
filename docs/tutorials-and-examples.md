# HPM Tutorials and Examples

This document provides step-by-step tutorials and practical examples for common HPM (Houdini Package Manager) workflows. Whether you're creating your first Houdini package, managing complex dependencies, or setting up a development environment, these tutorials will guide you through real-world scenarios.

## Table of Contents

1. [Getting Started Tutorial](#getting-started-tutorial)
2. [Package Creation Workshop](#package-creation-workshop)
3. [Dependency Management Scenarios](#dependency-management-scenarios)
4. [Python Integration Examples](#python-integration-examples)
5. [Advanced Workflows](#advanced-workflows)
6. [Team and Studio Setup](#team-and-studio-setup)
7. [Troubleshooting Common Issues](#troubleshooting-common-issues)
8. [Best Practices Guide](#best-practices-guide)

## Getting Started Tutorial

This tutorial walks you through your first experience with HPM, from installation to creating and managing your first package.

### Prerequisites

Before starting, ensure you have:
- **Rust 1.70+** installed (`rustup` recommended)
- **SideFX Houdini 19.5+** for testing integration
- **Git** for version control (optional but recommended)

### Step 1: Installation

Since HPM is currently in development, you'll build from source:

```bash
# Clone the HPM repository
git clone https://github.com/hpm-org/hpm.git
cd hpm

# Build HPM (this may take a few minutes)
cargo build --release

# The hpm binary is now available at target/release/hpm
# Add it to your PATH for convenience
export PATH="$PWD/target/release:$PATH"

# Verify installation
hpm --version
# Output: hpm 0.1.0
```

### Step 2: Create Your First Package

Let's create a simple Houdini package for custom geometry tools:

```bash
# Create a new package
hpm init geometry-utilities \
  --description "Custom geometry processing tools for Houdini" \
  --author "Your Name <your.email@example.com>" \
  --license MIT \
  --houdini-min 20.0

# Navigate to the package directory
cd geometry-utilities

# Examine the generated structure
ls -la
```

You should see:
```
geometry-utilities/
├── hpm.toml              # Package manifest
├── package.json          # Generated Houdini package file
├── README.md             # Package documentation
├── .gitignore           # Git ignore file
├── otls/                # Digital assets directory
├── python/              # Python modules directory
│   └── __init__.py
├── scripts/             # Shelf tools and scripts
├── presets/             # Node presets
├── config/              # Configuration files
└── tests/               # Test files
```

### Step 3: Understanding the Package Manifest

Examine the generated `hpm.toml`:

```toml
[package]
name = "geometry-utilities"
version = "0.1.0"
description = "Custom geometry processing tools for Houdini"
authors = ["Your Name <your.email@example.com>"]
license = "MIT"
readme = "README.md"
keywords = ["houdini", "geometry"]

[houdini]
min_version = "20.0"

[dependencies]
# Add HPM package dependencies here

[python_dependencies]
# Add Python package dependencies here

[scripts]
# Add custom scripts here
```

### Step 4: Add Some Content

Let's add a simple Python utility:

```bash
# Create a Python utility module
cat > python/geometry_utils.py << 'EOF'
"""Geometry utilities for Houdini."""

import hou

def create_subdivided_sphere(name="subdivided_sphere", divisions=2):
    """Create a subdivided sphere geometry."""
    # Create geometry container
    geo = hou.node('/obj').createNode('geo', name)
    
    # Create sphere
    sphere = geo.createNode('sphere')
    sphere.setParms({'type': 1})  # Primitive sphere
    
    # Create subdivision surface
    subdivide = geo.createNode('subdivide')
    subdivide.setFirstInput(sphere)
    subdivide.setParms({'iterations': divisions})
    
    # Set display and render flags
    subdivide.setDisplayFlag(True)
    subdivide.setRenderFlag(True)
    
    return geo

def get_geometry_info(node):
    """Get basic information about geometry."""
    if not node or not hasattr(node, 'geometry'):
        return None
        
    geo = node.geometry()
    if not geo:
        return None
        
    return {
        'points': len(geo.points()),
        'prims': len(geo.prims()),
        'vertices': len(geo.vertices()),
        'groups': {
            'point_groups': [g.name() for g in geo.pointGroups()],
            'prim_groups': [g.name() for g in geo.primGroups()],
        }
    }
EOF

# Update the __init__.py to expose our utilities
cat > python/__init__.py << 'EOF'
"""Geometry Utilities Package"""

from .geometry_utils import create_subdivided_sphere, get_geometry_info

__version__ = "0.1.0"
__all__ = ["create_subdivided_sphere", "get_geometry_info"]
EOF
```

### Step 5: Add a Shelf Tool

Create a shelf tool that uses our Python utility:

```bash
# Create shelf tools directory if it doesn't exist
mkdir -p scripts

# Create a shelf tool script
cat > scripts/create_subdivided_sphere.py << 'EOF'
"""Shelf tool to create subdivided sphere."""

# Import our utility (this will work when the package is properly loaded)
try:
    from geometry_utilities import create_subdivided_sphere
    
    # Create the sphere with user input
    divisions = hou.ui.readInput("Subdivision Divisions", 
                                buttons=("OK", "Cancel"), 
                                initial_contents="2")[1]
    
    if divisions:
        divisions = int(divisions) if divisions.isdigit() else 2
        sphere_node = create_subdivided_sphere(divisions=divisions)
        
        # Frame the new geometry in viewport
        desktop = hou.ui.curDesktop()
        scene = desktop.paneTabOfType(hou.paneTabType.SceneViewer)
        if scene:
            scene.setCurrentState('select')
            scene.frameSelection()
            
        hou.ui.displayMessage(f"Created subdivided sphere with {divisions} divisions")
        
except ImportError as e:
    hou.ui.displayMessage(f"Error importing geometry utilities: {e}", 
                         severity=hou.severityType.Error)
except Exception as e:
    hou.ui.displayMessage(f"Error creating sphere: {e}", 
                         severity=hou.severityType.Error)
EOF
```

### Step 6: Test Your Package

Now let's test that our package works:

```bash
# Validate the package configuration
hpm check

# List the package details
hpm list

# Check if everything looks correct
cat hmp.toml
```

### Step 7: Add Dependencies

Let's add some Python dependencies for advanced geometry processing:

```bash
# Add numpy for numerical operations
hpm add numpy --version ">=1.20.0"

# The command above would add this to your hpm.toml:
# [python_dependencies]
# numpy = ">=1.20.0"
```

Let's manually add it for now and update our utilities:

```bash
# Edit hpm.toml to add Python dependencies
cat >> hpm.toml << 'EOF'

[python_dependencies]
numpy = ">=1.20.0"
scipy = ">=1.7.0"
EOF

# Update our Python utility to use numpy
cat >> python/geometry_utils.py << 'EOF'

def smooth_point_positions(node, strength=0.5):
    """Smooth point positions using numpy operations."""
    try:
        import numpy as np
    except ImportError:
        hou.ui.displayMessage("NumPy not available. Install with: pip install numpy", 
                             severity=hou.severityType.Error)
        return None
        
    geo = node.geometry()
    if not geo:
        return None
        
    # Get point positions as numpy array
    positions = np.array([pt.position() for pt in geo.points()])
    
    # Simple smoothing by averaging neighboring points
    # This is a simplified example - real smoothing would be more sophisticated
    smoothed = positions * (1 - strength) + np.mean(positions, axis=0) * strength
    
    # Apply smoothed positions back to points
    for i, pt in enumerate(geo.points()):
        pt.setPosition(smoothed[i])
    
    return geo
EOF
```

### Step 8: Install Dependencies

```bash
# Install Python dependencies (this would resolve and create virtual environments)
hmp install

# This would:
# 1. Parse hpm.toml for Python dependencies
# 2. Use UV to resolve exact versions
# 3. Create a content-addressable virtual environment
# 4. Generate package.json with PYTHONPATH setup
```

### Step 9: Version Control (Optional)

If you want to track your package with Git:

```bash
# Initialize Git repository (if not already done)
git init

# Add all files
git add .

# Commit initial version
git commit -m "Initial version of geometry-utilities package"

# Optional: Add remote repository
# git remote add origin https://github.com/yourusername/geometry-utilities.git
# git push -u origin main
```

### Step 10: Test in Houdini

To test your package in Houdini:

1. **Set up Houdini package path**: Add your package directory to Houdini's package path
2. **Load Houdini**: Start Houdini and check that your package loads
3. **Test Python imports**: In Houdini's Python console, try importing your utilities
4. **Use shelf tools**: If you've created shelf tools, add them to a shelf and test

### Congratulations!

You've successfully created your first HPM package! You now have:

- ✅ A properly structured Houdini package
- ✅ Python utilities with external dependencies
- ✅ Package manifest with metadata and dependencies
- ✅ Version control setup (optional)
- ✅ Understanding of the HPM workflow

**Next Steps:**
- Explore adding more complex dependencies
- Learn about package publishing (when registry is available)
- Try the advanced workflows in the following sections

## Package Creation Workshop

This workshop dives deeper into creating different types of Houdini packages with HPM, covering various scenarios and best practices.

### Workshop 1: Digital Asset Package

Create a package focused on Houdini Digital Assets (HDAs):

```bash
# Create a package for custom digital assets
hpm init vfx-nodes \
  --description "Custom VFX nodes and digital assets" \
  --author "VFX Artist <artist@studio.com>" \
  --license "Apache-2.0" \
  --houdini-min 19.5 \
  --houdini-max 21.0

cd vfx-nodes
```

#### Adding Digital Assets

```bash
# Create a placeholder for digital assets
mkdir -p otls/vfx

# Create documentation for asset creation
cat > otls/README.md << 'EOF'
# VFX Nodes Digital Assets

This directory contains custom digital assets for VFX workflows.

## Asset Categories

- **vfx/**: Core VFX nodes
- **utility/**: Utility nodes for pipeline
- **deform/**: Deformation and animation tools

## Asset Naming Convention

Use the following naming convention for assets:
- `studio_category_nodename.hda`
- Examples: `vfx_fx_particleBurst.hda`, `utility_geo_cleanupGeo.hda`

## Installation

When this package is installed via HPM, these assets will be automatically
available in Houdini's TAB menu under the VFX category.
EOF
```

#### Configuring Asset Loading

```bash
# Update hpm.toml with asset-specific configuration
cat >> hpm.toml << 'EOF'

[houdini.assets]
# Automatically scan for HDAs in otls directory
auto_scan = true
# Custom categories for TAB menu
categories = ["VFX", "Utility", "Deform"]

[scripts]
validate_assets = "python scripts/validate_assets.py"
build_release = "python scripts/build_release.py"
EOF

# Create asset validation script
mkdir -p scripts
cat > scripts/validate_assets.py << 'EOF'
"""Validate digital assets in the package."""

import os
import hou

def validate_assets():
    """Validate all .hda files in the otls directory."""
    otls_dir = os.path.join(os.path.dirname(__file__), '..', 'otls')
    
    if not os.path.exists(otls_dir):
        print("No otls directory found")
        return False
    
    hda_files = []
    for root, dirs, files in os.walk(otls_dir):
        for file in files:
            if file.endswith('.hda') or file.endswith('.otl'):
                hda_files.append(os.path.join(root, file))
    
    print(f"Found {len(hda_files)} digital assets:")
    for hda in hda_files:
        print(f"  - {os.path.basename(hda)}")
        
        # Basic validation could include:
        # - File exists and is readable
        # - File is a valid HDA file
        # - Asset definition follows naming conventions
        
    return len(hda_files) > 0

if __name__ == "__main__":
    validate_assets()
EOF
```

### Workshop 2: Python-Heavy Package

Create a package that heavily uses Python and external libraries:

```bash
# Create package for Python-based tools
hpm init python-pipeline-tools \
  --description "Python-based pipeline tools for Houdini production" \
  --author "Pipeline TD <pipeline@studio.com>" \
  --houdini-min 20.0

cd python-pipeline-tools
```

#### Setting up Python Dependencies

```bash
# Edit hpm.toml to add comprehensive Python dependencies
cat > hpm.toml << 'EOF'
[package]
name = "python-pipeline-tools"
version = "1.0.0"
description = "Python-based pipeline tools for Houdini production"
authors = ["Pipeline TD <pipeline@studio.com>"]
license = "MIT"
readme = "README.md"
keywords = ["houdini", "pipeline", "python"]

[houdini]
min_version = "20.0"

[python_dependencies]
# Core scientific computing
numpy = ">=1.20.0"
scipy = ">=1.7.0"
pandas = ">=1.3.0"

# Image and data processing
pillow = ">=8.0.0"
opencv-python = ">=4.5.0"

# Network and API communication
requests = { version = ">=2.25.0", extras = ["security"] }
pyyaml = ">=5.4.0"

# Optional dependencies for advanced features
matplotlib = { version = ">=3.5.0", optional = true }
plotly = { version = ">=5.0.0", optional = true }

[scripts]
setup_env = "python scripts/setup_environment.py"
run_tests = "python -m pytest tests/"
build_docs = "python scripts/build_documentation.py"
EOF
```

#### Creating Python Modules

```bash
# Create a comprehensive Python module structure
mkdir -p python/pipeline_tools/{core,io,utils,ui}

# Core pipeline module
cat > python/pipeline_tools/__init__.py << 'EOF'
"""Python Pipeline Tools for Houdini."""

__version__ = "1.0.0"
__author__ = "Pipeline TD"

# Import main modules
from .core import asset_manager, scene_manager, version_control
from .io import file_handler, database_connector
from .utils import houdini_utils, system_utils
from .ui import qt_widgets, houdini_panels

# Convenience imports
from .core.asset_manager import AssetManager
from .core.scene_manager import SceneManager
from .io.file_handler import FileHandler

__all__ = [
    "AssetManager",
    "SceneManager", 
    "FileHandler",
    "asset_manager",
    "scene_manager",
    "version_control",
    "file_handler",
    "database_connector",
    "houdini_utils",
    "system_utils",
    "qt_widgets",
    "houdini_panels"
]
EOF

# Asset management module
cat > python/pipeline_tools/core/asset_manager.py << 'EOF'
"""Asset management utilities for Houdini pipeline."""

import os
import json
from typing import Dict, List, Optional
import hou


class AssetManager:
    """Manage pipeline assets in Houdini."""
    
    def __init__(self, asset_root: str):
        self.asset_root = asset_root
        self.metadata_cache = {}
    
    def get_asset_info(self, asset_path: str) -> Optional[Dict]:
        """Get asset metadata from path."""
        metadata_file = os.path.join(asset_path, "asset_info.json")
        
        if not os.path.exists(metadata_file):
            return None
            
        if asset_path not in self.metadata_cache:
            with open(metadata_file, 'r') as f:
                self.metadata_cache[asset_path] = json.load(f)
                
        return self.metadata_cache[asset_path]
    
    def list_assets(self, category: Optional[str] = None) -> List[Dict]:
        """List all available assets, optionally filtered by category."""
        assets = []
        
        for root, dirs, files in os.walk(self.asset_root):
            if "asset_info.json" in files:
                asset_info = self.get_asset_info(root)
                if asset_info:
                    if not category or asset_info.get("category") == category:
                        asset_info["path"] = root
                        assets.append(asset_info)
        
        return sorted(assets, key=lambda x: x.get("name", ""))
    
    def load_asset(self, asset_name: str, parent_node: Optional[hou.Node] = None) -> Optional[hou.Node]:
        """Load an asset into Houdini scene."""
        assets = self.list_assets()
        asset_info = None
        
        for asset in assets:
            if asset.get("name") == asset_name:
                asset_info = asset
                break
        
        if not asset_info:
            hou.ui.displayMessage(f"Asset '{asset_name}' not found", 
                                 severity=hou.severityType.Error)
            return None
        
        # Load the asset based on type
        asset_type = asset_info.get("type", "geometry")
        asset_path = asset_info["path"]
        
        if asset_type == "geometry":
            return self._load_geometry_asset(asset_path, parent_node)
        elif asset_type == "hda":
            return self._load_hda_asset(asset_path, parent_node)
        else:
            hou.ui.displayMessage(f"Unknown asset type: {asset_type}", 
                                 severity=hou.severityType.Warning)
            return None
    
    def _load_geometry_asset(self, asset_path: str, parent_node: Optional[hou.Node]) -> Optional[hou.Node]:
        """Load geometry asset."""
        if not parent_node:
            parent_node = hou.node("/obj")
        
        # Create file node and load geometry
        file_node = parent_node.createNode("file")
        geo_file = os.path.join(asset_path, "geometry.bgeo.sc")
        
        if os.path.exists(geo_file):
            file_node.parm("file").set(geo_file)
            return file_node
        
        return None
    
    def _load_hda_asset(self, asset_path: str, parent_node: Optional[hou.Node]) -> Optional[hou.Node]:
        """Load HDA asset."""
        hda_files = [f for f in os.listdir(asset_path) if f.endswith('.hda')]
        
        if not hda_files:
            return None
        
        hda_file = os.path.join(asset_path, hda_files[0])
        
        # Install HDA definition
        hou.hda.installFile(hda_file)
        
        # Get the node type from HDA
        definitions = hou.hda.definitionsInFile(hda_file)
        if not definitions:
            return None
        
        node_type = definitions[0].nodeType()
        
        if not parent_node:
            parent_node = hou.node("/obj")
        
        # Create node instance
        return parent_node.createNode(node_type.name())
EOF

# Create the core module init
cat > python/pipeline_tools/core/__init__.py << 'EOF'
"""Core pipeline functionality."""

from . import asset_manager, scene_manager, version_control

__all__ = ["asset_manager", "scene_manager", "version_control"]
EOF
```

#### Creating Tests

```bash
# Create test structure
mkdir -p tests/unit tests/integration

# Unit test for asset manager
cat > tests/unit/test_asset_manager.py << 'EOF'
"""Unit tests for AssetManager."""

import unittest
import tempfile
import os
import json
from unittest.mock import Mock, patch

# Import our module
import sys
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', '..', 'python'))

from pipeline_tools.core.asset_manager import AssetManager


class TestAssetManager(unittest.TestCase):
    """Test AssetManager functionality."""
    
    def setUp(self):
        """Set up test environment."""
        self.temp_dir = tempfile.mkdtemp()
        self.asset_manager = AssetManager(self.temp_dir)
    
    def tearDown(self):
        """Clean up test environment."""
        import shutil
        shutil.rmtree(self.temp_dir)
    
    def test_get_asset_info_nonexistent(self):
        """Test getting info for nonexistent asset."""
        result = self.asset_manager.get_asset_info("/nonexistent/path")
        self.assertIsNone(result)
    
    def test_get_asset_info_existing(self):
        """Test getting info for existing asset."""
        # Create test asset
        asset_dir = os.path.join(self.temp_dir, "test_asset")
        os.makedirs(asset_dir)
        
        asset_info = {
            "name": "test_asset",
            "version": "1.0.0",
            "category": "geometry",
            "description": "Test asset"
        }
        
        info_file = os.path.join(asset_dir, "asset_info.json")
        with open(info_file, 'w') as f:
            json.dump(asset_info, f)
        
        result = self.asset_manager.get_asset_info(asset_dir)
        self.assertEqual(result["name"], "test_asset")
        self.assertEqual(result["version"], "1.0.0")
    
    def test_list_assets_empty(self):
        """Test listing assets when none exist."""
        assets = self.asset_manager.list_assets()
        self.assertEqual(len(assets), 0)
    
    def test_list_assets_with_category_filter(self):
        """Test listing assets with category filter."""
        # Create test assets with different categories
        for i, category in enumerate(["geometry", "effects", "geometry"]):
            asset_dir = os.path.join(self.temp_dir, f"asset_{i}")
            os.makedirs(asset_dir)
            
            asset_info = {
                "name": f"asset_{i}",
                "category": category
            }
            
            info_file = os.path.join(asset_dir, "asset_info.json")
            with open(info_file, 'w') as f:
                json.dump(asset_info, f)
        
        # Test filtering
        all_assets = self.asset_manager.list_assets()
        self.assertEqual(len(all_assets), 3)
        
        geometry_assets = self.asset_manager.list_assets(category="geometry")
        self.assertEqual(len(geometry_assets), 2)
        
        effects_assets = self.asset_manager.list_assets(category="effects")
        self.assertEqual(len(effects_assets), 1)


if __name__ == "__main__":
    unittest.main()
EOF

# Create test runner script
cat > scripts/run_tests.py << 'EOF'
"""Run test suite for pipeline tools."""

import sys
import os
import unittest

# Add python directory to path
python_dir = os.path.join(os.path.dirname(__file__), '..', 'python')
sys.path.insert(0, python_dir)

def run_tests():
    """Run all tests in the tests directory."""
    tests_dir = os.path.join(os.path.dirname(__file__), '..', 'tests')
    
    # Discover and run tests
    loader = unittest.TestLoader()
    start_dir = tests_dir
    suite = loader.discover(start_dir, pattern='test_*.py')
    
    runner = unittest.TextTestRunner(verbosity=2)
    result = runner.run(suite)
    
    return result.wasSuccessful()

if __name__ == "__main__":
    success = run_tests()
    sys.exit(0 if success else 1)
EOF
```

### Workshop 3: Minimal/Bare Package

Sometimes you need a minimal package structure for specific use cases:

```bash
# Create minimal package with only hpm.toml
hmp init config-package --bare \
  --description "Configuration and settings package"

cd config-package

# Add only necessary configuration files
mkdir -p config/houdini config/pipeline

# Create Houdini environment configuration
cat > config/houdini/houdini.env << 'EOF'
# Custom Houdini environment variables
HOUDINI_SCRIPT_PATH = $HOUDINI_SCRIPT_PATH:$HPM_PACKAGE_ROOT/scripts
HOUDINI_PYTHON_PATH = $HOUDINI_PYTHON_PATH:$HPM_PACKAGE_ROOT/python
HOUDINI_TOOLBAR_PATH = $HOUDINI_TOOLBAR_PATH:$HPM_PACKAGE_ROOT/toolbar
EOF

# Create pipeline configuration
cat > config/pipeline/pipeline.yaml << 'EOF'
# Pipeline configuration
project:
  name: "Studio Project"
  version: "1.0"
  
paths:
  assets: "/project/assets"
  shots: "/project/shots"
  cache: "/project/cache"
  
render:
  engine: "mantra"
  output_path: "/project/render"
  
publishing:
  versioning: "semantic"
  auto_increment: true
EOF

# Update hmp.toml to reflect the minimal structure
cat > hpm.toml << 'EOF'
[package]
name = "config-package"
version = "0.1.0"
description = "Configuration and settings package"
authors = ["Pipeline Team"]
license = "MIT"

[houdini]
min_version = "19.5"

# No dependencies needed for configuration package
[dependencies]

# Scripts for configuration management
[scripts]
validate_config = "python -c \"import yaml; yaml.safe_load(open('config/pipeline/pipeline.yaml'))\""
EOF
```

## Dependency Management Scenarios

This section covers various dependency management scenarios, from simple to complex.

### Scenario 1: Adding Your First Dependencies

Starting with a basic package, let's add various types of dependencies:

```bash
# Create a test package for dependency examples
hmp init dependency-demo --description "Dependency management examples"
cd dependency-demo
```

#### HPM Package Dependencies

```bash
# Add latest version of a package
hpm add utility-nodes

# Add specific version
hpm add geometry-tools --version "2.1.0"

# Add version with constraints
hmp add material-library --version "^1.5.0"  # Compatible with 1.5.x
hpm add mesh-utils --version "~2.0.0"        # Compatible with 2.0.x

# Add optional dependency
hpm add visualization-tools --optional

# Add dependency from Git repository
hmp add custom-nodes --git "https://github.com/studio/custom-nodes" --tag "v1.0.0"
```

This would result in `hpm.toml`:

```toml
[dependencies]
utility-nodes = "latest"
geometry-tools = "2.1.0"
material-library = "^1.5.0"
mesh-utils = "~2.0.0"
visualization-tools = { version = "latest", optional = true }
custom-nodes = { git = "https://github.com/studio/custom-nodes", tag = "v1.0.0" }
```

#### Python Dependencies

```bash
# Edit hpm.toml to add Python dependencies
cat >> hpm.toml << 'EOF'

[python_dependencies]
# Basic scientific computing
numpy = ">=1.20.0"
scipy = ">=1.7.0"

# Version constraints examples
requests = "^2.28.0"              # Caret: >=2.28.0, <3.0.0  
pillow = "~8.3.0"                 # Tilde: >=8.3.0, <8.4.0
matplotlib = ">=3.5.0,<4.0.0"    # Range specification

# Dependencies with extras
pandas = { version = ">=1.3.0", extras = ["performance"] }
scikit-learn = { version = ">=1.0.0", extras = ["plot"] }

# Optional dependencies
plotly = { version = ">=5.0.0", optional = true }
seaborn = { version = ">=0.11.0", optional = true }
EOF
```

### Scenario 2: Resolving Version Conflicts

When you have conflicting version requirements, HPM helps identify and resolve them:

```bash
# Simulate adding conflicting dependencies
cat >> hpm.toml << 'EOF'

# These might create conflicts:
[dependencies]
package-a = "1.0.0"    # Requires numpy >=1.20.0
package-b = "2.0.0"    # Requires numpy >=1.25.0

[python_dependencies]
numpy = ">=1.20.0"     # This might conflict with package-b requirements
EOF

# When you run install, HPM will detect conflicts:
hpm install
# Output would show:
# Error: Dependency conflict detected
#   numpy: package-a requires ">=1.20.0", package-b requires ">=1.25.0"
#   Resolution suggestion: Update numpy requirement to ">=1.25.0"
```

#### Manual Conflict Resolution

```bash
# Update conflicting dependency to resolve the issue
cat > hpm.toml << 'EOF'
[package]
name = "dependency-demo"
version = "0.1.0"
description = "Dependency management examples"

[dependencies]
package-a = "1.0.0"
package-b = "2.0.0"

[python_dependencies]
numpy = ">=1.25.0"     # Updated to satisfy both packages
EOF

# Now installation should work
hmp install
```

### Scenario 3: Development Dependencies

Separate dependencies needed only during development:

```bash
cat >> hpm.toml << 'EOF'

[dev-dependencies]
# Testing frameworks
pytest = ">=6.0.0"
pytest-cov = ">=2.12.0"

# Code quality tools
black = ">=21.0.0"
flake8 = ">=3.9.0"
mypy = ">=0.910"

# Documentation tools
sphinx = ">=4.0.0"
sphinx-rtd-theme = ">=0.5.0"

# Development utilities
ipython = ">=7.0.0"
jupyter = ">=1.0.0"
EOF

# Install including development dependencies (future feature)
# hmp install --dev
```

### Scenario 4: Updating Dependencies

Managing dependency updates safely:

```bash
# Check for available updates
hpm update --dry-run

# Example output:
# Available updates:
#   numpy: 1.24.0 → 1.25.1
#   scipy: 1.9.0 → 1.9.3  
#   matplotlib: 3.6.0 → 3.7.1

# Update all packages
hpm update

# Update specific packages only
hpm update numpy scipy

# Update with specific constraints
hmp update numpy --version ">=1.25.0,<2.0.0"
```

### Scenario 5: Dependency Lockfiles

Understanding and managing `hpm.lock`:

```bash
# After running hpm install, examine the lockfile
cat hpm.lock
```

Example `hmp.lock`:
```toml
# This file is automatically generated by HPM
# Do not modify manually

[[package]]
name = "numpy"
version = "1.24.0"
source = "python"
python_version = "3.9"

[[package]]
name = "scipy"  
version = "1.9.0"
source = "python"
python_version = "3.9"
dependencies = ["numpy"]

[[package]]
name = "utility-nodes"
version = "2.1.0"
source = "registry+https://packages.houdini.org/"
dependencies = []

[metadata]
resolution_time = "2024-01-15T10:30:00Z"
hpm_version = "0.1.0"
```

#### Working with Lockfiles

```bash
# Install exact versions from lockfile
hpm install --locked

# Update lockfile with new resolutions
hpm update --update-lockfile

# Check lockfile consistency
hpm check --verify-lockfile
```

## Python Integration Examples

This section demonstrates HPM's Python integration capabilities with practical examples.

### Example 1: Basic Python Integration

Create a package that demonstrates Python environment management:

```bash
hpm init python-integration-demo \
  --description "Demonstrate Python integration features" \
  --houdini-min 20.0

cd python-integration-demo
```

#### Setting Up Python Dependencies

```bash
cat > hpm.toml << 'EOF'
[package]
name = "python-integration-demo"
version = "1.0.0"
description = "Demonstrate Python integration features"
authors = ["Developer"]
license = "MIT"

[houdini]
min_version = "20.0"

[python_dependencies]
numpy = ">=1.20.0"
matplotlib = ">=3.5.0"
scipy = ">=1.7.0"
pillow = ">=8.0.0"
EOF

# Install Python dependencies
hpm install
```

This will:
1. Resolve Python dependencies using UV
2. Create a content-addressable virtual environment
3. Generate package.json with PYTHONPATH integration

#### Using Python Dependencies in Houdini

```bash
cat > python/scientific_viz.py << 'EOF'
"""Scientific visualization utilities using external Python packages."""

import numpy as np
import matplotlib.pyplot as plt
from PIL import Image
import hou
import os


def plot_geometry_data(node, attribute="P", output_path=None):
    """Plot geometry attribute data using matplotlib."""
    if not node or not hasattr(node, 'geometry'):
        raise ValueError("Node must have geometry")
    
    geo = node.geometry()
    if not geo:
        raise ValueError("Node has no geometry")
    
    # Get attribute data
    if attribute == "P":
        data = np.array([pt.position() for pt in geo.points()])
        labels = ["X", "Y", "Z"]
    else:
        attrib = geo.findPointAttrib(attribute)
        if not attrib:
            raise ValueError(f"Attribute '{attribute}' not found")
        
        data = np.array([pt.attribValue(attrib) for pt in geo.points()])
        if data.ndim == 1:
            data = data.reshape(-1, 1)
            labels = [attribute]
        else:
            labels = [f"{attribute}_{i}" for i in range(data.shape[1])]
    
    # Create plots
    fig, axes = plt.subplots(1, data.shape[1], figsize=(12, 4))
    if data.shape[1] == 1:
        axes = [axes]
    
    for i, (ax, label) in enumerate(zip(axes, labels)):
        ax.hist(data[:, i], bins=30, alpha=0.7, edgecolor='black')
        ax.set_title(f'Distribution of {label}')
        ax.set_xlabel('Value')
        ax.set_ylabel('Frequency')
        ax.grid(True, alpha=0.3)
    
    plt.tight_layout()
    
    # Save or display
    if output_path:
        plt.savefig(output_path, dpi=150, bbox_inches='tight')
        print(f"Plot saved to: {output_path}")
    else:
        # Save to temp file and display in Houdini
        import tempfile
        temp_path = os.path.join(tempfile.gettempdir(), "houdini_plot.png")
        plt.savefig(temp_path, dpi=150, bbox_inches='tight')
        
        # Display in Houdini's image viewer (if available)
        try:
            desktop = hou.ui.curDesktop()
            image_pane = desktop.createPane(hou.paneTabType.IPRViewer)
            # Note: This is a simplified example
            print(f"Plot saved to: {temp_path}")
        except:
            print(f"Plot saved to: {temp_path}")
    
    plt.close()
    return output_path or temp_path


def create_noise_texture(width=512, height=512, scale=0.1, octaves=4):
    """Create noise texture using numpy and PIL."""
    # Generate Perlin-like noise using numpy
    x = np.linspace(0, scale * width, width)
    y = np.linspace(0, scale * height, height)
    X, Y = np.meshgrid(x, y)
    
    # Simple noise generation (simplified for example)
    noise = np.zeros((height, width))
    
    for octave in range(octaves):
        freq = 2 ** octave
        amplitude = 1 / (2 ** octave)
        
        # Simplified noise function
        octave_noise = np.sin(X * freq) * np.cos(Y * freq) * amplitude
        noise += octave_noise
    
    # Normalize to 0-255 range
    noise = (noise - noise.min()) / (noise.max() - noise.min())
    noise = (noise * 255).astype(np.uint8)
    
    # Create PIL image
    image = Image.fromarray(noise, mode='L')
    
    return image


def apply_image_filter(image_path, filter_type="blur", strength=1.0):
    """Apply image filters using PIL."""
    from PIL import ImageFilter
    
    # Load image
    image = Image.open(image_path)
    
    # Apply filter
    if filter_type == "blur":
        filtered = image.filter(ImageFilter.GaussianBlur(radius=strength))
    elif filter_type == "sharpen":
        filtered = image.filter(ImageFilter.UnsharpMask(radius=2, percent=150, threshold=3))
    elif filter_type == "edge":
        filtered = image.filter(ImageFilter.FIND_EDGES)
    else:
        filtered = image
    
    return filtered


# Houdini integration examples
def create_numpy_geometry():
    """Create geometry using numpy and add to Houdini scene."""
    # Generate data with numpy
    t = np.linspace(0, 4 * np.pi, 1000)
    x = np.cos(t) * (1 + 0.5 * np.cos(8 * t))
    y = np.sin(t) * (1 + 0.5 * np.cos(8 * t))
    z = 0.1 * np.sin(16 * t)
    
    # Create geometry node
    obj = hou.node("/obj")
    geo_node = obj.createNode("geo", "numpy_curve")
    
    # Create add node to manually add points
    add_node = geo_node.createNode("add")
    
    # Add points from numpy arrays
    geo = add_node.geometry()
    for i in range(len(x)):
        point = geo.createPoint()
        point.setPosition((float(x[i]), float(y[i]), float(z[i])))
    
    # Create curve from points
    curve_node = geo_node.createNode("sort")
    curve_node.setFirstInput(add_node)
    
    add_node.setDisplayFlag(True)
    add_node.setRenderFlag(True)
    
    return geo_node
EOF
```

#### Testing Python Integration

```bash
# Create a test script to verify Python environment
cat > scripts/test_python_integration.py << 'EOF'
"""Test Python integration in Houdini environment."""

def test_imports():
    """Test that all required packages can be imported."""
    try:
        import numpy as np
        print(f"✓ NumPy {np.__version__} imported successfully")
        
        import matplotlib
        print(f"✓ Matplotlib {matplotlib.__version__} imported successfully")
        
        import scipy
        print(f"✓ SciPy {scipy.__version__} imported successfully")
        
        from PIL import Image
        print(f"✓ Pillow imported successfully")
        
        print("\n✅ All Python dependencies imported successfully!")
        return True
        
    except ImportError as e:
        print(f"❌ Import error: {e}")
        return False


def test_numpy_functionality():
    """Test numpy functionality."""
    try:
        import numpy as np
        
        # Create test array
        arr = np.random.random((100, 3))
        mean = np.mean(arr, axis=0)
        std = np.std(arr, axis=0)
        
        print(f"✓ NumPy array operations working")
        print(f"  Array shape: {arr.shape}")
        print(f"  Mean: {mean}")
        print(f"  Std: {std}")
        
        return True
        
    except Exception as e:
        print(f"❌ NumPy test failed: {e}")
        return False


if __name__ == "__main__":
    print("Testing Python Integration...")
    print("=" * 40)
    
    imports_ok = test_imports()
    numpy_ok = test_numpy_functionality()
    
    if imports_ok and numpy_ok:
        print("\n🎉 Python integration test passed!")
    else:
        print("\n💥 Python integration test failed!")
EOF
```

### Example 2: Content-Addressable Virtual Environments

Demonstrate how HPM shares virtual environments between packages:

```bash
# Create two packages with identical Python dependencies
hpm init package-a --description "First package with Python deps"
hpm init package-b --description "Second package with Python deps"

# Configure identical Python dependencies in both packages
for pkg in package-a package-b; do
    cat > $pkg/hpm.toml << 'EOF'
[package]
name = "package-a"
version = "1.0.0"
description = "Package with shared Python environment"

[houdini]
min_version = "20.0"

[python_dependencies]
numpy = "1.24.0"
requests = "2.28.0"
EOF
done

# Update names appropriately
sed -i 's/name = "package-a"/name = "package-b"/' package-b/hpm.toml

# Install both packages
cd package-a && hpm install && cd ..
cd package-b && hpm install && cd ..

# Both packages will share the same virtual environment
# Check shared virtual environment
ls ~/.hpm/venvs/
# Should show one environment hash used by both packages
```

### Example 3: Python Environment Cleanup

Demonstrate cleanup of orphaned Python environments:

```bash
# Check current virtual environments
hpm clean --python-only --dry-run

# Example output:
# Python Environment Cleanup Analysis:
# 
# Virtual Environments:
#   a1b2c3d4e5f6 - Used by: package-a, package-b (2.1 GB)
#   f6e5d4c3b2a1 - Orphaned (1.8 GB) - Last used: 30 days ago
# 
# Would remove:
#   1 orphaned virtual environment
#   Total space to free: 1.8 GB

# Perform cleanup
hmp clean --python-only

# Comprehensive cleanup (packages + Python)
hpm clean --comprehensive --dry-run
```

## Advanced Workflows

This section covers advanced HPM usage patterns for experienced users and complex scenarios.

### Workflow 1: Multi-Project Development Environment

Setting up HPM for a complex multi-project development environment:

```bash
# Create project structure
mkdir -p /projects/houdini/{core-tools,vfx-library,pipeline-utils}

# Create core tools package
cd /projects/houdini/core-tools
hpm init core-tools \
  --description "Core Houdini tools and utilities" \
  --houdini-min 20.0

# Create VFX library that depends on core tools
cd /projects/houdini/vfx-library  
hpm init vfx-library \
  --description "VFX-specific tools and effects" \
  --houdini-min 20.0

# Add local dependency to core-tools
cat >> hpm.toml << 'EOF'

[dependencies]
core-tools = { path = "../core-tools" }

[python_dependencies]
numpy = ">=1.24.0"
opencv-python = ">=4.7.0"
EOF

# Create pipeline utilities
cd /projects/houdini/pipeline-utils
hpm init pipeline-utils \
  --description "Pipeline integration and automation" \
  --houdini-min 20.0

# Add dependencies to both packages
cat >> hpm.toml << 'EOF'

[dependencies]
core-tools = { path = "../core-tools" }
vfx-library = { path = "../vfx-library" }

[python_dependencies]
pyyaml = ">=6.0"
jinja2 = ">=3.0.0"
click = ">=8.0.0"
EOF
```

#### Global Configuration for Multi-Project Setup

```bash
# Configure HPM for multi-project development
mkdir -p ~/.hpm
cat > ~/.hpm/config.toml << 'EOF'
[projects]
# Explicit paths for important projects
explicit_paths = [
  "/projects/houdini/core-tools",
  "/projects/houdini/vfx-library", 
  "/projects/houdini/pipeline-utils"
]

# Search roots for automatic discovery
search_roots = ["/projects/houdini", "/work/houdini-packages"]
max_search_depth = 3
ignore_patterns = [".git", "__pycache__", "*.tmp", ".DS_Store"]

[storage]
# Increase parallel operations for development
max_parallel_operations = 8

[python]
# Keep more virtual environments for development
max_venvs = 20
cleanup_threshold_days = 60

[ui]
# Developer-friendly settings
verbosity = "verbose"
progress_bars = true
confirm_destructive = true
EOF
```

### Workflow 2: Package Publishing Pipeline (Future)

Set up automated package publishing with validation:

```bash
# Create publishable package
hmp init publishable-package \
  --description "Package ready for registry publishing" \
  --author "Studio Developer <dev@studio.com>" \
  --license Apache-2.0

cd publishable-package

# Add comprehensive metadata for publishing
cat > hpm.toml << 'EOF'
[package]
name = "studio-geometry-tools"
version = "1.0.0"
description = "Professional geometry tools for Houdini production"
authors = ["Studio Developer <dev@studio.com>"]
license = "Apache-2.0"
readme = "README.md"
homepage = "https://studio.com/houdini-tools"
repository = "https://github.com/studio/houdini-geometry-tools"
keywords = ["houdini", "geometry", "modeling", "studio"]
categories = ["tools", "geometry-processing"]

[houdini]
min_version = "20.0"
max_version = "21.5"

# Production-ready dependencies with exact versions
[dependencies]
utility-library = "^2.1.0"

[python_dependencies]
numpy = "^1.24.0"

[scripts]
# Publishing pipeline scripts
validate = "python scripts/validate_package.py"
test = "python -m pytest tests/ -v"
build = "python scripts/build_release.py"
publish = "python scripts/publish_package.py"
EOF

# Create validation script
mkdir -p scripts
cat > scripts/validate_package.py << 'EOF'
"""Validate package before publishing."""

import os
import sys
import json
import toml
from pathlib import Path


def validate_manifest():
    """Validate package manifest."""
    print("Validating hpm.toml...")
    
    if not os.path.exists("hpm.toml"):
        print("❌ hpm.toml not found")
        return False
    
    try:
        with open("hpm.toml", 'r') as f:
            manifest = toml.load(f)
    except Exception as e:
        print(f"❌ Failed to parse hpm.toml: {e}")
        return False
    
    # Check required fields
    required_fields = ["name", "version", "description", "authors", "license"]
    package_section = manifest.get("package", {})
    
    for field in required_fields:
        if field not in package_section:
            print(f"❌ Missing required field: package.{field}")
            return False
    
    print("✓ hpm.toml validation passed")
    return True


def validate_readme():
    """Validate README exists and has content."""
    print("Validating README...")
    
    readme_files = ["README.md", "README.rst", "README.txt"]
    readme_found = None
    
    for readme in readme_files:
        if os.path.exists(readme):
            readme_found = readme
            break
    
    if not readme_found:
        print("❌ README file not found")
        return False
    
    with open(readme_found, 'r') as f:
        content = f.read().strip()
    
    if len(content) < 100:
        print("❌ README is too short (minimum 100 characters)")
        return False
    
    print(f"✓ {readme_found} validation passed")
    return True


def validate_structure():
    """Validate package structure."""
    print("Validating package structure...")
    
    # Expected directories
    expected_dirs = ["python", "otls", "scripts", "tests"]
    
    # Check if at least some content exists
    has_content = False
    for directory in expected_dirs:
        if os.path.exists(directory):
            if os.listdir(directory):
                has_content = True
                break
    
    if not has_content:
        print("❌ Package appears to be empty (no content in expected directories)")
        return False
    
    print("✓ Package structure validation passed")
    return True


def validate_tests():
    """Validate that tests exist and pass."""
    print("Validating tests...")
    
    if not os.path.exists("tests"):
        print("⚠ No tests directory found")
        return True  # Not fatal for publishing
    
    test_files = list(Path("tests").rglob("test_*.py"))
    if not test_files:
        print("⚠ No test files found")
        return True
    
    # Run tests
    import subprocess
    result = subprocess.run([sys.executable, "-m", "pytest", "tests/", "-v"], 
                           capture_output=True, text=True)
    
    if result.returncode != 0:
        print("❌ Tests failed:")
        print(result.stdout)
        print(result.stderr)
        return False
    
    print("✓ Tests passed")
    return True


def main():
    """Run all validations."""
    print("Package Publishing Validation")
    print("=" * 40)
    
    validations = [
        validate_manifest(),
        validate_readme(),
        validate_structure(),
        validate_tests()
    ]
    
    if all(validations):
        print("\n🎉 Package validation passed! Ready for publishing.")
        return True
    else:
        print("\n💥 Package validation failed. Fix issues before publishing.")
        return False


if __name__ == "__main__":
    success = main()
    sys.exit(0 if success else 1)
EOF

# Create comprehensive README
cat > README.md << 'EOF'
# Studio Geometry Tools

Professional geometry tools for Houdini production workflows.

## Description

This package provides a comprehensive set of geometry processing tools designed for professional VFX and animation workflows. Built with performance and reliability in mind, these tools integrate seamlessly with existing Houdini production pipelines.

## Features

- **Advanced Geometry Processing**: High-performance algorithms for complex geometry operations
- **Python Integration**: Extensive Python API for pipeline integration
- **Production Ready**: Tested in professional production environments
- **Modular Design**: Use individual tools or the complete suite

## Installation

Install using HPM (Houdini Package Manager):

```bash
hpm add studio-geometry-tools
```

## Requirements

- SideFX Houdini 20.0 or later
- Python 3.9+ (included with Houdini)

## Usage

### Python API

```python
from studio_geometry_tools import GeometryProcessor

processor = GeometryProcessor()
result = processor.optimize_mesh(input_geometry, quality=0.8)
```

### Houdini Integration

Tools are automatically available in Houdini's TAB menu under the "Studio" category.

## Documentation

Full documentation is available at: [https://studio.com/houdini-tools/docs](https://studio.com/houdini-tools/docs)

## Support

- Issues: [GitHub Issues](https://github.com/studio/houdini-geometry-tools/issues)
- Documentation: [Online Docs](https://studio.com/houdini-tools/docs)
- Email: [dev@studio.com](mailto:dev@studio.com)

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for details.

## Contributing

Contributions welcome! Please read our [Contributing Guidelines](CONTRIBUTING.md) for details.
EOF
```

### Workflow 3: Automated Dependency Updates

Set up automated dependency update workflows:

```bash
# Create package with update automation
hpm init auto-update-demo --description "Automated dependency updates demo"
cd auto-update-demo

# Create update automation script
cat > scripts/update_dependencies.py << 'EOF'
"""Automated dependency update script."""

import subprocess
import sys
import json
from datetime import datetime


def check_for_updates():
    """Check for available dependency updates."""
    print("Checking for dependency updates...")
    
    try:
        result = subprocess.run(
            ["hpm", "update", "--dry-run", "--output", "json"],
            capture_output=True, text=True, check=True
        )
        
        if result.stdout.strip():
            updates = json.loads(result.stdout)
            return updates
        else:
            return {"updates": []}
            
    except subprocess.CalledProcessError as e:
        print(f"Failed to check for updates: {e}")
        return None
    except json.JSONDecodeError as e:
        print(f"Failed to parse update info: {e}")
        return None


def apply_updates(updates, auto_approve_patch=True):
    """Apply dependency updates with approval logic."""
    if not updates or not updates.get("updates"):
        print("No updates available.")
        return True
    
    for update in updates["updates"]:
        package_name = update["package"]
        from_version = update["from_version"]
        to_version = update["to_version"]
        update_type = update.get("update_type", "unknown")
        
        print(f"\nUpdate available: {package_name}")
        print(f"  From: {from_version}")
        print(f"  To: {to_version}")
        print(f"  Type: {update_type}")
        
        # Auto-approve patch updates
        if auto_approve_patch and update_type == "patch":
            print("  → Auto-approving patch update")
            apply_single_update(package_name, to_version)
        else:
            # Ask for approval for major/minor updates
            response = input(f"  Apply update? [y/N]: ").lower().strip()
            if response in ["y", "yes"]:
                apply_single_update(package_name, to_version)
            else:
                print("  → Skipped")


def apply_single_update(package_name, version):
    """Apply single package update."""
    try:
        result = subprocess.run(
            ["hpm", "update", package_name, "--version", version],
            capture_output=True, text=True, check=True
        )
        print(f"  ✓ Successfully updated {package_name} to {version}")
        return True
    except subprocess.CalledProcessError as e:
        print(f"  ❌ Failed to update {package_name}: {e}")
        return False


def log_update_activity(updates_applied):
    """Log update activity to file."""
    log_entry = {
        "timestamp": datetime.now().isoformat(),
        "updates_applied": updates_applied,
        "hpm_version": get_hpm_version()
    }
    
    with open("update_log.json", "a") as f:
        f.write(json.dumps(log_entry) + "\n")


def get_hpm_version():
    """Get HPM version."""
    try:
        result = subprocess.run(
            ["hpm", "--version"],
            capture_output=True, text=True, check=True
        )
        return result.stdout.strip()
    except:
        return "unknown"


def main():
    """Main update workflow."""
    print("HPM Automated Dependency Update")
    print("=" * 40)
    
    updates = check_for_updates()
    if updates is None:
        sys.exit(1)
    
    if not updates.get("updates"):
        print("All dependencies are up to date!")
        return
    
    print(f"\nFound {len(updates['updates'])} available updates:")
    
    # Apply updates with approval logic
    apply_updates(updates, auto_approve_patch=True)
    
    # Log activity
    log_update_activity(len(updates.get("updates", [])))
    
    print("\nUpdate workflow completed!")


if __name__ == "__main__":
    main()
EOF

# Create automated update schedule script
cat > scripts/schedule_updates.py << 'EOF'
"""Schedule automated dependency updates."""

import schedule
import time
import subprocess
import logging
from datetime import datetime


# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(levelname)s - %(message)s',
    handlers=[
        logging.FileHandler('update_scheduler.log'),
        logging.StreamHandler()
    ]
)


def run_update_check():
    """Run dependency update check."""
    logging.info("Starting scheduled dependency update check")
    
    try:
        result = subprocess.run(
            ["python", "scripts/update_dependencies.py"],
            capture_output=True, text=True, check=True
        )
        
        logging.info("Update check completed successfully")
        logging.info(f"Output: {result.stdout}")
        
        if result.stderr:
            logging.warning(f"Warnings: {result.stderr}")
            
    except subprocess.CalledProcessError as e:
        logging.error(f"Update check failed: {e}")
        logging.error(f"Error output: {e.stderr}")


def main():
    """Main scheduler."""
    logging.info("Starting HPM dependency update scheduler")
    
    # Schedule updates
    schedule.every().monday.at("09:00").do(run_update_check)
    schedule.every().friday.at("17:00").do(run_update_check)
    
    logging.info("Scheduled updates:")
    logging.info("  - Monday at 09:00")
    logging.info("  - Friday at 17:00")
    
    # Run scheduler
    while True:
        schedule.run_pending()
        time.sleep(60)  # Check every minute


if __name__ == "__main__":
    main()
EOF
```

## Team and Studio Setup

This section covers setting up HPM for team development and studio environments.

### Studio Configuration Template

```bash
# Create studio-wide configuration
mkdir -p /studio/shared/hpm-config
cat > /studio/shared/hpm-config/studio.toml << 'EOF'
# Studio-wide HPM configuration template

[registry]
# Studio internal registry
default = "https://packages.studio.com"
sources.internal = { url = "https://packages.studio.com", priority = 1 }
sources.public = { url = "https://packages.houdini.org", priority = 2 }

[storage]
# Studio shared storage location
root_path = "/studio/shared/hpm-packages"
cache_dir = "cache"
max_parallel_operations = 16
compression = true
cache_retention = "30d"

[projects]
# Studio project locations
search_roots = [
  "/studio/projects",
  "/studio/tools/houdini-packages",
  "/users/*/houdini-dev"
]
explicit_paths = [
  "/studio/tools/core-pipeline",
  "/studio/tools/rendering-toolkit",
  "/studio/tools/modeling-suite"
]
max_search_depth = 4
ignore_patterns = [
  ".git", "__pycache__", "*.tmp", ".DS_Store",
  "*.pyc", ".pytest_cache", "node_modules"
]

[python]
# Studio Python environment settings
venvs_dir = "/studio/shared/python-venvs"
max_venvs = 50
cleanup_threshold_days = 90
default_python_version = "3.9"

[ui]
# Studio UI preferences
color = "auto"
progress_bars = true
confirm_destructive = true
use_emojis = false  # Professional environment
EOF

# Create user setup script
cat > /studio/shared/hpm-config/setup-user-hpm.sh << 'EOF'
#!/bin/bash
# Setup HPM for studio users

set -e

echo "Setting up HPM for studio use..."

# Create user HPM directory
mkdir -p ~/.hmp

# Create user configuration that inherits from studio config
cat > ~/.hpm/config.toml << 'USEREOF'
# User HPM configuration - inherits from studio config

[storage]
# User-specific cache location
cache_dir = "~/.hpm/cache"

[projects]
# Add user-specific project paths
explicit_paths = [
  "~/houdini-dev/personal-tools",
  "~/houdini-dev/experiments"
]
search_roots = [
  "~/houdini-dev",
  "~/Desktop/houdini-projects"
]

[python]
# User Python environment preferences
venvs_dir = "~/.hpm/venvs"
max_venvs = 10
cleanup_threshold_days = 30

[ui]
# User interface preferences
verbosity = "normal"
output_format = "human"
USEREOF

# Set up studio configuration inheritance
echo "# Studio configuration" >> ~/.hpm/config.toml
echo "include = \"/studio/shared/hpm-config/studio.toml\"" >> ~/.hpm/config.toml

echo "✓ HPM studio configuration completed!"
echo ""
echo "Usage:"
echo "  hpm --help"
echo "  hpm init my-project"
echo ""
echo "Studio resources:"
echo "  Registry: https://packages.studio.com"
echo "  Documentation: /studio/shared/docs/hpm/"
echo "  Support: #hpm-support on Slack"
EOF

chmod +x /studio/shared/hpm-config/setup-user-hpm.sh
```

### Team Development Workflow

```bash
# Create team development package template
mkdir -p /studio/templates/hpm-package-template
cd /studio/templates/hpm-package-template

# Create package template
cat > hpm.toml << 'EOF'
[package]
name = "TEMPLATE_PACKAGE_NAME"
version = "0.1.0"
description = "TEMPLATE_DESCRIPTION"
authors = ["TEMPLATE_AUTHOR"]
license = "Studio-Internal"
readme = "README.md"
keywords = ["houdini", "studio", "pipeline"]

[houdini]
min_version = "20.0"
max_version = "21.5"

# Studio standard dependencies
[dependencies]
studio-core = "^2.0.0"
studio-pipeline = "^1.5.0"

[python_dependencies]
# Studio-approved Python packages
numpy = "^1.24.0"
pyyaml = "^6.0"
requests = "^2.28.0"

[scripts]
test = "python -m pytest tests/"
lint = "flake8 python/ && mypy python/"
format = "black python/ && isort python/"
validate = "hpm check && python scripts/validate.py"

[dev-dependencies]
pytest = "^7.0.0"
black = "^23.0.0"
flake8 = "^5.0.0"
mypy = "^1.0.0"
isort = "^5.0.0"
EOF

# Create team package creation script
cat > /studio/shared/bin/create-houdini-package << 'EOF'
#!/bin/bash
# Studio script to create new Houdini packages with team standards

set -e

# Get package information
read -p "Package name: " PACKAGE_NAME
read -p "Description: " DESCRIPTION
read -p "Your name: " AUTHOR_NAME
read -p "Your email: " AUTHOR_EMAIL

# Validate inputs
if [[ -z "$PACKAGE_NAME" || -z "$DESCRIPTION" || -z "$AUTHOR_NAME" || -z "$AUTHOR_EMAIL" ]]; then
    echo "Error: All fields are required"
    exit 1
fi

# Create package directory
PACKAGE_DIR="/studio/projects/houdini-packages/$PACKAGE_NAME"
if [[ -d "$PACKAGE_DIR" ]]; then
    echo "Error: Package directory already exists: $PACKAGE_DIR"
    exit 1
fi

echo "Creating package: $PACKAGE_NAME"
mkdir -p "$PACKAGE_DIR"
cd "$PACKAGE_DIR"

# Copy template
cp -r /studio/templates/hpm-package-template/* .

# Replace template variables
sed -i "s/TEMPLATE_PACKAGE_NAME/$PACKAGE_NAME/g" hpm.toml
sed -i "s/TEMPLATE_DESCRIPTION/$DESCRIPTION/g" hpm.toml
sed -i "s/TEMPLATE_AUTHOR/$AUTHOR_NAME <$AUTHOR_EMAIL>/g" hpm.toml

# Create README
cat > README.md << READMEEOF
# $PACKAGE_NAME

$DESCRIPTION

## Installation

\`\`\`bash
hpm add $PACKAGE_NAME
\`\`\`

## Development

\`\`\`bash
# Clone repository
git clone /studio/git/$PACKAGE_NAME.git
cd $PACKAGE_NAME

# Install dependencies
hpm install --dev

# Run tests
hpm run test

# Format code
hpm run format
\`\`\`

## License

Studio Internal - Not for external distribution
READMEEOF

# Initialize Git repository
git init
git add .
git commit -m "Initial commit: $PACKAGE_NAME package"

# Create remote repository (if Git server available)
# git remote add origin /studio/git/$PACKAGE_NAME.git
# git push -u origin main

echo "✓ Package created successfully at: $PACKAGE_DIR"
echo ""
echo "Next steps:"
echo "1. cd $PACKAGE_DIR"
echo "2. hpm install"
echo "3. Start developing!"
EOF

chmod +x /studio/shared/bin/create-houdini-package
```

## Troubleshooting Common Issues

This section provides solutions for common problems encountered when using HPM.

### Issue 1: Installation and Build Problems

#### Problem: HPM fails to build from source

**Symptoms:**
```bash
cargo build --release
# Error: failed to run custom build command for `hpm-registry v0.1.0`
```

**Solutions:**

1. **Check Rust version:**
```bash
rustc --version
# Should be 1.70 or later
rustup update stable
```

2. **Install Protocol Buffers compiler:**
```bash
# macOS
brew install protobuf

# Ubuntu/Debian
sudo apt install protobuf-compiler

# Windows
# Download from: https://github.com/protocolbuffers/protobuf/releases
```

3. **Clean and rebuild:**
```bash
cargo clean
cargo update
cargo build --release --verbose
```

### Issue 2: Package Creation Problems

#### Problem: `hpm init` fails with permission errors

**Symptoms:**
```bash
hpm init my-package
# Error: Permission denied (os error 13)
```

**Solutions:**

1. **Check directory permissions:**
```bash
ls -la $(pwd)
# Ensure you have write permissions to current directory
```

2. **Try in home directory:**
```bash
cd ~
hpm init my-package --description "Test package"
```

3. **Check disk space:**
```bash
df -h .
# Ensure sufficient disk space
```

### Issue 3: Dependency Resolution Issues

#### Problem: Conflicting Python dependencies

**Symptoms:**
```bash
hpm install
# Error: Dependency conflict detected
#   numpy: package-a requires ">=1.20.0", package-b requires ">=1.25.0"
```

**Solutions:**

1. **Update conflicting dependency:**
```toml
[python_dependencies]
numpy = ">=1.25.0"  # Update to satisfy both requirements
```

2. **Make conflicting dependency optional:**
```toml
[dependencies]
package-a = { version = "1.0.0", optional = true }
```

3. **Use version ranges:**
```toml
[python_dependencies]
numpy = ">=1.20.0,<2.0.0"
```

### Issue 4: Python Environment Issues

#### Problem: Python packages not found in Houdini

**Symptoms:**
```python
import numpy  # In Houdini Python console
# ImportError: No module named 'numpy'
```

**Solutions:**

1. **Check virtual environment creation:**
```bash
ls ~/.hpm/venvs/
# Should show virtual environment directories
```

2. **Verify package.json generation:**
```bash
cat .hpm/packages/your-package.json
# Should show PYTHONPATH environment variable
```

3. **Manual PYTHONPATH setup (temporary):**
```python
import sys
sys.path.append('/path/to/.hpm/venvs/your-venv/lib/python3.9/site-packages')
import numpy  # Should work now
```

### Issue 5: Performance Issues

#### Problem: Slow dependency resolution

**Symptoms:**
```bash
hpm install
# Takes several minutes to resolve dependencies
```

**Solutions:**

1. **Clear cache:**
```bash
rm -rf ~/.hpm/cache
hpm install
```

2. **Increase parallel operations:**
```toml
# ~/.hpm/config.toml
[storage]
max_parallel_operations = 16
```

3. **Use specific versions:**
```toml
[python_dependencies]
numpy = "1.24.0"  # Instead of ">=1.20.0"
```

### Issue 6: Cleanup Issues

#### Problem: Cleanup removes needed packages

**Symptoms:**
```bash
hpm clean
# Removes packages that are still needed
```

**Solutions:**

1. **Use dry-run first:**
```bash
hpm clean --dry-run
# Review what will be removed
```

2. **Check project discovery:**
```bash
# Ensure all projects are discovered
cat ~/.hpm/config.toml
# Check [projects] section
```

3. **Add explicit project paths:**
```toml
[projects]
explicit_paths = [
  "/path/to/important/project1",
  "/path/to/important/project2"
]
```

## Best Practices Guide

This section provides best practices for using HPM effectively in development and production environments.

### Package Development Best Practices

#### 1. Package Naming and Versioning

**Good Package Names:**
- `studio-geometry-tools` (descriptive, includes organization)
- `vfx-particle-system` (clear purpose)
- `modeling-utilities` (category and purpose)

**Poor Package Names:**
- `tools` (too generic)
- `stuff` (not descriptive)
- `mypackage` (not informative)

**Version Management:**
```toml
# Use semantic versioning
[package]
version = "1.2.3"  # Major.Minor.Patch

# Breaking changes: increment major (1.x.x → 2.0.0)
# New features: increment minor (1.2.x → 1.3.0) 
# Bug fixes: increment patch (1.2.3 → 1.2.4)
```

#### 2. Dependency Management

**Specify version constraints appropriately:**
```toml
[dependencies]
# Good: Use caret for compatible updates
utility-library = "^2.1.0"  # >=2.1.0, <3.0.0

# Good: Use tilde for patch updates only
stable-package = "~1.5.0"   # >=1.5.0, <1.6.0

# Avoid: Too restrictive
# exact-package = "1.0.0"   # Prevents any updates

# Avoid: Too permissive  
# any-package = "*"         # Could break compatibility
```

**Python dependency best practices:**
```toml
[python_dependencies]
# Pin major versions to avoid breaking changes
numpy = "^1.24.0"        # Safe for API stability
requests = "^2.28.0"     # Major version pinning

# Use minimum versions for libraries
pyyaml = ">=5.4.0"       # Minimum required version
pillow = ">=8.0.0"       # Allows latest security updates
```

#### 3. Package Structure Organization

**Recommended structure:**
```
my-package/
├── hpm.toml              # Package manifest
├── README.md             # Essential documentation
├── LICENSE               # License file
├── CHANGELOG.md          # Version history
├── .gitignore           # Git ignore rules
├── otls/                # Houdini Digital Assets
│   ├── modeling/        # Organized by category
│   ├── effects/
│   └── utility/
├── python/              # Python modules
│   ├── __init__.py      # Package initialization
│   ├── core/            # Core functionality
│   ├── utils/           # Utilities
│   └── ui/              # User interface
├── scripts/             # Automation scripts
│   ├── build.py         # Build automation
│   ├── test.py          # Testing
│   └── validate.py      # Validation
├── tests/               # Test files
│   ├── unit/            # Unit tests
│   ├── integration/     # Integration tests
│   └── fixtures/        # Test data
├── docs/                # Documentation
│   ├── api.md           # API documentation
│   ├── tutorials.md     # Usage tutorials
│   └── examples/        # Example files
└── config/              # Configuration files
    ├── houdini/         # Houdini-specific config
    └── pipeline/        # Pipeline configuration
```

#### 4. Documentation Standards

**Essential documentation files:**

**README.md:**
```markdown
# Package Name

Brief description of what the package does.

## Installation

\`\`\`bash
hpm add package-name
\`\`\`

## Quick Start

\`\`\`python
from package_name import UtilityClass
utility = UtilityClass()
result = utility.do_something()
\`\`\`

## Requirements

- Houdini 20.0+
- Python 3.9+

## Documentation

- [API Reference](docs/api.md)
- [Tutorials](docs/tutorials.md)
- [Examples](docs/examples/)

## Support

- Issues: [GitHub Issues](link)
- Discussions: [Forum](link)

## License

Licensed under [License Name]. See LICENSE file for details.
```

**CHANGELOG.md:**
```markdown
# Changelog

## [1.2.0] - 2024-01-15

### Added
- New geometry processing tools
- Python API for batch operations
- Support for Houdini 21.0

### Changed
- Improved performance of mesh optimization
- Updated UI layout for better usability

### Fixed
- Memory leak in large geometry processing
- Compatibility issue with older Houdini versions

## [1.1.0] - 2024-01-01
...
```

### Development Workflow Best Practices

#### 1. Testing Strategy

**Test organization:**
```python
# tests/unit/test_geometry_utils.py
import unittest
from my_package.geometry_utils import optimize_mesh

class TestGeometryUtils(unittest.TestCase):
    def test_optimize_mesh_basic(self):
        """Test basic mesh optimization."""
        # Arrange
        input_data = create_test_mesh()
        
        # Act
        result = optimize_mesh(input_data)
        
        # Assert
        self.assertIsNotNone(result)
        self.assertGreater(result.quality_score, 0.8)
    
    def test_optimize_mesh_empty_input(self):
        """Test behavior with empty input."""
        with self.assertRaises(ValueError):
            optimize_mesh(None)
```

**Integration testing:**
```python
# tests/integration/test_houdini_integration.py
import hou
import unittest

class TestHoudiniIntegration(unittest.TestCase):
    def setUp(self):
        """Set up Houdini test environment."""
        # Create test scene
        hou.hipFile.clear()
        self.obj_context = hou.node("/obj")
    
    def test_create_geometry_node(self):
        """Test creating geometry in Houdini."""
        from my_package.houdini_utils import create_test_geometry
        
        geo_node = create_test_geometry(self.obj_context)
        self.assertIsNotNone(geo_node)
        self.assertTrue(geo_node.geometry().points())
```

#### 2. Git Workflow Integration

**Recommended Git workflow:**
```bash
# Feature development
git checkout -b feature/new-geometry-tool
# ... develop feature ...
git commit -m "feat: add advanced mesh subdivision tool"

# Before pushing, run quality checks
hpm run test
hpm run lint
hpm run validate

# Push and create pull request
git push origin feature/new-geometry-tool
```

**.gitignore for Houdini packages:**
```gitignore
# HPM generated files
.hpm/
hpm.lock

# Python
__pycache__/
*.py[cod]
*$py.class
*.so
.Python
env/
venv/
.env

# Houdini
*.hip~
*.hda~
*.otl~
*.temp
*.backup*

# OS
.DS_Store
.DS_Store?
._*
.Spotlight-V100
.Trashes
ehthumbs.db
Thumbs.db
```

#### 3. Continuous Integration

**Example CI configuration (GitHub Actions):**
```yaml
# .github/workflows/hpm-package.yml
name: HPM Package CI

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    
    steps:
    - uses: actions/checkout@v4
    
    - name: Setup Rust
      uses: dtolnay/rust-toolchain@stable
      
    - name: Build HPM
      run: |
        git clone https://github.com/hpm-org/hpm.git /tmp/hpm
        cd /tmp/hpm
        cargo build --release
        echo "/tmp/hpm/target/release" >> $GITHUB_PATH
    
    - name: Install Package Dependencies
      run: hpm install
      
    - name: Run Tests
      run: hpm run test
      
    - name: Validate Package
      run: hpm check
      
    - name: Lint Code
      run: hmp run lint
```

### Production Deployment Best Practices

#### 1. Environment Configuration

**Production configuration:**
```toml
# ~/.hpm/config.toml (production)
[registry]
default = "https://packages.prod.studio.com"
timeout = "60s"
max_retries = 5

[storage] 
root_path = "/shared/hpm-packages"
max_parallel_operations = 32
compression = true
cache_retention = "7d"

[python]
max_venvs = 100
cleanup_threshold_days = 30

[ui]
verbosity = "normal"
confirm_destructive = true
use_emojis = false
```

#### 2. Package Publishing Checklist

Before publishing packages:

- [ ] All tests pass (`hpm run test`)
- [ ] Code is properly formatted (`hpm run format`)
- [ ] Documentation is up to date
- [ ] Version number follows semantic versioning
- [ ] CHANGELOG.md is updated
- [ ] No security vulnerabilities (`hpm audit`)
- [ ] Package validates successfully (`hmp check`)
- [ ] Dependencies are properly specified
- [ ] License is clearly specified
- [ ] README includes installation and usage instructions

#### 3. Monitoring and Maintenance

**Regular maintenance tasks:**
```bash
# Weekly: Check for dependency updates
hpm update --dry-run

# Monthly: Clean up unused packages and environments
hpm clean --comprehensive --dry-run
hpm clean --comprehensive

# Quarterly: Security audit
hpm audit

# As needed: Performance analysis
hyperfine 'hmp install' --warmup 3
```

**Health monitoring:**
```bash
#!/bin/bash
# health-check.sh - Monitor HPM installation health

echo "HPM Health Check $(date)"
echo "================================"

# Check disk usage
echo "Disk Usage:"
du -sh ~/.hmp/

# Check virtual environments
echo -e "\nVirtual Environments:"
ls ~/.hpm/venvs/ | wc -l | xargs echo "Count:"
du -sh ~/.hmp/venvs/

# Check for issues
echo -e "\nConfiguration Check:"
hpm check 2>&1 | grep -E "(Error|Warning)" || echo "No issues found"

echo -e "\nHealth check completed."
```

### Security Best Practices

#### 1. Dependency Security

```bash
# Regular security audits
hpm audit

# Pin security-critical dependencies
[python_dependencies]
requests = { version = "^2.28.0", extras = ["security"] }
cryptography = ">=38.0.0"  # Always use latest for security
```

#### 2. Access Control

**Studio environment:**
```toml
# Restrict package sources
[registry]
sources.internal = { url = "https://internal.studio.com", priority = 1 }
sources.approved = { url = "https://approved.studio.com", priority = 2 }
# Don't include public registries in production
```

These best practices help ensure reliable, maintainable, and secure HPM package development and deployment.