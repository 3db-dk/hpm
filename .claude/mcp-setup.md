# HPM MCP Server Setup

This document describes the Model Context Protocol (MCP) servers configured for the HPM project development.

## Overview

MCP servers extend Claude Code's capabilities by providing access to external tools and services. The following servers are configured for optimal HPM development:

## Configured MCP Servers

### ✅ Filesystem Server
**Purpose**: File system operations and project file management  
**Status**: ✓ Connected  
**Command**: `npx @modelcontextprotocol/server-filesystem /Users/soren-n/Documents/workspace/hpm`

**Capabilities**:
- Read and write files within the project directory
- Search for files by name or content
- Directory operations and file management
- Safe, sandboxed access to project files only

**Usage Examples**:
- `@filesystem` - Access filesystem resources
- File operations for code generation and editing
- Project structure analysis and modification

### ✅ GitHub Server  
**Purpose**: GitHub API integration for repository management  
**Status**: ✓ Connected  
**Command**: `npx @modelcontextprotocol/server-github`

**Capabilities**:
- Read and manage GitHub issues and pull requests
- Analyze repository activity and commits  
- Trigger workflows and manage releases
- Repository metadata and collaboration features

**Usage Examples**:
- Issue tracking and management
- Pull request analysis and creation
- Release automation
- Repository analytics

### ✅ Sequential Thinking Server
**Purpose**: Complex task breakdown and systematic problem solving  
**Status**: ✓ Connected  
**Command**: `npx @modelcontextprotocol/server-sequential-thinking`

**Capabilities**:
- Break down complex development tasks into logical steps
- Structured problem-solving approach
- Multi-step workflow planning
- Systematic code architecture decisions

**Usage Examples**:
- `/thinking` - Start structured thinking process
- Complex refactoring planning
- Architecture decision documentation
- Step-by-step implementation guides

### ✅ PostgreSQL Server
**Purpose**: Database operations for package registry development  
**Status**: ✓ Connected  
**Command**: `npx @modelcontextprotocol/server-postgres postgresql://localhost:5432`

**Capabilities**:
- Database schema design and management
- Query execution and optimization
- Data analysis and reporting
- Package registry database operations

**Usage Examples**:
- Package metadata storage design
- Registry database schema creation
- Query performance optimization
- Data migration and maintenance

## Configuration Details

### Local Configuration
All MCP servers are configured at the **local scope**, meaning they are:
- Private to the HPM project
- Stored in project-specific settings
- Not shared across other projects

### Security Considerations
- **Filesystem server**: Limited to project directory only
- **GitHub server**: Requires proper authentication for write operations  
- **Database server**: Uses local PostgreSQL connection
- All servers run with user account permissions

## Troubleshooting

### Server Connection Issues
If a server shows as disconnected:

1. Check server status: `claude mcp list`
2. Get server details: `claude mcp get <server-name>`  
3. Remove and re-add if needed: `claude mcp remove <server-name>` then `claude mcp add ...`

### Common Issues
- **NPX not found**: Ensure Node.js and npm are installed
- **Permission errors**: Check directory access permissions
- **Database connection**: Verify PostgreSQL is running locally
- **GitHub API limits**: Check authentication and rate limits

## Adding New Servers

To add additional MCP servers for the HPM project:

```bash
# Generic format
claude mcp add <name> <command> [args...]

# Examples
claude mcp add sqlite npx @modelcontextprotocol/server-sqlite /path/to/database.db
claude mcp add web npx @modelcontextprotocol/server-web-search
```

## Benefits for HPM Development

### Package Management Development
- **Filesystem**: Direct access to package files and configurations
- **GitHub**: Integration with package repositories and releases
- **Database**: Registry data management and queries
- **Sequential**: Complex dependency resolution planning

### Quality Assurance
- File-based test data management
- Repository-based CI/CD integration  
- Database-driven testing scenarios
- Systematic code review processes

### Collaboration
- GitHub issue and PR management
- Shared problem-solving workflows
- Database schema collaboration
- File-based documentation management

## Maintenance

### Regular Tasks
- Monitor server health: `claude mcp list`
- Update server packages as needed
- Review and rotate authentication tokens
- Clean up unused servers

### Performance
- All servers use async I/O for optimal performance
- Filesystem operations are sandboxed for security
- Database connections are pooled efficiently
- GitHub API calls are rate-limited appropriately

---

Last Updated: $(date '+%Y-%m-%d')  
Project: HPM (Houdini Package Manager)  
Configuration File: `/Users/soren-n/.claude.json`