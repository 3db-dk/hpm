//! Conditional `[env]` value support: hpm.toml schema and lowering to
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
pub enum EnvValueSpec {
    Flat(String),
    Conditional(Vec<EnvValueVariant>),
}

impl EnvValueSpec {
    /// The flat value, if any. Returns `None` for the conditional shape.
    pub fn as_flat(&self) -> Option<&str> {
        match self {
            EnvValueSpec::Flat(s) => Some(s.as_str()),
            EnvValueSpec::Conditional(_) => None,
        }
    }
}

impl From<String> for EnvValueSpec {
    fn from(s: String) -> Self {
        EnvValueSpec::Flat(s)
    }
}

impl From<&str> for EnvValueSpec {
    fn from(s: &str) -> Self {
        EnvValueSpec::Flat(s.to_string())
    }
}

/// One branch of a conditional value: a selector plus the string to use when
/// it matches.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnvValueVariant {
    #[serde(default, skip_serializing_if = "WhenSelector::is_empty")]
    pub when: WhenSelector,
    pub set: String,
}

/// Selector axes for a conditional env-value branch.
///
/// All present axes combine with `and`. An empty selector is "always-true"
/// and produces a fallback branch that matches every Houdini.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WhenSelector {
    /// Cargo-style version requirement against `houdini_version`.
    /// Examples: `"^21"`, `"~21.5"`, `">=21, <22.5"`, `"21"` (alias for `^21`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub houdini: Option<String>,
    /// Houdini OS keyword: `"linux"`, `"macos"`, or `"windows"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    /// Houdini Python identifier: `"3.11"`, `"3.10"`, etc. Compiles to
    /// `houdini_python == 'python3.11'`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub python: Option<String>,
}

impl WhenSelector {
    pub fn is_empty(&self) -> bool {
        self.houdini.is_none() && self.os.is_none() && self.python.is_none()
    }
}

