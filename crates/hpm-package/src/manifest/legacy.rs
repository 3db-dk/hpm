//! Backwards-compatible reading of pre-0.16 ("Manifest 1.x") `hpm.toml`
//! files.
//!
//! The 0.16.0 "Manifest 2.0" refactor renamed and reshaped five sections:
//!
//! | Pre-0.16 | Current |
//! |----------|---------|
//! | `[houdini]` `min_version`/`max_version` | `[compat].houdini` range |
//! | `[env]` + `[dev.env]` | `[runtime]` with `install_source` axis |
//! | `[native]` `platforms` + `[native.<plat>].files` | `[compat].platforms` + `[stage.platform.<plat>].place` |
//! | `[scripts.platform.<os>]` | conditional `cmd` on the script entry |
//!
//! The current typed parser silently drops the old top-level sections
//! (nothing sets `deny_unknown_fields`), so an old package would install
//! with its Houdini range, env vars, and native placement silently gone.
//! This module detects the old shape up front and converts it to a current
//! [`PackageManifest`], surfacing the lossy bits (notably `[native]` ->
//! `[stage]`, where the placement destination has to be guessed) as a
//! [`MigrationReport`].
//!
//! Reading is transparent (see [`crate::manifest::PackageManifest::from_path`]);
//! the `hpm migrate` command uses the same conversion to rewrite a manifest
//! to the current schema. Legacy support is removed in
//! [`crate::manifest::LEGACY_MANIFEST_SUNSET`].

use indexmap::IndexMap;
use serde::Deserialize;

use crate::dependency::DependencySpec;
use crate::env_value::{Condition, EnvValue, EnvValueBranch, HoudiniRange};
use crate::platform::Platform;
use crate::python::PythonDependencySpec;

use super::PackageManifest;
use super::compat::CompatConfig;
use super::env::{EnvMethod, ManifestEnvEntry};
use super::info::PackageInfo;
use super::registry::RegistryConfig;
use super::scripts::{PackageScripts, ScriptEntry, ScriptEnv};
use super::stage::{PlaceRule, StageConfig, StagePlatformRules};

/// The pre-0.16 manifest shape. Only the sections that changed are modelled
/// here; everything else (`[package]`, `[dependencies]`,
/// `[python_dependencies]`, `[[registries]]`) shares the current types and
/// is carried over verbatim.
#[derive(Debug, Clone, Deserialize)]
pub struct LegacyManifest {
    pub package: PackageInfo,
    #[serde(default)]
    pub houdini: Option<LegacyHoudini>,
    /// `[env]` — same per-entry shape as the current `[runtime]`.
    #[serde(default)]
    pub env: IndexMap<String, ManifestEnvEntry>,
    /// `[dev]` — only `dev.env` is meaningful.
    #[serde(default)]
    pub dev: Option<LegacyDev>,
    #[serde(default)]
    pub native: Option<LegacyNative>,
    #[serde(default)]
    pub scripts: Option<LegacyScripts>,
    // Passthrough sections — unchanged across the refactor.
    #[serde(default)]
    pub dependencies: IndexMap<String, DependencySpec>,
    #[serde(default)]
    pub python_dependencies: IndexMap<String, PythonDependencySpec>,
    #[serde(default)]
    pub registries: Vec<RegistryConfig>,
}

/// `[houdini]` — the old min/max version pair.
#[derive(Debug, Clone, Deserialize)]
pub struct LegacyHoudini {
    #[serde(default)]
    pub min_version: Option<String>,
    #[serde(default)]
    pub max_version: Option<String>,
}

/// `[dev]` — the dev-only env overrides.
#[derive(Debug, Clone, Deserialize)]
pub struct LegacyDev {
    #[serde(default)]
    pub env: IndexMap<String, ManifestEnvEntry>,
}

/// `[native]` — platform list plus per-platform file filters.
#[derive(Debug, Clone, Deserialize)]
pub struct LegacyNative {
    #[serde(default)]
    pub platforms: Vec<Platform>,
    /// `[native.<plat>]` sub-tables.
    #[serde(flatten)]
    pub per_platform: IndexMap<String, LegacyNativeFiles>,
}

/// `[native.<plat>]` — the file glob filter for one platform.
#[derive(Debug, Clone, Deserialize)]
pub struct LegacyNativeFiles {
    #[serde(default)]
    pub files: Vec<String>,
}

/// `[scripts]` — base entries plus an optional `[scripts.platform]` table.
#[derive(Debug, Clone, Deserialize)]
pub struct LegacyScripts {
    /// `[scripts.platform.<os>]` -> (script name -> command).
    #[serde(default)]
    pub platform: Option<IndexMap<String, IndexMap<String, String>>>,
    /// Non-platform script entries (`[scripts]` keys other than `platform`).
    #[serde(flatten)]
    pub base: IndexMap<String, ScriptEntry>,
}

