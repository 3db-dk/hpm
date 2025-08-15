# HPM Project Agents

Project-specific agents with deep domain knowledge for HPM (Houdini Package Manager) development.

## Agent Philosophy
- Focus on HPM-specific domain requirements and architecture
- Leverage deep knowledge of package management patterns
- Understand Houdini integration requirements
- Maintain HPM's established development standards
- Use formal, professional language standards

## Available Agents

### hpm-developer (Sonnet)
**Purpose**: HPM-specific development with deep project knowledge  
**Scope**: Package manager domain expertise, Houdini integration, HPM architecture  
**Responsibilities**:
- HPM package lifecycle management
- Project-aware cleanup and storage systems  
- Python dependency isolation with virtual environments
- Registry client/server development with QUIC
- Houdini integration and package.json generation
- Dependency graph analysis and resolution

**HPM-Specific Knowledge**:
- Two-tier storage architecture (global + project)
- Content-addressable Python virtual environments
- Project discovery and dependency analysis
- Registry system with QUIC/gRPC transport
- Houdini package system integration patterns

## General-Purpose Agents (User-Level)

The following agents have been moved to user level (`~/.claude/agents/`) as they contain general knowledge applicable to any project:

- **architecture-advisor**: General software architecture guidance
- **documentation-writer**: Cross-language technical documentation  
- **performance-specialist**: General performance optimization
- **rust-formatter**: General Rust code formatting
- **security-auditor**: Cross-language security practices
- **test-specialist**: General testing strategies and patterns

## Agent Scope Strategy

### Project-Level Agents (This Directory)
- 🎯 **HPM Domain Knowledge**: Package management, dependency resolution
- 🎯 **Houdini Integration**: Understanding of Houdini package system  
- 🎯 **HPM Architecture**: Storage systems, cleanup logic, registry design
- 🎯 **Project Workflows**: HPM-specific development and testing patterns

### User-Level Agents (`~/.claude/agents/`)  
- ✅ **General Best Practices**: Industry-standard approaches
- ✅ **Cross-Project Utility**: Reusable across different codebases
- ✅ **Technology-Agnostic**: Not tied to specific project domains
- ✅ **Cost Efficiency**: Optimized for general use cases

## Usage Guidelines

### When to Use Project-Level Agents
- HPM package manager implementation details
- Houdini integration and workflow patterns
- Project-aware cleanup and storage logic
- Registry system development and QUIC optimization
- Python virtual environment management

### When to Use User-Level Agents
- General Rust code formatting and style
- Technical documentation writing
- Security auditing and vulnerability scanning  
- Performance profiling and optimization
- General testing strategies and implementation
- Architectural guidance not specific to package management

## HPM Development Best Practices

### Explore → Plan → Code → Commit with HPM Agent
Apply official Claude Code best practices to HPM development:

```bash
# 1. Explore Phase - Understand HPM architecture
claude "Use the hpm-developer agent to explore the current package storage and cleanup systems"

# 2. Plan Phase - Design HPM-specific solutions
claude "Create a detailed plan for implementing [HPM feature] that considers project-aware cleanup and Houdini integration"

# 3. Code Phase - Implement incrementally
claude "Use the hpm-developer agent to implement the first component, focusing on storage architecture consistency"

# 4. Commit Phase - Document with HPM context
claude "Help me commit these changes with explanations specific to HPM's package management patterns"
```

### HPM-Specific Agent Workflows

#### Package Manager Development
```bash
# Test-driven development for HPM features
claude "Use the hpm-developer agent to write tests for package installation workflow, then implement"

# Registry development with domain expertise
claude "Use the hpm-developer agent to implement QUIC transport optimization for the package registry"
```

#### Integration with General Agents
```bash
# Combine domain expertise with general capabilities
# Terminal 1: HPM-specific implementation
claude "Use the hpm-developer agent to implement Python dependency isolation"

# Terminal 2: General security review
claude "Use the security-auditor agent to review the Python virtual environment implementation"

# Terminal 3: Performance optimization
claude "Use the performance-specialist agent to analyze package installation performance"
```

#### Houdini Integration Workflows
```bash
# HPM-Houdini integration patterns
claude "Use the hmp-developer agent to implement package.json generation that integrates seamlessly with Houdini's HOUDINI_PACKAGE_PATH system"

# Visual iteration for package management UI
claude "Use the hpm-developer agent to create the CLI interface, then take a screenshot of the help output for UX review"
```

## Development Standards
- Leverage HPM's established architecture patterns
- Focus on package manager domain requirements
- Consider Houdini workflow integration in all decisions
- Maintain project-aware cleanup safety guarantees
- Use MCP tools for token efficiency

## MCP Integration
- **postgres**: HPM registry development and testing
- **awesome-claude-code**: Access to Claude Code best practices
- Built-in tools: Comprehensive development workflow support

This focused approach ensures agents have the right level of domain expertise while maintaining cost efficiency through appropriate scope separation.