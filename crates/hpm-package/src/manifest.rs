//! Package manifest types and implementation.
//!
//! `PackageManifest` is the in-memory representation of `hpm.toml`. The
//! section types live in submodules under [`crate::manifest`]:
//!
//! - [`compat`] — `[compat]` (Houdini range, supported platforms)
//! - [`env`][mod@env] — `[runtime]` entries and `EnvMethod`
//! - [`error`] — load-time errors
//! - [`info`] — `[package]` metadata
//! - [`operators`] — `[[operators]]` bundled-operator declarations
//! - [`registry`] — `[[registries]]` entries
//! - [`scripts`] — `[scripts]` entries
//! - [`stage`] — `[stage]` and per-platform place rules
//!
//! Submodule items are re-exported here so downstream callers reach for
//! `hpm_package::manifest::CompatConfig` without needing to know which
//! submodule it lives in.

pub mod compat;
pub mod env;
pub mod error;
pub mod info;
pub mod operators;
pub mod registry;
pub mod scripts;
pub mod stage;
pub mod validation;

pub use compat::CompatConfig;
pub use env::{EnvMethod, ManifestEnvEntry};
pub use error::ManifestLoadError;
pub use info::PackageInfo;
pub use operators::{OperatorDecl, OperatorKind, OperatorSource, SourceResolution};
pub use registry::{RegistryConfig, RegistryType};
pub use scripts::{PackageScripts, ScriptEntry, ScriptEnv};
pub use stage::{PlaceRule, PlatformStaging, StageConfig, StagePlatformRules};
pub use validation::{ValidationLevel, ValidationReport};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::dependency::DependencySpec;
use crate::env_value::{Condition, EnvValue, ExpressionError, HoudiniRange};
use crate::houdini::{HoudiniEnvValue, HoudiniNativePackage, HoudiniPackage, HpackageMetadata};
use crate::package_path::PackagePath;
use crate::platform::Platform;
use crate::python::PythonDependencySpec;

use env::validate_env_table;

/// HPM package manifest (hpm.toml)
///
/// Uses `IndexMap` for dependencies and python_dependencies to preserve
/// insertion order during serialization, ensuring deterministic TOML output.
// No `Default` impl: a manifest without a `package.path` is meaningless.
// Construct via `PackageManifest::new` or full struct literal.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageManifest {
    pub package: PackageInfo,
    #[serde(default, skip_serializing_if = "CompatConfig::is_empty")]
    pub compat: CompatConfig,
    #[serde(default, skip_serializing_if = "StageConfig::is_empty")]
    pub stage: StageConfig,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub registries: Vec<RegistryConfig>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub dependencies: IndexMap<String, DependencySpec>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub python_dependencies: IndexMap<String, PythonDependencySpec>,
    /// `[runtime]` — env-var contributions to the generated Houdini
    /// `package.json`. Replaces the prior `[env]` + `[dev.env]` pair; the
    /// dev/registry distinction now lives in the `when.install_source` axis
    /// on individual conditional variants.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub runtime: IndexMap<String, ManifestEnvEntry>,
    #[serde(default, skip_serializing_if = "PackageScripts::is_empty")]
    pub scripts: PackageScripts,
    /// `[[operators]]` — the operators (node types) this package bundles,
    /// declared by the author. `hpm pack` emits these as a searchable asset
    /// index. Declarations are used rather than parsing the package files
    /// because the HDA format is undocumented/unstable and DSOs do not expose
    /// operator names offline. See [`operators`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operators: Vec<OperatorDecl>,
}

/// Parse `hpm.toml` content into a [`PackageManifest`].
pub fn parse_manifest_str(content: &str) -> Result<PackageManifest, toml::de::Error> {
    toml::from_str(content)
}

