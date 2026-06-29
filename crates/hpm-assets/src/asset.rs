//! The asset index model emitted by `hpm pack`.
//!
//! An [`Asset`] describes one operator (node type) that a package bundles.
//! Two kinds exist, with an important asymmetry:
//!
//! - [`AssetKind::HdaOperator`] — extracted fully offline from an `.hda`/`.otl`
//!   container (see [`crate::hda`]). Type name, label, category, namespaced
//!   parts, TAB submenu, and icon are all available.
//! - [`AssetKind::HdkOperator`] — a compiled DSO is opaque to offline parsing
//!   (operator names/labels are C++ constructor arguments that only exist at
//!   Houdini runtime). These entries therefore come from the author's
//!   `[[hdk_operators]]` declaration in `hpm.toml` (see
//!   [`AssetSource::Declared`]).
//!
//! The struct serializes to the wire shape consumed by the registry: a single
//! flat object per asset with a `kind` discriminator and `None` fields omitted.

use serde::{Deserialize, Serialize};

/// Which kind of bundled operator an [`Asset`] describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    /// An operator defined by an HDA/OTL digital asset, parsed offline.
    HdaOperator,
    /// An operator registered by a compiled HDK plugin (DSO).
    HdkOperator,
}

/// Where an HDK operator entry's metadata came from. Only meaningful for
/// [`AssetKind::HdkOperator`]; HDA entries are always parsed and leave this
/// `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetSource {
    /// Declared by the package author in `[[hdk_operators]]`.
    Declared,
    /// Recovered by loading the DSO in Houdini's runtime (`hython`). Reserved
    /// for the opt-in introspection path; not produced by offline packing.
    Introspected,
}

/// One indexed operator that a package ships.
///
/// Field presence varies by [`AssetKind`]: HDA entries populate the descriptive
/// fields from the asset's sections, while HDK entries carry whatever the author
/// declared. `None` fields are omitted from the serialized index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    /// Discriminator: HDA vs HDK operator.
    pub kind: AssetKind,

    /// Namespaced operator type name (e.g. `studio::rbd_configure::2.0`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,

    /// TAB-menu display label (e.g. `RBD Configure`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,

    /// Operator table / network category (`Sop`, `Object`, `Dop`, `Lop`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,

    /// Namespace component of `type_name`, if it was namespaced
    /// (`studio` for `studio::rbd_configure::2.0`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,

    /// Version component of `type_name`, if present (`2.0` for
    /// `studio::rbd_configure::2.0`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op_version: Option<String>,

    /// TAB submenu path the operator files itself under (`Studio/Dynamics`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_submenu: Option<String>,

    /// Icon identifier, if the asset declares one (`SOP_rbd`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,

    /// Archive-relative path of the file this operator came from
    /// (`otls/rbd.hda`, `dso/scatter.so`).
    pub source_file: String,

    /// For HDK operators: how the metadata was obtained. Omitted for HDA
    /// operators (always parsed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<AssetSource>,
}

impl Asset {
    /// Construct an HDA operator entry. Descriptive fields are filled in by the
    /// parser; this just stamps the kind and source file.
    pub fn hda(source_file: impl Into<String>) -> Self {
        Self {
            kind: AssetKind::HdaOperator,
            type_name: None,
            label: None,
            category: None,
            namespace: None,
            op_version: None,
            tab_submenu: None,
            icon: None,
            source_file: source_file.into(),
            source: None,
        }
    }
}
