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

/// The value of a `[runtime.<KEY>]` entry (and of a script's `cmd`) in
/// `hpm.toml`.
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
    op: Op,
    rhs: String,
}

impl ConditionAtom {
    fn new(lhs: &'static str, op: Op, rhs: impl Into<String>) -> Self {
        Self {
            lhs,
            op,
            rhs: rhs.into(),
        }
    }

    fn compile(&self) -> String {
        format!("{} {} '{}'", self.lhs, self.op.as_str(), self.rhs)
    }

    fn compile_negated(&self) -> String {
        format!("{} {} '{}'", self.lhs, self.op.negated(), self.rhs)
    }
}

/// A comparison operator of the expression grammar. Every range form
/// reduces to these: `=` is `==`, and caret and tilde are a `>=` and a `<`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Ge,
    Le,
    Gt,
    Lt,
    Eq,
}

impl Op {
    fn as_str(self) -> &'static str {
        match self {
            Op::Ge => ">=",
            Op::Le => "<=",
            Op::Gt => ">",
            Op::Lt => "<",
            Op::Eq => "==",
        }
    }

    /// The operator of the negated comparison. Houdini's grammar has no
    /// `not`, so negation flips each comparison (`==` becomes `!=`).
    fn negated(self) -> &'static str {
        match self {
            Op::Ge => "<",
            Op::Le => ">",
            Op::Gt => "<=",
            Op::Lt => ">=",
            Op::Eq => "!=",
        }
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
        atoms.extend(range.atoms());
    }
    if let Some(os) = &selector.os {
        atoms.push(ConditionAtom::new("houdini_os", Op::Eq, os.as_str()));
    }
    if let Some(py) = &selector.python {
        atoms.push(ConditionAtom::new(
            "houdini_python",
            Op::Eq,
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
/// Parsed once into comparators, from which everything else derives: the
/// `enable` expression Houdini evaluates, its SideFX-hpackage form, the
/// lower bound, whether it bounds above, and whether a concrete Houdini
/// satisfies it. The authored text is kept for serialization and messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoudiniRange {
    text: String,
    /// One entry per comma-separated clause, each the comparators that
    /// clause stands for (caret and tilde stand for two).
    clauses: Vec<Vec<Comparator>>,
}

/// A version operand: one to three numeric components, with the text it
/// is written as. Houdini compares numerically and only over the operand's
/// components (measured with hconfig on 22.0.368: 22.0.368 equals `'22'`
/// and `'22.00'`, and is less than `'22.1'`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct SimpleVersion {
    text: String,
    parts: Vec<u64>,
}

impl SimpleVersion {
    fn parse(text: &str) -> Option<Self> {
        let parts = text
            .split('.')
            .map(|seg| seg.parse::<u64>().ok())
            .collect::<Option<Vec<_>>>()?;
        (1..=3).contains(&parts.len()).then(|| Self {
            text: text.to_string(),
            parts,
        })
    }

    /// The version with these components, written canonically.
    fn from_parts(parts: Vec<u64>) -> Self {
        let text = parts
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(".");
        Self { text, parts }
    }

    /// Compare `version` with this operand over the operand's components.
    fn compare(&self, version: [u64; 3]) -> std::cmp::Ordering {
        version[..self.parts.len()].cmp(&self.parts)
    }

    /// This operand as the `major.minor` SideFX's hpackage repository
    /// reads: a major alone is padded (`21` → `21.0`), and a build segment is
    /// dropped only when it is zero, since `21.0.0` trims back to `21.0`. A
    /// non-zero build names a point inside the line, so dropping it would
    /// widen the range for hpm installs too (the same `enable` ships there).
    fn hpackage(&self, req: &str) -> Result<Self, ExpressionError> {
        match self.parts.as_slice() {
            [major] => Ok(Self::from_parts(vec![*major, 0])),
            [major, minor] => Ok(Self::from_parts(vec![*major, *minor])),
            [major, minor, 0] => Ok(Self::from_parts(vec![*major, *minor])),
            _ => Err(ExpressionError::HpackageOperand {
                req: req.to_string(),
                operand: self.text.clone(),
            }),
        }
    }
}

/// One comparison of `houdini_version` against an operand.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Comparator {
    op: Op,
    version: SimpleVersion,
}

impl Comparator {
    fn atom(&self) -> ConditionAtom {
        ConditionAtom::new("houdini_version", self.op, self.version.text.clone())
    }

    fn matches(&self, version: [u64; 3]) -> bool {
        use std::cmp::Ordering::*;
        let o = self.version.compare(version);
        match self.op {
            Op::Ge => o != Less,
            Op::Le => o != Greater,
            Op::Gt => o == Greater,
            Op::Lt => o == Less,
            Op::Eq => o == Equal,
        }
    }
}

