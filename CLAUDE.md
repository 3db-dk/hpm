# CLAUDE.md

This document provides development guidelines for the HPM (Houdini Package Manager) repository.

## Language Standards
- **Professional**: No use of colorful language, no emojis, etc.
- **Concise**: Brief, to the point, no fluff or fibbing.
- **Formal**: No buzzwords, no marketing language or boasting.

## Project Overview

HPM is a Rust-based package management system for SideFX Houdini, providing modern package management capabilities equivalent to npm for Node.js, uv for Python or cargo for Rust.

### Core Functionality

HPM delivers comprehensive package management for Houdini:
- **Authoring**: Package creation with standardized structure and metadata
- **Publishing**: Registry-based package distribution
- **Installation**: Automated package installation with dependency resolution
- **Management**: Package updates, removal, and lifecycle maintenance

### Architecture Benefits

- **Modern Workflows**: Industry-standard package management patterns
- **Dependency Resolution**: Automated dependency graph management
- **Version Management**: Semantic versioning with compatibility validation
- **Performance**: Concurrent operations via Rust and Tokio
- **Compatibility**: Seamless integration with existing Houdini packages
- **Discovery**: Centralized package registry and search capabilities

## Technology Stack

### Core Framework
- **Language**: Rust (stable channel)
- **Build System**: Cargo
- **Runtime**: Tokio async runtime
- **CLI Framework**: Clap (derive API)
- **Configuration**: TOML format with Serde
- **Testing**: Built-in Rust testing + tokio-test for async

### Registry System
- **Transport**: QUIC with s2n-quic for high-performance networking
- **RPC Protocol**: gRPC with Protocol Buffers for efficient serialization
- **Authentication**: Token-based auth with scoped permissions
- **Storage**: Trait-based storage abstraction (Memory, PostgreSQL, S3)
- **Compression**: zstd for package data compression
- **Security**: SHA-256 checksums, mandatory TLS encryption

## MCP Integration

HPM leverages Claude Code's built-in capabilities and selective MCP servers for enhanced development functionality:

### Built-in Claude Code Tools
Claude Code provides comprehensive built-in tools that eliminate the need for redundant MCP server configurations:
- **Filesystem Operations**: `Read`, `Write`, `Edit`, `MultiEdit`, `Glob`, `LS`, `Bash`
- **GitHub Integration**: Complete GitHub API access via `mcp__github__*` tools
- **Sequential Thinking**: Advanced reasoning via `mcp__sequential__sequentialthinking`
- **IDE Integration**: VSCode integration with file references, diagnostics, and diff viewing

### Configured MCP Servers
Current optimized configuration includes only non-redundant servers:

#### Global (User-level)
- **awesome-claude-code**: Access to Claude Code best practices and community resources

#### Local (Project-level)  
- **postgres**: Database operations for HPM registry development

## Claude Code Configuration

HPM follows official Claude Code configuration standards with project-specific optimizations.

### Official Configuration Structure

#### Settings.json Configuration
Claude Code officially supports `.claude/settings.json` for project configuration:

```json
{
  "permissions": {
    "allow": ["Task(*)", "Bash(cargo *)", "Read(**/*.rs)"],
    "deny": ["Bash(rm *)", "Read(./.env*)"]
  },
  "model": "claude-3-5-sonnet-20241022",
  "env": {
    "HPM_DEV": "1"
  },
  "hooks": {
    "pre-edit": {
      "command": "cargo clippy --manifest-path $WORKSPACE/Cargo.toml -- -D warnings",
      "description": "Lint check before editing Rust files",
      "patterns": ["**/*.rs"]
    }
  }
}
```

#### Agent Configuration
Agents are configured as separate `.md` files in `.claude/agents/` directories:

**User-Level Agents** (`~/.claude/agents/`):
- `architecture-advisor.md` - System design and architectural decisions
- `documentation-writer.md` - Technical documentation and guides
- `performance-specialist.md` - Performance analysis and optimization
- `rust-formatter.md` - Code formatting and style consistency
- `security-auditor.md` - Security analysis and vulnerability scanning
- `test-specialist.md` - Unit and integration testing

**Project-Level Agents** (`.claude/agents/`):
- `hpm-developer.md` - HPM-specific implementation and maintenance

### Configuration Hierarchy
1. **User-Level** (`~/.claude/settings.json`) - General permissions and global settings
2. **Project-Level** (`.claude/settings.json`) - Project-specific permissions and hooks
3. **Agent Files** (`.claude/agents/*.md`) - Specialized agent configurations

### Best Practices
- **Agent Scope Separation**: General-purpose agents at user level, project-specific agents at project level
- **Permission Management**: Broad permissions at user level, restrictive overrides at project level
- **Interactive Maintenance**: Use Claude Code's built-in features for configuration management

## Official Claude Code Maintenance

HPM follows the official Claude Code maintenance approach using built-in features rather than custom scripts.

### Recommended Maintenance Routine

#### Monthly Interactive Maintenance
```bash
# Start interactive maintenance session
claude "Help me with monthly Claude Code maintenance for the HPM project"
```

#### Built-in Maintenance Commands
```bash
# Health diagnostics
claude doctor

# Configuration management
claude config list
claude config get <setting>
claude config set <setting> <value>

# Interactive maintenance sessions
claude "Help me with monthly Claude Code maintenance for the HPM project"
claude "Help me review my configuration and optimize my setup"
claude "Help me manage my custom agents and ensure they're working properly"

# Updates
claude update

# MCP server management
claude mcp list
claude mcp add <name> <transport>
claude mcp remove <name>
```

#### Configuration Management
```bash
# View current configuration
claude config list

# Modify settings using official commands
claude config set model claude-3-5-sonnet-20241022
claude config add permissions.allow "Bash(cargo test)"
claude config remove permissions.allow "old-permission"
```

### Cross-Project Analysis
For workspace-level insights that complement built-in features:
```bash
./scripts/workspace-analysis.sh
```

This provides cross-project configuration discovery and recommendations for using official Claude Code features.

### Official vs. Custom Approach
- **Primary**: Use Claude Code's built-in interactive features for maintenance
- **Supplementary**: Use minimal scripts only for cross-project analysis
- **Philosophy**: Let Claude Code be your maintenance assistant, don't replace it with automation

