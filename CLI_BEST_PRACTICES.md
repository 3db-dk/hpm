# HPM CLI Best Practices and Usage Guide

## Overview

This guide provides best practices for using HPM's improved CLI interface, error handling, and console output system. It covers both interactive use and automation scenarios.

## General Principles

### User-Friendly Defaults
- **Default behavior is optimized for interactive use**
- Colors and styling enabled automatically when appropriate
- Normal verbosity provides useful information without clutter
- Error messages include helpful suggestions when possible

### Automation Support
- **Machine-readable output available for all commands**
- Predictable exit codes following Unix conventions
- JSON output schemas for consistent programmatic parsing
- Quiet modes for script-friendly operation

## Interactive Usage

### Daily Development Workflow

#### Package Creation
```bash
# Standard package initialization
hpm init my-houdini-tool --description "Custom geometry tools"

# Minimal package for custom structures
hpm init my-minimal-tool --bare

# With specific Houdini version requirements
hpm init my-tool --houdini-min 20.0 --houdini-max 21.0
```

#### Dependency Management
```bash
# Add dependencies with version specifications
hpm add utility-nodes --version "^2.1.0"
hpm add experimental-tools --version "latest" --optional

# Remove dependencies
hpm remove outdated-package

# List current dependencies
hpm list --package /path/to/project
```

#### Error Investigation
```bash
# Verbose mode for troubleshooting
hpm --verbose install failing-package

# Full error context with colors
hpm --color always --verbose clean --dry-run

# Debug logging for complex issues
RUST_LOG=debug hpm --verbose install complex-package
```

### Verbosity Control

#### Silent Mode (`--quiet`)
- Use for: Scripts, automated workflows, minimal output needs
- Shows: Only success confirmations, warnings, and errors
- Suppresses: Info messages, debug output, progress details

```bash
# Script-friendly package installation
hpm --quiet install package-name

# Automated cleanup with minimal output
hpm --quiet clean --yes
```

#### Normal Mode (default)
- Use for: Interactive terminal sessions, regular development
- Shows: Success, info, warning, and error messages
- Suppresses: Debug details, verbose logging

```bash
# Standard interactive use
hpm init my-package
hpm add dependencies
```

#### Verbose Mode (`--verbose`)
- Use for: Debugging, troubleshooting, learning system behavior
- Shows: All message types including debug information
- Includes: Detailed operation logs, timing information

```bash
# Debug package resolution issues
hpm --verbose install complex-dependencies

# Understand cleanup decisions
hpm --verbose clean --dry-run
```

### Color Management

#### Automatic Detection (default)
```bash
# Colors enabled when terminal supports them
hpm init package-name
```

#### Force Colors
```bash
# Ensure colors even when piping through tools
hpm --color always install package | less -R
```

#### Disable Colors
```bash
# Plain text output for logging or accessibility
hpm --color never install package >> build.log
```

## Automation and Scripting

### Exit Code Handling

```bash
#!/bin/bash
# Proper exit code checking

if hpm --quiet install required-package; then
    echo "Installation successful"
else
    case $? in
        1) echo "User error - check configuration" ;;
        2) echo "Internal error - report bug" ;;
        *) echo "External command failed with code $?" ;;
    esac
    exit 1
fi
```

### JSON Output Processing

#### Basic Success Checking
```bash
# Check if command succeeded
success=$(hpm --output json --quiet install package | jq -r '.success')
if [ "$success" != "true" ]; then
    echo "Installation failed"
    exit 1
fi
```

#### Error Information Extraction
```bash
# Extract error details for logging
hpm --output json install failing-package 2>&1 | \
jq -r 'if .success then "Success" else "Error: " + .error end'
```

#### Streaming Processing
```bash
# Process cleanup results line by line
hpm --output json-lines clean --comprehensive --dry-run | \
while IFS= read -r line; do
    package=$(echo "$line" | jq -r '.packages_removed[]? // empty')
    if [ -n "$package" ]; then
        echo "Would remove: $package"
    fi
done
```

### CI/CD Integration

#### GitHub Actions Example
```yaml
- name: Install HPM Dependencies
  run: |
    if ! hpm --output json --quiet install; then
      echo "::error::HPM dependency installation failed"
      exit 1
    fi
    
    # Extract and display installation summary
    hpm --output json list | jq -r '.dependencies[].name' | \
      sed 's/^/::notice::Installed dependency: /'
```

#### Jenkins Pipeline Example
```groovy
stage('HPM Dependencies') {
    steps {
        script {
            def result = sh(
                script: 'hpm --output json-compact --quiet install',
                returnStdout: true
            ).trim()
            
            def json = readJSON text: result
            if (!json.success) {
                error("HPM installation failed: ${json.error}")
            }
            
            echo "HPM installation completed in ${json.elapsed_ms}ms"
        }
    }
}
```

### Monitoring and Logging

#### Structured Logging
```bash
# Generate structured logs for analysis
hpm --output json-lines --quiet clean --yes 2>&1 | \
jq -c '. + {timestamp: now, hostname: env.HOSTNAME}' >> hpm-operations.jsonl
```

#### Performance Monitoring
```bash
# Track command performance over time
{
    echo "timestamp,command,success,elapsed_ms"
    hpm --output json --quiet install package 2>&1 | \
    jq -r '[now, .command, .success, .elapsed_ms] | @csv'
} >> hpm-performance.csv
```

#### Error Aggregation
```bash
# Collect and analyze error patterns
hpm --output json install package 2>&1 | \
jq -r 'select(.success == false) | [.error_type, .error] | @tsv' >> error-log.txt
```

## Advanced Usage Patterns

### Conditional Operations