/// What had to be guessed or could not be carried over cleanly during a
/// migration. Surfaced to the user so they can review the result.
#[derive(Debug, Clone)]
pub enum MigrationWarning {
    /// A `[native.<plat>].files` glob became a `[stage.platform]` place rule
    /// whose `to` destination was derived heuristically — verify it.
    ReviewPlaceRule {
        platform: String,
        from: String,
        to: String,
    },
    /// `[env]` and `[dev.env]` declared the same key with different
    /// `method`s; the `[env]` (base) method was kept.
    EnvMethodMismatch {
        key: String,
        base: EnvMethod,
        dev: EnvMethod,
    },
    /// The `[houdini]` min/max pair did not form a valid range; the Houdini
    /// constraint was dropped.
    InvalidHoudiniRange { range: String, error: String },
    /// A `[native.<plat>]` key is not a recognised platform identifier; its
    /// place rules were dropped.
    UnknownPlatform { platform: String },
}

impl std::fmt::Display for MigrationWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MigrationWarning::ReviewPlaceRule { platform, from, to } => write!(
                f,
                "[stage.platform.{platform}]: review place rule `from = \"{from}\"`, \
                 `to = \"{to}\"` (the destination was derived from the old glob and may need adjusting)"
            ),
            MigrationWarning::EnvMethodMismatch { key, base, dev } => write!(
                f,
                "env var '{key}': [env] used method = \"{}\" but [dev.env] used \"{}\"; \
                 kept \"{}\"",
                base.as_str(),
                dev.as_str(),
                base.as_str()
            ),
            MigrationWarning::InvalidHoudiniRange { range, error } => write!(
                f,
                "[houdini] did not form a valid range (\"{range}\": {error}); \
                 the Houdini compatibility constraint was dropped"
            ),
            MigrationWarning::UnknownPlatform { platform } => write!(
                f,
                "[native.{platform}]: '{platform}' is not a recognised platform identifier; \
                 its place rules were dropped"
            ),
        }
    }
}

/// The result of converting a [`LegacyManifest`]: any review items raised
/// along the way.
#[derive(Debug, Clone, Default)]
pub struct MigrationReport {
    pub warnings: Vec<MigrationWarning>,
}

impl MigrationReport {
    pub fn is_empty(&self) -> bool {
        self.warnings.is_empty()
    }
}

/// Detect a pre-0.16 manifest by its top-level shape.
///
/// Marker-based rather than error-based: the current parser silently drops
/// the old top-level sections instead of erroring, so we cannot rely on a
/// failed typed parse to spot them. The current schema has no top-level
/// `houdini`/`env`/`dev`/`native` keys, so any of those means "legacy". A
/// `[scripts.platform]` *table without a `cmd` key* is the old per-OS script
/// table (a current script entry named `platform` would carry a `cmd`).
pub fn is_legacy(table: &toml::Table) -> bool {
    if table.contains_key("houdini")
        || table.contains_key("env")
        || table.contains_key("dev")
        || table.contains_key("native")
    {
        return true;
    }
    if let Some(scripts) = table.get("scripts").and_then(|v| v.as_table())
        && let Some(platform) = scripts.get("platform").and_then(|v| v.as_table())
        && !platform.contains_key("cmd")
    {
        return true;
    }
    false
}

/// Convert a pre-0.16 manifest to the current [`PackageManifest`], collecting
/// review items in the returned [`MigrationReport`].
pub fn migrate_legacy(legacy: LegacyManifest) -> (PackageManifest, MigrationReport) {
    let mut report = MigrationReport::default();

    let compat = migrate_compat(legacy.houdini, legacy.native.as_ref(), &mut report);
    let stage = migrate_stage(legacy.native.as_ref(), &mut report);
    let runtime = migrate_runtime(legacy.env, legacy.dev, &mut report);
    let scripts = migrate_scripts(legacy.scripts);

    let manifest = PackageManifest {
        package: legacy.package,
        compat,
        stage,
        registries: legacy.registries,
        dependencies: legacy.dependencies,
        python_dependencies: legacy.python_dependencies,
        runtime,
        scripts,
        // The pre-0.16 format had no operator declarations; nothing to migrate.
        operators: Vec::new(),
    };

    (manifest, report)
}

