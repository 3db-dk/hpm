//! Package manifest types and implementation.
//!
//! `PackageManifest` is the in-memory representation of `hpm.toml`. The
//! section types live in submodules under [`crate::manifest`]:
//!
//! - [`compat`] — `[compat]` (Houdini range, supported platforms)
//! - [`env`] — `[runtime]` entries and `EnvMethod`
//! - [`error`] — load-time errors
//! - [`info`] — `[package]` metadata
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
pub mod registry;
pub mod scripts;
pub mod stage;

pub use compat::CompatConfig;
pub use env::{EnvMethod, ManifestEnvEntry};
pub use error::ManifestLoadError;
pub use info::PackageInfo;
pub use registry::{RegistryConfig, RegistryType};
pub use scripts::{PackageScripts, ScriptEntry, ScriptEnv};
pub use stage::{PlaceRule, PlatformStaging, StageConfig, StagePlatformRules};

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
        toml::from_str(&content).map_err(|source| ManifestLoadError::Parse {
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

    /// Validate the package manifest for common errors.
    ///
    /// Note: `package.path` is a [`PackagePath`] and was already validated
    /// at deserialization, so it isn't checked again here.
    pub fn validate(&self) -> Result<(), String> {
        if self.package.name.is_empty() {
            return Err("Package name cannot be empty".to_string());
        }

        if self.package.version.is_empty() {
            return Err("Package version cannot be empty".to_string());
        }

        // Basic semver validation
        if !self.is_valid_semver(&self.package.version) {
            return Err("Package version must be valid semantic version".to_string());
        }

        // `[compat].houdini` and `[compat].platforms` both validate at
        // deserialize time — `HoudiniRange` via its newtype, and
        // `Vec<Platform>` via `Platform`'s `TryFrom<String>` — so neither
        // needs a syntax check here.

        // Validate [stage]: per-platform keys must appear in [compat].platforms,
        // and place rules must declare both `from` and `to`. Place rules with
        // empty `from` would match nothing useful; reject those at load time.
        for (platform_str, rules) in &self.stage.platform.entries {
            let platform = platform_str
                .parse::<Platform>()
                .map_err(|e| format!("[stage.platform.{}]: {}", platform_str, e))?;
            if !self.compat.platforms.contains(&platform) {
                return Err(format!(
                    "[stage.platform.{}] declared but '{}' not listed in [compat].platforms",
                    platform_str, platform_str
                ));
            }
            for (i, rule) in rules.place.iter().enumerate() {
                if rule.from.trim().is_empty() {
                    return Err(format!(
                        "[stage.platform.{}].place[{}]: `from` must not be empty",
                        platform_str, i
                    ));
                }
                if rule.to.trim().is_empty() {
                    return Err(format!(
                        "[stage.platform.{}].place[{}]: `to` must not be empty (use \"./\" for the archive root)",
                        platform_str, i
                    ));
                }
            }
        }

        // Validate [runtime] entries: a missing value is only legal as a
        // required-placeholder for project-level [runtime] to fill in.
        // Conditional values get every branch's `when` selector compiled here
        // so malformed expressions surface at manifest load time, not at
        // install/emit time.
        validate_env_table("runtime", &self.runtime)?;

        // Validate [scripts] entries: a conditional `cmd` may only gate on
        // the `os` axis. Other axes (`houdini`, `python`, `install_source`)
        // require runtime context HPM doesn't have at `hpm run` time, so we
        // reject them up front rather than silently dropping variants.
        {
            for (name, entry) in &self.scripts.commands {
                let ScriptEntry::WithEnv(env) = entry else {
                    continue;
                };
                let EnvValue::Conditional(variants) = &env.cmd else {
                    continue;
                };
                if variants.is_empty() {
                    return Err(format!(
                        "script '{}': conditional cmd list must not be empty",
                        name
                    ));
                }
                for variant in variants {
                    if variant.when.houdini.is_some()
                        || variant.when.python.is_some()
                        || variant.when.install_source.is_some()
                    {
                        return Err(format!(
                            "script '{}': only the `os` axis is supported in script `when` selectors; \
                             `houdini`, `python`, and `install_source` axes have no meaning at `hpm run` time",
                            name
                        ));
                    }
                    if let Some(os) = &variant.when.os {
                        crate::env_value::compile_condition(&Condition {
                            os: Some(os.clone()),
                            ..Default::default()
                        })
                        .map_err(|e| format!("script '{}': {}", name, e))?;
                    }
                }
            }
        }

        Ok(())
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
            HoudiniEnvValue::Detailed {
                method: "prepend".to_string(),
                value: "$HPM_PACKAGE_ROOT/python".to_string(),
            },
        );
        env.push(python_env);

        // Scripts path environment
        let mut scripts_env = HashMap::new();
        scripts_env.insert(
            "HOUDINI_SCRIPT_PATH".to_string(),
            HoudiniEnvValue::Detailed {
                method: "prepend".to_string(),
                value: "$HPM_PACKAGE_ROOT/scripts".to_string(),
            },
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
mod tests {
    use super::*;

    // empty_name/empty_version validation tests removed - covered by
    // prop_malformed_package_names_rejected and prop_malformed_versions_rejected
    // in proptest_helpers.rs which test validation with randomized inputs

    #[test]
    fn houdini_package_no_version_constraints() {
        // Edge case: Houdini package generation without version constraints
        let mut manifest = PackageManifest::new(
            PackagePath::new("studio/test").unwrap(),
            "Test".to_string(),
            "1.0.0".to_string(),
            None,
            Vec::new(),
            None,
        );

        manifest.compat = CompatConfig {
            houdini: None,
            platforms: Vec::new(),
        };

        let houdini_pkg = manifest
            .generate_houdini_package()
            .expect("test manifest produces valid Houdini expr");
        assert!(houdini_pkg.enable.is_none());
    }

    #[test]
    fn compat_houdini_compiles_to_enable_expression() {
        let toml_str = r#"
[package]
path = "studio/test"
name = "Test"
version = "1.0.0"

[compat]
houdini = ">=20.5, <22"
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let pkg = manifest
            .generate_houdini_package()
            .expect("test manifest produces valid Houdini expr");
        assert_eq!(
            pkg.enable.as_deref(),
            Some("(houdini_version >= '20.5' and houdini_version < '22')")
        );
    }

    #[test]
    fn compat_houdini_invalid_range_rejected_at_parse() {
        // HoudiniRange validates at deserialize time, so a malformed
        // range fails the TOML parse rather than reaching validate().
        let toml_str = r#"
[package]
path = "studio/test"
name = "Test"
version = "1.0.0"

[compat]
houdini = "not-a-version"
"#;
        let err = toml::from_str::<PackageManifest>(toml_str)
            .expect_err("invalid houdini range should fail at deserialize");
        let msg = err.to_string();
        assert!(
            msg.contains("houdini") || msg.contains("version requirement"),
            "error must point at the houdini range: {msg}"
        );
    }

    #[test]
    fn compat_houdini_min_extracts_lower_bound() {
        let compat = CompatConfig {
            houdini: Some(HoudiniRange::parse(">=20.5, <22").unwrap()),
            platforms: Vec::new(),
        };
        assert_eq!(compat.houdini_min(), Some("20.5".to_string()));
        let compat = CompatConfig {
            houdini: Some(HoudiniRange::parse("^21").unwrap()),
            platforms: Vec::new(),
        };
        assert_eq!(compat.houdini_min(), Some("21".to_string()));
        let compat = CompatConfig::default();
        assert_eq!(compat.houdini_min(), None);
    }

    #[test]
    fn registries_deserialize_from_toml() {
        let toml_str = r#"
[package]
path = "studio/my-context"
name = "My Context"
version = "0.1.0"

[[registries]]
name = "houdinihub"
url = "https://api.3db.dk/v1/registry"
type = "api"

[[registries]]
name = "studio-internal"
url = "https://packages.studio.com/v1/registry"
type = "git"

[dependencies]
"studio/test" = "0.2.0"
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let registries = manifest.registries;
        assert_eq!(registries.len(), 2);
        assert_eq!(registries[0].name, "houdinihub");
        assert_eq!(registries[0].registry_type, RegistryType::Api);
        assert_eq!(registries[1].name, "studio-internal");
        assert_eq!(registries[1].registry_type, RegistryType::Git);
    }

    #[test]
    fn runtime_deserialize_from_toml() {
        let toml_str = r#"
[package]
path = "studio/my-package"
name = "My Package"
version = "0.1.0"

[runtime]
MY_PLUGIN_ROOT = { method = "set", value = "$HPM_PACKAGE_ROOT/config" }
HOUDINI_TOOLBAR_PATH = { method = "prepend", value = "$HPM_PACKAGE_ROOT/toolbar" }
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let runtime = manifest.runtime;
        assert_eq!(runtime.len(), 2);
        assert_eq!(runtime["MY_PLUGIN_ROOT"].method, EnvMethod::Set);
        assert_eq!(
            runtime["MY_PLUGIN_ROOT"]
                .value
                .as_ref()
                .and_then(EnvValue::as_flat),
            Some("$HPM_PACKAGE_ROOT/config")
        );
        assert!(!runtime["MY_PLUGIN_ROOT"].required);
        assert_eq!(runtime["HOUDINI_TOOLBAR_PATH"].method, EnvMethod::Prepend);
    }

    #[test]
    fn runtime_required_without_value_deserializes() {
        let toml_str = r#"
[package]
path = "studio/my-package"
name = "My Package"
version = "0.1.0"

[runtime]
PROJECT_ROOT = { method = "set", required = true }
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let runtime = &manifest.runtime;
        assert_eq!(runtime["PROJECT_ROOT"].method, EnvMethod::Set);
        assert!(runtime["PROJECT_ROOT"].value.is_none());
        assert!(runtime["PROJECT_ROOT"].required);
        assert!(manifest.validate().is_ok());
    }

    #[test]
    fn runtime_missing_value_without_required_is_invalid() {
        // serde happily accepts the missing value (it's now Option), but
        // validate() rejects it because non-required entries must declare a
        // value.
        let toml_str = r#"
[package]
path = "studio/my-package"
name = "My Package"
version = "0.1.0"

[runtime]
LEAKED = { method = "set" }
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let err = manifest.validate().unwrap_err();
        assert!(err.contains("LEAKED"));
        assert!(err.contains("required"));
    }

    #[test]
    fn generate_houdini_package_skips_required_placeholders() {
        let mut manifest = PackageManifest::new(
            PackagePath::new("studio/test").unwrap(),
            "Test".to_string(),
            "1.0.0".to_string(),
            None,
            Vec::new(),
            None,
        );
        let mut runtime = IndexMap::new();
        runtime.insert(
            "PROJECT_ROOT".to_string(),
            ManifestEnvEntry {
                method: EnvMethod::Set,
                value: None,
                required: true,
            },
        );
        runtime.insert(
            "WITH_VALUE".to_string(),
            ManifestEnvEntry {
                method: EnvMethod::Set,
                value: Some("/somewhere".into()),
                required: false,
            },
        );
        manifest.runtime = runtime;

        let pkg = manifest
            .generate_houdini_package()
            .expect("test manifest produces valid Houdini expr");
        let env_list = pkg.env.unwrap();
        // 2 hardcoded (PYTHONPATH, HOUDINI_SCRIPT_PATH) + WITH_VALUE only.
        assert_eq!(env_list.len(), 3);
        assert!(
            env_list.iter().all(|m| !m.contains_key("PROJECT_ROOT")),
            "required-but-unsupplied placeholder should be skipped"
        );
        assert!(env_list.iter().any(|m| m.contains_key("WITH_VALUE")));
    }

    #[test]
    fn runtime_invalid_method_rejected() {
        let toml_str = r#"
[package]
path = "studio/my-package"
name = "My Package"
version = "0.1.0"

[runtime]
MY_VAR = { method = "invalid", value = "foo" }
"#;
        let result: Result<PackageManifest, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn runtime_install_source_dev_variant_drops_for_published_consumer() {
        // The HDK plugin pattern, expressed in the new shape. A single
        // [runtime] entry with two variants: dev-only build path + the
        // fallback published location. For a published consumer the dev
        // variant is filtered out so only the fallback ships.
        let toml_str = r#"
[package]
path = "studio/my-package"
name = "My Package"
version = "0.1.0"

[runtime.HOUDINI_DSO_PATH]
method = "prepend"
value = [
  { when = { install_source = "dev" }, set = "$HPM_PACKAGE_ROOT/build/Release" },
  { when = {}, set = "$HPM_PACKAGE_ROOT/dso" },
]
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        assert!(manifest.validate().is_ok());

        let pkg = manifest
            .generate_houdini_package()
            .expect("test manifest produces valid Houdini expr");
        let env_list = pkg.env.unwrap();
        // The HOUDINI_DSO_PATH entry must appear, but only the fallback
        // variant should be present (dev gate dropped).
        let dso_entry = env_list
            .iter()
            .find(|m| m.contains_key("HOUDINI_DSO_PATH"))
            .expect("HOUDINI_DSO_PATH should be emitted for published consumer");
        let value = &dso_entry["HOUDINI_DSO_PATH"];
        match value {
            HoudiniEnvValue::DetailedConditional { value, .. } => {
                assert_eq!(value.len(), 1);
                // The single surviving branch is the empty `when = {}` fallback,
                // which lowers to the literal "true" expression.
                assert_eq!(value[0]["true"], "$HPM_PACKAGE_ROOT/dso");
            }
            other => panic!("expected conditional value, got {other:?}"),
        }
    }

    #[test]
    fn runtime_install_source_only_drops_entry_for_published_consumer() {
        // When every variant is gated `install_source = "dev"`, the entry
        // disappears from a published consumer's package.json entirely.
        let toml_str = r#"
[package]
path = "studio/my-package"
name = "My Package"
version = "0.1.0"

[runtime.HOUDINI_DSO_PATH]
method = "prepend"
value = [
  { when = { install_source = "dev" }, set = "$HPM_PACKAGE_ROOT/build/Release" },
]
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let pkg = manifest
            .generate_houdini_package()
            .expect("test manifest produces valid Houdini expr");
        let env_list = pkg.env.unwrap();
        assert!(
            env_list.iter().all(|m| !m.contains_key("HOUDINI_DSO_PATH")),
            "dev-only entries must not leak into the published Houdini manifest"
        );
    }

    #[test]
    fn runtime_none_when_absent() {
        let toml_str = r#"
[package]
path = "studio/my-package"
name = "My Package"
version = "0.1.0"
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        assert!(manifest.runtime.is_empty());
    }

    #[test]
    fn generate_houdini_package_includes_user_env() {
        let mut manifest = PackageManifest::new(
            PackagePath::new("studio/test").unwrap(),
            "Test".to_string(),
            "1.0.0".to_string(),
            None,
            Vec::new(),
            None,
        );

        let mut runtime = IndexMap::new();
        runtime.insert(
            "MY_VAR".to_string(),
            ManifestEnvEntry {
                method: EnvMethod::Set,
                value: Some("$HPM_PACKAGE_ROOT/data".into()),
                required: false,
            },
        );
        manifest.runtime = runtime;

        let houdini_pkg = manifest
            .generate_houdini_package()
            .expect("test manifest produces valid Houdini expr");
        assert_eq!(
            houdini_pkg.hpath,
            Some(vec!["$HPM_PACKAGE_ROOT".to_string()])
        );
        let env_list = houdini_pkg.env.unwrap();
        // 2 hardcoded (PYTHONPATH, HOUDINI_SCRIPT_PATH) + 1 user-defined
        assert_eq!(env_list.len(), 3);
        let last = &env_list[2];
        let val = last.get("MY_VAR").unwrap();
        match val {
            HoudiniEnvValue::Detailed { method, value } => {
                assert_eq!(method, "set");
                assert_eq!(value, "$HPM_PACKAGE_ROOT/data");
            }
            _ => panic!("Expected Detailed variant"),
        }
    }

    #[test]
    fn stage_deserialize_from_toml() {
        let toml_str = r#"
[package]
path = "studio/my-native-pkg"
name = "My Native Pkg"
version = "1.0.0"

[compat]
platforms = ["linux-x86_64", "macos-aarch64"]

[stage]
prepack = ["build-dso"]
include = ["python/**"]
exclude = ["src/**", "build/**"]

[stage.platform.linux-x86_64]
place = [
  { from = "build/linux/*.so", to = "dso/" },
]

[stage.platform.macos-aarch64]
place = [
  { from = "build/macos/*.dylib", to = "dso/" },
]
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        manifest.validate().unwrap();
        let compat = manifest.compat;
        assert_eq!(compat.platforms.len(), 2);
        let stage = &manifest.stage;
        assert_eq!(stage.prepack, vec!["build-dso".to_string()]);
        assert_eq!(stage.platform.entries.len(), 2);
        assert_eq!(
            stage.platform.entries["linux-x86_64"].place[0].from,
            "build/linux/*.so"
        );
        assert_eq!(stage.platform.entries["linux-x86_64"].place[0].to, "dso/");
    }

    #[test]
    fn stage_empty_when_absent() {
        let toml_str = r#"
[package]
path = "studio/my-package"
name = "My Package"
version = "0.1.0"
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        assert!(manifest.stage.is_empty());
    }

    #[test]
    fn compat_platforms_unknown_rejected() {
        // Unknown platform identifiers are rejected at deserialize time
        // by `Platform::TryFrom<String>`, so the manifest fails to parse
        // before validate ever runs.
        let toml_str = r#"
[package]
path = "studio/test"
name = "Test"
version = "1.0.0"

[compat]
platforms = ["linux-arm64"]
"#;
        let err = toml::from_str::<PackageManifest>(toml_str).unwrap_err();
        assert!(err.to_string().contains("linux-arm64"));
    }

    #[test]
    fn stage_platform_not_in_compat_rejected() {
        let mut manifest = PackageManifest::new(
            PackagePath::new("studio/test").unwrap(),
            "Test".to_string(),
            "1.0.0".to_string(),
            None,
            Vec::new(),
            None,
        );
        manifest.compat = CompatConfig {
            houdini: None,
            platforms: vec![Platform::LinuxX86_64],
        };
        let mut entries = IndexMap::new();
        entries.insert(
            "windows-x86_64".to_string(),
            StagePlatformRules {
                place: vec![PlaceRule {
                    from: "lib/*".to_string(),
                    to: "lib/".to_string(),
                }],
            },
        );
        manifest.stage = StageConfig {
            platform: PlatformStaging { entries },
            ..Default::default()
        };
        let err = manifest.validate().unwrap_err();
        assert!(err.contains("not listed in [compat].platforms"), "{err}");
    }

    #[test]
    fn stage_place_empty_from_rejected() {
        let mut manifest = PackageManifest::new(
            PackagePath::new("studio/test").unwrap(),
            "Test".to_string(),
            "1.0.0".to_string(),
            None,
            Vec::new(),
            None,
        );
        manifest.compat = CompatConfig {
            houdini: None,
            platforms: vec![Platform::LinuxX86_64],
        };
        let mut entries = IndexMap::new();
        entries.insert(
            "linux-x86_64".to_string(),
            StagePlatformRules {
                place: vec![PlaceRule {
                    from: "".to_string(),
                    to: "dso/".to_string(),
                }],
            },
        );
        manifest.stage = StageConfig {
            platform: PlatformStaging { entries },
            ..Default::default()
        };
        let err = manifest.validate().unwrap_err();
        assert!(err.contains("`from` must not be empty"), "{err}");
    }

    // Path-format validation lives in `package_path.rs` — see
    // `PackagePath`'s tests for the well-formed/malformed cases.

    #[test]
    fn package_info_helpers() {
        let info = PackageInfo {
            path: PackagePath::new("tumblehead/tumble-rig").unwrap(),
            name: "TumbleRig".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            authors: Vec::new(),
            license: None,
            readme: None,
            homepage: None,
            repository: None,
            documentation: None,
            keywords: Vec::new(),
            categories: Vec::new(),
        };
        assert_eq!(info.identifier(), "tumblehead/tumble-rig");
        assert_eq!(info.creator(), "tumblehead");
        assert_eq!(info.slug(), "tumble-rig");
    }

    #[test]
    fn manifest_toml_roundtrip_with_path() {
        let manifest = PackageManifest::new(
            PackagePath::new("tumblehead/tumble-rig").unwrap(),
            "TumbleRig".to_string(),
            "1.0.0".to_string(),
            Some("A rig tool".to_string()),
            Vec::new(),
            Some("MIT".to_string()),
        );
        let toml_str = toml::to_string(&manifest).unwrap();
        assert!(toml_str.contains("path = \"tumblehead/tumble-rig\""));
        assert!(toml_str.contains("name = \"TumbleRig\""));

        let deserialized: PackageManifest = toml::from_str(&toml_str).unwrap();
        assert_eq!(deserialized.package.path, "tumblehead/tumble-rig");
        assert_eq!(deserialized.package.name, "TumbleRig");
    }

    #[test]
    fn registries_none_when_absent() {
        let toml_str = r#"
[package]
path = "studio/my-context"
name = "My Context"
version = "0.1.0"
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        assert!(manifest.registries.is_empty());
    }

    #[test]
    fn generate_houdini_native_package_full() {
        let toml_str = r#"
[package]
path = "creator/my-tool"
name = "My Cool Tool"
version = "1.2.3"

[compat]
houdini = ">=21.0"

[dependencies]
"studio/some-dep" = "1.0.0"

[runtime]
MY_VAR = { method = "prepend", value = "$HPM_PACKAGE_ROOT/scripts" }
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let (filename, pkg) = manifest.generate_houdini_native_package().unwrap();

        assert_eq!(filename, "my-tool.json");
        assert_eq!(pkg.name, "my-tool");
        assert_eq!(pkg.hpath, "$HOUDINI_PACKAGE_PATH/my-tool");
        assert!(pkg.load_package_once);
        assert!(pkg.show);
        assert_eq!(pkg.enable.as_deref(), Some("houdini_version >= '21.0'"));
        assert_eq!(pkg.hpackage.version, "1.2.3");

        // First env entry is PKG_MY_TOOL
        let first_env = &pkg.env[0];
        assert!(first_env.contains_key("PKG_MY_TOOL"));

        // Second env entry has $HPM_PACKAGE_ROOT replaced
        let second_env = &pkg.env[1];
        let my_var = second_env.get("MY_VAR").unwrap();
        match my_var {
            crate::houdini::HoudiniEnvValue::Detailed { value, method } => {
                assert_eq!(value, "$HOUDINI_PACKAGE_PATH/my-tool/scripts");
                assert_eq!(method, "prepend");
            }
            _ => panic!("Expected Detailed variant"),
        }

        // Requires uses slug only
        assert_eq!(pkg.requires, Some(vec!["some-dep".to_string()]));
    }

    #[test]
    fn generate_houdini_native_package_no_deps_no_houdini() {
        let toml_str = r#"
[package]
path = "studio/bare-pkg"
name = "Bare Package"
version = "0.1.0"
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let (filename, pkg) = manifest.generate_houdini_native_package().unwrap();

        assert_eq!(filename, "bare-pkg.json");
        assert!(pkg.enable.is_none());
        assert!(pkg.requires.is_none());
        // Only the PKG_ env entry
        assert_eq!(pkg.env.len(), 1);
        assert!(pkg.env[0].contains_key("PKG_BARE_PKG"));
    }

    #[test]
    fn scripts_flat_map_roundtrip() {
        let toml_str = r#"
[package]
path = "studio/my-package"
name = "My Package"
version = "0.1.0"

[scripts]
build = "cargo build"
test = "cargo test"
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let scripts = manifest.scripts;
        assert_eq!(scripts.commands.len(), 2);
        assert_eq!(
            scripts.commands["build"].resolve_cmd(None),
            Some("cargo build".to_string())
        );
        assert_eq!(
            scripts.commands["test"].resolve_cmd(None),
            Some("cargo test".to_string())
        );

        // Plain entries don't carry venv hints.
        assert!(!scripts.commands["build"].needs_venv());

        // Preserves declaration order in the flattened commands map.
        let names: Vec<&String> = scripts.commands.keys().collect();
        assert_eq!(names, vec!["build", "test"]);
    }

    #[test]
    fn scripts_conditional_cmd_resolves_per_host_os() {
        let toml_str = r#"
[package]
path = "studio/claudini"
name = "Claudini"
version = "1.0.0"

[scripts]
build = "cargo build"

[scripts.register]
cmd = [
  { when = { os = "windows" }, set = "\"$HPM_PACKAGE_ROOT/plugin/bin/claudini2.exe\" register" },
  { when = { os = "macos"   }, set = "\"$HPM_PACKAGE_ROOT/plugin/bin/claudini2\" register" },
  { when = { os = "linux"   }, set = "\"$HPM_PACKAGE_ROOT/plugin/bin/claudini2\" register" },
]
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        manifest.validate().unwrap();

        let scripts = manifest.scripts;
        // Plain shorthand resolves unconditionally.
        assert_eq!(
            scripts.commands["build"].resolve_cmd(Some("linux")),
            Some("cargo build".to_string())
        );
        // Conditional cmd picks the host-specific variant.
        let register = &scripts.commands["register"];
        assert!(
            register
                .resolve_cmd(Some("windows"))
                .unwrap()
                .contains("claudini2.exe")
        );
        assert_eq!(
            register.resolve_cmd(Some("macos")),
            Some("\"$HPM_PACKAGE_ROOT/plugin/bin/claudini2\" register".to_string())
        );
        // No host OS supplied → no variant matches.
        assert_eq!(register.resolve_cmd(None), None);
    }

    #[test]
    fn scripts_conditional_with_fallback_branch_matches_any_host() {
        let toml_str = r#"
[package]
path = "studio/tool"
name = "Tool"
version = "1.0.0"

[scripts.register]
cmd = [
  { when = { os = "windows" }, set = "tool.exe register" },
  { when = {},                  set = "tool register" },
]
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let entry = &manifest.scripts.commands["register"];
        assert_eq!(
            entry.resolve_cmd(Some("windows")),
            Some("tool.exe register".to_string())
        );
        // Empty `when = {}` matches any other host as a fallback.
        assert_eq!(
            entry.resolve_cmd(Some("macos")),
            Some("tool register".to_string())
        );
    }

    #[test]
    fn scripts_table_form_with_python_and_requirements() {
        let toml_str = r#"
[package]
path = "studio/tt"
name = "TT"
version = "1.0.0"

[scripts]
build = "cargo build"

[scripts.tt_setup]
cmd = "python scripts/tt_setup.py"
python = "3.11"
requirements = ["PySide6>=6.6"]
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let scripts = manifest.scripts;

        assert_eq!(
            scripts.commands["build"].resolve_cmd(None),
            Some("cargo build".to_string())
        );
        assert!(!scripts.commands["build"].needs_venv());

        let setup = &scripts.commands["tt_setup"];
        assert_eq!(
            setup.resolve_cmd(None),
            Some("python scripts/tt_setup.py".to_string())
        );
        assert_eq!(setup.python(), Some("3.11"));
        assert_eq!(setup.requirements(), &["PySide6>=6.6".to_string()]);
        assert!(setup.needs_venv());
    }

    #[test]
    fn scripts_table_form_inline_object() {
        let toml_str = r#"
[package]
path = "studio/tt"
name = "TT"
version = "1.0.0"

[scripts]
tt_setup = { cmd = "python scripts/tt_setup.py", python = "3.11", requirements = ["PySide6>=6.6"] }
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let setup = &manifest.scripts.commands["tt_setup"];
        assert_eq!(
            setup.resolve_cmd(None),
            Some("python scripts/tt_setup.py".to_string())
        );
        assert_eq!(setup.python(), Some("3.11"));
        assert_eq!(setup.requirements(), &["PySide6>=6.6".to_string()]);
    }

    #[test]
    fn scripts_table_form_without_venv_hints_is_legal() {
        let toml_str = r#"
[package]
path = "studio/tt"
name = "TT"
version = "1.0.0"

[scripts.lint]
cmd = "ruff ."
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let lint = &manifest.scripts.commands["lint"];
        assert_eq!(lint.resolve_cmd(None), Some("ruff .".to_string()));
        assert!(!lint.needs_venv());
    }

    #[test]
    fn scripts_conditional_cmd_with_python_hints() {
        // The table form combines conditional cmd and venv hints.
        let toml_str = r#"
[package]
path = "studio/tt"
name = "TT"
version = "1.0.0"

[scripts.regen]
cmd = [
  { when = { os = "windows" }, set = "python scripts\\regen.py" },
  { when = {},                  set = "python scripts/regen.py" },
]
python = "3.11"
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        manifest.validate().unwrap();
        let regen = &manifest.scripts.commands["regen"];
        assert!(regen.needs_venv());
        assert_eq!(regen.python(), Some("3.11"));
        assert!(
            regen
                .resolve_cmd(Some("linux"))
                .unwrap()
                .contains("scripts/regen.py")
        );
        assert!(
            regen
                .resolve_cmd(Some("windows"))
                .unwrap()
                .contains("scripts\\regen.py")
        );
    }

    #[test]
    fn scripts_when_rejects_non_os_axes() {
        // Only `os` is meaningful in script when selectors — HPM has no
        // Houdini/python/install_source context at `hpm run` time.
        let toml_str = r#"
[package]
path = "studio/tt"
name = "TT"
version = "1.0.0"

[scripts.bad]
cmd = [
  { when = { houdini = "^21" }, set = "x" },
]
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let err = manifest.validate().unwrap_err();
        assert!(
            err.contains("only the `os` axis"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn scripts_when_rejects_install_source() {
        let toml_str = r#"
[package]
path = "studio/tt"
name = "TT"
version = "1.0.0"

[scripts.bad]
cmd = [
  { when = { install_source = "dev" }, set = "x" },
]
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn scripts_absent_resolves_empty() {
        let toml_str = r#"
[package]
path = "studio/pkg"
name = "Pkg"
version = "0.1.0"
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        assert!(manifest.scripts.is_empty());
        assert!(manifest.resolved_scripts().is_empty());
        assert!(manifest.script_for("anything").is_none());
    }

    #[test]
    fn scripts_toml_roundtrip_preserves_conditional_cmd() {
        let toml_str = r#"
[package]
path = "studio/tool"
name = "Tool"
version = "1.0.0"

[scripts]
build = "cargo build"

[scripts.register]
cmd = [
  { when = { os = "windows" }, set = "tool.exe register" },
  { when = {},                  set = "tool register" },
]
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let roundtrip = toml::to_string(&manifest).unwrap();
        let back: PackageManifest = toml::from_str(&roundtrip).unwrap();
        let scripts = back.scripts;
        assert_eq!(
            scripts.commands["build"].resolve_cmd(None),
            Some("cargo build".to_string())
        );
        assert_eq!(
            scripts.commands["register"].resolve_cmd(Some("windows")),
            Some("tool.exe register".to_string())
        );
        assert_eq!(
            scripts.commands["register"].resolve_cmd(Some("macos")),
            Some("tool register".to_string())
        );
    }

    #[test]
    fn generate_houdini_native_package_env_root_replacement() {
        let mut manifest = PackageManifest::new(
            PackagePath::new("studio/test-pkg").unwrap(),
            "Test".to_string(),
            "1.0.0".to_string(),
            None,
            Vec::new(),
            None,
        );
        let mut runtime = IndexMap::new();
        runtime.insert(
            "PATH_A".to_string(),
            ManifestEnvEntry {
                method: EnvMethod::Set,
                value: Some("$HPM_PACKAGE_ROOT/a".into()),
                required: false,
            },
        );
        runtime.insert(
            "PATH_B".to_string(),
            ManifestEnvEntry {
                method: EnvMethod::Append,
                value: Some("$HPM_PACKAGE_ROOT/b:$HPM_PACKAGE_ROOT/c".into()),
                required: false,
            },
        );
        manifest.runtime = runtime;

        let (_, pkg) = manifest.generate_houdini_native_package().unwrap();

        // PATH_A
        match pkg.env[1].get("PATH_A").unwrap() {
            crate::houdini::HoudiniEnvValue::Detailed { value, .. } => {
                assert_eq!(value, "$HOUDINI_PACKAGE_PATH/test-pkg/a");
            }
            _ => panic!("Expected Detailed"),
        }
        // PATH_B with multiple replacements
        match pkg.env[2].get("PATH_B").unwrap() {
            crate::houdini::HoudiniEnvValue::Detailed { value, method } => {
                assert_eq!(
                    value,
                    "$HOUDINI_PACKAGE_PATH/test-pkg/b:$HOUDINI_PACKAGE_PATH/test-pkg/c"
                );
                assert_eq!(method, "append");
            }
            _ => panic!("Expected Detailed"),
        }
    }

    #[test]
    fn env_conditional_value_parses_from_toml() {
        let toml_str = r#"
[package]
path = "studio/multi-houdini"
name = "Multi Houdini"
version = "0.1.0"

[runtime.PXR_PLUGINPATH_NAME]
method = "prepend"
value = [
  { when = { houdini = "^21" }, set = "$HPM_PACKAGE_ROOT/resolver/houdini21/r" },
  { when = { houdini = "^22" }, set = "$HPM_PACKAGE_ROOT/resolver/houdini22/r" },
]
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        manifest.validate().unwrap();
        let entry = manifest.runtime.get("PXR_PLUGINPATH_NAME").unwrap();
        assert_eq!(entry.method, EnvMethod::Prepend);
        match entry.value.as_ref().unwrap() {
            EnvValue::Conditional(v) => {
                assert_eq!(v.len(), 2);
                assert_eq!(
                    v[0].when.houdini.as_ref().map(HoudiniRange::as_str),
                    Some("^21")
                );
                assert_eq!(
                    v[1].when.houdini.as_ref().map(HoudiniRange::as_str),
                    Some("^22")
                );
            }
            EnvValue::Flat(_) => panic!("expected conditional"),
        }
    }

    #[test]
    fn env_conditional_value_lowers_to_houdini_array() {
        let toml_str = r#"
[package]
path = "studio/multi"
name = "Multi"
version = "0.1.0"

[runtime.PXR_PLUGINPATH_NAME]
method = "prepend"
value = [
  { when = { houdini = "^21" }, set = "$HPM_PACKAGE_ROOT/h21/r" },
  { when = { houdini = "^22", os = "linux" }, set = "$HPM_PACKAGE_ROOT/h22/r" },
]
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let entry = manifest.runtime.get("PXR_PLUGINPATH_NAME").unwrap();
        let lowered = entry
            .lower(&[("$HPM_PACKAGE_ROOT", "/abs/pkg")], false)
            .unwrap()
            .unwrap();
        match lowered {
            HoudiniEnvValue::DetailedConditional { method, value } => {
                assert_eq!(method, "prepend");
                assert_eq!(value.len(), 2);
                let first = &value[0];
                let key = first.keys().next().unwrap();
                assert_eq!(key, "houdini_version >= '21' and houdini_version < '22'");
                assert_eq!(first[key], "/abs/pkg/h21/r");
                let second = &value[1];
                let key2 = second.keys().next().unwrap();
                assert_eq!(
                    key2,
                    "houdini_version >= '22' and houdini_version < '23' and houdini_os == 'linux'"
                );
                assert_eq!(second[key2], "/abs/pkg/h22/r");
            }
            _ => panic!("expected DetailedConditional"),
        }
    }

    #[test]
    fn env_conditional_value_serializes_in_native_package() {
        let toml_str = r#"
[package]
path = "studio/multi-pkg"
name = "Multi"
version = "0.1.0"

[runtime.PXR_PLUGINPATH_NAME]
method = "prepend"
value = [
  { when = { houdini = "^21" }, set = "$HPM_PACKAGE_ROOT/h21/r" },
]
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let (_, pkg) = manifest.generate_houdini_native_package().unwrap();
        let json = serde_json::to_string(&pkg).unwrap();
        // The conditional-object array form must round-trip into JSON with
        // method, value, and the embedded expression as the inner object key.
        assert!(json.contains("\"method\":\"prepend\""));
        assert!(json.contains("houdini_version >= '21' and houdini_version < '22'"));
        assert!(json.contains("$HOUDINI_PACKAGE_PATH/multi-pkg/h21/r"));
    }

    #[test]
    fn env_conditional_value_with_invalid_req_fails_at_parse() {
        // Condition.houdini is a HoudiniRange newtype that validates
        // at deserialize, so a malformed range fails the TOML parse
        // rather than reaching validate().
        let toml_str = r#"
[package]
path = "studio/bad"
name = "Bad"
version = "0.1.0"

[runtime.X]
method = "set"
value = [
  { when = { houdini = "garbage" }, set = "x" },
]
"#;
        // Untagged enum (EnvValue) flattens the inner HoudiniRange
        // error into a generic "did not match any variant" message, so we
        // can only assert the parse fails — the specific error text is
        // upstream and not stable.
        assert!(
            toml::from_str::<PackageManifest>(toml_str).is_err(),
            "invalid houdini range should fail at deserialize"
        );
    }

    #[test]
    fn env_conditional_value_empty_list_fails_validate() {
        // An empty conditional list is meaningless — flag it at validate()
        // rather than emitting an empty Houdini env entry.
        let toml_str = r#"
[package]
path = "studio/bad"
name = "Bad"
version = "0.1.0"

[runtime.X]
method = "set"
value = []
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let err = manifest.validate().unwrap_err();
        assert!(err.contains("empty conditional"));
    }

    #[test]
    fn env_pass_through_preserves_houdini_vars_in_flat_value() {
        // Regression: hpm only substitutes $HPM_PACKAGE_ROOT. Anything else
        // (notably $HOUDINI_MAJOR_RELEASE, $HFS, $HOUDINI_USER_PREF_DIR) must
        // pass through verbatim into the emitted package.json so Houdini's
        // own variable expansion does the work at startup.
        let mut manifest = PackageManifest::new(
            PackagePath::new("studio/passthrough").unwrap(),
            "Passthrough".to_string(),
            "1.0.0".to_string(),
            None,
            Vec::new(),
            None,
        );
        let mut runtime = IndexMap::new();
        runtime.insert(
            "PXR_PLUGINPATH_NAME".to_string(),
            ManifestEnvEntry {
                method: EnvMethod::Prepend,
                value: Some("$HPM_PACKAGE_ROOT/resolver/houdini$HOUDINI_MAJOR_RELEASE/r".into()),
                required: false,
            },
        );
        manifest.runtime = runtime;

        let pkg = manifest
            .generate_houdini_package()
            .expect("test manifest produces valid Houdini expr");
        let env_list = pkg.env.unwrap();
        let entry = env_list
            .iter()
            .find_map(|m| m.get("PXR_PLUGINPATH_NAME"))
            .unwrap();
        match entry {
            HoudiniEnvValue::Detailed { value, .. } => {
                assert!(
                    value.contains("$HOUDINI_MAJOR_RELEASE"),
                    "Houdini var must pass through verbatim, got: {}",
                    value
                );
            }
            _ => panic!("expected Detailed flat value"),
        }
    }
}
