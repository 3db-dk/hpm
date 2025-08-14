#!/usr/bin/env python3
"""
Emoji detection script for HPM project.

This script scans source code files for emoji usage and reports violations
according to the CLAUDE.md language standards which prohibit emoji usage
in source code.
"""

import argparse
import re
import sys
from pathlib import Path
from typing import List, Set, Tuple


# Comprehensive Unicode emoji ranges based on Unicode 16.0
EMOJI_PATTERN = re.compile(
    r'[\U0001F1E0-\U0001F1FF]|'  # Flags (Regional Indicator Symbols)
    r'[\U0001F300-\U0001F5FF]|'  # Miscellaneous Symbols and Pictographs
    r'[\U0001F600-\U0001F64F]|'  # Emoticons
    r'[\U0001F680-\U0001F6FF]|'  # Transport and Map Symbols
    r'[\U0001F700-\U0001F77F]|'  # Alchemical Symbols
    r'[\U0001F780-\U0001F7FF]|'  # Geometric Shapes Extended
    r'[\U0001F800-\U0001F8FF]|'  # Supplemental Arrows-C
    r'[\U0001F900-\U0001F9FF]|'  # Supplemental Symbols and Pictographs
    r'[\U0001FA00-\U0001FA6F]|'  # Chess Symbols
    r'[\U0001FA70-\U0001FAFF]|'  # Symbols and Pictographs Extended-A
    r'[\U0001FB00-\U0001FBFF]|'  # Symbols for Legacy Computing
    r'[\U00002600-\U000026FF]|'  # Miscellaneous Symbols
    r'[\U00002700-\U000027BF]|'  # Dingbats
    r'[\U0000FE00-\U0000FE0F]|'  # Variation Selectors
    r'[\U0001F3FB-\U0001F3FF]|'  # Skin tone modifiers
    r'[\U000020E3]|'             # Combining Enclosing Keycap
    r'[\U0000FE0F]|'             # Variation Selector-16 (emoji style)
    r'[\U0000200D]'              # Zero Width Joiner (for complex emoji)
)


def get_supported_extensions() -> Set[str]:
    """Return set of supported file extensions for source code."""
    return {
        '.rs', '.toml', '.json', '.yml', '.yaml', '.sh', '.py', 
        '.js', '.ts', '.html', '.css', '.xml', '.svg', '.c', '.cpp',
        '.h', '.hpp', '.java', '.go', '.rb', '.php', '.swift', '.kt'
    }


def should_check_file(file_path: Path, exclude_patterns: List[str]) -> bool:
    """Determine if a file should be checked for emojis."""
    
    # Check if file extension is supported
    if file_path.suffix not in get_supported_extensions():
        return False
    
    # Skip documentation files
    if file_path.suffix in {'.md', '.txt', '.rst'}:
        return False
    
    # Skip common directories
    parts = file_path.parts
    skip_dirs = {'target', 'node_modules', '.git', 'build', 'dist', '.cache'}
    if any(part in skip_dirs for part in parts):
        return False
    
    # Skip this script itself
    if file_path.name == 'check-emojis.py':
        return False
    
    # Check exclude patterns
    for pattern in exclude_patterns:
        if file_path.match(pattern):
            return False
    
    return True


def find_emojis_in_file(file_path: Path) -> List[Tuple[int, str]]:
    """Find emoji occurrences in a file.
    
    Returns:
        List of tuples: (line_number, line_content)
    """
    try:
        with open(file_path, 'r', encoding='utf-8', errors='ignore') as f:
            lines = f.readlines()
    except (IOError, OSError) as e:
        print(f"Warning: Could not read {file_path}: {e}", file=sys.stderr)
        return []
    
    emoji_lines = []
    
    for line_num, line in enumerate(lines, 1):
        if EMOJI_PATTERN.search(line):
            emoji_lines.append((line_num, line.rstrip()))
    
    return emoji_lines


