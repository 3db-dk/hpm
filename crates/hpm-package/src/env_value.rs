//! Conditional `[runtime]` value support: hpm.toml schema and lowering to
//! Houdini's `package.json` expression form.
//!
//! Authors can write a flat string (today's behaviour) or a list of
//! `{ when, set }` variants. Each `when` is a structured selector that hpm
//! translates to a Houdini-side expression like
//! `houdini_version >= '21' and houdini_version < '22' and houdini_os == 'linux'`.
//! Houdini evaluates the expressions at startup — and applies **every**
//! matching element, not the first match, so hpm compiles the branches to
//! be mutually exclusive (see [`lower_conditional`]) to deliver the
//! documented first-match semantics.
//! See <https://www.sidefx.com/docs/houdini/ref/plugins.html>.

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

/// Houdini OS keyword for `when.os` selectors — the values Houdini's
/// `houdini_os` variable takes. Matches [`crate::Platform::os_key`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OsKey {
    Linux,
    Macos,
    Windows,
}

impl OsKey {
    /// The `houdini_os` keyword (`"linux"`, `"macos"`, `"windows"`).
    pub fn as_str(&self) -> &'static str {
        match self {
            OsKey::Linux => "linux",
            OsKey::Macos => "macos",
            OsKey::Windows => "windows",
        }
    }
}

impl std::str::FromStr for OsKey {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "linux" => Ok(OsKey::Linux),
            "macos" => Ok(OsKey::Macos),
            "windows" => Ok(OsKey::Windows),
            other => Err(format!(
                "unknown os '{other}' (expected 'linux', 'macos', or 'windows')"
            )),
        }
    }
}

impl std::fmt::Display for OsKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Install-source axis for `when.install_source` selectors: `dev` matches a
/// path-installed (dev) package, `registry` a registry/URL install.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InstallSource {
    Dev,
    Registry,
}

impl InstallSource {
    /// Manifest string form (`"dev"` / `"registry"`).
    pub fn as_str(&self) -> &'static str {
        match self {
            InstallSource::Dev => "dev",
            InstallSource::Registry => "registry",
        }
    }
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
    /// Houdini OS keyword: `"linux"`, `"macos"`, or `"windows"`. Typed, so
    /// an unknown keyword fails at manifest load.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<OsKey>,
    /// Houdini Python identifier: `"3.11"`, `"3.10"`, etc. Compiles to
    /// `houdini_python == 'python3.11'`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub python: Option<String>,
    /// Install-source filter: `"dev"` (path dependency) or `"registry"`
    /// (registry/URL install). Filtered at install time and never emitted to
    /// Houdini — so a branch gated `install_source = "dev"` disappears
    /// entirely from a published consumer's `package.json`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_source: Option<InstallSource>,
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
    pub fn matches_install_source(&self, is_dev: bool) -> bool {
        match self.install_source {
            None => true,
            Some(InstallSource::Dev) => is_dev,
            Some(InstallSource::Registry) => !is_dev,
        }
    }
}

/// One atomic comparison in a compiled runtime condition
/// (`houdini_version >= '21'`, `houdini_os == 'linux'`, ...).
///
/// Kept structured so a branch condition can also be emitted *negated*.
/// Houdini's package-expression grammar (verified with hconfig on
/// 21.0.688 / 21.0.729) has `and`, `or`, parentheses, and the comparison
/// operators — but no `not` and no boolean literals — so negation happens
/// by flipping each comparison and joining with `or` (De Morgan).
#[derive(Debug, Clone)]
struct ConditionAtom {
    lhs: &'static str,
    op: &'static str,
    rhs: String,
}

impl ConditionAtom {
    fn new(lhs: &'static str, op: &'static str, rhs: impl Into<String>) -> Self {
        Self {
            lhs,
            op,
            rhs: rhs.into(),
        }
    }

    fn compile(&self) -> String {
        format!("{} {} '{}'", self.lhs, self.op, self.rhs)
    }

    fn compile_negated(&self) -> String {
        let flipped = match self.op {
            ">=" => "<",
            "<" => ">=",
            "<=" => ">",
            ">" => "<=",
            "==" => "!=",
            "!=" => "==",
            other => unreachable!("unknown comparison operator {other}"),
        };
        format!("{} {} '{}'", self.lhs, flipped, self.rhs)
    }
}