For comprehensive guidance, see `.claude/claude-code-maintenance-guide.md`.

### MCP Configuration Management

#### Redundancy Detection
Before adding new MCP servers, verify they don't duplicate built-in functionality:

```bash
# Check current MCP servers
claude mcp list

# Analyze server details and scope
claude mcp get <server-name>

# Review available built-in tools in Claude Code context
```

#### Server Management Commands
```bash
# Add servers (prefer user scope for cross-project utility)
claude mcp add -s user <name> <url>                    # Global server
claude mcp add -s local <name> <url>                   # Project-specific server

# Remove redundant servers
claude mcp remove <server-name> -s <scope>

# Check server health and connectivity
claude mcp list
```

### MCP Maintenance Workflow

Regular maintenance prevents configuration bloat and ensures optimal performance:

1. **Monthly Review**: Audit configured servers against built-in capabilities
2. **Redundancy Check**: Remove servers that duplicate VSCode/Claude Code built-in tools
3. **Scope Optimization**: Move servers to appropriate scope (user vs local)
4. **Documentation Sync**: Update this documentation with configuration changes

For comprehensive MCP maintenance procedures, see the MCP Maintenance Workflow section below.

## Project Architecture Analysis

### Critical Lessons Learned

#### Dependency Management
- **Avoid over-dependencies**: Initial setup had 18 unused dependencies across crates
- **Use cargo-machete**: Essential tool for identifying unused dependencies in workspaces
- **Principle**: Only add dependencies when implementing actual functionality
- **Result**: Reduced dependencies from 35+ to 17, achieving 50% faster build times

#### Crate Architecture
- **Avoid circular dependencies**: Core crate should not depend on all other crates
- **Separate concerns**: Each crate should have a single, well-defined responsibility
- **Minimize coupling**: Use trait boundaries and explicit interfaces between crates
- **Error handling**: Define errors in the crate where they originate, not globally

#### Testing Strategy
- **Start with tests**: Empty crates provide no confidence in code quality
- **Unit tests first**: Focus on data structures and validation logic
- **Integration tests**: Add after basic functionality exists
- **Test coverage**: Aim for meaningful tests, not just coverage percentage

### Workspace Best Practices

#### Structure Guidelines
- **Flat layout**: Prefer `crates/` directory over nested hierarchies
- **Consistent naming**: Use project prefix (hpm-*) for all workspace crates
- **Virtual manifest**: Keep root workspace as virtual manifest, avoid main crate in root
- **Centralized dependencies**: Use workspace.dependencies for version consistency

#### Development Workflow
- **Single task runner**: Choose either Makefile OR justfile, not both
- **Quality gates**: Implement comprehensive checks (fmt, clippy, tests, audit)
- **Pre-commit hooks**: Automate quality enforcement but provide fallbacks
- **Documentation**: Maintain both high-level (CLAUDE.md) and detailed guides

### Common Pitfalls

#### What Not To Do
- **Empty placeholder crates**: Create functionality before crate structure
- **Tokio everywhere**: Only add async dependencies where async is actually needed
- **Makefile + justfile**: Redundant tooling creates confusion
- **No tests**: Zero tests means zero confidence in functionality
- **Circular imports**: Core crate importing all other crates violates separation

#### Red Flags
- More than 20% unused dependencies detected by cargo-machete
- Crates with only `// TODO` comments and no real functionality
- Build times over 10 seconds for small workspaces
- Pre-commit hooks failing due to tooling issues
- Missing or outdated documentation

### Success Metrics

#### Quality Indicators
- cargo-machete reports minimal unused dependencies
- All quality checks pass consistently
- Build times under 5 seconds for clean builds
- Working unit tests with real functionality
- Documentation stays current with implementation

#### Architecture Health
- Clear crate boundaries with minimal coupling
- Each crate has a single, well-defined purpose
- Dependencies flow in one direction without cycles
- Error types defined close to their usage
- Public APIs are well-documented and tested

### Future Development Guidelines

#### Before Adding New Crates
1. Verify the functionality justifies a separate crate
2. Define clear API boundaries and public interface
3. Implement basic functionality before adding to workspace
4. Add comprehensive unit tests from the beginning
5. Document the crate's purpose and integration points

#### Before Adding Dependencies
1. Check if functionality can be implemented in standard library
2. Verify the dependency is actively maintained
3. Consider the impact on build times and binary size
4. Add to workspace.dependencies for version consistency
5. Run cargo-machete regularly to catch unused dependencies

#### Development Process
1. Write tests first for new functionality
2. Use MCP servers for complex task planning
3. Maintain quality gates on every commit
4. Update documentation alongside code changes
5. Regular dependency audits and security scanning

For comprehensive analysis of architectural decisions and lessons learned, see `.claude/architecture-analysis.md`.

## Development Commands

### Build and Test
```bash
cargo build                    # Standard build
cargo build --release        # Optimized build
cargo test                   # Execute test suite
cargo test -- --nocapture   # Test with output
cargo run -- --help         # Run with help flag
```

### Code Quality
```bash
cargo fmt                            # Format source code
cargo clippy --all-features -- -D warnings  # Lint all features
cargo check                          # Validate without building
cargo-machete                        # Check for unused dependencies
python3 scripts/check-emojis.py      # Enforce no-emoji policy (platform-agnostic)
```

### Package-Specific Testing
```bash
cargo test -p hpm-config      # Test configuration management
cargo test -p hpm-core        # Test core functionality and storage
cargo test -p hpm-package     # Test package manifest handling
cargo test -p hpm-registry    # Test registry client/server functionality
cargo test --workspace       # Test entire workspace
```

### Development Operations
```bash
RUST_LOG=debug cargo run -- install <package>  # Debug logging
cargo run -- init <name>                       # Initialize package
cargo test <module>::tests                     # Module-specific tests
cargo test --test integration                  # Integration tests only
cargo doc --open                               # Generate documentation
python3 scripts/check-emojis.py                # Check for emoji usage (platform-agnostic)
```

