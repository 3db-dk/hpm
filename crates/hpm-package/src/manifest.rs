//! Package manifest types and implementation.
//!
//! This module defines the core `PackageManifest` type that represents an `hpm.toml` file,
//! along with related configuration types for package metadata and Houdini integration.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::dependency::DependencySpec;
use crate::env_value::{
    EnvValueSpec, ExpressionError, WhenSelector, compile_houdini_req, houdini_req_lower_bound,
    lower_conditional,
};
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

/// An environment variable entry declared in `[runtime]`.
///
/// `required = true` with no `value` declares a placeholder that the
/// consuming project's `[runtime]` must override; otherwise the package
/// fails to install. `required = true` alongside a `value` is allowed (the
/// value acts as a default) and behaves the same as a non-required entry
/// with that value.
///
/// `value` accepts either a flat string or a list of `{ when, set }`
/// variants — see [`EnvValueSpec`]. Conditional variants may gate on
/// `install_source = "dev"` / `"registry"` (filtered by hpm at install
/// time) or on `houdini` / `os` / `python` (compiled into Houdini's
/// expression form per <https://www.sidefx.com/docs/houdini/ref/plugins.html>).
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

/// Validate a `[runtime]`-shaped table. The `section` label is used
/// verbatim in error messages so the source is obvious to authors.
fn validate_env_table(
    section: &str,
    env: &IndexMap<String, ManifestEnvEntry>,
) -> Result<(), String> {
    for (key, entry) in env {
        match &entry.value {
            None => {
                if !entry.required {
                    return Err(format!(
                        "{section} var '{key}' has no value and is not marked required = true"
                    ));
                }
            }
            Some(EnvValueSpec::Flat(_)) => {}
            Some(EnvValueSpec::Conditional(variants)) => {
                if variants.is_empty() {
                    return Err(format!(
                        "{section} var '{key}' has an empty conditional value list"
                    ));
                }
                for variant in variants {
                    crate::env_value::compile_when(&variant.when)
                        .map_err(|e| format!("{section} var '{key}': {e}"))?;
                }
            }
        }
    }
    Ok(())
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
///
/// `cmd` is an [`EnvValueSpec`] — either a flat string or an ordered list
/// of `{ when, set }` variants. For scripts only the `os` axis of `when`
/// is meaningful (HPM doesn't know the user's Houdini version or Python
/// at `hpm run` time); other axes on a script variant are rejected at
/// manifest validation time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptEnv {
    pub cmd: EnvValueSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub python: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requirements: Vec<String>,
}