/// Compile a `Condition` into the Houdini-side expression string.
///
/// Returns `Ok(None)` if the selector contributes nothing to the runtime
/// expression (empty selector, or `install_source`-only — that axis is
/// filtered at install time, not by Houdini). There is no expression
/// encoding for "always true" (Houdini has no boolean literals in this
/// grammar); [`lower_conditional`] handles unconditional branches
/// structurally instead.
pub fn compile_condition(selector: &Condition) -> Result<Option<String>, ExpressionError> {
    let atoms = condition_atoms(selector)?;
    if atoms.is_empty() {
        Ok(None)
    } else {
        Ok(Some(
            atoms
                .iter()
                .map(ConditionAtom::compile)
                .collect::<Vec<_>>()
                .join(" and "),
        ))
    }
}

/// The atomic comparisons of a `Condition`'s runtime axes, in axis order
/// (houdini, os, python). Empty for an always-true selector. The `os` and
/// `install_source` axes are typed, so only the string-shaped axes
/// (`houdini`, `python`) can fail here.
fn condition_atoms(selector: &Condition) -> Result<Vec<ConditionAtom>, ExpressionError> {
    let mut atoms: Vec<ConditionAtom> = Vec::new();

    if let Some(range) = &selector.houdini {
        atoms.extend(houdini_req_atoms(range.as_str())?);
    }
    if let Some(os) = &selector.os {
        atoms.push(ConditionAtom::new("houdini_os", "==", os.as_str()));
    }
    if let Some(py) = &selector.python {
        atoms.push(ConditionAtom::new(
            "houdini_python",
            "==",
            python_identifier(py)?,
        ));
    }

    Ok(atoms)
}

/// Normalize a python axis value (`"3.11"` or `"python3.11"`) to the
/// `python<v>` identifier Houdini compares `houdini_python` against.
fn python_identifier(py: &str) -> Result<String, ExpressionError> {
    if py.is_empty() {
        return Err(ExpressionError::InvalidPython(py.to_string()));
    }
    let trimmed = py.strip_prefix("python").unwrap_or(py);
    if !trimmed.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return Err(ExpressionError::InvalidPython(py.to_string()));
    }
    Ok(format!("python{}", trimmed))
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
    let atoms = comparator_atoms(part)?;
    Some(
        atoms
            .iter()
            .map(ConditionAtom::compile)
            .collect::<Vec<_>>()
            .join(" and "),
    )
}

/// The whole requirement as a flat conjunction of atoms (comma-separated
/// comparators all AND together).
fn houdini_req_atoms(req: &str) -> Result<Vec<ConditionAtom>, ExpressionError> {
    let trimmed = req.trim();
    if trimmed.is_empty() {
        return Err(ExpressionError::InvalidHoudiniReq(req.to_string()));
    }
    let mut atoms: Vec<ConditionAtom> = Vec::new();
    for raw_part in trimmed.split(',') {
        let part = raw_part.trim();
        if part.is_empty() {
            return Err(ExpressionError::InvalidHoudiniReq(req.to_string()));
        }
        atoms.extend(
            comparator_atoms(part)
                .ok_or_else(|| ExpressionError::InvalidHoudiniReq(req.to_string()))?,
        );
    }
    Ok(atoms)
}

fn comparator_atoms(part: &str) -> Option<Vec<ConditionAtom>> {
    // Prefix order matters: two-character operators before their
    // one-character prefixes, and `=` (alias of `==`) after both.
    for (prefix, op) in [
        (">=", ">="),
        ("<=", "<="),
        ("==", "=="),
        (">", ">"),
        ("<", "<"),
        ("=", "=="),
    ] {
        if let Some(rest) = part.strip_prefix(prefix) {
            let v = rest.trim();
            if !is_simple_version(v) {
                return None;
            }
            return Some(vec![ConditionAtom::new("houdini_version", op, v)]);
        }
    }

    if let Some(rest) = part.strip_prefix('^') {
        return caret_atoms(rest.trim());
    }
    if let Some(rest) = part.strip_prefix('~') {
        return tilde_atoms(rest.trim());
    }

    // Bare version is shorthand for caret.
    caret_atoms(part)
}

fn caret_atoms(v: &str) -> Option<Vec<ConditionAtom>> {
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
    Some(vec![
        ConditionAtom::new("houdini_version", ">=", lower),
        ConditionAtom::new("houdini_version", "<", upper),
    ])
}