/// Compile a `WhenSelector` into the Houdini-side expression string.
///
/// Returns `Ok(None)` if the selector is empty (always-true). The caller
/// decides how to encode that — typically as a literal `true` expression
/// at the end of the conditional array.
pub fn compile_when(selector: &WhenSelector) -> Result<Option<String>, ExpressionError> {
    let mut parts: Vec<String> = Vec::new();

    if let Some(req) = &selector.houdini {
        parts.push(compile_houdini_req(req)?);
    }
    if let Some(os) = &selector.os {
        parts.push(compile_os(os)?);
    }
    if let Some(py) = &selector.python {
        parts.push(compile_python(py)?);
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
}

/// Lower a `Conditional` env value into Houdini's `[{ "<expr>": "<val>" }, …]`
/// shape, applying the supplied substitutions to each branch's `set` string.
///
/// Substitutions are applied verbatim with `String::replace`, mirroring the
/// flat-value path. An empty `when` is encoded as the literal `"true"`
/// expression so it acts as a fallback branch.
pub fn lower_conditional(
    variants: &[EnvValueVariant],
    substitutions: &[(&str, &str)],
) -> Result<Vec<HashMap<String, String>>, ExpressionError> {
    let mut out = Vec::with_capacity(variants.len());
    for variant in variants {
        let expr = compile_when(&variant.when)?.unwrap_or_else(|| "true".to_string());
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
mod tests {
    use super::*;

    #[test]
    fn caret_major_only_expands_to_major_range() {
        let s = compile_houdini_req("^21").unwrap();
        assert_eq!(s, "houdini_version >= '21' and houdini_version < '22'");
    }

    #[test]
    fn bare_major_aliases_caret() {
        let s = compile_houdini_req("21").unwrap();
        assert_eq!(s, "houdini_version >= '21' and houdini_version < '22'");
    }

    #[test]
    fn tilde_major_minor_expands_to_minor_range() {
        let s = compile_houdini_req("~21.5").unwrap();
        assert_eq!(s, "houdini_version >= '21.5' and houdini_version < '21.6'");
    }

    #[test]
    fn comma_separated_comparators_combine_with_and() {
        let s = compile_houdini_req(">=21, <22.5").unwrap();
        assert_eq!(s, "(houdini_version >= '21' and houdini_version < '22.5')");
    }

    #[test]
    fn houdini_lower_bound_extraction() {
        assert_eq!(houdini_req_lower_bound("20.5"), Some("20.5".to_string()));
        assert_eq!(houdini_req_lower_bound("^21"), Some("21".to_string()));
        assert_eq!(houdini_req_lower_bound("~21.5"), Some("21.5".to_string()));
        assert_eq!(houdini_req_lower_bound(">=20.5"), Some("20.5".to_string()));
        assert_eq!(
            houdini_req_lower_bound(">=20.5, <22"),
            Some("20.5".to_string())
        );
        assert_eq!(
            houdini_req_lower_bound("<22, >=20.5"),
            Some("20.5".to_string())
        );
        assert_eq!(houdini_req_lower_bound("<22"), None);
        assert_eq!(houdini_req_lower_bound("<=21"), None);
        assert_eq!(houdini_req_lower_bound(""), None);
        assert_eq!(houdini_req_lower_bound("garbage"), None);
    }

    #[test]
    fn invalid_houdini_req_rejected() {
        assert!(compile_houdini_req("not-a-version").is_err());
        assert!(compile_houdini_req("").is_err());
        assert!(compile_houdini_req(">=").is_err());
    }

    #[test]
    fn os_translates_to_houdini_os() {
        assert_eq!(
            compile_when(&WhenSelector {
                os: Some("linux".to_string()),
                ..Default::default()
            })
            .unwrap()
            .unwrap(),
            "houdini_os == 'linux'"
        );
    }

    #[test]
    fn unknown_os_rejected() {
        assert!(
            compile_when(&WhenSelector {
                os: Some("bsd".to_string()),
                ..Default::default()
            })
            .is_err()
        );
    }

    #[test]
    fn python_translates_with_or_without_python_prefix() {
        assert_eq!(
            compile_when(&WhenSelector {
                python: Some("3.11".to_string()),
                ..Default::default()
            })
            .unwrap()
            .unwrap(),
            "houdini_python == 'python3.11'"
        );
        assert_eq!(
            compile_when(&WhenSelector {
                python: Some("python3.10".to_string()),
                ..Default::default()
            })
            .unwrap()
            .unwrap(),
            "houdini_python == 'python3.10'"
        );
    }

    #[test]
    fn multiple_axes_combine_with_and() {
        let s = compile_when(&WhenSelector {
            houdini: Some("^21".to_string()),
            os: Some("linux".to_string()),
            python: None,
        })
        .unwrap()
        .unwrap();
        assert_eq!(
            s,
            "houdini_version >= '21' and houdini_version < '22' and houdini_os == 'linux'"
        );
    }

    #[test]
    fn empty_when_compiles_to_none() {
        assert!(compile_when(&WhenSelector::default()).unwrap().is_none());
    }

    #[test]
    fn lower_conditional_substitutes_pkg_root_per_branch() {
        let variants = vec![
            EnvValueVariant {
                when: WhenSelector {
                    houdini: Some("^21".to_string()),
                    ..Default::default()
                },
                set: "$HPM_PACKAGE_ROOT/h21/x".to_string(),
            },
            EnvValueVariant {
                when: WhenSelector {
                    houdini: Some("^22".to_string()),
                    ..Default::default()
                },
                set: "$HPM_PACKAGE_ROOT/h22/x".to_string(),
            },
        ];
        let lowered = lower_conditional(&variants, &[("$HPM_PACKAGE_ROOT", "/abs/pkg")]).unwrap();
        assert_eq!(lowered.len(), 2);
        let first = &lowered[0];
        let key = first.keys().next().unwrap();
        assert!(key.contains("21"));
        assert_eq!(first[key], "/abs/pkg/h21/x");
    }

    #[test]
    fn empty_when_lowered_as_true_branch() {
        let variants = vec![EnvValueVariant {
            when: WhenSelector::default(),
            set: "default".to_string(),
        }];
        let lowered = lower_conditional(&variants, &[]).unwrap();
        assert_eq!(lowered.len(), 1);
        assert_eq!(lowered[0]["true"], "default");
    }

    #[test]
    fn env_value_spec_round_trips_flat() {
        let toml_str = r#"value = "hello""#;
        #[derive(Deserialize, Serialize)]
        struct Holder {
            value: EnvValueSpec,
        }
        let h: Holder = toml::from_str(toml_str).unwrap();
        assert_eq!(h.value.as_flat(), Some("hello"));
    }

    #[test]
    fn env_value_spec_round_trips_conditional() {
        let toml_str = r#"
value = [
  { when = { houdini = "^21" }, set = "a" },
  { when = { houdini = "^22" }, set = "b" },
]
"#;
        #[derive(Deserialize)]
        struct Holder {
            value: EnvValueSpec,
        }
        let h: Holder = toml::from_str(toml_str).unwrap();
        match h.value {
            EnvValueSpec::Conditional(v) => {
                assert_eq!(v.len(), 2);
                assert_eq!(v[0].set, "a");
                assert_eq!(v[1].when.houdini.as_deref(), Some("^22"));
            }
            EnvValueSpec::Flat(_) => panic!("expected conditional"),
        }
    }

    #[test]
    fn unknown_when_axis_rejected() {
        let toml_str = r#"
value = [
  { when = { weather = "sunny" }, set = "a" },
]
"#;
        #[derive(Deserialize)]
        struct Holder {
            #[allow(dead_code)]
            value: EnvValueSpec,
        }
        let res: Result<Holder, _> = toml::from_str(toml_str);
        assert!(res.is_err(), "deny_unknown_fields should reject 'weather'");
    }
}