/// The comparators one comma-separated clause stands for, or `None` when it
/// is not part of the grammar.
fn parse_clause(part: &str) -> Option<Vec<Comparator>> {
    // Two-character operators before their one-character prefixes, and `=`
    // (an alias of `==`) after both.
    for (prefix, op) in [
        (">=", Op::Ge),
        ("<=", Op::Le),
        ("==", Op::Eq),
        (">", Op::Gt),
        ("<", Op::Lt),
        ("=", Op::Eq),
    ] {
        if let Some(rest) = part.strip_prefix(prefix) {
            let version = SimpleVersion::parse(rest.trim())?;
            return Some(vec![Comparator { op, version }]);
        }
    }
    let (tilde, rest) = match (part.strip_prefix('~'), part.strip_prefix('^')) {
        (Some(rest), _) => (true, rest),
        (None, Some(rest)) => (false, rest),
        // A bare version is shorthand for caret.
        (None, None) => (false, part),
    };
    let lower = SimpleVersion::parse(rest.trim())?;
    let upper = if tilde {
        // Below the next minor, or the next major when only a major is given.
        match lower.parts.as_slice() {
            [major] => vec![major + 1],
            [major, minor, ..] => vec![*major, minor + 1],
            _ => return None,
        }
    } else {
        // Below the next change of the leftmost non-zero component.
        match lower.parts.as_slice() {
            [0, 0, patch] => vec![0, 0, patch + 1],
            [0, minor, ..] => vec![0, minor + 1],
            [major, ..] => vec![major + 1],
            _ => return None,
        }
    };
    // The lower operand is written canonically, as the emitted expression
    // always has.
    let lower = SimpleVersion::from_parts(lower.parts);
    Some(vec![
        Comparator {
            op: Op::Ge,
            version: lower,
        },
        Comparator {
            op: Op::Lt,
            version: SimpleVersion::from_parts(upper),
        },
    ])
}

/// Join compiled clauses with `and`, parenthesised when there are several
/// so the whole composes with other axes.
fn join_clauses(clauses: Vec<String>) -> String {
    if clauses.len() == 1 {
        clauses.into_iter().next().expect("one clause")
    } else {
        format!("({})", clauses.join(" and "))
    }
}

