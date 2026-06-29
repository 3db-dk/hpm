//! Houdini package asset indexing for HPM.
//!
//! `hpm pack` uses this crate to enumerate the operators (nodes) a package
//! bundles and emit them as a searchable index. The registry never opens an
//! archive, so packing is the one moment the files are in hand at the right
//! time.
//!
//! Two asset sources are covered:
//!
//! - **HDAs** (`otls/*.hda`, `*.otl`) — parsed fully offline by [`hda`]. An
//!   HDA is a self-describing, section-keyed container; operator type names,
//!   labels, categories, TAB submenus, and icons all live in plain-text
//!   sections.
//! - **HDK plugins** (`dso/*.so`, `*.dll`, `*.dylib`) — a compiled DSO does not
//!   expose operator names offline, so those entries come from the author's
//!   `[[hdk_operators]]` declaration in the manifest, assembled by the caller.
//!
//! The output model lives in [`asset`].

pub mod asset;
pub mod hda;

pub use asset::{Asset, AssetKind, AssetSource};
pub use hda::{HdaParseError, parse_hda_bytes};
