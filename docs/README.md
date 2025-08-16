# HPM Documentation

Welcome to the comprehensive documentation for HPM (Houdini Package Manager) - a modern, Rust-based package management system for SideFX Houdini.

## 📚 Documentation Overview

### 🎯 For Users
- **[User Guide](user-guide.md)** - Complete user documentation covering installation, usage, and troubleshooting
- **[CLI Reference](cli-reference.md)** - Comprehensive reference for all CLI commands and options
- **[Python User Guide](python-user-guide.md)** - Managing Python dependencies in Houdini packages
- **[Tutorials](tutorials.md)** - Step-by-step guides for common workflows

### 👨‍💻 For Developers
- **[Developer Guide](developer-guide.md)** - Architecture overview, development setup, and contribution guidelines
- **[API Reference](api-reference.md)** - Complete API documentation for all public interfaces
- **[Testing Guide](testing-guide.md)** - Comprehensive testing documentation and best practices
- **[Property-Based Testing Guide](property-based-testing-guide.md)** - Advanced testing with property-based techniques

### 🏗️ Technical Documentation
- **[Architecture Overview](architecture-overview.md)** - High-level system architecture and design decisions
- **[Registry System](registry-architecture.md)** - QUIC/gRPC registry implementation details
- **[Dependency Resolution](dependency-resolution.md)** - PubGrub algorithm implementation and optimization
- **[Python Dependency Management](python-dependency-management.md)** - Virtual environment isolation and content-addressable sharing
- **[Cleanup System](cleanup-system.md)** - Project-aware cleanup with orphan detection

### 📋 Implementation Details
- **[CLI Design](cli-design.md)** - Command-line interface design and implementation
- **[Update Command](update-command.md)** - Package update system design
- **[Registry Implementation](registry-implementation-plan.md)** - Registry server and client implementation
- **[Testing Configuration](testing-configuration.md)** - Test suite organization and configuration

## 🚀 Quick Navigation

### New to HPM?
Start with the **[User Guide](user-guide.md)** for installation and basic usage.

### Want to Contribute?
Read the **[Developer Guide](developer-guide.md)** and **[Testing Guide](testing-guide.md)**.

### Need Command Help?
Check the **[CLI Reference](cli-reference.md)** for detailed command documentation.

### Working with Python?
See the **[Python User Guide](python-user-guide.md)** for Python dependency management.

### Understanding the System?
Review the **[Architecture Overview](architecture-overview.md)** for system design.

## 📖 Documentation Standards

All HPM documentation follows these standards:
- **Accuracy**: All examples are tested and verified
- **Completeness**: Comprehensive coverage of features and use cases
- **Accessibility**: Clear language suitable for different experience levels
- **Maintainability**: Regular updates to reflect current implementation

## 🤝 Contributing to Documentation

Documentation improvements are welcome! See the **[Developer Guide](developer-guide.md)** for contribution guidelines.

### Documentation Structure
```
docs/
├── README.md                           # This overview
├── user-guide.md                       # Complete user documentation
├── developer-guide.md                  # Developer documentation
├── cli-reference.md                    # CLI command reference
├── api-reference.md                    # API documentation
├── testing-guide.md                    # Testing documentation
├── tutorials.md                        # Step-by-step tutorials
├── architecture-overview.md            # System architecture
├── registry-architecture.md            # Registry system details
├── dependency-resolution.md            # Dependency algorithm details
├── python-dependency-management.md     # Python integration
├── cleanup-system.md                   # Cleanup system details
├── cli-design.md                       # CLI implementation
├── update-command.md                   # Update system design
├── registry-implementation-plan.md     # Registry implementation
├── testing-configuration.md            # Test configuration
└── property-based-testing-guide.md     # Advanced testing
```

## 📊 Documentation Health

- ✅ **Complete Coverage**: All major features documented
- ✅ **Current**: Reflects latest implementation (v0.1.0)  
- ✅ **Tested**: All code examples verified
- ✅ **Organized**: Clear structure and navigation
- ✅ **Accessible**: Multiple entry points for different users

For questions or suggestions about documentation, please open an issue on GitHub.