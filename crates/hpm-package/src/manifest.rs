//! Package manifest types and implementation.
//!
//! This module defines the core `PackageManifest` type that represents an `hpm.toml` file,
//! along with related configuration types for package metadata and Houdini integration.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::dependency::DependencySpec;
use crate::env_value::{EnvValueSpec, ExpressionError, lower_conditional};
use crate::houdini::{HoudiniEnvValue, HoudiniNativePackage, HoudiniPackage, HpackageMetadata};
use crate::package_path::PackagePath;
use crate::platform::Platform;
use crate::python::PythonDependencySpec;

/// Errors that can occur while loading a [`PackageManifest`] from disk.
///
/// Each variant carries the source path so error messages stay actionable
/// when manifest loading is buried inside multi-package operations
/// (`list_installed`, registry installs, project sync).
#[derive(Debug, thiserror::Error)]
pub enum ManifestLoadError {
    #[error("manifest not found: {}", .path.display())]
    NotFound { path: PathBuf },

    #[error("failed to read manifest at {}: {source}", .path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse manifest at {}: {source}", .path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
}

/// The type of registry backend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RegistryType {
    Api,
    Git,
}

/// Method for applying an environment variable value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EnvMethod {
    Set,
    Prepend,
    Append,
}

/// An environment variable entry declared in `[env]`.
///
/// `required = true` with no `value` declares a placeholder that the consuming
/// project's `[env]` must override; otherwise the package fails to install.
/// `required = true` alongside a `value` is allowed (the value acts as a
/// default) and behaves the same as a non-required entry with that value.
///
/// `value` accepts either a flat string (today's case) or a list of
/// `{ when, set }` variants — see [`EnvValueSpec`]. The conditional form
/// lowers to Houdini's expression-object array per
/// <https://www.sidefx.com/docs/houdini/ref/plugins.html>.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEnvEntry {
    pub method: EnvMethod,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<EnvValueSpec>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub required: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// A single `[scripts]` entry.
///
/// The shorthand form is a bare command string. The table form opts the
/// script into a uv-managed Python environment scoped to that script:
///
/// ```toml
/// [scripts.tt_setup]
/// cmd = "python scripts/tt_setup.py"
/// python = "3.11"
/// requirements = ["PySide6>=6.6"]
/// ```
///
/// `python` and `requirements` are both optional; when either is set, hpm
/// resolves them through the same uv pipeline that backs `[python_dependencies]`
/// and runs `cmd` with the resolved interpreter on PATH. When both are absent,
/// the table form behaves identically to the shorthand.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ScriptEntry {
    Plain(String),
    WithEnv(ScriptEnv),
}

/// The table form of [`ScriptEntry`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptEnv {
    pub cmd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub python: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requirements: Vec<String>,
}

impl ScriptEntry {
    /// The raw shell command for this script.
    pub fn cmd(&self) -> &str {
        match self {
            ScriptEntry::Plain(s) => s,
            ScriptEntry::WithEnv(env) => &env.cmd,
        }
    }

    /// Pinned Python version (e.g. `"3.11"`), if the entry requested one.
    pub fn python(&self) -> Option<&str> {
        match self {
            ScriptEntry::Plain(_) => None,
            ScriptEntry::WithEnv(env) => env.python.as_deref(),
        }
    }

    /// Inline requirement specifiers (e.g. `"PySide6>=6.6"`), if any.
    pub fn requirements(&self) -> &[String] {
        match self {
            ScriptEntry::Plain(_) => &[],
            ScriptEntry::WithEnv(env) => &env.requirements,
        }
    }

    /// True when this script needs a uv-managed environment.
    pub fn needs_venv(&self) -> bool {
        self.python().is_some() || !self.requirements().is_empty()
    }
}

impl From<String> for ScriptEntry {
    fn from(s: String) -> Self {
        ScriptEntry::Plain(s)
    }
}

impl From<&str> for ScriptEntry {
    fn from(s: &str) -> Self {
        ScriptEntry::Plain(s.to_string())
    }
}

/// Platform-scoped script overrides.
///
/// Deserialized from `[scripts.platform.<os>]` sub-tables. Each entry is a map
/// of script name → [`ScriptEntry`] that overrides the top-level `[scripts]`
/// entry for the matching OS.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlatformScripts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linux: Option<IndexMap<String, ScriptEntry>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub macos: Option<IndexMap<String, ScriptEntry>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows: Option<IndexMap<String, ScriptEntry>>,
}