#### Dependency Existence Check
```bash
# Check if package is already installed before adding
if ! hpm --quiet --output json list | jq -e '.dependencies[] | select(.name == "target-package")' > /dev/null; then
    hpm add target-package
else
    echo "Package already installed"
fi
```

#### Environment-Specific Configuration
```bash
# Different behavior based on environment
case "${NODE_ENV:-development}" in
    "production")
        hpm --quiet --color never install --no-dev-dependencies
        ;;
    "development")
        hpm --verbose install --include-optional
        ;;
esac
```

### Batch Operations

#### Multiple Package Management
```bash
# Install multiple packages with error handling
packages=("geometry-tools" "material-library" "animation-utils")
failed_packages=()

for package in "${packages[@]}"; do
    if ! hpm --quiet add "$package"; then
        failed_packages+=("$package")
    fi
done

if [ ${#failed_packages[@]} -gt 0 ]; then
    echo "Failed to install: ${failed_packages[*]}"
    exit 1
fi
```

#### Parallel Processing
```bash
# Process multiple projects concurrently
projects=("/path/to/project1" "/path/to/project2" "/path/to/project3")

for project in "${projects[@]}"; do
    (
        cd "$project"
        hpm --quiet --output json install > "${project##*/}-install.json" 2>&1
    ) &
done

wait
echo "All installations complete"
```

### Integration with External Tools

#### Package Validation
```bash
# Validate packages before deployment
hpm --output json list | \
jq -r '.dependencies[].name' | \
while read package; do
    if ! validate_package "$package"; then
        echo "Package validation failed: $package"
        exit 1
    fi
done
```

#### Dependency Analysis
```bash
# Generate dependency reports
hpm --output json list | \
jq -r '.dependencies[] | [.name, .version, .optional] | @csv' > dependencies.csv

# Create dependency graph data
hpm --output json list | \
jq '.dependencies[] | {name: .name, version: .version, type: "hpm"}' | \
jq -s '.' > dependency-graph.json
```

## Error Handling Best Practices

### Graceful Degradation

```bash
#!/bin/bash
# Example of graceful error handling

install_package() {
    local package=$1
    local required=${2:-true}
    
    if hpm --quiet add "$package"; then
        echo "✓ Installed $package"
        return 0
    elif [ "$required" = "false" ]; then
        echo "⚠ Optional package $package failed to install, continuing..."
        return 0
    else
        echo "✗ Failed to install required package $package"
        return 1
    fi
}

# Install required packages
install_package "core-tools" true || exit 1

# Install optional packages
install_package "experimental-features" false
```

### User-Friendly Error Messages

```bash
# Provide helpful context in error messages
install_with_help() {
    local package=$1
    
    if ! hpm --quiet add "$package"; then
        echo "Failed to install $package"
        echo "Try:"
        echo "  1. Check package name with: hpm search $package"
        echo "  2. Verify network connection"
        echo "  3. Check HPM configuration with: hpm check"
        return 1
    fi
}
```

### Error Recovery

```bash
# Implement retry logic for transient failures
retry_install() {
    local package=$1
    local max_attempts=3
    local attempt=1
    
    while [ $attempt -le $max_attempts ]; do
        if hpm --quiet add "$package"; then
            return 0
        fi
        
        echo "Attempt $attempt failed, retrying in 5 seconds..."
        sleep 5
        attempt=$((attempt + 1))
    done
    
    echo "Failed to install $package after $max_attempts attempts"
    return 1
}
```

## Performance Optimization

### Efficient Command Usage

#### Batch Operations
```bash
# More efficient than individual commands
hpm add package1 package2 package3  # Future feature
# vs
hpm add package1 && hpm add package2 && hpm add package3
```

#### Parallel Execution
```bash
# Use background processes for independent operations
hpm --quiet clean --python-only &
hpm --quiet check &
wait
```

### Output Optimization

#### Minimal Output for Performance
```bash
# Use compact JSON for network efficiency
result=$(hpm --output json-compact --quiet install package)

# Use quiet mode to reduce I/O overhead
hpm --quiet --color never install package >> install.log
```

## Security Considerations

### Safe Scripting Practices

```bash
# Always validate package names
validate_package_name() {
    local package=$1
    if [[ ! "$package" =~ ^[a-zA-Z0-9_-]+$ ]]; then
        echo "Invalid package name: $package"
        return 1
    fi
}

# Use proper quoting
hpm add "$USER_PROVIDED_PACKAGE"  # Good
hpm add $USER_PROVIDED_PACKAGE   # Dangerous
```

### Credential Handling

```bash
# Never log commands with credentials
HPM_TOKEN="secret" hpm publish  # Token not visible in process list

# Use environment files for automation
export $(cat .env | xargs)
hpm publish
```

## Troubleshooting

### Common Issues and Solutions

#### Permission Errors
```bash
# Check file permissions
ls -la ~/.hpm/
# Solution: Fix ownership or run with appropriate permissions
```

#### Network Issues
```bash
# Test connectivity
hpm --verbose install test-package
# Check proxy settings and network configuration
```

#### Configuration Problems
```bash
# Validate configuration
hpm check
# Reset to defaults if needed
```

### Debug Information Collection

```bash
# Comprehensive debug information for bug reports
{
    echo "HPM Version:"
    hpm --version
    
    echo -e "\nSystem Information:"
    uname -a
    
    echo -e "\nEnvironment:"
    env | grep HPM
    
    echo -e "\nConfiguration:"
    hpm --output json check 2>&1 || echo "Check failed"
    
    echo -e "\nError Details:"
    hpm --verbose --output json failing-command 2>&1
} > debug-info.txt
```

This guide provides a comprehensive foundation for using HPM's improved CLI interface effectively in both interactive and automated scenarios.