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
/// - `[stage.profile.<name>]` — named build profiles (e.g. `debug`) that
///   layer their own `prepack`/`include`/`exclude`/place rules on top of the
///   base `[stage]`. Orthogonal to platform: `hpm build --profile debug`
///   merges the `debug` profile onto the base config. See
///   [`StageConfig::resolved_for_profile`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    /// Named build profiles. Deserialized from `[stage.profile.<name>]`
    /// sub-tables. Each profile layers overrides onto the base `[stage]`.
    #[serde(default, skip_serializing_if = "ProfileStaging::is_empty")]
    pub profile: ProfileStaging,
}

impl StageConfig {
    pub fn is_empty(&self) -> bool {
        self.output_dir.is_none()
            && self.prepack.is_empty()
            && self.include.is_empty()
            && self.exclude.is_empty()
            && self.platform.is_empty()
            && self.profile.is_empty()
    }

    /// Effective output directory ("dist" by default).
    pub fn effective_output_dir(&self) -> &str {
        self.output_dir.as_deref().unwrap_or("dist")
    }

    /// Whether a `[stage.profile.<name>]` table is declared.
    pub fn has_profile(&self, name: &str) -> bool {
        self.profile.entries.contains_key(name)
    }

    /// Resolve the effective staging config for the named build profile.
    ///
    /// The returned config has its own `profile` map cleared. When a matching
    /// `[stage.profile.<name>]` table exists, its overrides merge onto the
    /// base `[stage]`:
    /// - `prepack`: the profile's list replaces the base when non-empty;
    ///   otherwise the base `prepack` is kept.
    /// - `include` / `exclude`: profile entries are appended to the base.
    /// - `platform.<plat>.place`: profile place rules are appended to the
    ///   matching base platform entry (new platform keys are created as
    ///   needed).
    ///
    /// When no matching table exists (the common case for the default
    /// `release` profile), the base config is returned unchanged.
    pub fn resolved_for_profile(&self, profile: &str) -> StageConfig {
        let mut resolved = self.clone();
        let overrides = resolved.profile.entries.shift_remove(profile);
        resolved.profile = ProfileStaging::default();

        let Some(overrides) = overrides else {
            return resolved;
        };

        if !overrides.prepack.is_empty() {
            resolved.prepack = overrides.prepack;
        }
        resolved.include.extend(overrides.include);
        resolved.exclude.extend(overrides.exclude);
        for (platform, rules) in overrides.platform.entries {
            resolved
                .platform
                .entries
                .entry(platform)
                .or_default()
                .place
                .extend(rules.place);
        }
        resolved
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
#[serde(deny_unknown_fields)]
pub struct StagePlatformRules {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub place: Vec<PlaceRule>,
}

/// A single `from → to` placement: copy workspace files matching the `from`
/// glob into the install image at `to`. If `to` ends with `/`, files keep
/// their original basename; otherwise `to` is the literal archive path
/// (use when relocating a single file).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaceRule {
    pub from: String,
    pub to: String,
}

/// `[stage.profile.*]` table. Each entry is the set of overrides for a single
/// named build profile (`"debug"`, `"release"`, etc.). Profiles are
/// orthogonal to platforms — a profile may carry its own per-platform place
/// rules under `[stage.profile.<name>.platform.<plat>]`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProfileStaging {
    #[serde(flatten)]
    pub entries: IndexMap<String, StageProfileRules>,
}

impl ProfileStaging {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Overrides for a single build profile, layered onto the base `[stage]` by
/// [`StageConfig::resolved_for_profile`]. All fields are optional; an absent
/// field leaves the base value untouched.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageProfileRules {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prepack: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
    /// Per-platform place rules, deserialized from
    /// `[stage.profile.<name>.platform.<plat>]` sub-tables.
    #[serde(default, skip_serializing_if = "PlatformStaging::is_empty")]
    pub platform: PlatformStaging,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(from: &str, to: &str) -> PlaceRule {
        PlaceRule {
            from: from.to_string(),
            to: to.to_string(),
        }
    }