/// `[houdini]` + `[native].platforms` -> `[compat]`.
fn migrate_compat(
    houdini: Option<LegacyHoudini>,
    native: Option<&LegacyNative>,
    report: &mut MigrationReport,
) -> CompatConfig {
    let mut compat = CompatConfig::default();

    if let Some(h) = houdini
        && let Some(range) = houdini_range_string(&h)
    {
        match HoudiniRange::parse(&range) {
            Ok(parsed) => compat.houdini = Some(parsed),
            Err(e) => report.warnings.push(MigrationWarning::InvalidHoudiniRange {
                range,
                error: e.to_string(),
            }),
        }
    }

    if let Some(native) = native {
        // Declared platforms, plus any platform that only appears as a
        // `[native.<plat>]` sub-table, so the generated `[stage.platform.*]`
        // keys all resolve against `[compat].platforms` at validate time.
        for p in &native.platforms {
            if !compat.platforms.contains(p) {
                compat.platforms.push(*p);
            }
        }
        for plat in native.per_platform.keys() {
            match plat.parse::<Platform>() {
                Ok(p) => {
                    if !compat.platforms.contains(&p) {
                        compat.platforms.push(p);
                    }
                }
                Err(_) => report.warnings.push(MigrationWarning::UnknownPlatform {
                    platform: plat.clone(),
                }),
            }
        }
    }

    compat
}

/// Build the `[compat].houdini` range string from the old min/max pair.
fn houdini_range_string(h: &LegacyHoudini) -> Option<String> {
    let min = h
        .min_version
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let max = h
        .max_version
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    match (min, max) {
        (Some(min), Some(max)) => Some(format!(">={min}, <{max}")),
        (Some(min), None) => Some(format!(">={min}")),
        (None, Some(max)) => Some(format!("<{max}")),
        (None, None) => None,
    }
}

/// `[native.<plat>].files` -> `[stage.platform.<plat>].place`.
fn migrate_stage(native: Option<&LegacyNative>, report: &mut MigrationReport) -> StageConfig {
    let mut stage = StageConfig::default();
    let Some(native) = native else {
        return stage;
    };

    for (plat, files) in &native.per_platform {
        // Skip platforms that won't validate — already warned in
        // migrate_compat.
        if plat.parse::<Platform>().is_err() {
            continue;
        }
        let mut rules = StagePlatformRules::default();
        for glob in &files.files {
            let to = derive_place_to(glob);
            report.warnings.push(MigrationWarning::ReviewPlaceRule {
                platform: plat.clone(),
                from: glob.clone(),
                to: to.clone(),
            });
            rules.place.push(PlaceRule {
                from: glob.clone(),
                to,
            });
        }
        if !rules.place.is_empty() {
            stage.platform.entries.insert(plat.clone(), rules);
        }
    }

    stage
}

/// Derive a place-rule `to` from an old filter glob.
///
/// The old `files` entry was a *filter* (include matching files at their
/// workspace path); the new model needs a *destination*. We default `to` to
/// the directory portion of the glob (so files land where they already are),
/// which is correct for the common `dso/<plat>/*` layout but a guess for
/// relocating layouts — hence the review warning.
fn derive_place_to(glob: &str) -> String {
    // A `dir/*` or `dir/**` glob matches files *inside* `dir`, so that
    // directory is the destination.
    if glob.ends_with("/*") || glob.ends_with("/**") {
        let dir = glob.trim_end_matches("/**").trim_end_matches("/*");
        return if dir.is_empty() {
            "./".to_string()
        } else {
            format!("{dir}/")
        };
    }
    // Otherwise the glob names files within its parent directory (e.g.
    // `lib/foo.dll` -> `lib/`); a bare pattern (`*.dll`) lands at the root.
    match glob.rsplit_once('/') {
        Some((dir, _)) if !dir.is_empty() => format!("{dir}/"),
        _ => "./".to_string(),
    }
}

/// `[env]` + `[dev.env]` -> `[runtime]`, folding the dev/base distinction
/// onto the `install_source` axis.
fn migrate_runtime(
    env: IndexMap<String, ManifestEnvEntry>,
    dev: Option<LegacyDev>,
    report: &mut MigrationReport,
) -> IndexMap<String, ManifestEnvEntry> {
    let dev_env = dev.map(|d| d.env).unwrap_or_default();
    let mut runtime: IndexMap<String, ManifestEnvEntry> = IndexMap::new();

    // Base keys first (preserving order), merging any dev override.
    for (key, base) in &env {
        let entry = match dev_env.get(key) {
            Some(dev_entry) => {
                if dev_entry.method != base.method {
                    report.warnings.push(MigrationWarning::EnvMethodMismatch {
                        key: key.clone(),
                        base: base.method.clone(),
                        dev: dev_entry.method.clone(),
                    });
                }
                merge_env_entries(base, dev_entry)
            }
            None => base.clone(),
        };
        runtime.insert(key.clone(), entry);
    }

    // Dev-only keys: gate the whole value to install_source = "dev".
    for (key, dev_entry) in &dev_env {
        if env.contains_key(key) {
            continue;
        }
        runtime.insert(key.clone(), gate_entry_to_dev(dev_entry));
    }

    runtime
}