def scan_directory(
    directory: Path, 
    exclude_patterns: List[str], 
    verbose: bool = False
) -> List[Tuple[Path, List[Tuple[int, str]]]]:
    """Scan directory for emoji usage in source files.
    
    Returns:
        List of tuples: (file_path, emoji_occurrences)
    """
    results = []
    
    if verbose:
        print(f"Scanning directory: {directory}")
    
    for file_path in directory.rglob('*'):
        if file_path.is_file() and should_check_file(file_path, exclude_patterns):
            if verbose:
                print(f"Checking: {file_path}")
            
            emoji_occurrences = find_emojis_in_file(file_path)
            if emoji_occurrences:
                results.append((file_path, emoji_occurrences))
    
    return results


def scan_file(file_path: Path) -> List[Tuple[Path, List[Tuple[int, str]]]]:
    """Scan a single file for emoji usage.
    
    Returns:
        List with single tuple: (file_path, emoji_occurrences) if emojis found
    """
    if not file_path.exists():
        print(f"Error: File does not exist: {file_path}", file=sys.stderr)
        return []
    
    emoji_occurrences = find_emojis_in_file(file_path)
    return [(file_path, emoji_occurrences)] if emoji_occurrences else []


def format_results(results: List[Tuple[Path, List[Tuple[int, str]]]]) -> None:
    """Format and display emoji detection results."""
    if not results:
        print("\u001b[32m[SUCCESS] No emojis found in source code\u001b[0m")
        return
    
    print("\u001b[31m[ERROR] Emojis found in source code:\u001b[0m")
    print()
    
    for file_path, emoji_occurrences in results:
        for line_num, line_content in emoji_occurrences:
            print(f"  \u001b[33m{file_path}:{line_num}\u001b[0m")
            # Show the line content directly without complex highlighting to avoid issues
            print(f"    {line_content}")
            print()
    
    print("\u001b[31m[ERROR] Emojis detected in source code\u001b[0m")
    print("The CLAUDE.md language standards prohibit emoji usage.")
    print("Please remove all emojis from your source code.")
    print()
    print("If you need to represent emojis in documentation or examples,")
    print("consider using their Unicode code points or textual descriptions.")


def main():
    """Main entry point for the emoji detection script."""
    parser = argparse.ArgumentParser(
        description="Check for emoji usage in source code files",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  %(prog)s                     Check current directory
  %(prog)s src/                Check src directory
  %(prog)s --exclude "*.md" .  Check all files except Markdown files
  %(prog)s --verbose src/      Check src directory with verbose output
  %(prog)s file.rs             Check a single file
        """
    )
    
    parser.add_argument(
        'paths',
        nargs='*',
        default=['.'],
        help='Directory or file paths to check (default: current directory)'
    )
    
    parser.add_argument(
        '-v', '--verbose',
        action='store_true',
        help='Show verbose output'
    )
    
    parser.add_argument(
        '--exclude',
        action='append',
        default=[],
        dest='exclude_patterns',
        help='Exclude files matching glob pattern (can be used multiple times)'
    )
    
    args = parser.parse_args()
    
    if args.verbose:
        print("Emoji Detection Script for HPM Project")
        print("======================================")
        print(f"Checking paths: {args.paths}")
        if args.exclude_patterns:
            print(f"Excluding patterns: {args.exclude_patterns}")
        print()
    
    all_results = []
    
    for path_str in args.paths:
        path = Path(path_str).resolve()
        
        if not path.exists():
            print(f"Error: Path does not exist: {path}", file=sys.stderr)
            continue
        
        if path.is_file():
            results = scan_file(path)
        else:
            results = scan_directory(path, args.exclude_patterns, args.verbose)
        
        all_results.extend(results)
    
    format_results(all_results)
    
    # Exit with error code if emojis were found
    if all_results:
        sys.exit(1)
    
    if args.verbose:
        print("Emoji check completed successfully!")


if __name__ == '__main__':
    main()