    fn base_stage() -> StageConfig {
        let mut stage = StageConfig {
            prepack: vec!["build-release".to_string()],
            include: vec!["python/**".to_string()],
            exclude: vec!["src/**".to_string()],
            ..Default::default()
        };
        stage.platform.entries.insert(
            "windows-x86_64".to_string(),
            StagePlatformRules {
                place: vec![place("build/Release/*.dll", "dso/")],
            },
        );
        stage
    }

    fn with_debug_profile(mut stage: StageConfig, rules: StageProfileRules) -> StageConfig {
        stage.profile.entries.insert("debug".to_string(), rules);
        stage
    }

    #[test]
    fn unknown_profile_returns_base_unchanged() {
        let stage = base_stage();
        let resolved = stage.resolved_for_profile("release");
        assert_eq!(resolved.prepack, vec!["build-release".to_string()]);
        assert_eq!(resolved.include, vec!["python/**".to_string()]);
        assert!(resolved.profile.is_empty());
        assert_eq!(resolved.platform.entries.len(), 1);
    }

    #[test]
    fn profile_prepack_replaces_base() {
        let stage = with_debug_profile(
            base_stage(),
            StageProfileRules {
                prepack: vec!["build-debug".to_string()],
                ..Default::default()
            },
        );
        let resolved = stage.resolved_for_profile("debug");
        assert_eq!(resolved.prepack, vec!["build-debug".to_string()]);
    }

    #[test]
    fn empty_profile_prepack_keeps_base() {
        let stage = with_debug_profile(
            base_stage(),
            StageProfileRules {
                include: vec!["debug-symbols/**".to_string()],
                ..Default::default()
            },
        );
        let resolved = stage.resolved_for_profile("debug");
        assert_eq!(resolved.prepack, vec!["build-release".to_string()]);
    }

    #[test]
    fn profile_include_exclude_append() {
        let stage = with_debug_profile(
            base_stage(),
            StageProfileRules {
                include: vec!["pdb/**".to_string()],
                exclude: vec!["build/Release/**".to_string()],
                ..Default::default()
            },
        );
        let resolved = stage.resolved_for_profile("debug");
        assert_eq!(resolved.include, vec!["python/**", "pdb/**"]);
        assert_eq!(resolved.exclude, vec!["src/**", "build/Release/**"]);
    }

    #[test]
    fn profile_place_rules_append_to_existing_platform() {
        let mut profile_platform = PlatformStaging::default();
        profile_platform.entries.insert(
            "windows-x86_64".to_string(),
            StagePlatformRules {
                place: vec![place("build/Debug/*.pdb", "dso/")],
            },
        );
        let stage = with_debug_profile(
            base_stage(),
            StageProfileRules {
                platform: profile_platform,
                ..Default::default()
            },
        );
        let resolved = stage.resolved_for_profile("debug");
        let win = &resolved.platform.entries["windows-x86_64"].place;
        assert_eq!(win.len(), 2);
        assert_eq!(win[0].from, "build/Release/*.dll");
        assert_eq!(win[1].from, "build/Debug/*.pdb");
    }

    #[test]
    fn profile_place_rules_create_new_platform_key() {
        let mut profile_platform = PlatformStaging::default();
        profile_platform.entries.insert(
            "linux-x86_64".to_string(),
            StagePlatformRules {
                place: vec![place("build/debug/*.so", "dso/")],
            },
        );
        let stage = with_debug_profile(
            base_stage(),
            StageProfileRules {
                platform: profile_platform,
                ..Default::default()
            },
        );
        let resolved = stage.resolved_for_profile("debug");
        assert!(resolved.platform.entries.contains_key("linux-x86_64"));
        assert!(resolved.platform.entries.contains_key("windows-x86_64"));
    }
}
