//! `[scripts]` entries: shorthand command strings or table-form entries
//! with optional per-script uv-managed Python environments.

use crate::env_value::{Condition, EnvValue};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

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
/// label = "Set up TT"
/// description = "Provisions the TT working environment"
/// ```
///
/// `label` and `description` are optional, consumer-agnostic metadata for
/// tools that present scripts to users (menus, tooltips); hpm itself never
/// acts on them. `python` and `requirements` are both optional; when either is set, hpm
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
/// `cmd` is an [`EnvValue`] — either a flat string or an ordered list
/// of `{ when, set }` variants. For scripts only the `os` axis of `when`
/// is meaningful (HPM doesn't know the user's Houdini version or Python
/// at `hpm run` time); other axes on a script variant are rejected at
/// manifest validation time.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptEnv {
    pub cmd: EnvValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub python: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requirements: Vec<String>,
    /// Optional human-readable display name for this script, for tools that
    /// present scripts to end users (menus, buttons). Consumer-agnostic
    /// metadata — hpm itself never acts on it; UIs fall back to the entry
    /// key when it's absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Optional one-line description of what this script does, for tooltips
    /// and help text in tools that surface scripts. Purely informational.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Run this script inside the package's full resolved environment: the
    /// merged uv venv built from `[python_dependencies]` across the project
    /// and its installed hpm dependencies, with every involved package's
    /// `python/` directory on `PYTHONPATH`. This lets a script import the
    /// package it ships in (and that package's deps) without re-implementing
    /// the venv/deps/PYTHONPATH dance — see `hpm run`.
    ///
    /// When set, the interpreter is the project's Houdini-mapped CPython
    /// (authoritative for ABI), so a per-script `python` pin is ignored; any
    /// `requirements` here are layered on top of the package environment.
    ///
    /// Honoured only by callers with a project context (`hpm run`); embedders
    /// that resolve scripts in isolation (the desktop hook runner) ignore it.
    #[serde(
        default,
        rename = "package-env",
        alias = "package_env",
        skip_serializing_if = "is_false"
    )]
    pub package_env: bool,
}

// serde `skip_serializing_if` helper — omit `package-env = false` from
// round-tripped manifests so an unset flag stays unwritten.
use super::env::is_false;

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
            EnvValue::Flat(s) => Some(s.clone()),
            EnvValue::Conditional(variants) => variants
                .iter()
                .find(|v| script_condition_matches(&v.when, host_os))
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

    /// Human-readable display name, if the entry set one. Plain entries and
    /// table entries without `label` return `None`; consumers fall back to the
    /// script's key.
    pub fn label(&self) -> Option<&str> {
        match self {
            ScriptEntry::Plain(_) => None,
            ScriptEntry::WithEnv(env) => env.label.as_deref(),
        }
    }

    /// One-line description of what the script does, if the entry set one.
    pub fn description(&self) -> Option<&str> {
        match self {
            ScriptEntry::Plain(_) => None,
            ScriptEntry::WithEnv(env) => env.description.as_deref(),
        }
    }

    /// True when this script needs a uv-managed environment.
    ///
    /// Covers only the per-script venv path (`python` / `requirements`). The
    /// package-environment path is gated separately by [`Self::uses_package_env`]
    /// because it needs a project context the script-venv path doesn't.
    pub fn needs_venv(&self) -> bool {
        self.python().is_some() || !self.requirements().is_empty()
    }

    /// True when this script opts into the package's full resolved environment
    /// (`package-env = true`). See [`ScriptEnv::package_env`].
    pub fn uses_package_env(&self) -> bool {
        match self {
            ScriptEntry::Plain(_) => false,
            ScriptEntry::WithEnv(env) => env.package_env,
        }
    }
}