### HPM CLI Testing
```bash
# Test CLI functionality
cargo run -- init test-package --description "Test package"
cargo run -- init --bare minimal-package
cargo run -- install                                   # Install dependencies from hpm.toml in current directory
cargo run -- install --manifest /path/to/hpm.toml     # Install dependencies from specific manifest
cargo run -- add utility-nodes                        # Add a specific package
cargo run -- list
cargo run -- search "geometry tools"

# Test cleanup system
cargo run -- clean --dry-run                   # Preview cleanup operations
cargo run -- clean --yes                      # Automated cleanup
cargo run -- clean --python-only --dry-run    # Preview Python virtual environment cleanup
cargo run -- clean --comprehensive --dry-run  # Preview comprehensive cleanup (packages + Python)
RUST_LOG=debug cargo run -- clean --dry-run    # Debug cleanup analysis
```

### Registry Development
```bash
# Start registry server (development)
cargo run --bin registry-server -p hpm-registry

# Test registry client
cargo run --example basic_client -p hpm-registry

# Run registry integration tests
cargo test --test integration_tests -p hpm-registry

# Build registry with all features
cargo build --release --all-features -p hpm-registry
```

### MCP Configuration Maintenance
```bash
# MCP server health check and redundancy audit
claude mcp list                           # List all configured servers with status

# Server analysis and cleanup
claude mcp get <server-name>              # Get detailed server information
claude mcp remove <server-name> -s <scope>  # Remove redundant servers

# Configuration optimization
claude mcp add -s user <name> <url>       # Add global servers
claude mcp add -s local <name> <url>      # Add project-specific servers

# Regular maintenance check (run monthly)
echo "MCP Maintenance Checklist:"
echo "1. claude mcp list - Check all server status"
echo "2. Compare with built-in Claude Code tools"
echo "3. Remove redundant filesystem/github/sequential servers"
echo "4. Verify awesome-claude-code global accessibility"
echo "5. Update CLAUDE.md documentation"
```

## MCP Maintenance Workflow

### Overview

This workflow ensures optimal MCP configuration by preventing redundancy with Claude Code's built-in capabilities and maintaining clean, efficient server configurations.

### Built-in vs External MCP Capabilities

#### ✅ Built-in Tools (No MCP Server Needed)
- **Filesystem Operations**: `Read`, `Write`, `Edit`, `MultiEdit`, `Glob`, `LS`, `Bash`
- **GitHub Integration**: `mcp__github__*` tools for comprehensive GitHub API access
- **Sequential Thinking**: `mcp__sequential__sequentialthinking` for complex reasoning
- **IDE Integration**: VSCode extension provides file references, diagnostics, diff viewing

#### ✅ Valid External MCP Servers
- **Content Sources**: Git repositories, documentation sites, knowledge bases
- **Specialized APIs**: Database connections, external services, domain-specific tools
- **Project-specific Tools**: Custom tooling not covered by built-in capabilities

### Monthly Maintenance Procedure

#### 1. Server Health Assessment
```bash
# Check all configured servers
claude mcp list

# Analyze each server's purpose and status
for server in $(claude mcp list --format=names); do
    claude mcp get "$server"
done
```

#### 2. Redundancy Analysis
Review each server against built-in capabilities:

| MCP Server Type | Built-in Alternative | Action |
|----------------|---------------------|--------|
| `@modelcontextprotocol/server-filesystem` | `Read`, `Write`, `Edit`, `Glob`, `LS` | ❌ Remove |
| `@modelcontextprotocol/server-github` | `mcp__github__*` tools | ❌ Remove |
| `@modelcontextprotocol/server-sequential-thinking` | `mcp__sequential__sequentialthinking` | ❌ Remove |
| Content repositories (git-mcp) | No built-in equivalent | ✅ Keep |
| Database connections | No built-in equivalent | ✅ Keep |

#### 3. Cleanup Implementation
```bash
# Remove redundant servers (examples from our cleanup)
claude mcp remove filesystem -s local     # Redundant with built-in file operations
claude mcp remove github -s local        # Redundant with built-in GitHub integration
claude mcp remove sequential -s local    # Redundant with built-in sequential thinking

# Verify cleanup
claude mcp list
```

#### 4. Scope Optimization
Move servers to appropriate scope based on usage:

- **User scope (Global)**: Cross-project utilities, content sources, reference materials
- **Local scope (Project)**: Project-specific databases, custom APIs, specialized tooling

```bash
# Example: Move content source to global scope
claude mcp remove awesome-claude-code -s local
claude mcp add -s user awesome-claude-code https://gitmcp.io/hesreallyhim/awesome-claude-code
```

#### 5. Documentation Update
Update this document with:
- Current server configurations
- Rationale for each server
- Changes made during maintenance

### Best Practices

#### Before Adding New MCP Servers
1. **Check Built-in Capabilities**: Review available tools in Claude Code context
2. **Verify Uniqueness**: Ensure the server provides functionality not covered by built-ins
3. **Choose Appropriate Scope**: Global for cross-project, local for project-specific
4. **Document Purpose**: Add clear rationale to CLAUDE.md

#### Server Addition Guidelines
```bash
# Good: Unique content source
claude mcp add -s user awesome-claude-code https://gitmcp.io/hesreallyhim/awesome-claude-code

# Good: Project-specific database
claude mcp add -s local postgres npx @modelcontextprotocol/server-postgres postgresql://localhost:5432

# Bad: Redundant filesystem server
# claude mcp add filesystem npx @modelcontextprotocol/server-filesystem  # DON'T DO THIS
```

#### Configuration Health Indicators
- ✅ **Healthy**: 2-4 MCP servers total, no redundant functionality
- ⚠️ **Review Needed**: 5+ servers, potential overlap with built-ins  
- ❌ **Problematic**: 10+ servers, clear redundancy with Claude Code capabilities

### Troubleshooting

#### Common Issues
- **Server Connection Failed**: Check network connectivity and server URL
- **Redundant Functionality**: Remove servers that duplicate built-in tools
- **Scope Confusion**: Move cross-project servers to user scope, project-specific to local

#### Recovery Procedures
```bash
# Reset MCP configuration (nuclear option)
# Backup first: cp ~/.claude.json ~/.claude.json.backup

# Remove all local servers
claude mcp list --local | xargs -I {} claude mcp remove {} -s local

# Remove all user servers  
claude mcp list --user | xargs -I {} claude mcp remove {} -s user

# Re-add only essential servers
claude mcp add -s user awesome-claude-code https://gitmcp.io/hesreallyhim/awesome-claude-code
```

