# HPM Improved Error Handling and Console Output

## Overview

HPM's CLI has been significantly improved with UV-inspired error handling, console styling, and machine-readable output capabilities. This follows industry best practices for professional command-line tools.

## New Features

### 1. Console Styling and Colors

- **Styled Output**: Success messages in green, errors in red, info in blue, warnings in yellow
- **Smart Color Detection**: Automatically detects terminal capability and user preferences  
- **Verbosity Control**: `--quiet`, `--verbose` flags control output levels
- **Color Control**: `--color auto|always|never` for explicit color management

Example:
```bash
hpm init my-package --description "Test package"
# ✓ Package 'my-package' initialized successfully (in green)

hpm init existing-dir
# ✗ error: Package error: Directory 'existing-dir' already exists (in red)
```

### 2. Machine-Readable Output

- **JSON Output**: `--output json` for structured data consumption
- **JSON Lines**: `--output json-lines` for streaming applications  
- **JSON Compact**: `--output json-compact` for minimal bandwidth
- **Programmatic Integration**: Exit codes and structured error information

Example:
```bash
hpm --output json init existing-dir
{
  "success": false,
  "error": "Package error",
  "error_type": "package", 
  "elapsed_ms": 12
}
```

### 3. Structured Error Handling

- **Error Categories**: Config, Package, Network, I/O, Internal, External
- **Exit Codes**: Following Unix conventions (0=success, 1=user error, 2=internal error)
- **Contextual Help**: Helpful suggestions for common error scenarios
- **Error Chains**: Proper error propagation with root cause analysis

### 4. Verbosity Levels

- **Silent**: Only critical errors (`--quiet` when you need minimal output)
- **Quiet**: Warnings, errors, essential info (`--quiet`)
- **Normal**: Standard output level (default)
- **Verbose**: Debug information included (`--verbose`)

### 5. Professional CLI Features

- **Progress Indicators**: Foundation for future progress bars
- **User Interaction**: Confirmation prompts and input handling
- **Tree Display**: Formatted directory tree output
- **Consistent Styling**: Professional appearance across all commands

## Implementation Details

### Architecture

The system is built on three main modules:

1. **console.rs**: Terminal styling, colors, verbosity control
2. **error.rs**: Structured error types, exit codes, error reporting  
3. **output.rs**: Machine-readable output formats

### Dependencies Added

Following UV's approach, we've added:
- `anstream`: Cross-platform terminal output
- `console`: Terminal interaction utilities
- `owo-colors`: Terminal colors and styling
- `indicatif`: Progress indicators (ready for future use)
- `miette`: Fancy error diagnostics (foundation for enhanced error reporting)

### Error Types

```rust
pub enum CliError {
    Config { source: anyhow::Error, help: Option<String> },
    Package { source: anyhow::Error, help: Option<String> },
    Network { source: anyhow::Error, help: Option<String> },
    Io { source: anyhow::Error, help: Option<String> },
    Internal { source: anyhow::Error, help: Option<String> },
    External { command: String, exit_code: u8, help: Option<String> },
}
```

### Console Output Levels

```rust
pub enum Verbosity {
    Silent,   // Critical errors only
    Quiet,    // Warnings, errors, essential info  
    Normal,   // Standard output (default)
    Verbose,  // Include debug information
}
```

## Usage Examples

### Basic Usage with Styling
```bash
# Standard command with colored output
hpm init my-package

# Quiet mode - minimal output
hpm --quiet init my-package

# Verbose mode - detailed information  
hpm --verbose init my-package

# Disable colors
hpm --color never init my-package
```

### Machine-Readable Output
```bash
# JSON output for automation
hpm --output json list --package /path/to/project

# JSON Lines for streaming
hpm --output json-lines search "geometry tools"

# Compact JSON for APIs
hpm --output json-compact install --manifest project.toml
```

### Error Handling Examples
```bash
# Helpful error messages
hpm init existing-directory
# error: Package error: Directory 'existing-directory' already exists
#   help: Choose a different name or remove the existing directory

# JSON error output  
hpm --output json init existing-directory
{
  "success": false,
  "error": "Package error", 
  "error_type": "package",
  "elapsed_ms": 8
}
```

## Future Enhancements

The foundation is now in place for:

1. **Progress Bars**: Using `indicatif` for long-running operations
2. **Enhanced Diagnostics**: Using `miette` for source-code-like error reporting
3. **Interactive Prompts**: User confirmations and input handling
4. **Structured Logging**: JSON-formatted logs for debugging
5. **Machine-Readable Commands**: Full JSON input/output for all operations

## Compatibility

- **Backward Compatible**: All existing commands work unchanged
- **Terminal Agnostic**: Works on Windows, macOS, Linux terminals
- **CI/CD Friendly**: Proper exit codes and machine-readable output
- **Accessibility**: Color-blind friendly with shape indicators (✓, ✗, ⚠, ℹ)

## Benefits

1. **Developer Experience**: Professional, polished CLI interface
2. **Automation**: Machine-readable output for scripts and CI/CD
3. **Debugging**: Structured errors with helpful context
4. **Performance**: Proper verbosity controls reduce noise
5. **Standards Compliance**: Follows Unix CLI conventions

This improvement brings HPM up to modern CLI tool standards, matching the quality and user experience of tools like `uv`, `cargo`, and other professional package managers.