fn tilde_atoms(v: &str) -> Option<Vec<ConditionAtom>> {
    let parts = parse_simple_version(v)?;
    let lower = format!("{}", DisplayVersion(&parts));
    let upper = match parts.as_slice() {
        [maj, m, _] => format!("{}.{}", maj, m + 1),
        [maj, m] => format!("{}.{}", maj, m + 1),
        [maj] => format!("{}", maj + 1),
        _ => return None,
    };
    Some(vec![
        ConditionAtom::new("houdini_version", ">=", lower),
        ConditionAtom::new("houdini_version", "<", upper),
    ])
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
    #[error("invalid python identifier: {0}")]
    InvalidPython(String),
}

/// Lower a `Conditional` env value into Houdini's `[{ "<expr>": "<val>" }, …]`
/// shape, applying the supplied substitutions to each branch's `set` string.
///
/// Result of lowering a `Conditional` env value for one install context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoweredConditional {
    /// The first surviving branch is unconditional, so the whole value
    /// collapses to its string — under first-match semantics the later
    /// branches are unreachable and are dropped.
    Unconditional(String),
    /// Ordered `{ "<expr>": "<value>" }` branches for Houdini's
    /// conditional-object array form. May be empty when every branch was
    /// filtered out by `install_source`.
    Branches(Vec<HashMap<String, String>>),
}

/// Variants whose `install_source` axis does not match `is_dev` are
/// filtered out before lowering, so install-time gates never reach the
/// Houdini-side expression. The `install_source` axis is also stripped
/// from surviving variants when compiling the runtime `when` expression.
/// Substitutions are applied verbatim with `String::replace`, mirroring
/// the flat-value path.
///
/// The manifest promises first-match semantics, but Houdini's package
/// system applies *every* element of a conditional-object array whose
/// expression matches (verified with hconfig on 21.0.688 / 21.0.729). So
/// each branch's expression is emitted with the negation of every earlier
/// branch AND-ed on — negation by comparison-flipping joined with `or`,
/// since the expression grammar has no `not`. Houdini also has no boolean
/// literals (`{"true": v}` silently defines a *variable named `true`*
/// instead of applying `v` — the old encoding of unconditional branches
/// was broken), so unconditional branches are handled structurally: a
/// leading one collapses the whole value to
/// [`LoweredConditional::Unconditional`], and a trailing fallback's
/// expression is just the accumulated negations. Either way, anything
/// after an unconditional branch is unreachable and is dropped.
pub fn lower_conditional(
    variants: &[EnvValueBranch],
    substitutions: &[(&str, &str)],
    is_dev: bool,
) -> Result<LoweredConditional, ExpressionError> {
    let mut branches: Vec<HashMap<String, String>> = Vec::new();
    // One entry per emitted branch: its condition negated, parenthesized
    // when it is a multi-atom disjunction.
    let mut prior_negations: Vec<String> = Vec::new();

    for variant in variants {
        if !variant.when.matches_install_source(is_dev) {
            continue;
        }
        // Strip the install_source axis before compiling — it must not
        // appear in the Houdini-side expression.
        let runtime_when = Condition {
            install_source: None,
            ..variant.when.clone()
        };
        let atoms = condition_atoms(&runtime_when)?;

        let mut value = variant.set.clone();
        for (from, to) in substitutions {
            value = value.replace(from, to);
        }

        if atoms.is_empty() && prior_negations.is_empty() {
            // The first surviving branch always matches: the value is
            // effectively flat in this install context.
            return Ok(LoweredConditional::Unconditional(value));
        }

        let mut parts: Vec<String> = atoms.iter().map(ConditionAtom::compile).collect();
        parts.extend(prior_negations.iter().cloned());
        let mut map = HashMap::new();
        map.insert(parts.join(" and "), value);
        branches.push(map);

        if atoms.is_empty() {
            // Unconditional fallback after conditional branches: it fires
            // iff none of them matched, and shadows everything after it.
            break;
        }

        let negated: Vec<String> = atoms.iter().map(ConditionAtom::compile_negated).collect();
        prior_negations.push(if negated.len() == 1 {
            negated.into_iter().next().expect("len checked")
        } else {
            format!("( {} )", negated.join(" or "))
        });
    }

    Ok(LoweredConditional::Branches(branches))
}

#[cfg(test)]
#[path = "env_value_tests.rs"]
mod tests;