### Success Metrics
- **Minimal Configuration**: Only non-redundant servers configured
- **Fast Startup**: No connection delays from unnecessary servers
- **Clear Documentation**: Each server's purpose documented and justified
- **Scope Alignment**: Servers in appropriate scope (user vs local)

## User-Level Development Workflows

### Overview

These workflows provide systematic approaches to software development and project management tasks, ensuring consistency and quality across all development activities.

### Claude Maintenance Workflow

#### Purpose
Maintain optimal Claude Code configuration, performance, and integration across all projects with intelligent learning from project configurations.

#### Frequency: Monthly

#### Enhanced Features
- **Configuration Learning**: Analyzes project setups to improve user-level configuration
- **Redundancy Elimination**: Removes duplicate settings between user and project levels
- **Agent Optimization**: Recommends moving generalizable agents to user level
- **Pattern Recognition**: Identifies common development patterns across projects

#### Checklist
```bash
#!/bin/bash
# Claude Maintenance Workflow
# Run monthly to maintain optimal Claude Code setup

echo "🔧 Claude Code Maintenance Workflow"
echo "=================================="

# 1. MCP Configuration Audit
echo "1. MCP Configuration Audit"
claude mcp list
echo "   ✓ Review server health and status"
echo "   ✓ Check for redundant servers vs built-in tools"
echo "   ✓ Verify scope alignment (user vs local)"

# 2. Configuration Health Check
echo -e "\n2. Configuration Health Check"
echo "   Current Claude Code version:"
claude --version
echo "   ✓ Check for Claude Code updates"
echo "   ✓ Review ~/.claude.json for anomalies"

# 3. Memory Management
echo -e "\n3. Memory Management"
echo "   ✓ Review CLAUDE.md files across projects"
echo "   ✓ Update project-specific context"
echo "   ✓ Archive outdated memory entries"

# 4. Integration Testing
echo -e "\n4. Integration Testing"
echo "   ✓ Test VSCode extension functionality"
echo "   ✓ Verify MCP server connectivity"
echo "   ✓ Check file operations and GitHub integration"

# 5. Performance Optimization
echo -e "\n5. Performance Optimization"
echo "   ✓ Clear unnecessary cache data"
echo "   ✓ Review startup time and responsiveness"
echo "   ✓ Optimize token usage patterns"

# 6. Agent Configuration Review
echo -e "\n6. Agent Configuration Review"
echo "   ✓ Review user-level vs project-level agent separation"
echo "   ✓ Verify agent cost optimization (model selection)"

# 7. Configuration Consolidation
echo -e "\n7. Configuration Consolidation"
echo "   ✓ Scan project configurations for learning opportunities"
echo "   ✓ Enhance user-level settings with common patterns"
echo "   ✓ Remove redundant project-level configurations"
echo "   ✓ Recommend agent migrations to user level"

# 8. Documentation Update
echo -e "\n8. Documentation Update"
echo "   ✓ Update CLAUDE.md with new learnings"
echo "   ✓ Document workflow improvements"
echo "   ✓ Sync configuration changes"

echo -e "\n✅ Claude Maintenance Complete"
```

#### Detailed Actions

##### 1. MCP Configuration Audit
```bash
# Check current configuration
claude mcp list

# Remove redundant servers
for server in filesystem github sequential; do
    if claude mcp get "$server" 2>/dev/null; then
        echo "⚠️  Redundant server detected: $server"
        claude mcp remove "$server" -s local
    fi
done

# Verify essential servers
claude mcp get awesome-claude-code || echo "❌ Missing awesome-claude-code server"
claude mcp get postgres || echo "ℹ️  Project-specific postgres server not configured"
```

##### 2. Configuration Health Check
```bash
# Check Claude Code version and updates
claude --version
echo "Check https://github.com/anthropics/claude-code/releases for updates"

# Validate configuration file
if jq empty ~/.claude.json 2>/dev/null; then
    echo "✅ ~/.claude.json is valid JSON"
else
    echo "❌ ~/.claude.json has JSON syntax errors"
fi
```

##### 3. Memory Management
```bash
# Review CLAUDE.md files across projects
find ~/Documents/workspace -name "CLAUDE.md" -exec echo "Project: {}" \; -exec head -3 {} \;

# Check memory usage patterns
echo "Review /memory command usage and clean up outdated entries"
```

#### Success Metrics
- ✅ All MCP servers healthy and non-redundant
- ✅ Claude Code version up to date
- ✅ Configuration files valid and optimized
- ✅ Memory entries current and relevant
- ✅ Performance within acceptable ranges

### Project Setup Workflow

#### Purpose
Standardize new project initialization with Claude Code integration and best practices.

#### Frequency: Per new project

#### Checklist
```bash
#!/bin/bash
# Project Setup Workflow
# Run when starting a new project

PROJECT_NAME="$1"
PROJECT_TYPE="$2"  # rust, python, typescript, etc.

echo "🚀 Project Setup Workflow: $PROJECT_NAME ($PROJECT_TYPE)"
echo "=================================================="

# 1. Project Structure Creation
echo "1. Creating project structure..."
mkdir -p "$PROJECT_NAME"
cd "$PROJECT_NAME"

# 2. CLAUDE.md Creation
echo "2. Creating CLAUDE.md..."
cat > CLAUDE.md << EOF
# $PROJECT_NAME

## Project Overview
[Brief description of the project]

## Technology Stack
- **Primary Language**: $PROJECT_TYPE
- **Build System**: [Build tool]
- **Testing**: [Test framework]
- **Dependencies**: [Key dependencies]

## Development Commands
\`\`\`bash
# Build
[build command]

# Test
[test command]

# Lint
[lint command]
\`\`\`

## Architecture
[Project structure and design decisions]

## Contributing
[Development workflow and standards]
EOF

# 3. Git Initialization
echo "3. Initializing Git repository..."
git init
git add CLAUDE.md
git commit -m "feat: initialize project with Claude Code integration"

# 4. Claude Code Integration
echo "4. Setting up Claude Code integration..."
echo "   ✓ CLAUDE.md created and committed"
echo "   ✓ Ready for Claude Code development"

echo -e "\n✅ Project setup complete for $PROJECT_NAME"
```

### Code Quality Workflow

#### Purpose
Maintain consistent code quality standards across all development activities.

