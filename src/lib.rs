//! # HPM - Houdini Package Manager
//!
//! A modern, Rust-based package management system for SideFX Houdini, providing industry-standard
//! dependency management capabilities equivalent to npm for Node.js or cargo for Rust.
//!
//! ## Workspace Architecture
//!
//! HPM is organized as a multi-crate workspace with clear separation of concerns:
//!
//! ### Core Libraries
//!
//! - [`hpm-core`] - Core functionality including storage, discovery, and dependency management
//! - [`hpm-config`] - Configuration management and project settings
//! - [`hpm-package`] - Package manifest processing and Houdini integration
//! - [`hpm-python`] - Python dependency management with virtual environment support
//! - [`hpm-registry`] - QUIC/gRPC package registry implementation
//! - [`hpm-error`] - Error handling infrastructure
//!
//! ### Applications
//!
//! - [`hpm-cli`] - Command-line interface providing all user-facing functionality
//!
//! ## Key Features
//!
//! ### Fully Implemented
//! - **Package Initialization** - Create standardized Houdini packages
//! - **Dependency Management** - Add, remove, and manage package dependencies
//! - **Python Integration** - Virtual environment support with content-addressable sharing
//! - **Project Cleanup** - Intelligent orphan package detection and removal
//! - **Configuration Validation** - Comprehensive package and configuration validation
//!
//! ### In Development
//! - **Registry Integration** - CLI integration with the implemented registry system
//! - **Package Search** - Find and discover packages in registries
//! - **Publishing** - Publish packages to registries
//! - **Script Execution** - Run package-defined scripts
//!
//! ## Quick Start
//!
//! The primary entry point is the `hpm` CLI binary:
//!
//! ```bash
//! # Initialize a new package
//! hpm init my-houdini-tools --description "Custom Houdini tools"
//!
//! # Add dependencies
//! hpm add utility-nodes --version "^2.1.0"
//! hpm add numpy --python-dep
//!
//! # Install all dependencies
//! hpm install
//!
//! # List dependencies
//! hpm list
//!
//! # Clean up orphaned packages
//! hpm clean --dry-run
//! ```
//!
//! ## Development
//!
//! See [`CLAUDE.md`](../CLAUDE.md) for comprehensive development guidelines and
//! [`README.md`](../README.md) for user-facing documentation.

// This is a workspace-level documentation crate
// Individual functionality is implemented in the workspace member crates
