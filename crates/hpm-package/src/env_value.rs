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
    /// Install-source filter: `"dev"` (path dependency) or `"registry"`
    /// (registry/URL install). Filtered at install time and never emitted to
    /// Houdini — so a branch gated `install_source = "dev"` disappears
    /// entirely from a published consumer's `package.json`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_source: Option<String>,
}

impl WhenSelector {
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

/// Compile a `WhenSelector` into the Houdini-side expression string.
///
/// Returns `Ok(None)` if the selector contributes nothing to the runtime
/// expression (empty selector, or `install_source`-only — that axis is
/// filtered at install time, not by Houdini). The caller decides how to
/// encode `None` — typically as a literal `true` expression at the end of
/// the conditional array.
///
/// Also validates `install_source` is one of `"dev"` / `"registry"` —
/// unknown values would otherwise silently drop the branch at install time.
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
    variants: &[EnvValueVariant],
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
        let runtime_when = WhenSelector {
            install_source: None,
            ..variant.when.clone()
        };
        let expr = compile_when(&runtime_when)?.unwrap_or_else(|| "true".to_string());
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
    fn houdini_upper_bound_detection() {
        // Lower-only forms — no upper bound.
        assert!(!houdini_req_has_upper_bound(">=20.5"));
        assert!(!houdini_req_has_upper_bound(">21"));
        assert!(!houdini_req_has_upper_bound(">=20.5, >=21"));
        // Everything else has an upper bound.
        assert!(houdini_req_has_upper_bound("^21"));
        assert!(houdini_req_has_upper_bound("~21.5"));
        assert!(houdini_req_has_upper_bound("21"));
        assert!(houdini_req_has_upper_bound("=21"));
        assert!(houdini_req_has_upper_bound("==21"));
        assert!(houdini_req_has_upper_bound("<22"));
        assert!(houdini_req_has_upper_bound("<=21.5"));
        // Mixed — any upper-bounding clause is enough.
        assert!(houdini_req_has_upper_bound(">=20.5, <22"));
        assert!(houdini_req_has_upper_bound(">=20.5, ^21"));
        // Empty / unparseable — no upper bound.
        assert!(!houdini_req_has_upper_bound(""));
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
            install_source: None,
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
        let lowered =
            lower_conditional(&variants, &[("$HPM_PACKAGE_ROOT", "/abs/pkg")], false).unwrap();
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
        let lowered = lower_conditional(&variants, &[], false).unwrap();
        assert_eq!(lowered.len(), 1);
        assert_eq!(lowered[0]["true"], "default");
    }

    #[test]
    fn install_source_dev_filters_out_for_registry_install() {
        let variants = vec![
            EnvValueVariant {
                when: WhenSelector {
                    install_source: Some("dev".to_string()),
                    ..Default::default()
                },
                set: "build/Release".to_string(),
            },
            EnvValueVariant {
                when: WhenSelector::default(),
                set: "dso".to_string(),
            },
        ];
        // Registry install: dev variant drops.
        let lowered = lower_conditional(&variants, &[], false).unwrap();
        assert_eq!(lowered.len(), 1);
        assert_eq!(lowered[0]["true"], "dso");
        // Dev install: both fire, dev first.
        let lowered = lower_conditional(&variants, &[], true).unwrap();
        assert_eq!(lowered.len(), 2);
        assert_eq!(lowered[0]["true"], "build/Release");
        assert_eq!(lowered[1]["true"], "dso");
    }

    #[test]
    fn install_source_strips_from_runtime_expression() {
        // A dev branch that also has a Houdini constraint should emit only
        // the Houdini constraint to the runtime expression — install_source
        // is hpm-side, not Houdini-side.
        let variants = vec![EnvValueVariant {
            when: WhenSelector {
                houdini: Some("^21".to_string()),
                install_source: Some("dev".to_string()),
                ..Default::default()
            },
            set: "x".to_string(),
        }];
        let lowered = lower_conditional(&variants, &[], true).unwrap();
        assert_eq!(lowered.len(), 1);
        let key = lowered[0].keys().next().unwrap();
        assert!(key.contains("houdini_version"));
        assert!(!key.contains("install_source"));
    }

    #[test]
    fn install_source_registry_filters_out_for_dev_install() {
        let variants = vec![EnvValueVariant {
            when: WhenSelector {
                install_source: Some("registry".to_string()),
                ..Default::default()
            },
            set: "dso".to_string(),
        }];
        assert_eq!(lower_conditional(&variants, &[], true).unwrap().len(), 0);
        assert_eq!(lower_conditional(&variants, &[], false).unwrap().len(), 1);
    }

    #[test]
    fn unknown_install_source_rejected() {
        let variants = vec![EnvValueVariant {
            when: WhenSelector {
                install_source: Some("ci".to_string()),
                ..Default::default()
            },
            set: "x".to_string(),
        }];
        assert!(lower_conditional(&variants, &[], true).is_err());
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
