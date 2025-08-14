# HPM Development Justfile
# Run 'just --list' to see all available commands

# Default recipe shows help
default:
    @just --list

# === BUILD COMMANDS ===

# Build the project
build:
    cargo build

# Build for release
build-release:
    cargo build --release

# Check code without building  
check:
    cargo check --workspace --all-targets --all-features

# === TESTING COMMANDS ===

# Run all tests
test:
    cargo test --workspace --all-features --package hpm-cli -- --test-threads=1
    cargo test --workspace --all-features --exclude hpm-cli

# Run tests with output
test-verbose:
    cargo test --workspace --all-features --package hpm-cli -- --test-threads=1 --nocapture
    cargo test --workspace --all-features --exclude hpm-cli -- --nocapture

# Run integration tests only
test-integration:
    cargo test --workspace --test '*'

# === CODE QUALITY ===

# Format code
fmt:
    cargo fmt --all

# Check formatting without changing files
fmt-check:
    cargo fmt --all -- --check

# Run clippy lints
clippy:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Check for emoji usage in source code
check-emojis:
    python3 scripts/check-emojis.py

# Fix clippy issues automatically where possible
clippy-fix:
    cargo clippy --workspace --all-targets --all-features --fix

# === UTILITY COMMANDS ===

# Clean build artifacts
clean:
    cargo clean

# Install the HPM binary
install:
    cargo install --path crates/hpm-cli --force

# === DEVELOPMENT WORKFLOW ===

# Development workflow: format, lint, and test
dev: fmt clippy check-emojis test

# Run all quality checks (CI equivalent)
quality: fmt-check clippy check-emojis test
    @echo "✅ All quality checks passed!"

# Pre-commit checks (used by git hooks)
pre-commit: quality audit machete
    @echo "✅ Pre-commit checks completed successfully!"

# === DEPENDENCIES ===

# Update dependencies
update:
    cargo update

# Security audit
audit:
    cargo audit

# Find unused dependencies
machete:
    cargo machete

# === DOCUMENTATION ===

# Generate and open documentation
doc:
    cargo doc --workspace --all-features --no-deps --open

# Check documentation builds
doc-check:
    cargo doc --workspace --all-features --no-deps

# === RELEASE PREPARATION ===

# Run comprehensive release checks
release-check: quality doc-check audit
    cargo build --release
    @echo "✅ Release checks completed!"

# === DEVELOPMENT TOOLS ===

# Install development tools
install-tools:
    cargo install cargo-audit cargo-machete cargo-watch cargo-nextest just

# Watch for changes and run tests
watch:
    cargo watch -x test

# Watch for changes and run quality checks
watch-quality:
    cargo watch -x "clippy --workspace --all-targets --all-features"

# === GIT HOOKS ===

# Install git hooks
install-hooks:
    ./scripts/install-git-hooks.sh

# === MCP SERVERS ===

# List all configured MCP servers with health status
mcp-status:
    ~/.claude/local/claude mcp list

# Show MCP server details
mcp-info server:
    ~/.claude/local/claude mcp get {{server}}

# Add a new MCP server
mcp-add name command *args:
    ~/.claude/local/claude mcp add {{name}} {{command}} {{args}}

# Remove an MCP server  
mcp-remove name:
    ~/.claude/local/claude mcp remove {{name}}

# Reset MCP configuration for project
mcp-reset:
    ~/.claude/local/claude mcp reset-project-choices

# === PROJECT MANAGEMENT ===

# Create a new crate in the workspace
new-crate name:
    mkdir -p crates/hpm-{{name}}/src
    echo '[package]\nname = "hpm-{{name}}"\nversion.workspace = true\nauthors.workspace = true\nedition.workspace = true\nlicense.workspace = true\nrepository.workspace = true\nhomepage.workspace = true\ndocumentation.workspace = true\nkeywords.workspace = true\ncategories.workspace = true\nrust-version.workspace = true\ndescription = "{{name}} functionality for HPM (Houdini Package Manager)"\n\n[dependencies]\n# Add dependencies as needed' > crates/hpm-{{name}}/Cargo.toml
    echo '// TODO: Implement hpm-{{name}} functionality' > crates/hpm-{{name}}/src/lib.rs
    @echo "✅ Created new crate: hpm-{{name}}"
    @echo "Don't forget to add it to the workspace members in Cargo.toml!"

# Show workspace tree
tree:
    @echo "HPM Workspace Structure:"
    @echo "========================"
    @find . -name "*.toml" -not -path "./target/*" | head -20 | sort

# Show current project statistics  
stats:
    @echo "HPM Project Statistics:"
    @echo "======================"
    @echo "Lines of Rust code:"
    @find . -name "*.rs" -not -path "./target/*" | xargs wc -l | tail -1
    @echo "Number of crates:"
    @find ./crates -name "Cargo.toml" | wc -l | xargs
    @echo "Dependencies:"
    @cargo tree --workspace --depth 1 | grep -v "│" | grep -v "├─" | grep -v "└─" | wc -l | xargs