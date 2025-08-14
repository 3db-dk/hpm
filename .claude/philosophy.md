# HPM Development Philosophy

## Language Standards
- Use formal, concise language in all documentation and code comments
- Avoid buzzwords, marketing language, and colloquialisms
- Never use emojis in any context
- Maintain technical precision in all communications

## Research-First Approach
- Always research current ecosystem standards before implementation
- Verify best practices through authoritative sources
- Stay current with dependency updates and ecosystem changes
- Use WebSearch and WebFetch tools to validate approaches

## Forward-Only Development
- No legacy compatibility considerations
- No fallback implementations
- Users must stay current with project changes
- Dependencies must be kept up-to-date
- Implement current best practices without backward compatibility

## Cost Optimization
- Minimize token usage through MCP tool integration
- Use structured responses over raw command output
- Leverage direct tool access when available
- Optimize agent specialization for focused contexts

## Current Ecosystem Standards (2024-2025)

### Tokio
- Canonical async runtime for Rust
- Use sparingly and deliberately
- Isolate async code from domain logic
- Consider thread pools for CPU-bound workloads

### Clap 4.5+ Derive API
- Use type-driven design patterns
- Leverage documentation comments as help text
- Implement subcommands with enum derivation
- Apply attributes effectively for CLI configuration

### Serde
- Use derive macros for simplicity
- Optimize with serde_bytes for byte handling
- Apply field customization attributes
- Maintain type safety through compile-time checks

## Implementation Principles
- Research before implementation
- Use current best practices exclusively
- No backward compatibility maintenance
- MCP tool integration for efficiency
- Formal language standards throughout