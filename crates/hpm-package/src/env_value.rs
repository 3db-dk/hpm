//! Conditional `[runtime]` value support: hpm.toml schema and lowering to
//! Houdini's `package.json` expression form.
//!
//! Authors can write a flat string (today's behaviour) or a list of
//! `{ when, set }` variants. Each `when` is a structured selector that hpm
//! translates to a Houdini-side expression like
//! `houdini_version >= '21' and houdini_version < '22' and houdini_os == 'linux'`.
//! Houdini evaluates the expressions at startup and picks the first matching
//! branch — see <https://www.sidefx.com/docs/houdini/ref/plugins.html>.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The value of a `[env.<KEY>]` entry in `hpm.toml`.
///
/// `Flat` is a single string, used directly (with `$HPM_PACKAGE_ROOT`
/// substituted). `Conditional` is an ordered list of variants — the first
/// whose `when` matches the running Houdini wins.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum EnvValue {
    Flat(String),
    Conditional(Vec<EnvValueBranch>),
}

impl EnvValue {
    /// The flat value, if any. Returns `None` for the conditional shape.
    pub fn as_flat(&self) -> Option<&str> {
        match self {
            EnvValue::Flat(s) => Some(s.as_str()),
            EnvValue::Conditional(_) => None,
        }
    }
}

impl From<String> for EnvValue {
    fn from(s: String) -> Self {
        EnvValue::Flat(s)
    }
}

impl From<&str> for EnvValue {
    fn from(s: &str) -> Self {
        EnvValue::Flat(s.to_string())
    }
}

/// One branch of a conditional value: a selector plus the string to use when
/// it matches.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnvValueBranch {
    #[serde(default, skip_serializing_if = "Condition::is_empty")]
    pub when: Condition,
    pub set: String,
}

/// Selector axes for a conditional env-value branch.
///
/// The `houdini`, `os`, and `python` axes are *runtime-evaluated by Houdini*
/// — they compile to a `package.json` expression that Houdini evaluates at
/// startup. `install_source` is *install-time evaluated by hpm* — it filters
/// out non-matching branches before the Houdini package.json is emitted, so
/// it never appears in the runtime expression.
///
/// All present axes combine with `and`. An empty selector is "always-true"
/// and produces a fallback branch.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    /// Cargo-style version requirement against `houdini_version`. Same
    /// grammar as `[compat].houdini`; parses to a [`HoudiniRange`] at
    /// deserialize so malformed branches fail at manifest load, not at
    /// install/emit time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub houdini: Option<HoudiniRange>,
    /// Houdini OS keyword: `"linux"`, `"macos"`, or `"windows"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    /// Houdini Python identifier: `"3.11"`, `"3.10"`, etc. Compiles to
    /// `houdini_python == 'python3.11'`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub python: Option<String>,
    /// Install-source filter: `"dev"` (path dependency) or `"registry"`
    /// (registry/URL install). Filtered at install time and never emitted to
    /// Houdini — so a branch gated `install_source = "dev"` disappears
    /// entirely from a published consumer's `package.json`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_source: Option<String>,
}

impl Condition {
    pub fn is_empty(&self) -> bool {
        self.houdini.is_none()
            && self.os.is_none()
            && self.python.is_none()
            && self.install_source.is_none()
    }

    /// True when this selector's `install_source` axis matches the given
    /// install context. A `None` axis matches both. Other axes are ignored
    /// here — those are evaluated by Houdini, not hpm.
    ///
    /// Errors on an unknown `install_source` value so the typo surfaces at
    /// install time rather than silently dropping the variant.
    pub fn matches_install_source(&self, is_dev: bool) -> Result<bool, ExpressionError> {
        match self.install_source.as_deref() {
            None => Ok(true),
            Some("dev") => Ok(is_dev),
            Some("registry") => Ok(!is_dev),
            Some(other) => Err(ExpressionError::UnknownInstallSource(other.to_string())),
        }
    }
}

