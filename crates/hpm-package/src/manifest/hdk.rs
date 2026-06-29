//! HDK operator declarations inside `hpm.toml`'s `[[hdk_operators]]` array.
//!
//! A compiled HDK plugin (DSO) is opaque to offline inspection: operator type
//! names and labels are C++ constructor arguments that only materialize when
//! Houdini's runtime calls the registration entry point. The author, however,
//! wrote that C++ and knows the names — so they declare them here, letting
//! `hpm pack` emit a complete asset index without a Houdini runtime.

use serde::{Deserialize, Serialize};

/// One HDK operator the package ships, declared by the author.
///
/// Mirrors the descriptive fields of an HDA operator so both kinds index
/// uniformly. `type_name` and `category` are required; `label` and `source`
/// are optional.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HdkOperator {
    /// Namespaced operator type name (e.g. `studio::rbd_configure::2.0`).
    pub type_name: String,
    /// TAB-menu display label. Optional; omit to leave it unset in the index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Operator table / network category (`Sop`, `Object`, `Dop`, `Lop`, …).
    pub category: String,
    /// Source DSO this operator is registered by, for provenance
    /// (`dso/rbd.so`). Optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}