#### Frequency: Pre-commit, weekly review

#### Checklist
```bash
#!/bin/bash
# Code Quality Workflow
# Run before commits and during weekly reviews

echo "🔍 Code Quality Workflow"
echo "======================="

# 1. Formatting and Style
echo "1. Code Formatting and Style"
case "$PROJECT_TYPE" in
    rust)
        cargo fmt --check
        cargo clippy --all-features -- -D warnings
        ;;
    python)
        black --check .
        flake8 .
        mypy .
        ;;
    typescript)
        npm run format:check
        npm run lint
        npm run type-check
        ;;
esac

# 2. Testing
echo -e "\n2. Testing"
case "$PROJECT_TYPE" in
    rust)
        cargo test --workspace
        cargo test --doc
        ;;
    python)
        pytest --cov
        ;;
    typescript)
        npm run test
        npm run test:integration
        ;;
esac

# 3. Security Audit
echo -e "\n3. Security Audit"
case "$PROJECT_TYPE" in
    rust)
        cargo audit
        ;;
    python)
        pip-audit
        ;;
    typescript)
        npm audit
        ;;
esac

# 4. Documentation
echo -e "\n4. Documentation"
echo "   ✓ CLAUDE.md up to date with changes"
echo "   ✓ API documentation generated"
echo "   ✓ README reflects current functionality"

# 5. Dependency Management
echo -e "\n5. Dependency Management"
case "$PROJECT_TYPE" in
    rust)
        cargo machete
        ;;
    python)
        pip-check
        ;;
    typescript)
        npm-check-updates
        ;;
esac

echo -e "\n✅ Code Quality Check Complete"
```

### Release Workflow

#### Purpose
Standardize release preparation and deployment processes.

#### Frequency: Per release

#### Checklist
```bash
#!/bin/bash
# Release Workflow
# Run when preparing for a new release

RELEASE_VERSION="$1"

echo "📦 Release Workflow: v$RELEASE_VERSION"
echo "================================="

# 1. Pre-release Validation
echo "1. Pre-release Validation"
echo "   ✓ All tests passing"
echo "   ✓ Code quality checks pass"
echo "   ✓ Documentation updated"
echo "   ✓ Dependencies audited"

# 2. Version Bumping
echo -e "\n2. Version Bumping"
case "$PROJECT_TYPE" in
    rust)
        # Update Cargo.toml versions
        sed -i '' "s/version = \"[^\"]*\"/version = \"$RELEASE_VERSION\"/" Cargo.toml
        ;;
    python)
        # Update pyproject.toml or setup.py
        sed -i '' "s/version = \"[^\"]*\"/version = \"$RELEASE_VERSION\"/" pyproject.toml
        ;;
    typescript)
        # Update package.json
        npm version "$RELEASE_VERSION" --no-git-tag-version
        ;;
esac

# 3. Changelog Generation
echo -e "\n3. Changelog Generation"
echo "   ✓ Update CHANGELOG.md with release notes"
echo "   ✓ Document breaking changes"
echo "   ✓ List new features and bug fixes"

# 4. Release Commit and Tag
echo -e "\n4. Release Commit and Tag"
git add -A
git commit -m "chore: release v$RELEASE_VERSION"
git tag -a "v$RELEASE_VERSION" -m "Release v$RELEASE_VERSION"

# 5. Release Validation
echo -e "\n5. Release Validation"
echo "   ✓ Final build successful"
echo "   ✓ Release artifacts generated"
echo "   ✓ Ready for deployment"

echo -e "\n✅ Release v$RELEASE_VERSION prepared"
```

### Weekly Development Review

#### Purpose
Regular assessment of development progress, code quality, and process improvements.

#### Frequency: Weekly

#### Template
```markdown
# Weekly Development Review - [Date]

## Accomplishments
- [ ] Features completed
- [ ] Bugs fixed
- [ ] Documentation updated
- [ ] Technical debt addressed

## Code Quality Metrics
- [ ] Test coverage: [%]
- [ ] Build times: [duration]
- [ ] Dependency count: [number]
- [ ] Code quality score: [metric]

## Process Improvements
- [ ] Workflow optimizations implemented
- [ ] Tool configurations updated
- [ ] Documentation improvements made
- [ ] Learning objectives achieved

## Next Week Priorities
- [ ] Feature development goals
- [ ] Technical debt items
- [ ] Process improvement initiatives
- [ ] Learning and development activities

## Action Items
- [ ] [Action item 1]
- [ ] [Action item 2]
- [ ] [Action item 3]

## Notes
[Any additional observations, concerns, or insights]
```

### Workflow Automation

#### Integration with Claude Code
```bash
# Create workflow scripts in project root
mkdir -p scripts/workflows

# Make workflows executable
chmod +x scripts/workflows/*.sh

# Add to CLAUDE.md for easy reference
echo "## Development Workflows" >> CLAUDE.md
echo "See scripts/workflows/ for automated development procedures" >> CLAUDE.md
```

#### Usage Examples
```bash
# Run Claude maintenance
./scripts/workflows/claude-maintenance.sh

# Setup new project
./scripts/workflows/project-setup.sh my-new-project rust

# Quality check before commit
./scripts/workflows/code-quality.sh

# Prepare release
./scripts/workflows/release.sh 1.2.0
```

## Project Architecture

HPM implements a modular workspace architecture optimized for package management operations.

### Workspace Structure
- **`crates/hpm-cli`** - Command-line interface and application entry point
- **`crates/hpm-core`** - Core functionality with storage, project discovery, and cleanup systems
- **`crates/hpm-config`** - Configuration management with project discovery settings
- **`crates/hpm-registry`** - High-performance package registry with QUIC transport and gRPC API
- **`crates/hpm-resolver`** - Dependency resolution engine
- **`crates/hpm-installer`** - Package installation subsystem
- **`crates/hpm-package`** - Package manifest processing and Houdini integration
- **`crates/hpm-python`** - Python dependency management with virtual environment isolation
- **`crates/hpm-error`** - Error handling infrastructure

#### Core Module Components (`crates/hpm-core/src/`)
- **`storage.rs`** - Global package storage with project-aware cleanup
- **`discovery.rs`** - Project discovery and filesystem scanning
- **`dependency.rs`** - Dependency graph construction and analysis
- **`project.rs`** - Project manifest management and Houdini integration
- **`manager.rs`** - High-level package management operations
- **`integration_test.rs`** - End-to-end testing for cleanup workflows