/// Compile a `Condition` into the Houdini-side expression string.
///
/// Returns `Ok(None)` if the selector contributes nothing to the runtime
/// expression (empty selector, or `install_source`-only — that axis is
/// filtered at install time, not by Houdini). The caller decides how to
/// encode `None` — typically as a literal `true` expression at the end of
/// the conditional array.
///
/// Also validates `install_source` is one of `"dev"` / `"registry"` —
/// unknown values would otherwise silently drop the branch at install time.
pub fn compile_condition(selector: &Condition) -> Result<Option<String>, ExpressionError> {
    let mut parts: Vec<String> = Vec::new();

    if let Some(range) = &selector.houdini {
        // Range parsed and validated at deserialize time; emission is
        // infallible.
        parts.push(range.to_enable_expression());
    }
    if let Some(os) = &selector.os {
        parts.push(compile_os(os)?);
    }
    if let Some(py) = &selector.python {
        parts.push(compile_python(py)?);
    }
    if let Some(src) = &selector.install_source {
        match src.as_str() {
            "dev" | "registry" => {}
            _ => return Err(ExpressionError::UnknownInstallSource(src.clone())),
        }
    }

    if parts.is_empty() {
        Ok(None)
    } else {
        Ok(Some(parts.join(" and ")))
    }
}

fn compile_os(os: &str) -> Result<String, ExpressionError> {
    match os {
        "linux" | "macos" | "windows" => Ok(format!("houdini_os == '{}'", os)),
        _ => Err(ExpressionError::UnknownOs(os.to_string())),
    }
}

fn compile_python(py: &str) -> Result<String, ExpressionError> {
    if py.is_empty() {
        return Err(ExpressionError::InvalidPython(py.to_string()));
    }
    let trimmed = py.strip_prefix("python").unwrap_or(py);
    if !trimmed.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return Err(ExpressionError::InvalidPython(py.to_string()));
    }
    Ok(format!("houdini_python == 'python{}'", trimmed))
}

/// A validated Cargo-style Houdini version range, e.g. `"^21"`,
/// `">=20.5, <22"`, or `"20.5"` (bare = caret).
///
/// Construction goes through [`Self::parse`], which compiles the range
/// via [`compile_houdini_req`] to confirm it is syntactically valid. By
/// the time you hold a `HoudiniRange`, you know:
///
/// - The range parses under the supported grammar.
/// - [`Self::to_enable_expression`] and [`Self::lower_bound`] are
///   infallible.
///
/// Stored as a String for cheap round-trips through TOML and so the
/// human-authored form survives in error messages and round-trip
/// serialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct HoudiniRange(String);

