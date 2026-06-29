//! Houdini package asset indexing for HPM.
//!
//! `hpm pack` uses this crate's [`Asset`] model to emit a searchable index of
//! the operators (node types) a package bundles, so the registry can offer
//! node-level search without ever opening an archive.
//!
//! The index is built from the author's `[[operators]]` declarations in
//! `hpm.toml`. HPM deliberately does not parse the package's own files to
//! discover operators: the HDA container format is officially undocumented and
//! may change between Houdini versions, and a compiled HDK plugin does not
//! expose its operator names offline at all. Author declarations are stable,
//! version-proof, and a single source of truth.

pub mod asset;

pub use asset::{Asset, AssetKind, split_type_name};
