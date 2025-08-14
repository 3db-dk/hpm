# HPM Agent Configuration

This directory contains specialized agents for the HPM (Houdini Package Manager) project.

## Available Agents

### Haiku Model Agents
- **rust-formatter** - Code formatting and style enforcement
- **documentation-writer** - API documentation and technical writing

### Sonnet Model Agents
- **rust-developer** - Feature development and dependency management
- **test-specialist** - Test implementation and quality assurance

### Opus Model Agents
- **architecture-advisor** - System design and architectural decisions

### Additional Sonnet Model Agents
- **performance-specialist** - Performance analysis and optimization
- **security-auditor** - Security analysis and vulnerability scanning

## Usage Guidelines

1. Use Haiku agents for routine maintenance tasks
2. Use Sonnet agents for implementation work
3. Use Opus agents only for architectural decisions

### Usage
```bash
/agent rust-formatter
/agent rust-developer
/agent test-specialist
/agent documentation-writer
/agent architecture-advisor
```

## Configuration

Each agent has:
- Specific responsibilities
- Limited tool permissions
- Model appropriate for task complexity
- MCP tool integration for token efficiency

## MCP Integration

Recommended MCP servers for token optimization:
- **cargo-mcp**: Secure Cargo command execution
- **rust-mcp-server**: Comprehensive Rust development tooling
- **cargo-audit**: Security vulnerability scanning
- **rust-analyzer**: Language server integration

MCP tools provide structured responses and direct tool access, reducing token consumption for routine development operations.

## Custom Commands

Available slash commands:
- **/architecture-sync**: Synchronize documentation with codebase
- **/security-audit**: Comprehensive security analysis
- **/performance-profile**: Performance analysis and profiling