impl HoudiniRange {
    /// Parse and validate a houdini range string.
    pub fn parse(req: impl Into<String>) -> Result<Self, ExpressionError> {
        let text = req.into();
        let invalid = || ExpressionError::InvalidHoudiniReq(text.clone());
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(invalid());
        }
        let clauses = trimmed
            .split(',')
            .map(|part| {
                let part = part.trim();
                if part.is_empty() {
                    None
                } else {
                    parse_clause(part)
                }
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(invalid)?;
        Ok(Self { text, clauses })
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    fn comparators(&self) -> impl Iterator<Item = &Comparator> {
        self.clauses.iter().flatten()
    }

    /// The range as a flat conjunction of atoms, for a conditional branch.
    fn atoms(&self) -> Vec<ConditionAtom> {
        self.comparators().map(Comparator::atom).collect()
    }

    /// Compile to the Houdini package.json `enable` expression.
    pub fn to_enable_expression(&self) -> String {
        join_clauses(
            self.clauses
                .iter()
                .map(|clause| {
                    clause
                        .iter()
                        .map(|c| c.atom().compile())
                        .collect::<Vec<_>>()
                        .join(" and ")
                })
                .collect(),
        )
    }

    /// The `enable` expression in the form SideFX's hpackage repository
    /// accepts, or an error naming the bound that has no representation
    /// there.
    ///
    /// Every operand is rewritten to `major.minor` (see
    /// [`SimpleVersion::hpackage`]), plus two checks. The range must compile
    /// to exactly one comparison, because hpackage matches the whole
    /// `enable` value rather than searching it — so a bounded range is
    /// refused here rather than at upload. And that comparison must be `>=`
    /// or `<=`, since a range compiling only to `<`, `>` or `==` reads to
    /// hpackage as a package declaring no Houdini version at all.
    ///
    /// Splitting a bounded range into one object key per clause is what the
    /// shape of hpackage's validator invites, and it is a trap: Houdini
    /// reads an object `enable` as a conditional map, not a conjunction, and
    /// an object whose keys do not all match is not gated at all. That holds
    /// for a single-key object too, so there is no safe subset.
    pub fn to_enable_expression_hpackage(&self) -> Result<String, ExpressionError> {
        let normalized = self
            .comparators()
            .map(|c| {
                Ok(Comparator {
                    op: c.op,
                    version: c.version.hpackage(&self.text)?,
                })
            })
            .collect::<Result<Vec<_>, ExpressionError>>()?;
        if normalized.len() > 1 {
            return Err(ExpressionError::HpackageMultipleClauses {
                req: self.text.clone(),
                count: normalized.len(),
            });
        }
        let only = &normalized[0];
        if !matches!(only.op, Op::Ge | Op::Le) {
            return Err(ExpressionError::HpackageNoBound(self.text.clone()));
        }
        Ok(only.atom().compile())
    }

    /// The lower bound: the operand of the first comparison that implies
    /// one (`>=`, `>`, `==`, and the lower half of caret, tilde and a bare
    /// version). `"<22"` has none; `"<22, >=20.5"` has `20.5`. Python ABI
    /// selection reads it.
    pub fn lower_bound(&self) -> Option<String> {
        self.comparators()
            .find(|c| matches!(c.op, Op::Ge | Op::Gt | Op::Eq))
            .map(|c| c.version.text.clone())
    }

    /// Whether the range bounds above (some comparison is `<`, `<=` or
    /// `==`, including the upper half of caret, tilde and a bare version).
    /// `hpm check` flags native packages whose range does not: a DSO built
    /// against one Houdini major typically won't load in the next.
    pub fn has_upper_bound(&self) -> bool {
        self.comparators()
            .any(|c| matches!(c.op, Op::Lt | Op::Le | Op::Eq))
    }

    /// Whether a concrete Houdini `major.minor` (and `build`, when known)
    /// satisfies this range, evaluated the way Houdini evaluates the emitted
    /// `enable` expression.
    ///
    /// Used to check a package against a target Houdini *before* writing a
    /// manifest for it: a package whose `enable` does not match fails
    /// silently in Houdini, loading nothing and reporting nothing.
    ///
    /// Pass `build: None` when only `major.minor` is known: the range is then
    /// evaluated against the *highest* build in that line, because a range
    /// like `">=20.5.445"` describes a point inside the 20.5 line, and
    /// testing build 0 against it would reject every build of a line that
    /// mostly satisfies it.
    pub fn matches_version(&self, major: u32, minor: u32, build: Option<u32>) -> bool {
        let version = [
            u64::from(major),
            u64::from(minor),
            build.map_or(u64::MAX, u64::from),
        ];
        self.comparators().all(|c| c.matches(version))
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
        f.write_str(&self.text)
    }
}

impl Serialize for HoudiniRange {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&self.text)
    }
}

impl<'de> Deserialize<'de> for HoudiniRange {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(de)?;
        Self::parse(raw).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExpressionError {
    #[error("invalid houdini version requirement: {0}")]
    InvalidHoudiniReq(String),
    #[error("invalid python identifier: {0}")]
    InvalidPython(String),
    #[error(
        "houdini version requirement `{req}` cannot be published to SideFX's hpackage \
         repository: the bound `{operand}` carries a non-zero build segment, and hpackage \
         reads only `major.minor`. Dropping the build would silently widen the range, so \
         write a two-segment bound in `[compat].houdini` if that is what you mean."
    )]
    HpackageOperand { req: String, operand: String },
    #[error(
        "houdini version requirement `{0}` leaves SideFX's hpackage repository no bound it \
         can read: it takes a package's Houdini version from `>=` and `<=` clauses only, and \
         this range compiles to neither. Express the lower bound with `>=`."
    )]
    HpackageNoBound(String),
    #[error(
        "houdini version requirement `{req}` compiles to {count} clauses, and SideFX's \
         hpackage repository accepts an `enable` of exactly one — it matches the whole value \
         against a single `houdini_version >= 'major.minor'` clause. There is no way to keep \
         the upper bound: the multi-clause string Houdini honours is the one hpackage cannot \
         read, and the per-clause object form hpackage's own docs suggest is not a narrower \
         gate but no gate at all (Houdini treats it as a conditional map, loading the package \
         when a key matches and warning `Unsupported value for enable` when none does — \
         measured on 21.0.729 and 22.0.368). Declare a single lower bound such as `>=21.0` to \
         publish there, or host the archive elsewhere and keep the bounded range."
    )]
    HpackageMultipleClauses { req: String, count: usize },
}

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

/// Lower a `Conditional` env value into Houdini's `[{ "<expr>": "<val>" }, …]`
/// shape, applying the supplied substitutions to each branch's `set` string.
///
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
/// instead of applying `v`), so unconditional branches are handled structurally: a
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