impl PlatformScripts {
    /// Entries for the given OS key (`"linux"`, `"macos"`, `"windows"`).
    pub fn for_os(&self, os: &str) -> Option<&IndexMap<String, ScriptEntry>> {
        match os {
            "linux" => self.linux.as_ref(),
            "macos" => self.macos.as_ref(),
            "windows" => self.windows.as_ref(),
            _ => None,
        }
    }

    fn is_empty(&self) -> bool {
        self.linux.is_none() && self.macos.is_none() && self.windows.is_none()
    }
}

fn platform_scripts_is_none_or_empty(p: &Option<PlatformScripts>) -> bool {
    p.as_ref().is_none_or(PlatformScripts::is_empty)
}

/// Package-defined scripts from `[scripts]`.
///
/// Top-level entries live in `commands` and apply to every platform. An
/// optional `[scripts.platform.<os>]` sub-table supplies per-OS overrides;
/// when a script name appears in both, the platform-specific entry wins for
/// that OS.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PackageScripts {
    #[serde(default, skip_serializing_if = "platform_scripts_is_none_or_empty")]
    pub platform: Option<PlatformScripts>,
    #[serde(flatten)]
    pub commands: IndexMap<String, ScriptEntry>,
}

impl PackageScripts {
    /// True when neither top-level nor platform entries exist.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty() && platform_scripts_is_none_or_empty(&self.platform)
    }
}

/// Native platform configuration for multi-architecture packaging.
///
/// Declares which files belong to which platform, enabling per-platform archives.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeConfig {
    /// Declared platforms this package supports.
    pub platforms: Vec<String>,
    /// Per-platform file glob patterns, keyed by platform string.
    /// Deserialized from `[native.linux-x86_64]` etc.
    #[serde(flatten)]
    pub platform_files: IndexMap<String, NativePlatformFiles>,
}

/// Files for a single platform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativePlatformFiles {
    /// Glob patterns matching files belonging to this platform.
    pub files: Vec<String>,
}

/// A registry declared in hpm.toml's `[[registries]]` array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryConfig {
    pub name: String,
    pub url: String,
    #[serde(rename = "type")]
    pub registry_type: RegistryType,
}

/// HPM package manifest (hpm.toml)
///
/// Uses `IndexMap` for dependencies and python_dependencies to preserve
/// insertion order during serialization, ensuring deterministic TOML output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageManifest {
    pub package: PackageInfo,
    pub houdini: Option<HoudiniConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native: Option<NativeConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registries: Option<Vec<RegistryConfig>>,
    pub dependencies: Option<IndexMap<String, DependencySpec>>,
    pub python_dependencies: Option<IndexMap<String, PythonDependencySpec>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<IndexMap<String, ManifestEnvEntry>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scripts: Option<PackageScripts>,
}

/// Package metadata information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageInfo {
    /// Scoped package path: `creator-slug/package-slug` (e.g. `tumblehead/tumble-rig`).
    /// Validated kebab-case at deserialization — see [`PackagePath`].
    pub path: PackagePath,
    /// Freeform display name (e.g. `TumbleRig`)
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub authors: Option<Vec<String>>,
    pub license: Option<String>,
    pub readme: Option<String>,
    pub homepage: Option<String>,
    pub repository: Option<String>,
    pub documentation: Option<String>,
    pub keywords: Option<Vec<String>>,
    pub categories: Option<Vec<String>>,
}

impl PackageInfo {
    /// Returns the scoped path (the canonical identifier).
    pub fn identifier(&self) -> &str {
        self.path.as_str()
    }

    /// Returns the creator segment, e.g. `tumblehead`.
    pub fn creator(&self) -> &str {
        self.path.creator()
    }

    /// Returns the slug segment, e.g. `tumble-rig`.
    pub fn slug(&self) -> &str {
        self.path.slug()
    }
}

/// Houdini version compatibility configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoudiniConfig {
    pub min_version: Option<String>,
    pub max_version: Option<String>,
}

impl ManifestEnvEntry {
    /// Convert to a Houdini environment variable value, if a value is set.
    ///
    /// Returns `None` for required-but-unsupplied placeholders; callers
    /// that have no override source (publish/scaffold paths) skip these,
    /// while project-sync paths surface a hard error instead.
    ///
    /// No substitution is applied — the returned value reflects the manifest
    /// verbatim, so `$HPM_PACKAGE_ROOT` is preserved. Use [`Self::lower`] when
    /// you have a concrete package path to substitute in.
    ///
    /// Conditional values with a malformed `when` selector are dropped
    /// silently; [`PackageManifest::validate`] catches those before this
    /// method is reached on any well-formed manifest.
    pub fn to_houdini_env_value(&self) -> Option<HoudiniEnvValue> {
        self.lower(&[]).ok().flatten()
    }

