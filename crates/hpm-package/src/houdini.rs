//! Houdini package.json types for integration.
//!
//! This module defines the output types for generating Houdini-compatible
//! `package.json` files from HPM manifests.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Houdini package.json structure for generation
///
/// This structure represents the format expected by Houdini's package system.
/// It's generated from an HPM manifest to enable seamless Houdini integration.
///
/// # Example Output
///
/// ```json
/// {
///   "hpath": ["$HPM_PACKAGE_ROOT"],
///   "env": [
///     {"PYTHONPATH": {"method": "prepend", "value": ["$HPM_PACKAGE_ROOT/python"]}}
///   ],
///   "enable": "houdini_version >= '20.5'"
/// }
/// ```
/// Absent fields are omitted from the JSON rather than serialized as
/// `null` — Houdini logs `WARNING: Unsupported value for requires` (etc.)
/// for explicit nulls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoudiniPackage {
    /// Houdini path entries (for HOUDINI_OTLSCAN_PATH, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hpath: Option<Vec<String>>,
    /// Environment variable definitions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<Vec<HashMap<String, HoudiniEnvValue>>>,
    /// Conditional enable expression
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable: Option<String>,
    /// Required packages
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires: Option<Vec<String>>,
    /// Recommended packages
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommends: Option<Vec<String>>,
}

/// Env-application method accepted by Houdini's package system.
///
/// These are the only method values Houdini accepts — anything else
/// (notably `set`) draws `WARNING: Unsupported method value`. Verified
/// against Houdini 21.0.688.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HoudiniMethod {
    Prepend,
    Append,
    Replace,
}

impl HoudiniMethod {
    /// The package.json string form (`"prepend"` / `"append"` / `"replace"`).
    pub fn as_str(&self) -> &'static str {
        match self {
            HoudiniMethod::Prepend => "prepend",
            HoudiniMethod::Append => "append",
            HoudiniMethod::Replace => "replace",
        }
    }
}

impl std::fmt::Display for HoudiniMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Environment variable value in Houdini package.json
///
/// Supports three formats:
/// - Simple: direct string value
/// - Detailed: method (prepend/append/replace) with a list value
/// - DetailedConditional: method plus an ordered list of `{ "<expr>": "<v>" }`
///   maps; every map whose expression matches contributes its value.
///
/// `Detailed` (`prepend` / `append`) values are always emitted as JSON
/// lists, never flat strings: Houdini only honors `method` on a custom
/// (non-registered) variable when the variable's first definition uses a
/// list value; with a flat string every later entry silently overwrites,
/// regardless of method.
///
/// hpm's `set` is the deliberate exception — it emits a bare `Simple`
/// string (see [`ManifestEnvEntry::lower`][crate::manifest::env]). Houdini
/// has no `set` method, and a list-form `replace` *appends* onto a
/// path-registered variable that Houdini seeded flat-first (OCIO,
/// PYTHONPATH, ...); a flat string overwrites it, which is what `set`
/// promises. Only a genuinely conditional `set` value falls back to
/// `Detailed`/`DetailedConditional` with [`HoudiniMethod::Replace`], since
/// the conditional-object array form cannot be a flat string.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HoudiniEnvValue {
    /// Simple string value (sets the variable directly)
    Simple(String),
    /// Detailed value with method specification
    Detailed {
        /// How to apply the value
        method: HoudiniMethod,
        /// The value elements to apply
        value: Vec<String>,
    },
    /// Detailed value where the value is a Houdini conditional-object array.
    /// Each map has a single entry `"<houdini-expression>": "<value>"`.
    /// Houdini applies *every* map whose expression matches, so hpm's
    /// lowering compiles the expressions to be mutually exclusive (each
    /// branch excludes all earlier branches' conditions) — at most one
    /// element fires, giving the manifest's first-match semantics.
    DetailedConditional {
        method: HoudiniMethod,
        value: Vec<HashMap<String, String>>,
    },
}

impl HoudiniEnvValue {
    /// Create a simple environment value.
    pub fn simple(value: impl Into<String>) -> Self {
        HoudiniEnvValue::Simple(value.into())
    }

