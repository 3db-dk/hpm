//! Bundled-operator declarations inside `hpm.toml`'s `[[operators]]` array.
//!
//! HPM does not parse the operators a package ships out of its files: the HDA
//! container format is officially undocumented and may change between Houdini
//! versions, and a compiled HDK plugin (DSO) does not expose its operator names
//! offline at all. Instead the author — who knows exactly what they bundle —
//! declares each operator here, giving `hpm pack` a stable, version-proof asset
//! index to emit for node-level registry search.

use serde::{Deserialize, Serialize};

/// Which kind of file an operator is defined by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OperatorKind {
    /// Defined by an HDA/OTL digital asset (`otls/*.hda`, `*.otl`).
    Hda,
    /// Registered by a compiled plugin shipped as a DSO (`dso/*.so`, `*.dll`,
    /// `*.dylib`) — i.e. authored against the HDK.
    Dso,
}

/// One operator (node type) the package ships, declared by the author.
///
/// `kind`, `type_name`, and `category` are required; the rest are optional
/// descriptive fields that enrich the emitted index.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperatorDecl {
    /// HDA vs DSO — sets the emitted asset's kind.
    pub kind: OperatorKind,
    /// Namespaced operator type name (e.g. `studio::rbd_configure::2.0`).
    pub type_name: String,
    /// Operator table / network category (`Sop`, `Object`, `Dop`, `Lop`, …).
    pub category: String,
    /// TAB-menu display label. Optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// TAB submenu path the operator files under (`Studio/Dynamics`). Optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_submenu: Option<String>,
    /// Icon identifier (`SOP_rbd`). Optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Archive-relative source file this operator lives in (`otls/rbd.hda`,
    /// `dso/scatter.so`). Optional, but recommended: `hpm pack` warns when a
    /// declared `source` is not present in the produced archive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}
