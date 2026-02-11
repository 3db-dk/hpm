#!/bin/bash

# Install Git hooks for HPM project
# This script sets up pre-commit hooks for code quality

set -euo pipefail

HOOKS_DIR=".git/hooks"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "Installing Git hooks for HPM project..."

# Create hooks directory if it doesn't exist
mkdir -p "$HOOKS_DIR"

# Install pre-commit hook
cat > "$HOOKS_DIR/pre-commit" << 'EOF'
#!/bin/bash

# HPM Pre-commit hook
# Runs quality checks before allowing commits

set -euo pipefail

echo "Running pre-commit quality checks..."

# Check if just is available, fallback to direct cargo commands
if command -v just >/dev/null 2>&1; then
    # Run just pre-commit target
    if ! just pre-commit; then
        echo "[ERROR] Pre-commit checks failed!"
        echo "Please fix the issues above before committing."
        exit 1
    fi
else
    # Fallback to direct cargo commands
    echo "just not found, running cargo commands directly..."
    if ! cargo fmt --all -- --check; then
        echo "[ERROR] Code formatting check failed!"
        echo "Run 'cargo fmt --all' to fix formatting issues."
        exit 1
    fi
    
    if ! cargo clippy --workspace --all-targets -- -D warnings; then
        echo "[ERROR] Clippy linting failed!"
        echo "Fix clippy warnings before committing."
        exit 1
    fi
    
    if ! python3 scripts/check-emojis.py; then
        echo "[ERROR] Emoji check failed!"
        echo "Remove emojis from source code before committing."
        exit 1
    fi
    
    if ! cargo test --workspace; then
        echo "[ERROR] Tests failed!"
        echo "Fix failing tests before committing."
        exit 1
    fi
fi

echo "[SUCCESS] Pre-commit checks passed!"
EOF

# Make hook executable
chmod +x "$HOOKS_DIR/pre-commit"

# Install commit-msg hook for conventional commits
cat > "$HOOKS_DIR/commit-msg" << 'EOF'
#!/bin/bash

# Validate commit message format
# Enforces conventional commit format: type(scope): description

commit_regex='^(feat|fix|docs|style|refactor|test|chore|perf|ci|build|revert)(\(.+\))?: .{1,50}'

if ! grep -qE "$commit_regex" "$1"; then
    echo "[ERROR] Invalid commit message format!"
    echo "Please use conventional commit format:"
    echo "  type(scope): description"
    echo ""
    echo "Types: feat, fix, docs, style, refactor, test, chore, perf, ci, build, revert"
    echo "Example: feat(cli): add install command"
    exit 1
fi
EOF

# Make hook executable
chmod +x "$HOOKS_DIR/commit-msg"

echo "[SUCCESS] Git hooks installed successfully!"
echo ""
echo "Hooks installed:"
echo "  - pre-commit: Runs quality checks (fmt, clippy, emoji check, tests)"
echo "  - commit-msg: Validates conventional commit format"
echo ""
echo "To bypass hooks (not recommended): git commit --no-verify"