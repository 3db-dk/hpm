//! Bundled-operator declarations inside `hpm.toml`'s `[[operators]]` array.
//!
//! HPM does not parse the operators a package ships out of its files: the HDA
//! container format is officially undocumented and may change between Houdini
//! versions, and a compiled HDK plugin (DSO) does not expose its operator names
//! offline at all. Instead the author — who knows exactly what they bundle —
//! declares each operator here, giving `hpm pack` a stable, version-proof asset
//! index to emit for node-level registry search.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::platform::Platform;

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

/// Where an operator's defining file lives in the produced package.
///
/// Universal assets (HDAs, single-platform DSOs) use a single archive-relative
/// path present in every archive. A DSO whose binary differs per platform
/// (`.so` / `.dll` / `.dylib`, often under `dso/<platform>/`) uses a
/// platform-keyed table, mirroring `[stage.platform.*]`. Each per-platform pack
/// then resolves and verifies the path for the platform it targets.
///
/// Deserializes from either form: a bare string is [`Single`](Self::Single); a
/// table keyed by platform identifier is [`PerPlatform`](Self::PerPlatform).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OperatorSource {
    /// A single archive-relative path, present in every archive.
    Single(String),
    /// Per-platform archive-relative paths, keyed by platform identifier
    /// (`linux-x86_64`, `macos-aarch64`, …). Keys must appear in
    /// `[compat].platforms`.
    PerPlatform(IndexMap<String, String>),
}

/// Outcome of resolving an [`OperatorDecl`]'s source against a target platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceResolution<'a> {
    /// A concrete archive path to emit (and verify against the archive).
    Path(&'a str),
    /// No source was declared — index the operator without a path or check.
    Unspecified,
    /// The source is platform-specific and the target platform is not among its
    /// keys, so this operator is not shipped in the target platform's package
    /// and is omitted from that platform's index.
    NotForPlatform,
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
    /// Where the operator's file lives in the produced package — a single
    /// archive path or a per-platform table. Optional, but recommended:
    /// `hpm pack` checks the resolved path against the produced archive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<OperatorSource>,
}

impl OperatorDecl {
    /// Resolve this operator's source for a pack targeting `platform`
    /// (`None` for a universal, platform-less pack).
    ///
    /// A [`Single`](OperatorSource::Single) source resolves to its path on every
    /// platform. A [`PerPlatform`](OperatorSource::PerPlatform) source resolves
    /// to the entry for `platform`, or [`NotForPlatform`](SourceResolution::NotForPlatform)
    /// when the target platform is not among its keys (or the pack is
    /// platform-less). No declared source yields
    /// [`Unspecified`](SourceResolution::Unspecified).
    pub fn resolved_source(&self, platform: Option<&Platform>) -> SourceResolution<'_> {
        match &self.source {
            None => SourceResolution::Unspecified,
            Some(OperatorSource::Single(path)) => SourceResolution::Path(path),
            Some(OperatorSource::PerPlatform(map)) => match platform {
                Some(p) => match map.get(p.as_str()) {
                    Some(path) => SourceResolution::Path(path),
                    None => SourceResolution::NotForPlatform,
                },
                None => SourceResolution::NotForPlatform,
            },
        }
    }
}
