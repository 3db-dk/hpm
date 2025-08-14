#!/bin/bash
set -euo pipefail

# Script to detect emoji usage in source code
# This enforces the CLAUDE.md language standard of "no emojis"

# Color codes for output
RED='\033[0;31m'
YELLOW='\033[1;33m'
GREEN='\033[0;32m'
NC='\033[0m' # No Color

# Exit codes
EXIT_SUCCESS=0
EXIT_EMOJI_FOUND=1

# Default values
VERBOSE=false
CHECK_COMMENTS=true
EXCLUDE_PATTERNS=()

usage() {
    cat <<EOF
Usage: $0 [OPTIONS] [PATH...]

Check for emoji usage in source code files.

OPTIONS:
    -h, --help              Show this help message
    -v, --verbose           Show verbose output
    --no-comments          Skip checking comments (only check string literals)
    --exclude PATTERN      Exclude files matching glob pattern (can be used multiple times)

ARGUMENTS:
    PATH                   Directory or file to check (default: current directory)

EXIT CODES:
    0                      No emojis found
    1                      Emojis detected in source code

EXAMPLES:
    $0                     Check current directory
    $0 src/                Check src directory
    $0 --exclude "*.md" .  Check all files except Markdown files
    $0 --no-comments src/  Only check string literals in src directory

EOF
}

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -h|--help)
            usage
            exit $EXIT_SUCCESS
            ;;
        -v|--verbose)
            VERBOSE=true
            shift
            ;;
        --no-comments)
            CHECK_COMMENTS=false
            shift
            ;;
        --exclude)
            if [[ $# -lt 2 ]]; then
                echo "Error: --exclude requires a pattern" >&2
                exit 1
            fi
            EXCLUDE_PATTERNS+=("$2")
            shift 2
            ;;
        --)
            shift
            break
            ;;
        -*)
            echo "Error: Unknown option $1" >&2
            usage
            exit 1
            ;;
        *)
            break
            ;;
    esac
done

# Default to current directory if no paths specified
PATHS=("${@:-.}")

# Find ripgrep executable
RG_CMD=""
if command -v rg >/dev/null 2>&1; then
    RG_CMD="rg"
elif command -v /opt/homebrew/bin/rg >/dev/null 2>&1; then
    RG_CMD="/opt/homebrew/bin/rg"
elif command -v /usr/local/bin/rg >/dev/null 2>&1; then
    RG_CMD="/usr/local/bin/rg"
elif [[ -f ~/.claude/local/node_modules/@anthropic-ai/claude-code/vendor/ripgrep/arm64-darwin/rg ]]; then
    RG_CMD="$HOME/.claude/local/node_modules/@anthropic-ai/claude-code/vendor/ripgrep/arm64-darwin/rg"
else
    echo "Error: ripgrep (rg) is required but not installed" >&2
    echo "Install it with: brew install ripgrep" >&2
    exit 1
fi

# More specific emoji detection pattern
# Focus on common emoji ranges that are likely to be used in source code
EMOJI_RANGES='[\x{1F600}-\x{1F64F}]|[\x{1F300}-\x{1F5FF}]|[\x{1F680}-\x{1F6FF}]|[\x{1F1E6}-\x{1F1FF}]|[\x{2600}-\x{26FF}]|[\x{2700}-\x{27BF}]|[\x{1F900}-\x{1F9FF}]'

# Common symbol emojis that might appear in code
SYMBOL_EMOJIS='[\x{2705}]|[\x{1F4C1}]|[\x{1F680}]|[\x{2705}]|[\x{274C}]|[\x{2139}]|[\x{2753}]|[\x{2757}]'

# Combine patterns
FULL_PATTERN="($EMOJI_RANGES)|($SYMBOL_EMOJIS)"

# File patterns to include (source code files only, not documentation)
INCLUDE_PATTERNS=(
    "*.rs"
    "*.toml"
    "*.json"
    "*.yml"
    "*.yaml"
    "*.sh"
    "*.py"
    "*.js"
    "*.ts"
    "*.html"
    "*.css"
    "*.xml"
    "*.svg"
)

# Build ripgrep arguments
RG_ARGS=(
    "--color=always"
    "--line-number"
    "--no-heading"
    "--smart-case"
    "--multiline"
)

# Add include patterns
for pattern in "${INCLUDE_PATTERNS[@]}"; do
    RG_ARGS+=("--glob" "$pattern")
done

# Add exclude patterns
if [[ ${#EXCLUDE_PATTERNS[@]} -gt 0 ]]; then
    for pattern in "${EXCLUDE_PATTERNS[@]}"; do
        RG_ARGS+=("--glob" "!$pattern")
    done
fi

# Exclude common directories that shouldn't be checked
RG_ARGS+=(
    "--glob" "!target/*"
    "--glob" "!node_modules/*"
    "--glob" "!.git/*"
    "--glob" "!*.lock"
    "--glob" "!*.log"
    "--glob" "!scripts/check-emojis.sh"  # Exclude this script itself
)

# Function to check for emojis
check_emojis() {
    local found_emojis=false
    local temp_file
    temp_file=$(mktemp)
    
    if [[ "$VERBOSE" == "true" ]]; then
        echo "Checking for emojis in: ${PATHS[*]}"
        echo "Using pattern: $FULL_PATTERN"
    fi
    
    # Run ripgrep and capture output
    if "$RG_CMD" "${RG_ARGS[@]}" "$FULL_PATTERN" "${PATHS[@]}" > "$temp_file" 2>/dev/null; then
        found_emojis=true
    fi
    
    # Process results if any emojis were found
    if [[ "$found_emojis" == "true" ]]; then
        echo -e "${RED}[ERROR] Emojis found in source code:${NC}"
        echo
        
        # Display the results (strip ANSI colors for parsing)
        cat "$temp_file" | sed 's/\x1b\[[0-9;]*m//g' | while IFS= read -r line; do
            if [[ -n "$line" ]]; then
                # Parse the ripgrep output: file:line:content
                if [[ "$line" =~ ^([^:]+):([0-9]+):(.*)$ ]]; then
                    local file="${BASH_REMATCH[1]}"
                    local line_num="${BASH_REMATCH[2]}"
                    local content="${BASH_REMATCH[3]}"
                    
                    echo -e "  ${YELLOW}$file:$line_num${NC}"
                    echo "    $content"
                    echo
                else
                    # If parsing fails, just show the line
                    echo "  $line"
                fi
            fi
        done
        
        echo -e "${RED}Error: Emojis detected in source code${NC}"
        echo "The CLAUDE.md language standards prohibit emoji usage."
        echo "Please remove all emojis from your source code."
        echo
        echo "If you need to represent emojis in documentation or examples,"
        echo "consider using their Unicode code points or textual descriptions."
        
        rm -f "$temp_file"
        return $EXIT_EMOJI_FOUND
    else
        if [[ "$VERBOSE" == "true" ]]; then
            echo -e "${GREEN}[SUCCESS] No emojis found in source code${NC}"
        fi
        rm -f "$temp_file"
        return $EXIT_SUCCESS
    fi
}

# Function to validate paths exist
validate_paths() {
    for path in "${PATHS[@]}"; do
        if [[ ! -e "$path" ]]; then
            echo "Error: Path does not exist: $path" >&2
            exit 1
        fi
    done
}

# Main execution
main() {
    validate_paths
    check_emojis
}

# Run main function
main