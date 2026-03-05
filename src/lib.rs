//! # HPM - Houdini Package Manager
//!
//! A package manager for SideFX Houdini, written in Rust.
//!
//! ## Workspace Architecture
//!
//! HPM is organized as a multi-crate workspace:
//!
//! - [`hpm-cli`] - Command-line interface (clap)
//! - [`hpm-core`] - Storage, installation, lock files, project discovery
//! - [`hpm-config`] - Configuration loading and management
//! - [`hpm-resolver`] - PubGrub-based dependency resolver
//! - [`hpm-package`] - Package manifest parsing and Houdini integration
//! - [`hpm-python`] - Python virtual environment management (uv integration)
//! - [`hpm-error`] - Shared error types
//!
//! ## Quick Start
//!
//! ```bash
//! hpm init my-houdini-tools
//! hpm add utility-nodes --git https://github.com/studio/utility-nodes --tag v1.0.0
//! hpm install
//! hpm list
//! ```

// This is a workspace-level documentation crate.
// Individual functionality is implemented in the workspace member crates.