#### Registry Module Components (`crates/hpm-registry/src/`)
- **`client/`** - Registry client with QUIC connections and authentication
- **`server/`** - gRPC server implementation with pluggable storage backends
- **`types/`** - Shared types for authentication, packages, and error handling
- **`utils/`** - Compression, validation, and checksum utilities
- **`proto/`** - Generated Protocol Buffer definitions for gRPC API

### Package Storage Architecture

HPM implements a two-tier storage system optimized for Houdini's package loading:

#### Global Storage (`~/.hpm/`)
```
~/.hpm/
├── packages/                     # Versioned package storage
│   ├── utility-nodes@2.1.0/     # Individual package installations
│   └── material-library@1.5.0/
├── cache/                        # Download cache and metadata
└── registry/                     # Registry index cache
```

#### Project Integration (`.hpm/packages/`)
```
project/
├── .hpm/
│   └── packages/                 # Houdini package manifests
│       ├── utility-nodes.json   # Links to global storage
│       └── material-library.json
├── hpm.toml                      # Project manifest
└── hpm.lock                      # Dependency lock file
```

**Key Benefits**:
- **Disk Efficiency**: Single global storage prevents duplicate installations
- **Version Management**: Multiple versions coexist in global storage
- **Houdini Integration**: Generated package.json files work with HOUDINI_PACKAGE_PATH
- **Project Isolation**: Each project can use different package versions

### Design Principles
- **Asynchronous Operations**: Tokio runtime for all I/O operations
- **Structured Error Handling**: Domain errors via `thiserror`, application errors via `anyhow`
- **Interface Abstraction**: Trait-based design for testability and modularity
- **Layered Configuration**: Hierarchical configuration management (global, project, runtime)
- **Modular Crates**: Clear separation of concerns with minimal coupling

## CLI Design and Package Management

### Command Structure

HPM provides comprehensive package management through industry-standard CLI patterns:

#### Core Commands
- `hpm init` - Initialize new Houdini packages with templates
- `hpm add` - Add packages and resolve dependencies
- `hpm install` - Install dependencies from hpm.toml manifest
- `hpm remove` - Remove installed packages
- `hpm update` - Update packages to latest versions
- `hpm list` - Display installed packages and dependency tree
- `hpm search` - Search registry for packages
- `hpm publish` - Publish packages to registry
- `hpm info` - Show detailed package information
- `hpm run` - Execute package scripts
- `hpm check` - Validate package configuration and Houdini compatibility
- `hpm clean` - Project-aware package cleanup with orphan detection

#### Package Templates
- **Standard** (default): Complete Houdini package with all standard directories
- **Bare**: Minimal structure with only hpm.toml for custom layouts

See `docs/cli-design.md` for comprehensive CLI specification.

### Install Command

The `hpm install` command provides comprehensive dependency installation from hpm.toml manifests, supporting both HPM packages and Python dependencies with virtual environment isolation.

#### Usage
```bash
# Install dependencies from hpm.toml in current directory
hpm install

# Install dependencies from specific manifest file
hpm install --manifest /path/to/hpm.toml
hpm install -m ../project/hpm.toml

# Install from directory containing hpm.toml
hpm install --manifest /path/to/project/
```

#### Functionality
- **Manifest Discovery**: Automatically locates hpm.toml in current directory or accepts explicit paths
- **Dependency Resolution**: Processes both HPM package dependencies and Python dependencies
- **Virtual Environment Management**: Creates content-addressable Python virtual environments for dependency isolation
- **Project Structure Setup**: Creates `.hpm/` directory with proper package organization
- **Lock File Generation**: Generates `hpm.lock` with resolved dependency information
- **Houdini Integration**: Configures PYTHONPATH and package manifests for seamless Houdini integration

#### Directory Structure Created
```
project/
├── hpm.toml                    # Project manifest
├── hpm.lock                    # Generated lock file with resolved dependencies
└── .hpm/                       # HPM project directory
    └── packages/               # Package installation references
```

#### Integration with Python Dependencies
When Python dependencies are specified in hpm.toml, the install command:
1. Maps Houdini version constraints to compatible Python versions
2. Resolves exact package versions using bundled UV
3. Creates or reuses content-addressable virtual environments in `~/.hpm/venvs/`
4. Generates Houdini package.json files with proper PYTHONPATH injection
5. Shares virtual environments between projects with identical resolved dependencies

#### Error Handling
- Clear error messages for missing or invalid hpm.toml files
- Manifest validation with helpful feedback
- Graceful handling of network issues during dependency resolution
- Comprehensive logging with RUST_LOG=debug for troubleshooting

## Project-Aware Cleanup System

HPM features an intelligent cleanup system that safely removes orphaned packages while preserving dependencies needed by active projects.

### Architecture Overview

The cleanup system consists of four integrated components:

1. **Project Discovery** (`crates/hpm-core/src/discovery.rs`)
   - Configurable filesystem scanning for HPM-managed projects
   - Depth-limited recursive traversal with ignore patterns
   - Manifest validation and project metadata extraction

2. **Dependency Graph Engine** (`crates/hpm-core/src/dependency.rs`)
   - Transitive dependency tracking and analysis
   - Cycle detection with detailed warnings
   - Root package identification and reachability analysis

3. **Storage Manager** (`crates/hpm-core/src/storage.rs`)
   - Project-aware cleanup logic with safety guarantees
   - Orphan detection through set difference operations
   - Safe removal with comprehensive error handling

4. **CLI Integration** (`crates/hpm-cli/src/commands/clean.rs`)
   - User-friendly interface with dry-run and force modes
   - Interactive confirmation and progress reporting

### Key Features

#### Safety Guarantees
- **Never removes packages required by active projects**
- **Preserves transitive dependencies automatically**
- **Warns when no projects found (prevents removing all packages)**
- **Comprehensive logging for troubleshooting**

#### Configuration-Driven Discovery
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

#### Usage Patterns
```bash
# Preview cleanup operations (recommended first step)
hpm clean --dry-run

# Interactive cleanup with confirmation prompts
hpm clean

# Automated cleanup for scripts and CI/CD
hpm clean --yes

# Debug cleanup analysis
RUST_LOG=debug hpm clean --dry-run
```