impl HoudiniRange {
    /// Parse and validate a houdini range string. Errors mirror
    /// [`compile_houdini_req`]'s — the same parser drives both.
    pub fn parse(req: impl Into<String>) -> Result<Self, ExpressionError> {
        let req = req.into();
        // Compile once to validate. We don't store the result because
        // callers may want the original text (for round-trip
        // serialization, error messages, etc.).
        compile_houdini_req(&req)?;
        Ok(Self(req))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Compile to the Houdini package.json `enable` expression.
    ///
    /// Infallible: parsing succeeded at construction time, so the
    /// re-compile here cannot fail. Returns the canonical Houdini-side
    /// expression string.
    pub fn to_enable_expression(&self) -> String {
        compile_houdini_req(&self.0).expect("HoudiniRange validated at parse time")
    }

    /// Lower bound of the range, if any clause implies one. See
    /// [`houdini_req_lower_bound`] for the matrix.
    pub fn lower_bound(&self) -> Option<String> {
        houdini_req_lower_bound(&self.0)
    }

    /// Whether the range bounds above. See [`houdini_req_has_upper_bound`].
    pub fn has_upper_bound(&self) -> bool {
        houdini_req_has_upper_bound(&self.0)
    }
}

impl std::str::FromStr for HoudiniRange {
    type Err = ExpressionError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl std::fmt::Display for HoudiniRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for HoudiniRange {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(de)?;
        Self::parse(raw).map_err(serde::de::Error::custom)
    }
}

/// Extract the lower bound of a Cargo-style houdini version requirement, if
/// any clause implies one.
///
/// Returns the version string from the first comparator with an implied lower
/// bound (`>=`, `>`, `==`, `^`, `~`, or a bare version — bare aliases caret).
/// Pure upper-bound comparators (`<`, `<=`) are skipped, so `"<22"` returns
/// `None` and `"<22, >=20.5"` returns `Some("20.5")`.
///
/// Used by Python ABI selection: the lower bound of `[compat].houdini`
/// determines which embedded CPython version a project's venv must match.
pub fn houdini_req_lower_bound(req: &str) -> Option<String> {
    let trimmed = req.trim();
    if trimmed.is_empty() {
        return None;
    }
    for raw_part in trimmed.split(',') {
        let part = raw_part.trim();
        if part.is_empty() {
            continue;
        }
        let version = if let Some(rest) = part.strip_prefix(">=") {
            rest.trim()
        } else if let Some(rest) = part.strip_prefix("==") {
            rest.trim()
        } else if let Some(rest) = part.strip_prefix(">") {
            rest.trim()
        } else if part.starts_with("<=") || part.starts_with("<") {
            continue;
        } else if let Some(rest) = part.strip_prefix('^') {
            rest.trim()
        } else if let Some(rest) = part.strip_prefix('~') {
            rest.trim()
        } else {
            part
        };
        if is_simple_version(version) {
            return Some(version.to_string());
        }
    }
    None
}

/// Whether a Cargo-style houdini version requirement has an upper bound.
///
/// Returns `true` if any comma-separated clause implies an upper bound
/// (`<`, `<=`, `=`, `==`, `^`, `~`, or a bare version — bare aliases
/// caret). Pure lower-bound clauses (`>=`, `>`) do not contribute.
///
/// Used by `hpm check` to flag packages that ship native binaries (have
/// `[compat].platforms` declared) yet leave their Houdini range
/// unbounded above — a footgun, since DSOs compiled against one Houdini
/// major typically won't load in the next.
pub fn houdini_req_has_upper_bound(req: &str) -> bool {
    req.trim()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .any(|p| {
            // Lower-only forms — these don't bound above.
            if let Some(rest) = p.strip_prefix(">=") {
                return !is_simple_version(rest.trim());
            }
            if let Some(rest) = p.strip_prefix('>') {
                return !is_simple_version(rest.trim());
            }
            // Everything else implies an upper bound when it parses:
            //   <X, <=X (explicit upper), =X, ==X (exact),
            //   ^X, ~X, bare X (semver compatibility ranges).
            true
        })
}

/// Translate a Cargo-style version requirement into a Houdini expression.
///
/// Each comma-separated comparator becomes one `houdini_version <op> '<v>'`
/// clause; caret/tilde/bare-version forms expand to `>= X and < Y` ranges
/// using semver upper bounds. All clauses combine with `and`, parenthesised
/// so they compose cleanly when joined with other axes.
pub fn compile_houdini_req(req: &str) -> Result<String, ExpressionError> {
    let trimmed = req.trim();
    if trimmed.is_empty() {
        return Err(ExpressionError::InvalidHoudiniReq(req.to_string()));
    }

    // Split top-level on commas (semver allows `>=21, <22`).
    let mut clauses: Vec<String> = Vec::new();
    for raw_part in trimmed.split(',') {
        let part = raw_part.trim();
        if part.is_empty() {
            return Err(ExpressionError::InvalidHoudiniReq(req.to_string()));
        }
        let expanded = expand_comparator(part)
            .ok_or_else(|| ExpressionError::InvalidHoudiniReq(req.to_string()))?;
        clauses.push(expanded);
    }

    if clauses.len() == 1 {
        Ok(clauses.into_iter().next().unwrap())
    } else {
        Ok(format!("({})", clauses.join(" and ")))
    }
}

fn expand_comparator(part: &str) -> Option<String> {
    if let Some(rest) = part.strip_prefix(">=") {
        let v = rest.trim();
        if !is_simple_version(v) {
            return None;
        }
        return Some(format!("houdini_version >= '{}'", v));
    }
    if let Some(rest) = part.strip_prefix("<=") {
        let v = rest.trim();
        if !is_simple_version(v) {
            return None;
        }
        return Some(format!("houdini_version <= '{}'", v));
    }
    if let Some(rest) = part.strip_prefix("==") {
        let v = rest.trim();
        if !is_simple_version(v) {
            return None;
        }
        return Some(format!("houdini_version == '{}'", v));
    }
    if let Some(rest) = part.strip_prefix(">") {
        let v = rest.trim();
        if !is_simple_version(v) {
            return None;
        }
        return Some(format!("houdini_version > '{}'", v));
    }
    if let Some(rest) = part.strip_prefix("<") {
        let v = rest.trim();
        if !is_simple_version(v) {
            return None;
        }
        return Some(format!("houdini_version < '{}'", v));
    }

    if let Some(rest) = part.strip_prefix('^') {
        let v = rest.trim();
        return caret_range(v);
    }
    if let Some(rest) = part.strip_prefix('~') {
        let v = rest.trim();
        return tilde_range(v);
    }

    // Bare version is shorthand for caret.
    caret_range(part)
}

fn caret_range(v: &str) -> Option<String> {
    let parts = parse_simple_version(v)?;
    let lower = format!("{}", DisplayVersion(&parts));
    let upper = match parts.as_slice() {
        [0, 0, p] => format!("0.0.{}", p + 1),
        [0, m, _] => format!("0.{}", m + 1),
        [maj, _, _] => format!("{}", maj + 1),
        [0, m] => format!("0.{}", m + 1),
        [maj, _] => format!("{}", maj + 1),
        [maj] => format!("{}", maj + 1),
        _ => return None,
    };
    Some(format!(
        "houdini_version >= '{}' and houdini_version < '{}'",
        lower, upper
    ))
}

fn tilde_range(v: &str) -> Option<String> {
    let parts = parse_simple_version(v)?;
    let lower = format!("{}", DisplayVersion(&parts));
    let upper = match parts.as_slice() {
        [maj, m, _] => format!("{}.{}", maj, m + 1),
        [maj, m] => format!("{}.{}", maj, m + 1),
        [maj] => format!("{}", maj + 1),
        _ => return None,
    };
    Some(format!(
        "houdini_version >= '{}' and houdini_version < '{}'",
        lower, upper
    ))
}

fn parse_simple_version(v: &str) -> Option<Vec<u64>> {
    if v.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    for seg in v.split('.') {
        let n: u64 = seg.parse().ok()?;
        out.push(n);
    }
    if out.is_empty() || out.len() > 3 {
        return None;
    }
    Some(out)
}

fn is_simple_version(v: &str) -> bool {
    parse_simple_version(v).is_some()
}

struct DisplayVersion<'a>(&'a [u64]);

