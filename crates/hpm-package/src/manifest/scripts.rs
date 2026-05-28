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
/// `cmd` is an [`EnvValue`] — either a flat string or an ordered list
/// of `{ when, set }` variants. For scripts only the `os` axis of `when`
/// is meaningful (HPM doesn't know the user's Houdini version or Python
/// at `hpm run` time); other axes on a script variant are rejected at
/// manifest validation time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptEnv {
    pub cmd: EnvValue,
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

    /// True when this script needs a uv-managed environment.
    pub fn needs_venv(&self) -> bool {
        self.python().is_some() || !self.requirements().is_empty()
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