### Implementation Highlights

#### Advanced Dependency Analysis
- **Transitive Resolution**: Follows complete dependency chains
- **Cycle Detection**: Identifies and warns about circular dependencies
- **Missing Package Handling**: Creates placeholder nodes for uninstalled dependencies
- **Performance Optimization**: Uses efficient graph algorithms (HashSet-based reachability)

#### Comprehensive Testing
- **Unit Tests**: 25+ tests covering core functionality
- **Integration Tests**: End-to-end scenarios with real filesystem operations
- **Transitive Dependency Preservation**: Validates complex dependency chain handling
- **Error Scenario Testing**: Ensures graceful handling of edge cases

For detailed technical documentation, see `docs/cleanup-system.md`.

## Python Dependency Management

HPM provides comprehensive Python dependency management for Houdini packages, addressing the challenge of conflicting Python dependencies across multiple packages through virtual environment isolation.

### Core Features

- **Content-Addressable Virtual Environments**: Packages with identical resolved dependencies share the same virtual environment
- **UV-Powered Resolution**: High-performance dependency resolution using bundled UV binary
- **Complete Isolation**: HPM's UV installation is completely isolated from system UV to prevent interference
- **Conflict Detection**: Automatic detection and reporting of dependency conflicts across packages
- **Houdini Integration**: Seamless PYTHONPATH injection via generated package.json files
- **Intelligent Cleanup**: Orphaned virtual environment detection and removal

### Architecture Overview

The Python dependency system uses hash-based virtual environment sharing:

```
~/.hpm/
├── packages/                     # HPM packages
├── venvs/                        # Python virtual environments
│   ├── a1b2c3d4e5f6/            # Virtual environment (content hash)
│   │   ├── metadata.json        # Environment metadata
│   │   └── lib/python/site-packages/  # Python packages
│   └── f6e5d4c3b2a1/            # Another virtual environment
└── uv-cache/                     # Isolated UV package cache
```

### HPM Package Manifest Python Dependencies

Add Python dependencies to your `hpm.toml`:

```toml
[package]
name = "my-houdini-tool"
version = "1.0.0"

[houdini]
min_version = "20.0"  # Maps to Python 3.9

# Python dependency specifications
[python_dependencies]
numpy = ">=1.20.0"
requests = { version = ">=2.25.0", extras = ["security"] }
matplotlib = { version = "^3.5.0", optional = true }
```

### Houdini Version to Python Version Mapping

HPM automatically maps Houdini versions to appropriate Python versions:

| Houdini Version | Python Version |
|----------------|----------------|
| 19.0 - 19.5    | Python 3.7     |
| 20.0           | Python 3.9     |
| 20.5           | Python 3.10    |
| 21.x           | Python 3.11    |

### Dependency Resolution Process

1. **Collection**: Extract Python dependencies from all package manifests
2. **Version Mapping**: Map Houdini version constraints to Python versions
3. **Conflict Detection**: Identify conflicting dependency versions
4. **Resolution**: Use UV to resolve exact package versions
5. **Environment Creation**: Create or reuse virtual environment based on content hash
6. **Integration**: Generate Houdini package.json with PYTHONPATH injection

### Usage Examples

#### Basic Python Package
```toml
[package]
name = "geometry-tools"
version = "1.0.0"

[houdini]
min_version = "20.0"

[python_dependencies]
numpy = ">=1.20.0"
scipy = ">=1.7.0"
```

#### Advanced with Optional Dependencies
```toml
[package]
name = "visualization-tools"
version = "2.1.0"

[houdini]
min_version = "20.0"

[python_dependencies]
matplotlib = "^3.5.0"
plotly = { version = ">=5.0.0", optional = true }
seaborn = { version = ">=0.11.0", extras = ["stats"] }
```

### Python Cleanup Operations

HPM extends its cleanup system to handle Python virtual environments:

```bash
# Preview Python virtual environment cleanup
hpm clean --python-only --dry-run

# Clean only orphaned Python environments
hpm clean --python-only

# Comprehensive cleanup (packages + Python environments)
hpm clean --comprehensive --dry-run
hpm clean --comprehensive

# Interactive cleanup with confirmation
hpm clean --comprehensive
```

### Virtual Environment Sharing

Multiple packages with identical resolved dependencies share the same virtual environment:

```bash
# Package A and B both need numpy==1.24.0, requests==2.28.0
# They share virtual environment hash: a1b2c3d4e5f6

Package A (geometry-tools) -> venv: a1b2c3d4e5f6
Package B (mesh-utilities)  -> venv: a1b2c3d4e5f6  # Same hash, shared environment
Package C (advanced-tools)  -> venv: f6e5d4c3b2a1  # Different dependencies, different environment
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

### Development Commands

```bash
# Test Python dependency features
cargo test -p hpm-python                    # Test Python dependency management

# Test Python integration with core functionality
cargo test -p hpm-core --features python   # Test core with Python features

# Debug Python dependency resolution
RUST_LOG=debug cargo run -- add geometry-tools  # See Python resolution process

# Manual Python environment operations
cargo run --example python_venv_demo -p hpm-python  # Development examples
```

### UV Isolation Strategy

HPM bundles its own UV binary and ensures complete isolation:

- **Bundled Binary**: UV is embedded in the HPM binary, no system dependency
- **Isolated Cache**: UV cache stored in `~/.hpm/uv-cache/`, not system cache
- **Isolated Config**: UV configuration in `~/.hpm/uv-config/`, separate from system
- **Environment Variables**: HPM sets UV-specific environment variables for isolation
- **No System Interference**: Zero impact on existing user UV installations

### Error Handling and Troubleshooting

Common Python dependency scenarios:

- **Conflicting Versions**: HPM detects and reports version conflicts with specific package names
- **Missing Python Version**: Automatic fallback to Python 3.9 if Houdini version mapping fails
- **Network Issues**: UV dependency resolution failures are properly reported with context
- **Virtual Environment Corruption**: Automatic recreation of corrupted environments
- **Cleanup Safety**: Never removes virtual environments needed by active projects

For comprehensive technical details, see `docs/python-dependency-management.md`.

## Houdini Integration

HPM extends Houdini's native package system with modern dependency management capabilities.

### HPM Package Manifest (hpm.toml)

The primary package descriptor supporting comprehensive metadata and dependency management:

```toml
[package]
name = "my-houdini-tool"
version = "1.0.0"
description = "Custom Houdini digital assets and tools"
authors = ["Author <email@example.com>"]
license = "MIT"
readme = "README.md"
keywords = ["houdini"]

