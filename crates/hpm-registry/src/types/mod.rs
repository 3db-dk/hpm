//! Common Types and Data Structures
//!
//! This module contains shared types used throughout the registry implementation,
//! including authentication tokens, package metadata, and error definitions.
//!
//! ## Key Types
//!
//! - [`AuthToken`] - Authentication token with scoped permissions
//! - [`TokenScope`] - Permission scopes for registry operations
//! - [`RegistryError`] - Comprehensive error types with context
//! - [`PackageVersion`] - Package metadata and version information
//! - [`PackageMetadata`] - Detailed package information and dependencies
//!
//! ## Authentication Model
//!
//! The registry uses a token-based authentication system with the following scopes:
//!
//! - `Read` - Access to public and private packages
//! - `Publish` - Ability to publish new packages and versions
//! - `Delete` - Permission to delete packages and versions
//! - `Admin` - Full administrative access
//!
//! Tokens can have multiple scopes and optional expiration times for security.

pub mod auth;
pub mod error;
pub mod package;

pub use auth::{AuthToken, TokenScope};
pub use error::RegistryError;
pub use package::*;
