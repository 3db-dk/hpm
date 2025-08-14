# MCP Integration for HPM Development

## Overview

Model Context Protocol (MCP) servers provide direct tool access to reduce token usage and improve development efficiency. These servers execute Rust development tasks locally rather than requiring LLM interpretation of command outputs.

## Recommended MCP Servers

### cargo-mcp
**Purpose**: Secure Cargo command execution
**Installation**: `cargo install cargo-mcp`
**Benefits**:
- Direct Cargo operation access (check, test, build, add, remove, update)
- Path validation for security
- No arbitrary command execution
- Reduces token overhead for build operations

### rust-mcp-server  
**Purpose**: Comprehensive Rust development tooling
**Benefits**:
- Automated dependency management
- Code quality checks (clippy, fmt)
- Security advisory scanning
- Compiler error analysis
- License compliance validation

## Configuration

Add to Claude Desktop configuration:
```json
{
  "mcpServers": {
    "cargo-mcp": {
      "command": "cargo-mcp",
      "args": ["serve"]
    },
    "rust-mcp-server": {
      "command": "rust-mcp-server",
      "args": ["--timeout", "30"]
    }
  }
}
```

## Token Optimization Benefits

**Direct Tool Access**: MCP servers execute commands without requiring LLM interpretation of outputs, reducing token consumption for routine operations.

**Structured Responses**: MCP tools return structured data rather than raw command output, minimizing context requirements.

**Error Context**: Tools provide focused error information rather than full command output, reducing noise in conversations.

## Integration with HPM Agents

- **rust-formatter**: Benefits from cargo-mcp for fmt operations
- **rust-developer**: Uses rust-mcp-server for comprehensive tooling
- **test-specialist**: Leverages cargo-mcp for test execution
- **architecture-advisor**: Accesses cargo metadata through MCP tools

## Implementation

MCP servers operate independently of Claude Code agents but provide the underlying tool layer that agents can leverage to reduce token usage while maintaining development workflow efficiency.