[houdini]
min_version = "19.5"
max_version = "20.5"

[dependencies]
utility-nodes = "^2.1.0"
material-library = { version = "1.5", optional = true }

[python_dependencies]
numpy = ">=1.20.0"
requests = { version = ">=2.25.0", extras = ["security"] }

[scripts]
build = "python scripts/build.py"
test = "python -m pytest tests/"
```

### Standard Package Structure
```
my-package/
├── hpm.toml           # HPM package manifest
├── package.json       # Generated Houdini package file
├── README.md          # Package documentation
├── otls/             # Digital assets (.hda, .otl files)
│   └── my_node.hda
├── python/           # Python modules
│   └── my_tool.py
├── scripts/          # Shelf tools and scripts
├── presets/          # Node presets
├── config/           # Configuration files
└── tests/            # Test files
```

### Package.json Generation
HPM automatically generates standard Houdini `package.json` files from `hpm.toml` configuration, ensuring seamless integration with existing Houdini workflows.

### Supported Asset Types
- **Digital Assets**: Houdini Digital Assets (.hda, .otl)
- **Python Modules**: Libraries and tools for Houdini Python environment
- **Scripts**: Shelf tools, event handlers, and automation scripts
- **Presets**: Node parameter presets and configurations
- **Configuration**: Environment and pipeline configuration files

## Development Standards

### Code Style
- Adhere to standard Rust formatting (`rustfmt`)
- Apply comprehensive linting (`cargo clippy`)
- Implement explicit error handling (avoid panics)
- Document all public APIs with doc comments

### Testing Framework

#### Core Testing Principles
- **Unit tests**: Module-level tests using `#[cfg(test)]`
- **Integration tests**: End-to-end testing in `tests/` directory
- **Mock implementations**: External dependency abstraction
- **Property-based testing**: Complex algorithm verification

#### File System Testing Standards
For functionality that creates files and directories (like `hpm init`):

**Test Fixtures and Cleanup**:
- Always use `tempfile::TempDir` for temporary file system operations
- Never rely on global file system state that could affect other tests
- Restore working directory after tests that change it

```rust
#[tokio::test]
async fn test_init_package_standard() {
    let temp_dir = TempDir::new().unwrap();
    let original_dir = env::current_dir().unwrap();
    
    env::set_current_dir(temp_dir.path()).unwrap();
    // ... test logic ...
    env::set_current_dir(original_dir).unwrap();
    
    // TempDir automatically cleans up when dropped
}
```

**Content Validation Requirements**:
- Verify both file/directory existence AND content correctness
- Test all expected files and directories, not just a subset
- Validate generated content matches expected structure and values
- Test edge cases with special characters, missing optional fields

```rust
// Validate file existence
assert!(package_path.join("hpm.toml").exists());
assert!(package_path.join("python").is_dir());

// Validate file content
let hpm_toml_content = fs::read_to_string(package_path.join("hpm.toml")).unwrap();
assert!(hpm_toml_content.contains("name = \"test-package\""));
assert!(hpm_toml_content.contains("version = \"1.0.0\""));
```

**Error Case Testing**:
- Test failure scenarios (directory already exists, invalid input)
- Verify error messages are helpful and accurate
- Ensure partial failures are handled gracefully

#### Test Organization
- Group related tests in modules using `#[cfg(test)]`
- Use descriptive test names that clearly indicate what is being tested
- Include helper functions for common validation patterns
- Run tests with `--test-threads=1` when tests modify working directory

### Error Handling
```rust
// Domain-specific errors using thiserror
#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("Package not found: {name}")]
    PackageNotFound { name: String },

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
}

// Application-level errors using anyhow
use anyhow::{Context, Result};

pub fn install_package(name: &str) -> Result<()> {
    download_package(name)
        .context("Package download failed")?;
    Ok(())
}
```

## Configuration

### Global Configuration (`~/.hpm/config.toml`)
```toml
[registry]
default = "https://packages.houdini.org"

[install]
path = "packages/hpm"
parallel_downloads = 8

[auth]
token = "your-registry-token"
```

### Project Configuration (`project/.hpm/hpm.toml`)
```toml
[dependencies]
utility-nodes = "^2.1.0"
material-library = { version = "1.5", optional = true }

[dev-dependencies]
test-assets = "0.1.0"
```

## System Integration

HPM integrates with Houdini through standardized mechanisms:
- **Package Discovery**: Installation to Houdini package directories
- **Manifest Translation**: Generation of `package.json` from `hpm.toml`
- **Path Management**: Configuration of `hpath`, `HOUDINI_PATH`, and environment variables
- **Version Compatibility**: Enforcement of Houdini version constraints
- **Asset Registration**: Automated registration of digital assets and Python modules

### Installation Paths
Package installation follows Houdini conventions:
- `$HOUDINI_USER_PREF_DIR/packages/` - User-specific packages
- `$HOUDINI_PACKAGE_DIR` - Project-specific installations
- `~/.hpm/` - HPM registry cache and metadata

## Security Framework

- **Package Verification**: Cryptographic signature validation for integrity assurance
- **Sandboxed Installation**: Isolated package extraction and installation processes
- **Path Validation**: Directory traversal attack prevention
- **Dependency Auditing**: Automated vulnerability scanning for package dependencies

## Contributing

### Contribution Process
1. **Repository Setup**: Fork repository and create feature branches
2. **Development**: Implement changes following project standards
3. **Testing**: Ensure comprehensive test coverage and validation
4. **Documentation**: Update documentation for API modifications
5. **Review**: Submit changes for peer review and approval

### Common Issues

| Issue | Resolution |
|-------|------------|
| Build Failures | Verify current Rust toolchain installation |
| Network Errors | Validate proxy configuration and registry connectivity |
| Permission Errors | Confirm write access to target installation directories |
| Version Conflicts | Analyze dependency tree using `cargo tree` |