    /// Create a prepend environment value.
    pub fn prepend(value: impl Into<String>) -> Self {
        HoudiniEnvValue::Detailed {
            method: HoudiniMethod::Prepend,
            value: vec![value.into()],
        }
    }

    /// Create a `replace` environment value. Used for the list form of a
    /// conditional `set`; flat/unconditional `set` emits [`Self::simple`]
    /// instead (see [`ManifestEnvEntry::lower`][crate::manifest::env]).
    pub fn replace(value: impl Into<String>) -> Self {
        HoudiniEnvValue::Detailed {
            method: HoudiniMethod::Replace,
            value: vec![value.into()],
        }
    }

    // No conditional() constructor on purpose: conditional-object arrays
    // must go through `lower_conditional`, which compiles the branch
    // expressions to be mutually exclusive. A hand-built array would
    // reintroduce Houdini's every-match behavior.
}

/// Houdini-native package.json for direct use by Houdini's package system.
///
/// Unlike `HoudiniPackage` (which uses `$HPM_PACKAGE_ROOT` for HPM runtime),
/// this uses `$HOUDINI_PACKAGE_PATH/{slug}` so the archive works directly
/// with Houdini's built-in package loading.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoudiniNativePackage {
    /// Package slug name
    pub name: String,
    /// Houdini package path
    pub hpath: String,
    /// Load this package only once
    pub load_package_once: bool,
    /// Show in Houdini's package browser
    pub show: bool,
    /// Conditional enable expression
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable: Option<String>,
    /// Environment variable definitions
    pub env: Vec<HashMap<String, HoudiniEnvValue>>,
    /// Hard requirements. Houdini **disables** this package outright when a
    /// listed package is missing, so the generator leaves this `None` and
    /// emits dependencies as [`recommends`](Self::recommends) instead — see
    /// [`generate_houdini_native_package`][crate::PackageManifest::generate_houdini_native_package].
    /// Retained so a hand-written `{slug}.json` using it round-trips.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires: Option<Vec<String>>,
    /// Soft requirements. Houdini warns (naming the missing package) but
    /// still loads this one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommends: Option<Vec<String>>,
    /// Package metadata
    pub hpackage: HpackageMetadata,
}

/// Metadata block within a Houdini native package.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HpackageMetadata {
    /// Package version string
    pub version: String,
}

/// Which consumer the bundled `{slug}.json` is generated for.
///
/// The file is inert on an hpm install — the extractor skips it, and nothing
/// reads it back — so the target only decides whether the emitted values are
/// additionally shaped to survive a third party's validator.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NativePackageTarget {
    /// Houdini's own package system, reached by unzipping the archive into a
    /// packages directory. Values are emitted as the manifest states them.
    #[default]
    Generic,
    /// SideFX's hpackage repository, which validates the json on upload and
    /// rejects what it cannot parse. Normalizes `hpackage.version` and the
    /// `enable` expression's version operands, and fails rather than emit a
    /// value whose meaning would change in the process.
    SideFxHpackage,
}

/// Trim a package version the way SideFX's hpackage `Version` does: drop
/// trailing zero segments while more than two remain.
///
/// `0.1.0` becomes `0.1`, which is also the version that appears in the
/// served archive's URL. `1.2.3` is already minimal and passes through.
///
/// hpackage versions are dot-separated numbers, so a pre-release or
/// build-metadata semver has no representation and is refused instead of
/// being mangled into one.
pub fn normalize_hpackage_version(version: &str) -> Result<String, String> {
    let mut parts: Vec<u64> = Vec::new();
    for segment in version.split('.') {
        let number = segment.parse::<u64>().map_err(|_| {
            format!(
                "package version `{version}` cannot be published to SideFX's hpackage \
                 repository: it accepts dot-separated numbers only, so a pre-release or \
                 build-metadata version has no form there"
            )
        })?;
        parts.push(number);
    }
    while parts.len() > 2 && parts.last() == Some(&0) {
        parts.pop();
    }
    Ok(parts
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join("."))
}

