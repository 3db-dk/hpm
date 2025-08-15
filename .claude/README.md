# Claude Code Configuration for HPM

This directory contains Claude Code configuration for the HPM (Houdini Package Manager) project.

## Essential Files

### Configuration
- `settings.json` - Project-specific permissions and hooks for Rust development
- `claude-code-maintenance-guide.md` - Official maintenance guide using verified Claude Code features

### Agents
- `agents/hpm-developer.md` - HPM-specific domain expert agent
- `agents/README.md` - Agent documentation

## Documentation Archive

### Architecture Documentation
- `architecture-analysis.md` - Project architecture analysis and lessons learned
- `package-storage-architecture.md` - Two-tier storage system design
- `package-storage-summary.md` - Storage architecture summary
- `project-aware-cleanup-design.md` - Cleanup system design documentation

### Historical Documentation
- `implementation-plan.md` - Development implementation roadmap
- `improvement-plan.md` - Project improvement recommendations
- `philosophy.md` - Project philosophy and design principles

### MCP Integration
- `mcp-integration.md` - MCP server integration details
- `mcp-setup.md` - MCP configuration and setup guide

### Legacy Directories
- `commands/` - Contains outdated slash command references (deprecated)
- `contexts/` - Historical context files

## Usage

For maintenance tasks, use the official Claude Code approach:
```bash
# Interactive maintenance
claude "Help me with HPM development and maintenance"

# Configuration management
claude config list
claude doctor
claude update
```

See `claude-code-maintenance-guide.md` for comprehensive maintenance procedures.