impl<'a> std::fmt::Display for DisplayVersion<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, n) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(".")?;
            }
            write!(f, "{}", n)?;
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExpressionError {
    #[error("invalid houdini version requirement: {0}")]
    InvalidHoudiniReq(String),
    #[error("unknown os '{0}' (expected 'linux', 'macos', or 'windows')")]
    UnknownOs(String),
    #[error("invalid python identifier: {0}")]
    InvalidPython(String),
    #[error("unknown install_source '{0}' (expected 'dev' or 'registry')")]
    UnknownInstallSource(String),
}

/// Lower a `Conditional` env value into Houdini's `[{ "<expr>": "<val>" }, …]`
/// shape, applying the supplied substitutions to each branch's `set` string.
///
/// Variants whose `install_source` axis does not match `is_dev` are
/// filtered out before lowering, so install-time gates never reach the
/// Houdini-side expression. The `install_source` axis is also stripped
/// from surviving variants when compiling the runtime `when` expression.
///
/// Returns an empty vec if every variant is filtered out. Callers that
/// treat that as "no effective value" can short-circuit emission.
/// Substitutions are applied verbatim with `String::replace`, mirroring the
/// flat-value path. An empty `when` is encoded as the literal `"true"`
/// expression so it acts as a fallback branch.
pub fn lower_conditional(
    variants: &[EnvValueBranch],
    substitutions: &[(&str, &str)],
    is_dev: bool,
) -> Result<Vec<HashMap<String, String>>, ExpressionError> {
    let mut out = Vec::with_capacity(variants.len());
    for variant in variants {
        if !variant.when.matches_install_source(is_dev)? {
            continue;
        }
        // Strip the install_source axis before compiling — it must not
        // appear in the Houdini-side expression.
        let runtime_when = Condition {
            install_source: None,
            ..variant.when.clone()
        };
        let expr = compile_condition(&runtime_when)?.unwrap_or_else(|| "true".to_string());
        let mut value = variant.set.clone();
        for (from, to) in substitutions {
            value = value.replace(from, to);
        }
        let mut map = HashMap::new();
        map.insert(expr, value);
        out.push(map);
    }
    Ok(out)
}

#[cfg(test)]
#[path = "env_value_tests.rs"]
mod tests;