/// Merge a base `[env]` entry and a `[dev.env]` entry for the same key into
/// one conditional entry: dev branches (gated `install_source = "dev"`)
/// first, then the base value as the fallback.
fn merge_env_entries(base: &ManifestEnvEntry, dev: &ManifestEnvEntry) -> ManifestEnvEntry {
    let mut branches = value_to_dev_branches(dev.value.as_ref());
    branches.extend(value_to_base_branches(base.value.as_ref()));
    ManifestEnvEntry {
        method: base.method.clone(),
        value: Some(EnvValue::Conditional(branches)),
        required: base.required || dev.required,
    }
}

/// Gate a dev-only entry's value to `install_source = "dev"`.
fn gate_entry_to_dev(dev: &ManifestEnvEntry) -> ManifestEnvEntry {
    let value = dev
        .value
        .as_ref()
        .map(|_| EnvValue::Conditional(value_to_dev_branches(dev.value.as_ref())));
    ManifestEnvEntry {
        method: dev.method.clone(),
        value,
        required: dev.required,
    }
}

/// Branches for a dev value: each gets `install_source = "dev"` added.
fn value_to_dev_branches(value: Option<&EnvValue>) -> Vec<EnvValueBranch> {
    branches_from_value(value)
        .into_iter()
        .map(|mut b| {
            if b.when.install_source.is_none() {
                b.when.install_source = Some("dev".to_string());
            }
            b
        })
        .collect()
}

/// Branches for a base value, used verbatim (an empty `when` is the
/// fallback).
fn value_to_base_branches(value: Option<&EnvValue>) -> Vec<EnvValueBranch> {
    branches_from_value(value)
}

/// Normalise an [`EnvValue`] into a list of branches. A flat value becomes a
/// single unconditional branch; a conditional value keeps its branches.
fn branches_from_value(value: Option<&EnvValue>) -> Vec<EnvValueBranch> {
    match value {
        Some(EnvValue::Flat(s)) => vec![EnvValueBranch {
            when: Condition::default(),
            set: s.clone(),
        }],
        Some(EnvValue::Conditional(branches)) => branches.clone(),
        None => Vec::new(),
    }
}

/// `[scripts]` + `[scripts.platform.<os>]` -> per-entry conditional `cmd`.
fn migrate_scripts(scripts: Option<LegacyScripts>) -> PackageScripts {
    let Some(scripts) = scripts else {
        return PackageScripts::default();
    };
    let platform = scripts.platform.unwrap_or_default();
    let mut out: IndexMap<String, ScriptEntry> = IndexMap::new();

    // Names in base order first, then platform-only names in first-seen
    // order.
    let mut names: Vec<String> = scripts.base.keys().cloned().collect();
    for table in platform.values() {
        for name in table.keys() {
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
    }

    for name in names {
        let base = scripts.base.get(&name);
        // Per-OS variants for this script, in the order the OS tables appear.
        let variants: Vec<(String, String)> = platform
            .iter()
            .filter_map(|(os, table)| table.get(&name).map(|cmd| (os.clone(), cmd.clone())))
            .collect();

        if variants.is_empty() {
            // No per-OS variation — carry the base entry over unchanged.
            if let Some(entry) = base {
                out.insert(name, entry.clone());
            }
            continue;
        }

        let mut branches: Vec<EnvValueBranch> = variants
            .into_iter()
            .map(|(os, cmd)| EnvValueBranch {
                when: Condition {
                    os: Some(os),
                    ..Default::default()
                },
                set: cmd,
            })
            .collect();
        // Fallback from the base command, if any.
        if let Some(cmd) = base.and_then(base_script_cmd) {
            branches.push(EnvValueBranch {
                when: Condition::default(),
                set: cmd,
            });
        }

        // Preserve any python/requirements/package-env the base table-form carried.
        let (python, requirements, package_env) = match base {
            Some(ScriptEntry::WithEnv(env)) => (
                env.python.clone(),
                env.requirements.clone(),
                env.package_env,
            ),
            _ => (None, Vec::new(), false),
        };

        out.insert(
            name,
            ScriptEntry::WithEnv(ScriptEnv {
                cmd: EnvValue::Conditional(branches),
                python,
                requirements,
                label: None,
                description: None,
                package_env,
            }),
        );
    }

    PackageScripts { commands: out }
}

/// The flat base command of a script entry, if it has one.
fn base_script_cmd(entry: &ScriptEntry) -> Option<String> {
    match entry {
        ScriptEntry::Plain(s) => Some(s.clone()),
        ScriptEntry::WithEnv(env) => match &env.cmd {
            EnvValue::Flat(s) => Some(s.clone()),
            // A conditional base cmd has no single fallback string; let its
            // own variants stand (this shape didn't exist in the old format).
            EnvValue::Conditional(_) => None,
        },
    }
}

#[cfg(test)]
#[path = "legacy_tests.rs"]
mod tests;