/// Per-script `when` matching: only the `os` axis is honoured. The other
/// axes (`houdini`, `python`, `install_source`) are rejected at manifest
/// validate time; if they survive here, treat as a non-match.
fn script_condition_matches(condition: &Condition, host_os: Option<&str>) -> bool {
    if condition.houdini.is_some()
        || condition.python.is_some()
        || condition.install_source.is_some()
    {
        return false;
    }
    match (&condition.os, host_os) {
        (None, _) => true,
        (Some(req), Some(host)) => req.as_str() == host,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_package_env_kebab_key() {
        let scripts: PackageScripts = toml::from_str(
            r#"
            [farm]
            cmd = "python -m tumblepipe.farm"
            package-env = true
            "#,
        )
        .unwrap();
        let entry = scripts.commands.get("farm").unwrap();
        assert!(entry.uses_package_env());
        // package-env alone doesn't trip the per-script venv path.
        assert!(!entry.needs_venv());
    }

    #[test]
    fn parses_package_env_snake_alias() {
        let scripts: PackageScripts = toml::from_str(
            r#"
            [farm]
            cmd = "python -m tumblepipe.farm"
            package_env = true
            "#,
        )
        .unwrap();
        assert!(scripts.commands.get("farm").unwrap().uses_package_env());
    }

    #[test]
    fn package_env_defaults_false_and_coexists_with_requirements() {
        let scripts: PackageScripts = toml::from_str(
            r#"
            plain = "ruff ."

            [tt]
            cmd = "python scripts/tt.py"
            requirements = ["PySide6>=6.6"]

            [farm]
            cmd = "python -m tumblepipe.farm"
            package-env = true
            requirements = ["extra-dep"]
            "#,
        )
        .unwrap();
        assert!(!scripts.commands.get("plain").unwrap().uses_package_env());
        assert!(!scripts.commands.get("tt").unwrap().uses_package_env());

        let farm = scripts.commands.get("farm").unwrap();
        assert!(farm.uses_package_env());
        assert_eq!(farm.requirements(), &["extra-dep".to_string()]);
    }

    #[test]
    fn package_env_false_is_not_serialized() {
        let entry = ScriptEntry::WithEnv(ScriptEnv {
            cmd: EnvValue::Flat("ruff .".to_string()),
            python: None,
            requirements: Vec::new(),
            label: None,
            description: None,
            package_env: false,
        });
        let toml = toml::to_string(&entry).unwrap();
        assert!(
            !toml.contains("package-env"),
            "unset package-env must not round-trip: {toml}"
        );
    }

    #[test]
    fn parses_label_and_description() {
        let scripts: PackageScripts = toml::from_str(
            r#"
            plain = "ruff ."

            [launch]
            cmd = "python -m mytool.ui"
            label = "Launch My Tool"
            description = "Open the main UI"
            "#,
        )
        .unwrap();

        // Plain entries carry no metadata.
        let plain = scripts.commands.get("plain").unwrap();
        assert_eq!(plain.label(), None);
        assert_eq!(plain.description(), None);

        let launch = scripts.commands.get("launch").unwrap();
        assert_eq!(launch.label(), Some("Launch My Tool"));
        assert_eq!(launch.description(), Some("Open the main UI"));
    }

    #[test]
    fn label_and_description_default_to_none() {
        let scripts: PackageScripts = toml::from_str(
            r#"
            [tt]
            cmd = "python scripts/tt.py"
            python = "3.11"
            "#,
        )
        .unwrap();
        let tt = scripts.commands.get("tt").unwrap();
        assert_eq!(tt.label(), None);
        assert_eq!(tt.description(), None);
    }

    #[test]
    fn unset_label_and_description_are_not_serialized() {
        let entry = ScriptEntry::WithEnv(ScriptEnv {
            cmd: EnvValue::Flat("ruff .".to_string()),
            python: None,
            requirements: Vec::new(),
            label: None,
            description: None,
            package_env: false,
        });
        let toml = toml::to_string(&entry).unwrap();
        assert!(
            !toml.contains("label") && !toml.contains("description"),
            "unset metadata must not round-trip: {toml}"
        );
    }

    #[test]
    fn label_and_description_round_trip() {
        let entry = ScriptEntry::WithEnv(ScriptEnv {
            cmd: EnvValue::Flat("python -m mytool.ui".to_string()),
            python: None,
            requirements: Vec::new(),
            label: Some("Launch My Tool".to_string()),
            description: Some("Open the main UI".to_string()),
            package_env: false,
        });
        let toml = toml::to_string(&entry).unwrap();
        let back: ScriptEntry = toml::from_str(&toml).unwrap();
        assert_eq!(back.label(), Some("Launch My Tool"));
        assert_eq!(back.description(), Some("Open the main UI"));
    }

    #[test]
    fn package_env_true_round_trips() {
        let entry = ScriptEntry::WithEnv(ScriptEnv {
            cmd: EnvValue::Flat("python -m tumblepipe.farm".to_string()),
            python: None,
            requirements: Vec::new(),
            label: None,
            description: None,
            package_env: true,
        });
        let toml = toml::to_string(&entry).unwrap();
        assert!(toml.contains("package-env = true"), "{toml}");
        let back: ScriptEntry = toml::from_str(&toml).unwrap();
        assert!(back.uses_package_env());
    }
}