/// Check a bundled `{slug}.json` against the rules SideFX's hpackage
/// repository enforces on upload.
///
/// [`generate_houdini_native_package_for`] already emits a conforming file,
/// but `hpm pack` ships a hand-written `{slug}.json` verbatim when the
/// package directory holds one — so under `--sidefx` the descriptor that
/// actually reaches the archive is the one that has to pass, whichever way it
/// came. Without this, the path hpm generates is the checked one and the path
/// an author hand-writes is not, and the failure surfaces at upload time in
/// whatever tool publishes, with a message that does not name the cause.
///
/// This reads named fields out of `descriptor` rather than deserializing it
/// into [`HoudiniNativePackage`]. That type models the subset hpm generates,
/// while Houdini's descriptor format is larger — and expressing what hpm does
/// not model is the reason to hand-write one at all. Round-tripping through
/// hpm's own type would reject exactly those files.
///
/// Mirrors hpackage's server-side `_validate_package_json` and
/// `Version.__str__` (`core.py`). It is a mirror of a third party's
/// validator, so it can drift; a descriptor it passes is one hpackage's
/// schema check should also pass, and a failure past that point is about
/// reservation, permissions, or transport.
/// On success, returns `hpackage.version` as SideFX will store it — trailing
/// zero segments trimmed. The caller gets it from here rather than
/// re-deriving it, so there is one answer: a descriptor declaring `0.1.0` is
/// published at `0.1`, and the served archive's URL uses the trimmed form.
pub fn validate_hpackage_descriptor(
    descriptor: &serde_json::Value,
    filename: &str,
) -> Result<String, String> {
    let object = descriptor.as_object().ok_or_else(|| {
        format!(
            "{filename} is not a JSON object, so neither Houdini nor SideFX's hpackage \
             repository can read it as a package descriptor"
        )
    })?;

    validate_enable(object.get("enable"), filename)?;

    match object.get("hpackage").and_then(|h| h.get("version")) {
        Some(serde_json::Value::String(v)) => normalize_hpackage_version(v).map_err(|e| {
            format!("{filename} declares an `hpackage.version` hpackage cannot store: {e}")
        }),
        Some(other) => Err(format!(
            "{filename} declares `hpackage.version` as {other}, but hpackage reads it as a \
             string of dot-separated numbers"
        )),
        None => Err(format!(
            "{filename} declares no `hpackage.version`, and SideFX's hpackage repository \
             refuses an upload without one. Add `\"hpackage\": {{ \"version\": \"…\" }}`, or \
             delete the file and let `hpm pack` generate the descriptor from hpm.toml."
        )),
    }
}

/// Assert `enable` is the one shape both Houdini and hpackage honour.
///
/// Returns the two-segment Houdini version the clause declares — the one
/// value hpackage reads back out of the expression. Nothing consumes it yet;
/// producing it *is* the check, since a clause hpackage cannot read a version
/// out of is one it refuses on upload.
///
/// The object form is refused however few keys it has. hpackage's own docs
/// suggest one key per clause, and Houdini does not read it as a conjunction:
/// an object `enable` is an expression-to-boolean map for inverting a
/// condition, so a key that matches loads the package regardless of the
/// others, and an object where none match draws `Unsupported value for
/// enable` and loads it anyway. Measured with hconfig on 22.0.368, for one
/// key and for two. There is no shape that satisfies both, which is why this
/// refuses rather than repairs.
fn validate_enable(enable: Option<&serde_json::Value>, filename: &str) -> Result<String, String> {
    let enable = enable.ok_or_else(|| no_readable_bound(filename, "declares no `enable`"))?;
    let text = match enable {
        serde_json::Value::String(text) => text.trim(),
        serde_json::Value::Object(_) => {
            return Err(format!(
                "{filename} declares `enable` as an object. hpackage's docs suggest that form for \
                 a multi-clause range, but Houdini reads an object `enable` as a conditional map \
                 rather than a conjunction: a key that matches loads the package whatever the \
                 others say, and an object where none match is not a gate at all. Declare a \
                 single lower bound as a string — `\"enable\": \"houdini_version >= '21.0'\"`."
            ));
        }
        other => {
            return Err(no_readable_bound(
                filename,
                &format!("declares `enable` as {other}"),
            ));
        }
    };

    // hpackage matches the whole `enable` value against one anchored clause
    // rather than searching it, so the conjunction Houdini honours is the one
    // form hpackage cannot read.
    if text.contains(" and ") {
        return Err(format!(
            "{filename} declares `enable` as {text:?}, which SideFX's hpackage repository cannot \
             represent: it matches the whole value against a single \
             `houdini_version >= 'major.minor'` clause. There is no way to keep the upper bound \
             — the multi-clause string Houdini honours is the one hpackage rejects, and the \
             per-clause object form hpackage documents does not gate in Houdini at all. Declare \
             a single lower bound to publish here, or host the archive elsewhere and keep the \
             bounded range."
        ));
    }

    let operand = parse_enable_clause(text)
        .ok_or_else(|| no_readable_bound(filename, &format!("declares `enable` as {text:?}")))?;
    two_segment_houdini_version(operand).ok_or_else(|| {
        format!(
            "{filename} declares `enable` as {text:?}, and hpackage cannot read `{operand}` as a \
             Houdini version: it takes exactly `major.minor` after trimming trailing zeros, so a \
             one-segment bound (`21`) and a bound carrying a real build (`20.5.445`) both leave \
             it with no version. Note that widening `20.5.445` to `20.5` would enable the package \
             on builds the bound excludes, so it is not a rewrite hpm will make for you."
        )
    })
}

