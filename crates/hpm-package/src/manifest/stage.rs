//! `[stage]` configuration: how the install image is derived from the
//! workspace (prepack scripts, include/exclude globs, per-platform place
//! rules).

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Staging configuration: how the install image is derived from the workspace.
///
/// `[stage]` is the single source of truth for what ends up in a published
/// archive and in a path-dependency install image. It replaces the prior
/// `[native]`-only filter model with a more general "where does each file
/// go" model — useful for HDK plugins whose `.dylib` lives at
/// `build/Release/foo.dylib` in the workspace but should be installed at
/// `dso/macos-aarch64/foo.dylib`.
///
/// Fields:
/// - `output_dir` (default `"dist"`) — where `hpm build` materialises the
///   install image on disk. Unused by `hpm pack` alone, which streams
///   directly from the workspace.
/// - `prepack` — list of `[scripts]` entries to run before staging
///   (compile DSO, collapse expanded HDAs, etc.). Sequential, fail-fast.
/// - `include` / `exclude` — gitignore-style glob lists applied on top of
///   `.gitignore` and `.hpmignore`. Empty `include` means "everything not
///   excluded". Always-excluded: `.git/`, `.hpm/`.
/// - `[stage.platform.<plat>]` — per-platform `place` rules: copy files
///   matching a workspace-relative `from` glob into the install image at a
///   rewritten `to` path. Files matched only by another platform's `place`
///   rule are excluded from this platform's archive; files matched by no
///   `place` rule ship as common content at their workspace-relative path.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StageConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prepack: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
    /// Per-platform place rules. Deserialized from
    /// `[stage.platform.<plat>]` sub-tables.
    #[serde(default, skip_serializing_if = "PlatformStaging::is_empty")]
    pub platform: PlatformStaging,
}

impl StageConfig {
    pub fn is_empty(&self) -> bool {
        self.output_dir.is_none()
            && self.prepack.is_empty()
            && self.include.is_empty()
            && self.exclude.is_empty()
            && self.platform.is_empty()
    }

    /// Effective output directory ("dist" by default).
    pub fn effective_output_dir(&self) -> &str {
        self.output_dir.as_deref().unwrap_or("dist")
    }
}

/// `[stage.platform.*]` table. Each entry is a list of place rules for a
/// single platform key (`"linux-x86_64"`, `"macos-aarch64"`, etc.).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlatformStaging {
    #[serde(flatten)]
    pub entries: IndexMap<String, StagePlatformRules>,
}

impl PlatformStaging {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Place rules for a single platform.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StagePlatformRules {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub place: Vec<PlaceRule>,
}

/// A single `from → to` placement: copy workspace files matching the `from`
/// glob into the install image at `to`. If `to` ends with `/`, files keep
/// their original basename; otherwise `to` is the literal archive path
/// (use when relocating a single file).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaceRule {
    pub from: String,
    pub to: String,
}
