//! Utility Functions and Helpers
//!
//! This module provides utility functions for package compression, validation,
//! and integrity checking used throughout the registry implementation.
//!
//! ## Compression
//!
//! The [`compression`] module provides functions for compressing and decompressing
//! package data using zstd compression, which offers an excellent balance of
//! compression ratio and speed for package distribution.
//!
//! ## Validation
//!
//! The [`validation`] module contains functions for:
//!
//! - Package name validation (lowercase, alphanumeric, hyphens only)
//! - Semantic version validation (X.Y.Z format)
//! - Package size limits (500MB maximum)
//! - SHA-256 checksum calculation and verification
//!
//! ## Security
//!
//! All validation functions are designed with security in mind:
//!
//! - Input sanitization prevents injection attacks
//! - Size limits prevent denial-of-service attacks
//! - Cryptographic checksums ensure data integrity
//! - Clear error messages aid in debugging without exposing internals

pub mod compression;
pub mod validation;

pub use compression::*;
pub use validation::*;