/// The error for a descriptor that leaves hpackage no Houdini version to
/// read. It takes one from `>=` and `<=` clauses only, and refuses the upload
/// with a message that does not name the cause — so this names it instead.
fn no_readable_bound(filename: &str, what: &str) -> String {
    format!(
        "{filename} {what}, leaving SideFX's hpackage repository no Houdini version to read. It \
         takes one from a single `houdini_version >= 'major.minor'` (or `<=`) clause and refuses \
         an upload without it. Declare one, or delete the file and let `hpm pack` generate the \
         descriptor from `[compat].houdini`."
    )
}

/// Pull the version operand out of an anchored `houdini_version >= '…'`
/// clause, the form hpackage regex-matches. Whitespace around the operator is
/// tolerated; hpm's own generator emits the single-spaced spelling.
fn parse_enable_clause(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("houdini_version")?.trim_start();
    let rest = rest
        .strip_prefix(">=")
        .or_else(|| rest.strip_prefix("<="))?
        .trim_start();
    let rest = rest.strip_prefix('\'')?;
    let end = rest.find('\'')?;
    // Anchored: nothing may follow the closing quote.
    if !rest[end + 1..].trim().is_empty() {
        return None;
    }
    Some(&rest[..end])
}

/// A Houdini version operand as hpackage reads it: dot-separated numbers
/// which, after its own trailing-zero trimming, leave exactly two parts.
fn two_segment_houdini_version(operand: &str) -> Option<String> {
    let mut parts: Vec<u64> = Vec::new();
    for segment in operand.split('.') {
        parts.push(segment.parse::<u64>().ok()?);
    }
    while parts.len() > 2 && parts.last() == Some(&0) {
        parts.pop();
    }
    if parts.len() != 2 {
        return None;
    }
    Some(format!("{}.{}", parts[0], parts[1]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(enable: serde_json::Value, version: &str) -> serde_json::Value {
        serde_json::json!({
            "name": "demo",
            "hpath": "$HOUDINI_PACKAGE_PATH/demo",
            "enable": enable,
            "hpackage": { "version": version },
        })
    }

    /// The shape hpm's own `--sidefx` generator emits, and the only one
    /// hpackage reads: a single lower bound with a two-segment operand.
    #[test]
    fn validate_descriptor_accepts_a_single_two_segment_lower_bound() {
        let stored = validate_hpackage_descriptor(
            &descriptor(serde_json::json!("houdini_version >= '21.0'"), "0.1"),
            "demo.json",
        )
        .unwrap();
        assert_eq!(stored, "0.1");
    }

    /// hpackage trims trailing zeros before reading, so a three-segment
    /// operand whose build is zero means the same range and is accepted. The
    /// same trim applies to the package version, and the trimmed value is
    /// what the served archive's URL is built from.
    #[test]
    fn validate_descriptor_reports_the_values_hpackage_will_store() {
        // The three-segment operand trims back to the same range and is
        // accepted; the version is reported in the form SideFX stores.
        let stored = validate_hpackage_descriptor(
            &descriptor(serde_json::json!("houdini_version >= '21.0.0'"), "0.1.0"),
            "demo.json",
        )
        .unwrap();
        assert_eq!(stored, "0.1");
    }

    /// A conjunction is the form Houdini honours and the one hpackage
    /// rejects, so a hand-written bounded range has to fail at pack time.
    #[test]
    fn validate_descriptor_refuses_a_multi_clause_enable() {
        let err = validate_hpackage_descriptor(
            &descriptor(
                serde_json::json!("(houdini_version >= '21.0' and houdini_version < '22')"),
                "0.1",
            ),
            "demo.json",
        )
        .unwrap_err();
        assert!(err.contains("single"), "{err}");
    }

    /// The object form hpackage's docs suggest is not a narrower gate but no
    /// gate at all in Houdini — including with a single key, which is why
    /// there is no safe subset to carve out here.
    #[test]
    fn validate_descriptor_refuses_an_object_enable_however_few_keys() {
        for enable in [
            serde_json::json!({ "houdini_version >= '21.0'": true }),
            serde_json::json!({
                "houdini_version >= '21.0'": true,
                "houdini_version < '22'": true,
            }),
        ] {
            let err =
                validate_hpackage_descriptor(&descriptor(enable, "0.1"), "demo.json").unwrap_err();
            assert!(err.contains("conditional map"), "{err}");
        }
    }

    /// hpackage reads the package's Houdini version out of the `enable`
    /// clause, so an operand it cannot parse as `major.minor` leaves it with
    /// no version — and its own refusal does not say so.
    #[test]
    fn validate_descriptor_refuses_an_operand_hpackage_cannot_read() {
        for operand in ["21", "20.5.445", "21.x", ""] {
            let enable = serde_json::json!(format!("houdini_version >= '{operand}'"));
            assert!(
                validate_hpackage_descriptor(&descriptor(enable, "0.1"), "demo.json").is_err(),
                "operand {operand:?} should be refused"
            );
        }
    }

    /// `>` and `==` compile to expressions Houdini evaluates but hpackage's
    /// regex does not match, so they read there as no declared version.
    #[test]
    fn validate_descriptor_refuses_a_clause_hpackage_does_not_match() {
        for enable in [
            "houdini_version > '21.0'",
            "houdini_version == '21.0'",
            "houdini_version >= '21.0' or houdini_version < '20'",
            "",
        ] {
            let err = validate_hpackage_descriptor(
                &descriptor(serde_json::json!(enable), "0.1"),
                "demo.json",
            )
            .unwrap_err();
            assert!(
                err.contains("no Houdini version to read"),
                "{enable:?}: {err}"
            );
        }
    }

    #[test]
    fn validate_descriptor_requires_an_enable_and_a_version() {
        let no_enable = serde_json::json!({ "hpackage": { "version": "0.1" } });
        assert!(validate_hpackage_descriptor(&no_enable, "demo.json").is_err());

        let no_version = serde_json::json!({ "enable": "houdini_version >= '21.0'" });
        let err = validate_hpackage_descriptor(&no_version, "demo.json").unwrap_err();
        assert!(err.contains("hpackage.version"), "{err}");

        // A pre-release version has no dot-separated-number form there.
        let prerelease = descriptor(serde_json::json!("houdini_version >= '21.0'"), "1.0.0-rc.1");
        assert!(validate_hpackage_descriptor(&prerelease, "demo.json").is_err());
    }

    /// The check reads named fields rather than deserializing into
    /// `HoudiniNativePackage`: a hand-written descriptor exists to express
    /// what hpm does not model, so keys hpm has no field for must pass
    /// through untouched.
    #[test]
    fn validate_descriptor_ignores_keys_hpm_does_not_model() {
        let mut descriptor = descriptor(serde_json::json!("houdini_version >= '21.0'"), "0.1");
        descriptor["path"] = serde_json::json!(["$HOUDINI_PACKAGE_PATH/demo/extra"]);
        descriptor["package_platform"] = serde_json::json!("linux");
        assert!(validate_hpackage_descriptor(&descriptor, "demo.json").is_ok());
    }

    #[test]
    fn validate_descriptor_refuses_a_non_object_document() {
        let err =
            validate_hpackage_descriptor(&serde_json::json!([1, 2]), "demo.json").unwrap_err();
        assert!(err.contains("not a JSON object"), "{err}");
    }

    #[test]
    fn houdini_env_value_constructors() {
        let simple = HoudiniEnvValue::simple("value");
        let prepend = HoudiniEnvValue::prepend("value");
        let replace = HoudiniEnvValue::replace("value");

        match simple {
            HoudiniEnvValue::Simple(v) => assert_eq!(v, "value"),
            _ => panic!("Expected Simple variant"),
        }

        match prepend {
            HoudiniEnvValue::Detailed { method, value } => {
                assert_eq!(method, HoudiniMethod::Prepend);
                assert_eq!(value, vec!["value"]);
            }
            _ => panic!("Expected Detailed variant"),
        }

        match replace {
            HoudiniEnvValue::Detailed { method, value } => {
                assert_eq!(method, HoudiniMethod::Replace);
                assert_eq!(value, vec!["value"]);
            }
            _ => panic!("Expected Detailed variant"),
        }
    }

    #[test]
    fn detailed_values_serialize_as_lists() {
        // Regression: a flat-string value marks a custom variable
        // non-mergeable in Houdini, so every Detailed value must hit the
        // package.json as a JSON array.
        let append = HoudiniEnvValue::Detailed {
            method: HoudiniMethod::Append,
            value: vec!["v".to_string()],
        };
        let json = serde_json::to_string(&append).unwrap();
        assert_eq!(json, r#"{"method":"append","value":["v"]}"#);
    }

    #[test]
    fn houdini_package_serialization() {
        let pkg = HoudiniPackage {
            hpath: Some(vec!["$HPM_PACKAGE_ROOT".to_string()]),
            env: Some(vec![{
                let mut m = HashMap::new();
                m.insert(
                    "PYTHONPATH".to_string(),
                    HoudiniEnvValue::prepend("$HPM_PACKAGE_ROOT/python"),
                );
                m
            }]),
            enable: None,
            requires: None,
            recommends: None,
        };

        let json = serde_json::to_string_pretty(&pkg).unwrap();
        assert!(json.contains("hpath"));
        assert!(json.contains("PYTHONPATH"));
        assert!(json.contains("prepend"));
    }

    #[test]
    fn houdini_native_package_serialization() {
        let pkg = HoudiniNativePackage {
            name: "my-tool".to_string(),
            hpath: "$HOUDINI_PACKAGE_PATH/my-tool".to_string(),
            load_package_once: true,
            show: true,
            enable: Some("houdini_version >= '21.0'".to_string()),
            env: vec![{
                let mut m = HashMap::new();
                m.insert(
                    "PKG_MY_TOOL".to_string(),
                    HoudiniEnvValue::simple("$HOUDINI_PACKAGE_PATH/my-tool"),
                );
                m
            }],
            requires: None,
            recommends: Some(vec!["some-dep".to_string()]),
            hpackage: HpackageMetadata {
                version: "1.2.3".to_string(),
            },
        };

        let json = serde_json::to_string_pretty(&pkg).unwrap();
        assert!(json.contains("\"name\": \"my-tool\""));
        assert!(json.contains("\"load_package_once\": true"));
        assert!(json.contains("\"show\": true"));
        assert!(json.contains("\"hpath\": \"$HOUDINI_PACKAGE_PATH/my-tool\""));
        assert!(json.contains("\"version\": \"1.2.3\""));
        assert!(json.contains("\"some-dep\""));
    }

    #[test]
    fn houdini_native_package_omits_none_fields() {
        let pkg = HoudiniNativePackage {
            name: "test".to_string(),
            hpath: "$HOUDINI_PACKAGE_PATH/test".to_string(),
            load_package_once: true,
            show: true,
            enable: None,
            env: vec![],
            requires: None,
            recommends: None,
            hpackage: HpackageMetadata {
                version: "1.0.0".to_string(),
            },
        };

        let json = serde_json::to_string_pretty(&pkg).unwrap();
        assert!(!json.contains("enable"));
        assert!(!json.contains("requires"));
        assert!(!json.contains("recommends"));
    }
}