impl PackageManifest {
    /// Load and parse a package manifest from `hpm.toml` at the given path.
    ///
    /// Returns [`ManifestLoadError::NotFound`] if the file is missing —
    /// callers that want to treat absence as "no project here" should match
    /// that variant explicitly. I/O and parse errors carry the source path
    /// so users can locate the bad file.
    pub fn from_path(path: &Path) -> Result<Self, ManifestLoadError> {
        if !path.exists() {
            return Err(ManifestLoadError::NotFound {
                path: path.to_path_buf(),
            });
        }
        let content = std::fs::read_to_string(path).map_err(|source| ManifestLoadError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        parse_manifest_str(&content).map_err(|source| ManifestLoadError::Parse {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Create a new package manifest with default values.
    ///
    /// Takes a validated [`PackagePath`] — callers can use
    /// `PackagePath::new("creator/slug").unwrap()` for static identifiers
    /// in tests, or propagate the parse error in production code.
    ///
    /// Pass an empty `Vec` for `authors` when unknown — empty vec encodes
    /// "no authors declared" exactly like the prior `None`.
    pub fn new(
        path: PackagePath,
        name: String,
        version: String,
        description: Option<String>,
        authors: Vec<String>,
        license: Option<String>,
    ) -> Self {
        Self {
            package: PackageInfo {
                path,
                name,
                version,
                description,
                authors,
                license,
                readme: Some("README.md".to_string()),
                homepage: None,
                repository: None,
                documentation: None,
                keywords: vec!["houdini".to_string()],
                categories: Vec::new(),
            },
            compat: CompatConfig {
                // Bounded-major default. `"^21"` lowers to
                // `>=21, <22` — Houdini 21.x only. This gives authors
                // who ship native binaries (`[compat].platforms`) a
                // safe starting point; authors of pure-data / pure-
                // Python packages can widen the range (e.g.
                // `">=20.5, <23"`) after testing.
                houdini: Some(HoudiniRange::parse("^21").expect("default range is well-formed")),
                platforms: Vec::new(),
            },
            stage: StageConfig::default(),
            registries: Vec::new(),
            dependencies: IndexMap::new(),
            python_dependencies: IndexMap::new(),
            runtime: IndexMap::new(),
            scripts: PackageScripts::default(),
            operators: Vec::new(),
        }
    }

    /// Every `[scripts]` entry, in declaration order. Per-host variation
    /// is resolved on-demand via [`ScriptEntry::resolve_cmd`] using the
    /// entry's conditional `cmd` value — there is no merging at this layer.
    pub fn resolved_scripts(&self) -> IndexMap<String, ScriptEntry> {
        self.scripts.commands.clone()
    }

    /// Look up a script entry by name. Per-host variation lives inside the
    /// returned [`ScriptEntry`]'s `cmd` value — call
    /// [`ScriptEntry::resolve_cmd`] on the result with the desired host OS.
    pub fn script_for(&self, name: &str) -> Option<ScriptEntry> {
        self.scripts.commands.get(name).cloned()
    }

    /// Validate the package manifest for structural correctness.
    ///
    /// This is the strict gate: errors here mean the manifest is not
    /// well-formed and downstream operations cannot proceed. For
    /// publish-quality advisory warnings (missing description, authors,
    /// keywords, `[compat].houdini`), use [`Self::validate_with`] with
    /// [`ValidationLevel::Publish`].
    ///
    /// Note: `package.path` is a [`PackagePath`] and was already validated
    /// at deserialization, so it isn't checked again here.
    pub fn validate(&self) -> Result<(), String> {
        let report = self.validate_with(ValidationLevel::Strict);
        match report.errors.into_iter().next() {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    /// Validate at the requested level, returning a [`ValidationReport`]
    /// that separates hard errors from advisory warnings.
    ///
    /// At [`ValidationLevel::Strict`] only structural errors are produced
    /// (no warnings). At [`ValidationLevel::Publish`] the same errors run,
    /// plus advisory warnings on publish-quality fields that are absent:
    /// `package.description`, `package.authors`, `package.keywords`, and
    /// `[compat].houdini`. `hpm check` shows the warnings; a future
    /// `hpm publish` can promote them to errors.
    pub fn validate_with(&self, level: ValidationLevel) -> ValidationReport {
        let mut report = ValidationReport::default();
        self.collect_structural_errors(&mut report);
        if matches!(level, ValidationLevel::Publish) {
            self.collect_publish_warnings(&mut report);
        }
        report
    }

    fn collect_structural_errors(&self, report: &mut ValidationReport) {
        // Stop at the first structural error per top-level concern so a
        // bad name doesn't drown out an unrelated bad stage entry — but
        // unrelated concerns still run independently so the report covers
        // as much of the manifest as possible in one pass.
        if self.package.name.is_empty() {
            report
                .errors
                .push("Package name cannot be empty".to_string());
        }
        if self.package.version.is_empty() {
            report
                .errors
                .push("Package version cannot be empty".to_string());
        } else if !self.is_valid_semver(&self.package.version) {
            report
                .errors
                .push("Package version must be valid semantic version".to_string());
        }

        // `[compat].houdini` and `[compat].platforms` both validate at
        // deserialize time — `HoudiniRange` via its newtype, and
        // `Vec<Platform>` via `Platform`'s `TryFrom<String>` — so neither
        // needs a syntax check here.

        // Validate [stage]: per-platform keys must appear in [compat].platforms,
        // and place rules must declare both `from` and `to`. Place rules with
        // empty `from` would match nothing useful; reject those at load time.
        // The same checks apply to per-profile place tables under
        // `[stage.profile.<name>.platform.<plat>]`.
        self.validate_platform_staging("stage.platform", &self.stage.platform, report);
        for (profile, rules) in &self.stage.profile.entries {
            let label = format!("stage.profile.{}.platform", profile);
            self.validate_platform_staging(&label, &rules.platform, report);
        }

        // Validate [runtime] entries: a missing value is only legal as a
        // required-placeholder for project-level [runtime] to fill in.
        // Conditional values get every branch's `when` selector compiled here
        // so malformed expressions surface at manifest load time, not at
        // install/emit time.
        if let Err(e) = validate_env_table("runtime", &self.runtime) {
            report.errors.push(e);
        }

        // Validate [[operators]]: each declaration must carry a non-empty
        // `type_name` and `category` — those are the fields the index keys on,
        // and an empty value would publish a useless entry. A per-platform
        // `source` table must key on platforms declared in [compat].platforms,
        // mirroring the [stage.platform.*] check.
        for (i, op) in self.operators.iter().enumerate() {
            if op.type_name.trim().is_empty() {
                report.errors.push(format!(
                    "[[operators]][{}]: `type_name` must not be empty",
                    i
                ));
            }
            if op.category.trim().is_empty() {
                report.errors.push(format!(
                    "[[operators]][{}]: `category` must not be empty",
                    i
                ));
            }
            if let Some(OperatorSource::PerPlatform(map)) = &op.source {
                for key in map.keys() {
                    match key.parse::<Platform>() {
                        Ok(platform) => {
                            if !self.compat.platforms.contains(&platform) {
                                report.errors.push(format!(
                                    "[[operators]][{}].source: platform '{}' not listed in [compat].platforms",
                                    i, key
                                ));
                            }
                        }
                        Err(e) => report
                            .errors
                            .push(format!("[[operators]][{}].source.{}: {}", i, key, e)),
                    }
                }
            }
        }

        // Validate [scripts] entries: a conditional `cmd` may only gate on
        // the `os` axis. Other axes (`houdini`, `python`, `install_source`)
        // require runtime context HPM doesn't have at `hpm run` time, so we
        // reject them up front rather than silently dropping variants.
        for (name, entry) in &self.scripts.commands {
            let ScriptEntry::WithEnv(env) = entry else {
                continue;
            };
            let EnvValue::Conditional(variants) = &env.cmd else {
                continue;
            };
            if variants.is_empty() {
                report.errors.push(format!(
                    "script '{}': conditional cmd list must not be empty",
                    name
                ));
                continue;
            }
            for variant in variants {
                if variant.when.houdini.is_some()
                    || variant.when.python.is_some()
                    || variant.when.install_source.is_some()
                {
                    report.errors.push(format!(
                        "script '{}': only the `os` axis is supported in script `when` selectors; \
                         `houdini`, `python`, and `install_source` axes have no meaning at `hpm run` time",
                        name
                    ));
                    continue;
                }
                if let Some(os) = &variant.when.os
                    && let Err(e) = crate::env_value::compile_condition(&Condition {
                        os: Some(os.clone()),
                        ..Default::default()
                    })
                {
                    report.errors.push(format!("script '{}': {}", name, e));
                }
            }
        }
    }

    /// Validate a `[stage(.profile.<name>)?.platform.*]` table: each platform
    /// key must parse and appear in `[compat].platforms`, and every place rule
    /// must declare a non-empty `from` and `to`. `label` is the table prefix
    /// used in error messages (e.g. `"stage.platform"`).
    fn validate_platform_staging(
        &self,
        label: &str,
        staging: &PlatformStaging,
        report: &mut ValidationReport,
    ) {
        for (platform_str, rules) in &staging.entries {
            let platform = match platform_str.parse::<Platform>() {
                Ok(p) => p,
                Err(e) => {
                    report
                        .errors
                        .push(format!("[{}.{}]: {}", label, platform_str, e));
                    continue;
                }
            };
            if !self.compat.platforms.contains(&platform) {
                report.errors.push(format!(
                    "[{}.{}] declared but '{}' not listed in [compat].platforms",
                    label, platform_str, platform_str
                ));
            }
            for (i, rule) in rules.place.iter().enumerate() {
                if rule.from.trim().is_empty() {
                    report.errors.push(format!(
                        "[{}.{}].place[{}]: `from` must not be empty",
                        label, platform_str, i
                    ));
                }
                if rule.to.trim().is_empty() {
                    report.errors.push(format!(
                        "[{}.{}].place[{}]: `to` must not be empty (use \"./\" for the archive root)",
                        label, platform_str, i
                    ));
                }
            }
        }
    }

    fn collect_publish_warnings(&self, report: &mut ValidationReport) {
        if self.package.description.is_none() {
            report.warnings.push(
                "Package description is missing - consider adding one for better discoverability"
                    .to_string(),
            );
        }
        if self.package.authors.is_empty() {
            report.warnings.push(
                "Package authors are missing - consider adding author information".to_string(),
            );
        }
        if self.package.keywords.is_empty() {
            report.warnings.push(
                "Package keywords are missing - consider adding keywords for better discoverability"
                    .to_string(),
            );
        }
        if self.compat.houdini.is_none() {
            report.warnings.push(
                "[compat].houdini is missing - consider declaring a Houdini version range"
                    .to_string(),
            );
        }
    }

    /// Generate Houdini package.json from manifest
    pub fn generate_houdini_package(&self) -> Result<HoudiniPackage, ExpressionError> {
        let mut hpath = vec![];
        let mut env = vec![];

        // Point hpath at the package root so Houdini auto-discovers convention
        // subdirs (otls/, desktop/, toolbar/, python_panels/, viewer_states/,
        // python3.11libs/, etc.). See sidefx.com/docs/houdini/ref/plugins.html.
        hpath.push("$HPM_PACKAGE_ROOT".to_string());

        // Python path environment
        let mut python_env = HashMap::new();
        python_env.insert(
            "PYTHONPATH".to_string(),
            HoudiniEnvValue::prepend("$HPM_PACKAGE_ROOT/python"),
        );
        env.push(python_env);

        // Scripts path environment
        let mut scripts_env = HashMap::new();
        scripts_env.insert(
            "HOUDINI_SCRIPT_PATH".to_string(),
            HoudiniEnvValue::prepend("$HPM_PACKAGE_ROOT/scripts"),
        );
        env.push(scripts_env);

        // Append user-defined env vars from [runtime]. The standalone
        // generator has no install context, so it filters as a published
        // (non-dev) consumer would: `install_source = "dev"` branches drop
        // out, the rest go through. Required-but-unsupplied placeholders
        // are skipped — project sync supplies them via `[runtime]`
        // overrides and errors if any remain unsupplied.
        for (key, entry) in &self.runtime {
            let Some(houdini_value) = entry.to_houdini_env_value()? else {
                continue;
            };
            let mut env_map = HashMap::new();
            env_map.insert(key.clone(), houdini_value);
            env.push(env_map);
        }

        let enable = self
            .compat
            .houdini
            .as_ref()
            .map(HoudiniRange::to_enable_expression);

        Ok(HoudiniPackage {
            hpath: Some(hpath),
            env: Some(env),
            enable,
            requires: None,
            recommends: None,
        })
    }

    /// Generate a Houdini-native package.json for direct use by Houdini's package system.
    ///
    /// Returns `(filename, package)` where filename is `{slug}.json`.
    /// Uses `$HOUDINI_PACKAGE_PATH/{slug}` instead of `$HPM_PACKAGE_ROOT`.
    pub fn generate_houdini_native_package(
        &self,
    ) -> Result<(String, HoudiniNativePackage), String> {
        let slug = self.package.slug().to_string();

        let pkg_root = format!("$HOUDINI_PACKAGE_PATH/{}", slug);

        // Build PKG_{SLUG_UPPER} env var name
        let slug_upper = slug.replace('-', "_").to_uppercase();
        let pkg_var_name = format!("PKG_{}", slug_upper);

        // First env entry: PKG_{SLUG_UPPER} pointing to package root
        let mut env = Vec::new();
        let mut pkg_env = HashMap::new();
        pkg_env.insert(pkg_var_name, HoudiniEnvValue::simple(&pkg_root));
        env.push(pkg_env);

        // User-defined env vars with $HPM_PACKAGE_ROOT replaced. This
        // generator emits the bundled `{slug}.json` shipped inside a packed
        // archive, so it filters as a published consumer would (is_dev=false).
        // Required-but-unsupplied placeholders are skipped (see
        // generate_houdini_package). Conditional value branches each get the
        // same substitution applied before the conditional-object array is
        // emitted.
        for (key, entry) in &self.runtime {
            let Some(houdini_value) = entry
                .lower(&[("$HPM_PACKAGE_ROOT", &pkg_root)], false)
                .map_err(|e| e.to_string())?
            else {
                continue;
            };
            let mut env_map = HashMap::new();
            env_map.insert(key.clone(), houdini_value);
            env.push(env_map);
        }

        let enable = self
            .compat
            .houdini
            .as_ref()
            .map(HoudiniRange::to_enable_expression);

        // Build requires from dependency keys (slug portion only)
        let requires = if self.dependencies.is_empty() {
            None
        } else {
            Some(
                self.dependencies
                    .keys()
                    .map(|key| {
                        key.split_once('/')
                            .map(|(_, s)| s)
                            .unwrap_or(key)
                            .to_string()
                    })
                    .collect(),
            )
        };

        let filename = format!("{}.json", slug);
        let package = HoudiniNativePackage {
            name: slug,
            hpath: pkg_root,
            load_package_once: true,
            show: true,
            enable,
            env,
            requires,
            hpackage: HpackageMetadata {
                version: self.package.version.clone(),
            },
        };

        Ok((filename, package))
    }

    fn is_valid_semver(&self, version: &str) -> bool {
        // Delegate to the semver crate so we accept pre-release and build
        // metadata (`1.0.0-alpha.1`, `1.0.0+build.5`) — the prior hand-
        // rolled `major.minor.patch` u32 split rejected both.
        semver::Version::parse(version).is_ok()
    }
}

#[cfg(test)]
#[path = "manifest_tests.rs"]
mod tests;
