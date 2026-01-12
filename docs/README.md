# HPM Documentation

Welcome to the comprehensive documentation for HPM (Houdini Package Manager) - a modern, Rust-based package management system for SideFX Houdini.

## Quick Navigation

### New to HPM?
Start with the **[User Guide](user-guide.md)** for installation and basic usage.

### Want to Contribute?
Read the **[Developer Documentation](developer-documentation.md)** and **[Testing Guide](testing.md)**.

### Need Command Help?
Check the **[CLI Design](cli-design.md)** for detailed command documentation.

### Working with Python?
See the **[Python Guide](python-guide.md)** for Python dependency management.

## Documentation Overview

### For Users
- **[User Guide](user-guide.md)** - Complete user documentation covering installation, usage, and troubleshooting
- **[CLI Design](cli-design.md)** - Command-line interface design and command reference
- **[Python Guide](python-guide.md)** - Managing Python dependencies in Houdini packages
- **[Tutorials & Examples](tutorials-and-examples.md)** - Step-by-step guides and real-world scenarios

### For Developers
- **[Developer Documentation](developer-documentation.md)** - Architecture overview, development setup, and contribution guidelines
- **[API Reference](api-reference.md)** - Complete API documentation for all public interfaces
- **[Testing Guide](testing.md)** - Comprehensive testing guide including property-based testing

### Technical Documentation
- **[Technical Architecture](technical-architecture.md)** - High-level system architecture and design decisions
- **[System Deep Dives](system-deep-dives.md)** - Detailed explanations of complex systems
- **[Cleanup System](cleanup-system.md)** - Project-aware cleanup with orphan detection
- **[Update Command](update-command.md)** - Package update system design

### Archived Documentation
Documentation for planned but not yet implemented features:
- **[Archive](archive/)** - Registry system design docs and historical implementation notes

## Documentation Structure

```
docs/
├── README.md                           # This overview
├── user-guide.md                       # Complete user documentation
├── developer-documentation.md          # Developer documentation
├── api-reference.md                    # API documentation
├── cli-design.md                       # CLI design and command reference
├── tutorials-and-examples.md           # Step-by-step tutorials
├── technical-architecture.md           # System architecture
├── system-deep-dives.md                # Detailed system explanations
├── python-guide.md                     # Python dependency management
├── cleanup-system.md                   # Cleanup system details
├── update-command.md                   # Update system design
├── testing.md                          # Comprehensive testing guide
└── archive/                            # Archived/future feature docs
    ├── README.md
    ├── registry-architecture.md
    ├── registry-overview.md
    ├── registry-implementation-plan.md
    ├── registry-system-summary.md
    └── update-implementation-summary.md
```

## Documentation Standards

All HPM documentation follows these standards:
- **Accuracy**: All examples are tested and verified
- **Completeness**: Comprehensive coverage of features and use cases
- **Accessibility**: Clear language suitable for different experience levels
- **Maintainability**: Regular updates to reflect current implementation

## Contributing to Documentation

Documentation improvements are welcome! See the **[Developer Documentation](developer-documentation.md)** for contribution guidelines.

For questions or suggestions about documentation, please open an issue on GitHub.