    /// Lower this entry into a Houdini env value, applying the supplied
    /// substitutions to each value branch.
    ///
    /// Returns `Ok(None)` when there is no value (a required-but-unsupplied
    /// placeholder); callers in publish/scaffold paths skip those, while
    /// project-sync paths convert that to a hard error themselves.
    pub fn lower(
        &self,
        substitutions: &[(&str, &str)],
    ) -> Result<Option<HoudiniEnvValue>, ExpressionError> {
        let Some(value) = self.value.as_ref() else {
            return Ok(None);
        };
        let method = self.method.as_str();
        let lowered = match value {
            EnvValueSpec::Flat(s) => {
                let mut out = s.clone();
                for (from, to) in substitutions {
                    out = out.replace(from, to);
                }
                HoudiniEnvValue::Detailed {
                    method: method.to_string(),
                    value: out,
                }
            }
            EnvValueSpec::Conditional(variants) => {
                let lowered = lower_conditional(variants, substitutions)?;
                HoudiniEnvValue::DetailedConditional {
                    method: method.to_string(),
                    value: lowered,
                }
            }
        };
        Ok(Some(lowered))
    }
}

impl EnvMethod {
    /// String form used in Houdini's package.json (`"set"`, `"prepend"`, `"append"`).
    pub fn as_str(&self) -> &'static str {
        match self {
            EnvMethod::Set => "set",
            EnvMethod::Prepend => "prepend",
            EnvMethod::Append => "append",
        }
    }
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
    pub fn new(
        path: PackagePath,
        name: String,
        version: String,
        description: Option<String>,
        authors: Option<Vec<String>>,
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
                keywords: Some(vec!["houdini".to_string()]),
                categories: None,
            },
            houdini: Some(HoudiniConfig {
                min_version: Some("20.5".to_string()),
                max_version: None,
            }),
            native: None,
            registries: None,
            dependencies: None,
            python_dependencies: None,
            env: None,
            scripts: None,
        }
    }

    /// Resolve the effective scripts for the given platform.
    ///
    /// Starts with the top-level `[scripts]` entries (which apply everywhere)
    /// and overlays any `[scripts.platform.<os>]` entries for the current OS,
    /// where platform-specific entries win on collision. Insertion order from
    /// the manifest is preserved; platform-only scripts appear after the
    /// top-level ones.
    ///
    /// Passing `None` (e.g. when the host platform cannot be identified)
    /// yields only the top-level commands.
    pub fn resolved_scripts(&self, platform: Option<Platform>) -> IndexMap<String, ScriptEntry> {
        let Some(scripts) = &self.scripts else {
            return IndexMap::new();
        };

        let mut out = scripts.commands.clone();

        if let (Some(platform), Some(platform_scripts)) = (platform, &scripts.platform)
            && let Some(os) = platform.os_key()
            && let Some(entries) = platform_scripts.for_os(os)
        {
            for (name, entry) in entries {
                out.insert(name.clone(), entry.clone());
            }
        }

        out
    }

    /// Resolve a single script entry for the given platform.
    ///
    /// Same precedence rule as [`resolved_scripts`](Self::resolved_scripts):
    /// platform override wins over the top-level entry.
    pub fn script_for(&self, name: &str, platform: Option<Platform>) -> Option<ScriptEntry> {
        let scripts = self.scripts.as_ref()?;

        if let (Some(platform), Some(platform_scripts)) = (platform, &scripts.platform)
            && let Some(os) = platform.os_key()
            && let Some(entries) = platform_scripts.for_os(os)
            && let Some(entry) = entries.get(name)
        {
            return Some(entry.clone());
        }

        scripts.commands.get(name).cloned()
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

        // Validate [native] section
        if let Some(native) = &self.native {
            for platform_str in &native.platforms {
                platform_str
                    .parse::<Platform>()
                    .map_err(|e| e.to_string())?;
            }
            for key in native.platform_files.keys() {
                if !native.platforms.contains(key) {
                    return Err(format!(
                        "[native.{}] declared but '{}' not listed in native.platforms",
                        key, key
                    ));
                }
            }
            for platform_str in &native.platforms {
                if !native.platform_files.contains_key(platform_str) {
                    return Err(format!(
                        "Platform '{}' listed in native.platforms but has no [native.{}] section",
                        platform_str, platform_str
                    ));
                }
            }
            for (platform_str, files) in &native.platform_files {
                if files.files.is_empty() {
                    return Err(format!(
                        "[native.{}] files array must not be empty",
                        platform_str
                    ));
                }
            }
        }

        // Validate [env] entries: a missing value is only legal as a
        // required-placeholder for project-level [env] to fill in. Conditional
        // values (the variant list shape) get every branch's `when` selector
        // compiled here so malformed expressions surface at manifest load
        // time, not at install/emit time.
        if let Some(env) = &self.env {
            for (key, entry) in env {
                match &entry.value {
                    None => {
                        if !entry.required {
                            return Err(format!(
                                "env var '{}' has no value and is not marked required = true",
                                key
                            ));
                        }
                    }
                    Some(EnvValueSpec::Flat(_)) => {}
                    Some(EnvValueSpec::Conditional(variants)) => {
                        if variants.is_empty() {
                            return Err(format!(
                                "env var '{}' has an empty conditional value list",
                                key
                            ));
                        }
                        for variant in variants {
                            crate::env_value::compile_when(&variant.when)
                                .map_err(|e| format!("env var '{}': {}", key, e))?;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Generate Houdini package.json from manifest
    pub fn generate_houdini_package(&self) -> HoudiniPackage {
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

        // Append user-defined env vars from [env] section. Required-but-
        // unsupplied placeholders are skipped here because this generator has
        // no project context to fill them in; project sync supplies them via
        // `[env]` overrides and errors if any remain unsupplied.
        if let Some(user_env) = &self.env {
            for (key, entry) in user_env {
                let Some(houdini_value) = entry.to_houdini_env_value() else {
                    continue;
                };
                let mut env_map = HashMap::new();
                env_map.insert(key.clone(), houdini_value);
                env.push(env_map);
            }
        }

        // Generate version constraint
        let enable = if let Some(houdini_config) = &self.houdini {
            let mut conditions = vec![];

            if let Some(min_version) = &houdini_config.min_version {
                conditions.push(format!("houdini_version >= '{}'", min_version));
            }

            if let Some(max_version) = &houdini_config.max_version {
                conditions.push(format!("houdini_version <= '{}'", max_version));
            }

            if conditions.is_empty() {
                None
            } else {
                Some(conditions.join(" and "))
            }
        } else {
            None
        };

        HoudiniPackage {
            hpath: Some(hpath),
            env: Some(env),
            enable,
            requires: None,
            recommends: None,
        }
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

        // User-defined env vars with $HPM_PACKAGE_ROOT replaced. Required-
        // but-unsupplied placeholders are skipped (see generate_houdini_package).
        // Conditional value branches each get the same substitution applied
        // before the conditional-object array is emitted.
        if let Some(user_env) = &self.env {
            for (key, entry) in user_env {
                let Some(houdini_value) = entry
                    .lower(&[("$HPM_PACKAGE_ROOT", &pkg_root)])
                    .map_err(|e| e.to_string())?
                else {
                    continue;
                };
                let mut env_map = HashMap::new();
                env_map.insert(key.clone(), houdini_value);
                env.push(env_map);
            }
        }

        // Build enable from houdini version constraints
        let enable = if let Some(houdini_config) = &self.houdini {
            let mut conditions = vec![];
            if let Some(min_version) = &houdini_config.min_version {
                conditions.push(format!("houdini_version >= '{}'", min_version));
            }
            if let Some(max_version) = &houdini_config.max_version {
                conditions.push(format!("houdini_version <= '{}'", max_version));
            }
            if conditions.is_empty() {
                None
            } else {
                Some(conditions.join(" and "))
            }
        } else {
            None
        };

        // Build requires from dependency keys (slug portion only)
        let requires = self.dependencies.as_ref().and_then(|deps| {
            let names: Vec<String> = deps
                .keys()
                .map(|key| {
                    key.split_once('/')
                        .map(|(_, s)| s)
                        .unwrap_or(key)
                        .to_string()
                })
                .collect();
            if names.is_empty() { None } else { Some(names) }
        });

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
            None,
            None,
        );

        manifest.houdini = Some(HoudiniConfig {
            min_version: None,
            max_version: None,
        });

        let houdini_pkg = manifest.generate_houdini_package();
        assert!(houdini_pkg.enable.is_none());
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
        let registries = manifest.registries.unwrap();
        assert_eq!(registries.len(), 2);
        assert_eq!(registries[0].name, "houdinihub");
        assert_eq!(registries[0].registry_type, RegistryType::Api);
        assert_eq!(registries[1].name, "studio-internal");
        assert_eq!(registries[1].registry_type, RegistryType::Git);
    }

    #[test]
    fn env_deserialize_from_toml() {
        let toml_str = r#"
[package]
path = "studio/my-package"
name = "My Package"
version = "0.1.0"

[env]
MY_PLUGIN_ROOT = { method = "set", value = "$HPM_PACKAGE_ROOT/config" }
HOUDINI_TOOLBAR_PATH = { method = "prepend", value = "$HPM_PACKAGE_ROOT/toolbar" }
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let env = manifest.env.unwrap();
        assert_eq!(env.len(), 2);
        assert_eq!(env["MY_PLUGIN_ROOT"].method, EnvMethod::Set);
        assert_eq!(
            env["MY_PLUGIN_ROOT"]
                .value
                .as_ref()
                .and_then(EnvValueSpec::as_flat),
            Some("$HPM_PACKAGE_ROOT/config")
        );
        assert!(!env["MY_PLUGIN_ROOT"].required);
        assert_eq!(env["HOUDINI_TOOLBAR_PATH"].method, EnvMethod::Prepend);
    }

    #[test]
    fn env_required_without_value_deserializes() {
        let toml_str = r#"
[package]
path = "studio/my-package"
name = "My Package"
version = "0.1.0"

[env]
PROJECT_ROOT = { method = "set", required = true }
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let env = manifest.env.as_ref().unwrap();
        assert_eq!(env["PROJECT_ROOT"].method, EnvMethod::Set);
        assert!(env["PROJECT_ROOT"].value.is_none());
        assert!(env["PROJECT_ROOT"].required);
        assert!(manifest.validate().is_ok());
    }

    #[test]
    fn env_missing_value_without_required_is_invalid() {
        // serde happily accepts the missing value (it's now Option), but
        // validate() rejects it because non-required entries must declare a
        // value.
        let toml_str = r#"
[package]
path = "studio/my-package"
name = "My Package"
version = "0.1.0"

[env]
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
            None,
            None,
        );
        let mut env = IndexMap::new();
        env.insert(
            "PROJECT_ROOT".to_string(),
            ManifestEnvEntry {
                method: EnvMethod::Set,
                value: None,
                required: true,
            },
        );
        env.insert(
            "WITH_VALUE".to_string(),
            ManifestEnvEntry {
                method: EnvMethod::Set,
                value: Some("/somewhere".into()),
                required: false,
            },
        );
        manifest.env = Some(env);

        let pkg = manifest.generate_houdini_package();
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
    fn env_invalid_method_rejected() {
        let toml_str = r#"
[package]
path = "studio/my-package"
name = "My Package"
version = "0.1.0"

[env]
MY_VAR = { method = "invalid", value = "foo" }
"#;
        let result: Result<PackageManifest, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn env_none_when_absent() {
        let toml_str = r#"
[package]
path = "studio/my-package"
name = "My Package"
version = "0.1.0"
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        assert!(manifest.env.is_none());
    }

    #[test]
    fn generate_houdini_package_includes_user_env() {
        let mut manifest = PackageManifest::new(
            PackagePath::new("studio/test").unwrap(),
            "Test".to_string(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );

        let mut env = IndexMap::new();
        env.insert(
            "MY_VAR".to_string(),
            ManifestEnvEntry {
                method: EnvMethod::Set,
                value: Some("$HPM_PACKAGE_ROOT/data".into()),
                required: false,
            },
        );
        manifest.env = Some(env);

        let houdini_pkg = manifest.generate_houdini_package();
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
    fn native_deserialize_from_toml() {
        let toml_str = r#"
[package]
path = "studio/my-native-pkg"
name = "My Native Pkg"
version = "1.0.0"

[native]
platforms = ["linux-x86_64", "macos-aarch64"]

[native.linux-x86_64]
files = ["lib/linux-x86_64/*"]

[native.macos-aarch64]
files = ["lib/macos-aarch64/*"]
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let native = manifest.native.unwrap();
        assert_eq!(native.platforms.len(), 2);
        assert_eq!(native.platform_files.len(), 2);
        assert_eq!(
            native.platform_files["linux-x86_64"].files,
            vec!["lib/linux-x86_64/*"]
        );
    }

    #[test]
    fn native_none_when_absent() {
        let toml_str = r#"
[package]
path = "studio/my-package"
name = "My Package"
version = "0.1.0"
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        assert!(manifest.native.is_none());
    }

    #[test]
    fn native_validation_unknown_platform() {
        let mut manifest = PackageManifest::new(
            PackagePath::new("studio/test").unwrap(),
            "Test".to_string(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );
        manifest.native = Some(NativeConfig {
            platforms: vec!["linux-arm64".to_string()],
            platform_files: IndexMap::new(),
        });
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn native_validation_missing_files_section() {
        let mut manifest = PackageManifest::new(
            PackagePath::new("studio/test").unwrap(),
            "Test".to_string(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );
        manifest.native = Some(NativeConfig {
            platforms: vec!["linux-x86_64".to_string()],
            platform_files: IndexMap::new(),
        });
        let err = manifest.validate().unwrap_err();
        assert!(err.contains("has no [native.linux-x86_64] section"));
    }

    #[test]
    fn native_validation_empty_files() {
        let mut manifest = PackageManifest::new(
            PackagePath::new("studio/test").unwrap(),
            "Test".to_string(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );
        let mut platform_files = IndexMap::new();
        platform_files.insert(
            "linux-x86_64".to_string(),
            NativePlatformFiles { files: vec![] },
        );
        manifest.native = Some(NativeConfig {
            platforms: vec!["linux-x86_64".to_string()],
            platform_files,
        });
        let err = manifest.validate().unwrap_err();
        assert!(err.contains("files array must not be empty"));
    }

    #[test]
    fn native_validation_extra_platform_files() {
        let mut manifest = PackageManifest::new(
            PackagePath::new("studio/test").unwrap(),
            "Test".to_string(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );
        let mut platform_files = IndexMap::new();
        platform_files.insert(
            "windows-x86_64".to_string(),
            NativePlatformFiles {
                files: vec!["lib/*".to_string()],
            },
        );
        manifest.native = Some(NativeConfig {
            platforms: vec!["linux-x86_64".to_string()],
            platform_files,
        });
        let err = manifest.validate().unwrap_err();
        assert!(err.contains("not listed in native.platforms"));
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
            authors: None,
            license: None,
            readme: None,
            homepage: None,
            repository: None,
            documentation: None,
            keywords: None,
            categories: None,
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
            None,
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
        assert!(manifest.registries.is_none());
    }

    #[test]
    fn generate_houdini_native_package_full() {
        let toml_str = r#"
[package]
path = "creator/my-tool"
name = "My Cool Tool"
version = "1.2.3"

[houdini]
min_version = "21.0"

[dependencies]
"studio/some-dep" = "1.0.0"

[env]
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
        let scripts = manifest.scripts.as_ref().unwrap();
        assert_eq!(scripts.commands.len(), 2);
        assert_eq!(scripts.commands["build"].cmd(), "cargo build");
        assert_eq!(scripts.commands["test"].cmd(), "cargo test");
        assert!(scripts.platform.is_none());

        // Plain entries don't carry venv hints.
        assert!(!scripts.commands["build"].needs_venv());

        // Preserves declaration order in the flattened commands map.
        let names: Vec<&String> = scripts.commands.keys().collect();
        assert_eq!(names, vec!["build", "test"]);
    }

    #[test]
    fn scripts_platform_overrides_parse() {
        let toml_str = r#"
[package]
path = "studio/claudini"
name = "Claudini"
version = "1.0.0"

[scripts]
build = "cargo build"

[scripts.platform.windows]
register = "\"$HPM_PACKAGE_ROOT/plugin/bin/claudini2.exe\" register"

[scripts.platform.macos]
register = "\"$HPM_PACKAGE_ROOT/plugin/bin/claudini2\" register"
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let scripts = manifest.scripts.as_ref().unwrap();
        assert_eq!(scripts.commands["build"].cmd(), "cargo build");

        let platform = scripts.platform.as_ref().unwrap();
        assert!(platform.linux.is_none());
        let win = platform.windows.as_ref().unwrap();
        assert!(win["register"].cmd().contains("claudini2.exe"));
        let mac = platform.macos.as_ref().unwrap();
        assert_eq!(
            mac["register"].cmd(),
            "\"$HPM_PACKAGE_ROOT/plugin/bin/claudini2\" register"
        );
    }

    #[test]
    fn scripts_resolve_merges_platform_over_flat() {
        let toml_str = r#"
[package]
path = "studio/claudini"
name = "Claudini"
version = "1.0.0"

[scripts]
build = "cargo build"
register = "fallback"

[scripts.platform.windows]
register = "windows-specific"
unregister = "windows-only"
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();

        let resolved = manifest.resolved_scripts(Some(Platform::WindowsX86_64));
        assert_eq!(resolved["build"].cmd(), "cargo build");
        assert_eq!(resolved["register"].cmd(), "windows-specific");
        assert_eq!(resolved["unregister"].cmd(), "windows-only");

        // macOS has no platform entry — falls back to the flat map.
        let resolved_mac = manifest.resolved_scripts(Some(Platform::MacosAarch64));
        assert_eq!(resolved_mac["register"].cmd(), "fallback");
        assert!(!resolved_mac.contains_key("unregister"));

        // Unknown platform returns flat-only.
        let resolved_none = manifest.resolved_scripts(None);
        assert_eq!(resolved_none.len(), 2);
        assert_eq!(resolved_none["register"].cmd(), "fallback");
    }

    #[test]
    fn script_for_respects_platform_precedence() {
        let toml_str = r#"
[package]
path = "studio/tool"
name = "Tool"
version = "1.0.0"

[scripts]
register = "fallback"

[scripts.platform.linux]
register = "linux-specific"
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();

        assert_eq!(
            manifest
                .script_for("register", Some(Platform::LinuxX86_64))
                .map(|e| e.cmd().to_string()),
            Some("linux-specific".to_string())
        );
        assert_eq!(
            manifest
                .script_for("register", Some(Platform::MacosAarch64))
                .map(|e| e.cmd().to_string()),
            Some("fallback".to_string())
        );
        assert!(
            manifest
                .script_for("missing", Some(Platform::LinuxX86_64))
                .is_none()
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
        let scripts = manifest.scripts.as_ref().unwrap();

        // Plain shorthand still parses.
        assert_eq!(scripts.commands["build"].cmd(), "cargo build");
        assert!(!scripts.commands["build"].needs_venv());

        // Table form carries the venv hints.
        let setup = &scripts.commands["tt_setup"];
        assert_eq!(setup.cmd(), "python scripts/tt_setup.py");
        assert_eq!(setup.python(), Some("3.11"));
        assert_eq!(setup.requirements(), &["PySide6>=6.6".to_string()]);
        assert!(setup.needs_venv());
    }

    #[test]
    fn scripts_table_form_inline_object() {
        // Inline table should parse the same as a [scripts.<name>] sub-table.
        let toml_str = r#"
[package]
path = "studio/tt"
name = "TT"
version = "1.0.0"

[scripts]
tt_setup = { cmd = "python scripts/tt_setup.py", python = "3.11", requirements = ["PySide6>=6.6"] }
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let setup = &manifest.scripts.as_ref().unwrap().commands["tt_setup"];
        assert_eq!(setup.cmd(), "python scripts/tt_setup.py");
        assert_eq!(setup.python(), Some("3.11"));
        assert_eq!(setup.requirements(), &["PySide6>=6.6".to_string()]);
    }

    #[test]
    fn scripts_table_form_without_venv_hints_is_legal() {
        // A table form with just `cmd` and no python/requirements behaves
        // like the shorthand — no venv work needed.
        let toml_str = r#"
[package]
path = "studio/tt"
name = "TT"
version = "1.0.0"

[scripts.lint]
cmd = "ruff ."
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let lint = &manifest.scripts.as_ref().unwrap().commands["lint"];
        assert_eq!(lint.cmd(), "ruff .");
        assert!(!lint.needs_venv());
    }

    #[test]
    fn scripts_table_form_in_platform_overrides() {
        let toml_str = r#"
[package]
path = "studio/tt"
name = "TT"
version = "1.0.0"

[scripts.platform.linux]
tt_setup = { cmd = "python scripts/tt_setup.py", python = "3.11", requirements = ["PySide6>=6.6"] }

[scripts.platform.windows]
tt_setup = "py -3 scripts\\tt_setup.py"
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();

        let linux = manifest
            .script_for("tt_setup", Some(Platform::LinuxX86_64))
            .unwrap();
        assert!(linux.needs_venv());
        assert_eq!(linux.python(), Some("3.11"));

        // Windows entry stays plain — different OSes can mix forms.
        let win = manifest
            .script_for("tt_setup", Some(Platform::WindowsX86_64))
            .unwrap();
        assert!(!win.needs_venv());
        assert_eq!(win.cmd(), "py -3 scripts\\tt_setup.py");
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
        assert!(manifest.scripts.is_none());
        assert!(
            manifest
                .resolved_scripts(Some(Platform::LinuxX86_64))
                .is_empty()
        );
        assert!(manifest.script_for("anything", None).is_none());
    }

    #[test]
    fn scripts_platform_only_no_flat() {
        let toml_str = r#"
[package]
path = "studio/pkg"
name = "Pkg"
version = "0.1.0"

[scripts.platform.linux]
register = "linux-register"
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let scripts = manifest.scripts.as_ref().unwrap();
        assert!(scripts.commands.is_empty());
        assert!(scripts.platform.as_ref().unwrap().linux.is_some());

        let resolved = manifest.resolved_scripts(Some(Platform::LinuxX86_64));
        assert_eq!(resolved["register"].cmd(), "linux-register");
        let resolved_mac = manifest.resolved_scripts(Some(Platform::MacosAarch64));
        assert!(resolved_mac.is_empty());
    }

    #[test]
    fn scripts_toml_roundtrip_preserves_platform_table() {
        let mut manifest = PackageManifest::new(
            PackagePath::new("studio/tool").unwrap(),
            "Tool".to_string(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );
        let mut commands = IndexMap::new();
        commands.insert("build".to_string(), ScriptEntry::from("cargo build"));

        let mut win = IndexMap::new();
        win.insert(
            "register".to_string(),
            ScriptEntry::from("tool.exe register"),
        );

        manifest.scripts = Some(PackageScripts {
            commands,
            platform: Some(PlatformScripts {
                linux: None,
                macos: None,
                windows: Some(win),
            }),
        });

        let toml_str = toml::to_string(&manifest).unwrap();
        let roundtripped: PackageManifest = toml::from_str(&toml_str).unwrap();
        let scripts = roundtripped.scripts.unwrap();
        assert_eq!(scripts.commands["build"].cmd(), "cargo build");
        assert_eq!(
            scripts.platform.unwrap().windows.unwrap()["register"].cmd(),
            "tool.exe register"
        );
    }

    #[test]
    fn generate_houdini_native_package_env_root_replacement() {
        let mut manifest = PackageManifest::new(
            PackagePath::new("studio/test-pkg").unwrap(),
            "Test".to_string(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );
        let mut env = IndexMap::new();
        env.insert(
            "PATH_A".to_string(),
            ManifestEnvEntry {
                method: EnvMethod::Set,
                value: Some("$HPM_PACKAGE_ROOT/a".into()),
                required: false,
            },
        );
        env.insert(
            "PATH_B".to_string(),
            ManifestEnvEntry {
                method: EnvMethod::Append,
                value: Some("$HPM_PACKAGE_ROOT/b:$HPM_PACKAGE_ROOT/c".into()),
                required: false,
            },
        );
        manifest.env = Some(env);

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

[env.PXR_PLUGINPATH_NAME]
method = "prepend"
value = [
  { when = { houdini = "^21" }, set = "$HPM_PACKAGE_ROOT/resolver/houdini21/r" },
  { when = { houdini = "^22" }, set = "$HPM_PACKAGE_ROOT/resolver/houdini22/r" },
]
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        manifest.validate().unwrap();
        let entry = manifest
            .env
            .as_ref()
            .unwrap()
            .get("PXR_PLUGINPATH_NAME")
            .unwrap();
        assert_eq!(entry.method, EnvMethod::Prepend);
        match entry.value.as_ref().unwrap() {
            EnvValueSpec::Conditional(v) => {
                assert_eq!(v.len(), 2);
                assert_eq!(v[0].when.houdini.as_deref(), Some("^21"));
                assert_eq!(v[1].when.houdini.as_deref(), Some("^22"));
            }
            EnvValueSpec::Flat(_) => panic!("expected conditional"),
        }
    }

    #[test]
    fn env_conditional_value_lowers_to_houdini_array() {
        let toml_str = r#"
[package]
path = "studio/multi"
name = "Multi"
version = "0.1.0"

[env.PXR_PLUGINPATH_NAME]
method = "prepend"
value = [
  { when = { houdini = "^21" }, set = "$HPM_PACKAGE_ROOT/h21/r" },
  { when = { houdini = "^22", os = "linux" }, set = "$HPM_PACKAGE_ROOT/h22/r" },
]
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let entry = manifest
            .env
            .as_ref()
            .unwrap()
            .get("PXR_PLUGINPATH_NAME")
            .unwrap();
        let lowered = entry
            .lower(&[("$HPM_PACKAGE_ROOT", "/abs/pkg")])
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

[env.PXR_PLUGINPATH_NAME]
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
    fn env_conditional_value_with_invalid_req_fails_validate() {
        let toml_str = r#"
[package]
path = "studio/bad"
name = "Bad"
version = "0.1.0"

[env.X]
method = "set"
value = [
  { when = { houdini = "garbage" }, set = "x" },
]
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let err = manifest.validate().unwrap_err();
        assert!(err.contains("X"));
        assert!(err.contains("garbage"));
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

[env.X]
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
            None,
            None,
        );
        let mut env = IndexMap::new();
        env.insert(
            "PXR_PLUGINPATH_NAME".to_string(),
            ManifestEnvEntry {
                method: EnvMethod::Prepend,
                value: Some("$HPM_PACKAGE_ROOT/resolver/houdini$HOUDINI_MAJOR_RELEASE/r".into()),
                required: false,
            },
        );
        manifest.env = Some(env);

        let pkg = manifest.generate_houdini_package();
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
