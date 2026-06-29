//! The asset index model emitted by `hpm pack`.
//!
//! An [`Asset`] describes one operator (node type) that a package bundles. The
//! entries are built from the author's `[[operators]]` declarations in
//! `hpm.toml` — HPM does not parse package files to discover operators, because
//! the HDA container format is officially undocumented and may change between
//! Houdini versions, and a compiled HDK plugin does not expose its operator
//! names offline at all.
//!
//! The struct serializes to the wire shape consumed by the registry: a single
//! flat object per asset with a `kind` discriminator and `None` fields omitted.

use serde::{Deserialize, Serialize};

/// Which kind of file an operator is defined by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    /// An operator defined by an HDA/OTL digital asset.
    HdaOperator,
    /// An operator registered by a compiled HDK plugin (DSO).
    HdkOperator,
}

/// One indexed operator that a package ships.
///
/// `None` fields are omitted from the serialized index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    /// Discriminator: HDA vs HDK operator.
    pub kind: AssetKind,

    /// Namespaced operator type name (e.g. `studio::rbd_configure::2.0`).
    pub type_name: String,

    /// Operator table / network category (`Sop`, `Object`, `Dop`, `Lop`, …).
    pub category: String,

    /// TAB-menu display label (e.g. `RBD Configure`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,

    /// Namespace component of `type_name`, if it was namespaced
    /// (`studio` for `studio::rbd_configure::2.0`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,

    /// Version component of `type_name`, if present (`2.0` for
    /// `studio::rbd_configure::2.0`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op_version: Option<String>,

    /// TAB submenu path the operator files under (`Studio/Dynamics`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_submenu: Option<String>,

    /// Icon identifier, if declared (`SOP_rbd`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,

    /// Archive-relative path of the file this operator comes from
    /// (`otls/rbd.hda`, `dso/scatter.so`), if the author declared one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,
}

/// Split a namespaced operator type name into `(namespace, base, version)`.
///
/// Houdini's namespacing grammar is `namespace::name::version`, where the
/// namespace typically contains a dot (`com.studio`) and the version is digits
/// and periods (`2.0`). Both the namespace and version are optional, so this is
/// necessarily a best-effort split:
///
/// - `studio::rbd_configure::2.0` → (`Some("studio")`, `rbd_configure`, `Some("2.0")`)
/// - `rbd_configure::2.0`         → (`None`, `rbd_configure`, `Some("2.0")`)
/// - `com.studio::rbd_configure`  → (`Some("com.studio")`, `rbd_configure`, `None`)
/// - `rbd_configure`              → (`None`, `rbd_configure`, `None`)
///
/// The two-component case is disambiguated by whether the trailing component
/// looks like a version (only digits and `.`).
pub fn split_type_name(type_name: &str) -> (Option<String>, String, Option<String>) {
    let parts: Vec<&str> = type_name.split("::").collect();
    let is_version = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit() || c == '.');

    match parts.as_slice() {
        [base] => (None, (*base).to_string(), None),
        [a, b] => {
            if is_version(b) {
                // name::version
                (None, (*a).to_string(), Some((*b).to_string()))
            } else {
                // namespace::name
                (Some((*a).to_string()), (*b).to_string(), None)
            }
        }
        [ns, base, ver, ..] => (
            Some((*ns).to_string()),
            (*base).to_string(),
            if is_version(ver) {
                Some((*ver).to_string())
            } else {
                None
            },
        ),
        [] => (None, String::new(), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_full_namespaced_name() {
        let (ns, base, ver) = split_type_name("studio::rbd_configure::2.0");
        assert_eq!(ns.as_deref(), Some("studio"));
        assert_eq!(base, "rbd_configure");
        assert_eq!(ver.as_deref(), Some("2.0"));
    }

    #[test]
    fn splits_name_version() {
        let (ns, base, ver) = split_type_name("rbd_configure::2.0");
        assert_eq!(ns, None);
        assert_eq!(base, "rbd_configure");
        assert_eq!(ver.as_deref(), Some("2.0"));
    }

    #[test]
    fn splits_namespace_name() {
        let (ns, base, ver) = split_type_name("com.studio::rbd_configure");
        assert_eq!(ns.as_deref(), Some("com.studio"));
        assert_eq!(base, "rbd_configure");
        assert_eq!(ver, None);
    }

    #[test]
    fn plain_name_has_no_namespace_or_version() {
        let (ns, base, ver) = split_type_name("fast_scatter");
        assert_eq!(ns, None);
        assert_eq!(base, "fast_scatter");
        assert_eq!(ver, None);
    }
}