impl ScriptEntry {
    /// Resolve the command for the given host OS.
    ///
    /// Returns `None` only when the entry is conditional and no variant
    /// matches the host (e.g. a `windows`-only command on a macOS host).
    /// Plain entries always return `Some`.
    pub fn resolve_cmd(&self, host_os: Option<&str>) -> Option<String> {
        let spec = match self {
            ScriptEntry::Plain(s) => return Some(s.clone()),
            ScriptEntry::WithEnv(env) => &env.cmd,
        };
        match spec {
            EnvValueSpec::Flat(s) => Some(s.clone()),
            EnvValueSpec::Conditional(variants) => variants
                .iter()
                .find(|v| script_when_matches(&v.when, host_os))
                .map(|v| v.set.clone()),
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

/// Per-script `when` matching: only the `os` axis is honoured. The other
/// axes (`houdini`, `python`, `install_source`) are rejected at manifest
/// validate time; if they survive here, treat as a non-match.
fn script_when_matches(when: &WhenSelector, host_os: Option<&str>) -> bool {
    if when.houdini.is_some() || when.python.is_some() || when.install_source.is_some() {
        return false;
    }
    match (&when.os, host_os) {
        (None, _) => true,
        (Some(req), Some(host)) => req == host,
        (Some(_), None) => false,
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

/// Package-defined scripts from `[scripts]`.
///
/// Each entry resolves to a single command for the host OS via
/// [`ScriptEntry::resolve_cmd`]. Per-host variation lives inside the
/// entry's `cmd` field as a list of `{ when, set }` variants — there is
/// no separate `[scripts.platform.<os>]` table.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PackageScripts {
    #[serde(flatten)]
    pub commands: IndexMap<String, ScriptEntry>,
}

impl PackageScripts {
    /// True when no entries exist.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

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

fn stage_is_none_or_empty(s: &Option<StageConfig>) -> bool {
    s.as_ref().is_none_or(StageConfig::is_empty)
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
    #[serde(default, skip_serializing_if = "compat_is_none_or_empty")]
    pub compat: Option<CompatConfig>,
    #[serde(default, skip_serializing_if = "stage_is_none_or_empty")]
    pub stage: Option<StageConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registries: Option<Vec<RegistryConfig>>,
    pub dependencies: Option<IndexMap<String, DependencySpec>>,
    pub python_dependencies: Option<IndexMap<String, PythonDependencySpec>>,
    /// `[runtime]` — env-var contributions to the generated Houdini
    /// `package.json`. Replaces the prior `[env]` + `[dev.env]` pair; the
    /// dev/registry distinction now lives in the `when.install_source` axis
    /// on individual conditional variants.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<IndexMap<String, ManifestEnvEntry>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scripts: Option<PackageScripts>,
}

fn compat_is_none_or_empty(c: &Option<CompatConfig>) -> bool {
    c.as_ref().is_none_or(CompatConfig::is_empty)
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

/// Target-environment compatibility for the package.
///
/// `houdini` is a Cargo-style version requirement (`"20.5"`, `"^21"`,
/// `">=20.5, <22"`). Bare versions alias caret semantics: `"20.5"` means
/// `>=20.5, <21`. See [`compile_houdini_req`] for the supported grammar.
///
/// `platforms` declares which native platforms this package supports.
/// Pure-data / pure-Python packages omit it (or use `["universal"]`);
/// HDK or DSO packages list the platforms they ship binaries for. The
/// per-platform staging rules live under `[stage.platform.<plat>]`.
///
/// An absent `[compat]` section, or an absent `houdini` field, leaves the
/// package's Houdini compatibility unconstrained — the generated package
/// manifest emits no `enable` clause.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompatConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub houdini: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub platforms: Vec<String>,
}

impl CompatConfig {
    pub fn is_empty(&self) -> bool {
        self.houdini.is_none() && self.platforms.is_empty()
    }

    /// Lower bound of the Houdini range, used for Python ABI selection.
    pub fn houdini_min(&self) -> Option<String> {
        self.houdini.as_deref().and_then(houdini_req_lower_bound)
    }
}

impl ManifestEnvEntry {
    /// Convert to a Houdini environment variable value for a published
    /// (non-dev) consumer. Returns `None` for required-but-unsupplied
    /// placeholders, and for conditional values whose every branch is
    /// gated to a non-matching `install_source`.
    ///
    /// No substitution is applied — the returned value reflects the
    /// manifest verbatim, so `$HPM_PACKAGE_ROOT` is preserved. Use
    /// [`Self::lower`] when you have a concrete package path and install
    /// context to substitute in.
    pub fn to_houdini_env_value(&self) -> Option<HoudiniEnvValue> {
        self.lower(&[], false).ok().flatten()
    }

    /// Lower this entry into a Houdini env value, applying the supplied
    /// substitutions to each value branch.
    ///
    /// `is_dev` controls the install-source filter for conditional
    /// variants: `true` means a path-installed (dev) package; `false`
    /// means a registry/URL-installed (published) consumer. Variants
    /// gated to a non-matching `install_source` are dropped before
    /// emission.
    ///
    /// Returns `Ok(None)` when the effective value is empty — either the
    /// entry was a required-but-unsupplied placeholder, or every branch
    /// of a conditional value got filtered out by `install_source`. Callers
    /// in publish/scaffold paths skip those; project-sync paths surface a
    /// hard error for the placeholder case via their own checks.
    pub fn lower(
        &self,
        substitutions: &[(&str, &str)],
        is_dev: bool,
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
                let lowered = lower_conditional(variants, substitutions, is_dev)?;
                if lowered.is_empty() {
                    // Every branch filtered out by install_source — treat
                    // the entry as inert in this install context.
                    return Ok(None);
                }
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
            compat: Some(CompatConfig {
                houdini: Some(">=20.5".to_string()),
                platforms: Vec::new(),
            }),
            stage: None,
            registries: None,
            dependencies: None,
            python_dependencies: None,
            runtime: None,
            scripts: None,
        }
    }

    /// Every `[scripts]` entry, in declaration order. Per-host variation
    /// is resolved on-demand via [`ScriptEntry::resolve_cmd`] using the
    /// entry's conditional `cmd` value — there is no merging at this layer.
    pub fn resolved_scripts(&self, _platform: Option<Platform>) -> IndexMap<String, ScriptEntry> {
        match &self.scripts {
            Some(scripts) => scripts.commands.clone(),
            None => IndexMap::new(),
        }
    }

    /// Resolve a single script entry by name. The `platform` argument is
    /// kept for API symmetry but no longer affects which entry is returned
    /// — variation lives inside the entry's `cmd` value.
    pub fn script_for(&self, name: &str, _platform: Option<Platform>) -> Option<ScriptEntry> {
        self.scripts.as_ref()?.commands.get(name).cloned()
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

        // Validate [compat].houdini is a parseable Cargo-style range. Empty
        // string is treated as a malformed constraint rather than "no
        // constraint" — authors who don't want a constraint omit the field.
        if let Some(compat) = &self.compat
            && let Some(req) = &compat.houdini
        {
            compile_houdini_req(req).map_err(|e| format!("[compat].houdini: {}", e))?;
        }

        // Validate [compat].platforms members are known.
        if let Some(compat) = &self.compat {
            for platform_str in &compat.platforms {
                platform_str
                    .parse::<Platform>()
                    .map_err(|e| e.to_string())?;
            }
        }

        // Validate [stage]: per-platform keys must appear in [compat].platforms,
        // and place rules must declare both `from` and `to`. Place rules with
        // empty `from` would match nothing useful; reject those at load time.
        if let Some(stage) = &self.stage {
            let declared_platforms: Vec<String> = self
                .compat
                .as_ref()
                .map(|c| c.platforms.clone())
                .unwrap_or_default();
            for (platform_str, rules) in &stage.platform.entries {
                platform_str
                    .parse::<Platform>()
                    .map_err(|e| format!("[stage.platform.{}]: {}", platform_str, e))?;
                if !declared_platforms.contains(platform_str) {
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
        }

        // Validate [runtime] entries: a missing value is only legal as a
        // required-placeholder for project-level [runtime] to fill in.
        // Conditional values get every branch's `when` selector compiled here
        // so malformed expressions surface at manifest load time, not at
        // install/emit time.
        if let Some(runtime) = &self.runtime {
            validate_env_table("runtime", runtime)?;
        }

        // Validate [scripts] entries: a conditional `cmd` may only gate on
        // the `os` axis. Other axes (`houdini`, `python`, `install_source`)
        // require runtime context HPM doesn't have at `hpm run` time, so we
        // reject them up front rather than silently dropping variants.
        if let Some(scripts) = &self.scripts {
            for (name, entry) in &scripts.commands {
                let ScriptEntry::WithEnv(env) = entry else {
                    continue;
                };
                let EnvValueSpec::Conditional(variants) = &env.cmd else {
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
                        crate::env_value::compile_when(&WhenSelector {
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

        // Append user-defined env vars from [runtime]. The standalone
        // generator has no install context, so it filters as a published
        // (non-dev) consumer would: `install_source = "dev"` branches drop
        // out, the rest go through. Required-but-unsupplied placeholders
        // are skipped — project sync supplies them via `[runtime]`
        // overrides and errors if any remain unsupplied.
        if let Some(user_runtime) = &self.runtime {
            for (key, entry) in user_runtime {
                let Some(houdini_value) = entry.to_houdini_env_value() else {
                    continue;
                };
                let mut env_map = HashMap::new();
                env_map.insert(key.clone(), houdini_value);
                env.push(env_map);
            }
        }

        let enable = self
            .compat
            .as_ref()
            .and_then(|c| c.houdini.as_deref())
            .map(compile_houdini_req)
            .transpose()
            .ok()
            .flatten();

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

        // User-defined env vars with $HPM_PACKAGE_ROOT replaced. This
        // generator emits the bundled `{slug}.json` shipped inside a packed
        // archive, so it filters as a published consumer would (is_dev=false).
        // Required-but-unsupplied placeholders are skipped (see
        // generate_houdini_package). Conditional value branches each get the
        // same substitution applied before the conditional-object array is
        // emitted.
        if let Some(user_runtime) = &self.runtime {
            for (key, entry) in user_runtime {
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
        }

        let enable = self
            .compat
            .as_ref()
            .and_then(|c| c.houdini.as_deref())
            .map(|req| compile_houdini_req(req).map_err(|e| e.to_string()))
            .transpose()?;

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

        manifest.compat = Some(CompatConfig {
            houdini: None,
            platforms: Vec::new(),
        });

        let houdini_pkg = manifest.generate_houdini_package();
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
        let pkg = manifest.generate_houdini_package();
        assert_eq!(
            pkg.enable.as_deref(),
            Some("(houdini_version >= '20.5' and houdini_version < '22')")
        );
    }

    #[test]
    fn compat_houdini_invalid_range_rejected_by_validate() {
        let toml_str = r#"
[package]
path = "studio/test"
name = "Test"
version = "1.0.0"

[compat]
houdini = "not-a-version"
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let err = manifest.validate().unwrap_err();
        assert!(err.contains("[compat].houdini"), "error: {err}");
    }

    #[test]
    fn compat_houdini_min_extracts_lower_bound() {
        let compat = CompatConfig {
            houdini: Some(">=20.5, <22".to_string()),
            platforms: Vec::new(),
        };
        assert_eq!(compat.houdini_min(), Some("20.5".to_string()));
        let compat = CompatConfig {
            houdini: Some("^21".to_string()),
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
        let registries = manifest.registries.unwrap();
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
        let runtime = manifest.runtime.unwrap();
        assert_eq!(runtime.len(), 2);
        assert_eq!(runtime["MY_PLUGIN_ROOT"].method, EnvMethod::Set);
        assert_eq!(
            runtime["MY_PLUGIN_ROOT"]
                .value
                .as_ref()
                .and_then(EnvValueSpec::as_flat),
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
        let runtime = manifest.runtime.as_ref().unwrap();
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
            None,
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
        manifest.runtime = Some(runtime);

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

        let pkg = manifest.generate_houdini_package();
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
        let pkg = manifest.generate_houdini_package();
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
        assert!(manifest.runtime.is_none());
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

        let mut runtime = IndexMap::new();
        runtime.insert(
            "MY_VAR".to_string(),
            ManifestEnvEntry {
                method: EnvMethod::Set,
                value: Some("$HPM_PACKAGE_ROOT/data".into()),
                required: false,
            },
        );
        manifest.runtime = Some(runtime);

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
        let compat = manifest.compat.as_ref().unwrap();
        assert_eq!(compat.platforms.len(), 2);
        let stage = manifest.stage.as_ref().unwrap();
        assert_eq!(stage.prepack, vec!["build-dso".to_string()]);
        assert_eq!(stage.platform.entries.len(), 2);
        assert_eq!(
            stage.platform.entries["linux-x86_64"].place[0].from,
            "build/linux/*.so"
        );
        assert_eq!(stage.platform.entries["linux-x86_64"].place[0].to, "dso/");
    }

    #[test]
    fn stage_none_when_absent() {
        let toml_str = r#"
[package]
path = "studio/my-package"
name = "My Package"
version = "0.1.0"
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        assert!(manifest.stage.is_none());
    }

    #[test]
    fn compat_platforms_unknown_rejected() {
        let mut manifest = PackageManifest::new(
            PackagePath::new("studio/test").unwrap(),
            "Test".to_string(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );
        manifest.compat = Some(CompatConfig {
            houdini: None,
            platforms: vec!["linux-arm64".to_string()],
        });
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn stage_platform_not_in_compat_rejected() {
        let mut manifest = PackageManifest::new(
            PackagePath::new("studio/test").unwrap(),
            "Test".to_string(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );
        manifest.compat = Some(CompatConfig {
            houdini: None,
            platforms: vec!["linux-x86_64".to_string()],
        });
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
        manifest.stage = Some(StageConfig {
            platform: PlatformStaging { entries },
            ..Default::default()
        });
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
            None,
            None,
        );
        manifest.compat = Some(CompatConfig {
            houdini: None,
            platforms: vec!["linux-x86_64".to_string()],
        });
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
        manifest.stage = Some(StageConfig {
            platform: PlatformStaging { entries },
            ..Default::default()
        });
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
        let scripts = manifest.scripts.as_ref().unwrap();
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

        let scripts = manifest.scripts.as_ref().unwrap();
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
        let entry = &manifest.scripts.as_ref().unwrap().commands["register"];
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
        let scripts = manifest.scripts.as_ref().unwrap();

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
        let setup = &manifest.scripts.as_ref().unwrap().commands["tt_setup"];
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
        let lint = &manifest.scripts.as_ref().unwrap().commands["lint"];
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
        let regen = &manifest.scripts.as_ref().unwrap().commands["regen"];
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
        assert!(manifest.scripts.is_none());
        assert!(
            manifest
                .resolved_scripts(Some(Platform::LinuxX86_64))
                .is_empty()
        );
        assert!(manifest.script_for("anything", None).is_none());
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
        let scripts = back.scripts.unwrap();
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
            None,
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
        manifest.runtime = Some(runtime);

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
        let entry = manifest
            .runtime
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

[runtime.PXR_PLUGINPATH_NAME]
method = "prepend"
value = [
  { when = { houdini = "^21" }, set = "$HPM_PACKAGE_ROOT/h21/r" },
  { when = { houdini = "^22", os = "linux" }, set = "$HPM_PACKAGE_ROOT/h22/r" },
]
"#;
        let manifest: PackageManifest = toml::from_str(toml_str).unwrap();
        let entry = manifest
            .runtime
            .as_ref()
            .unwrap()
            .get("PXR_PLUGINPATH_NAME")
            .unwrap();
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
    fn env_conditional_value_with_invalid_req_fails_validate() {
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
            None,
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
        manifest.runtime = Some(runtime);

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
