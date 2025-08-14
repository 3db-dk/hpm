# Claude Code Configuration for HPM (Houdini Package Manager)

This directory contains Claude Code configuration for **software development** of HPM, a Rust-based package manager for SideFX Houdini. HPM brings modern package management capabilities to the Houdini ecosystem, similar to what npm does for Node.js or uv does for Python.

## Directory Structure

### `/commands/`
Specialized slash commands for HPM development workflows:
- `/dev/create-dev-agents.md` - Create development sub-agents
- `/hpm/` - Package management operations and commands

### `/agents/`
Sub-agent configurations (individual .md files with YAML frontmatter):
- `rust-expert.md` - Rust development specialist
- `testing-specialist.md` - Testing and QA specialist
- `code-reviewer.md` - Code quality and security specialist  
- `documentation-expert.md` - Technical documentation specialist
- `package-expert.md` - Package management specialist

### `/workflows/`
Development workflow templates:
- `multi-agent-development.md` - Software development with sub-agents
- `package-development.md` - Package creation and publishing workflow
- `cli-development.md` - Command-line interface development workflow

### `/contexts/`
Project context information:
- `hpm-architecture.md` - HPM system architecture and design
- `hmp-manifest-spec.md` - HPM package manifest specification
- `rust-conventions.md` - Rust coding conventions and patterns

### `/templates/`
Code generation templates:
- `code-generation.md` - Rust development code templates
- `package-templates.md` - Package structure templates

## Usage

### Use Development Sub-Agents
```bash
# Sub-agents are ready to use with the Task tool
# Example: Use specialized agent for specific tasks
# Task tool will automatically select appropriate sub-agents
```

### Development Workflows
```bash
# Feature development with Task tool delegation
# The Task tool will automatically select appropriate sub-agents
# based on the task requirements and available expertise
```

### Package Operations
```bash
# Package management commands
/hpm/init
/hpm/build
/hpm/publish
```

## Token Optimization

This configuration is optimized for Rust development with:
- **Agent specialization** for context efficiency  
- **Parallel workflows** for faster development
- **Smart caching** for frequently accessed operations
- **Rust-specific tooling** integration

## Focus

This setup is specifically for:
- ✅ **Rust software development** of the HPM project
- ✅ **Houdini package management** functionality and workflows  
- ✅ **CLI development** and user experience
- ✅ **Development productivity** and code quality
- ✅ **Houdini ecosystem integration** and compatibility
- ❌ **Houdini content creation** (HDAs, scenes, etc.)
- ❌ **Production VFX workflows** (unless specifically requested)