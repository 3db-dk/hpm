//! `[runtime]` entries: env-var contributions baked into the generated
//! Houdini `package.json`.

use crate::env_value::{EnvValue, ExpressionError, LoweredConditional, lower_conditional};
use crate::houdini::{HoudiniEnvValue, HoudiniMethod};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Method for applying an environment variable value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EnvMethod {
    Set,
    Prepend,
    Append,
}

impl EnvMethod {
    /// Manifest string form (`"set"`, `"prepend"`, `"append"`), for error
    /// messages that quote `hpm.toml` back at the author.
    pub fn as_str(&self) -> &'static str {
        match self {
            EnvMethod::Set => "set",
            EnvMethod::Prepend => "prepend",
            EnvMethod::Append => "append",
        }
    }

    /// Method emitted into Houdini's package.json for the *list* value
    /// forms. Houdini accepts only `prepend` / `append` / `replace` — there
    /// is no `set` (it warns `Unsupported method value: set`), so when `set`
    /// does reach a list form (a genuinely conditional value) it maps to
    /// [`HoudiniMethod::Replace`]. Note that flat/unconditional `set` never
    /// reaches this method at all: [`ManifestEnvEntry::lower`] emits it as a
    /// bare [`HoudiniEnvValue::Simple`] string so a path-registered variable
    /// (OCIO, PYTHONPATH, ...) is overwritten rather than appended-onto.
    /// Verified against Houdini 21.0.688.
    pub fn houdini_method(&self) -> HoudiniMethod {
        match self {
            EnvMethod::Set => HoudiniMethod::Replace,
            EnvMethod::Prepend => HoudiniMethod::Prepend,
            EnvMethod::Append => HoudiniMethod::Append,
        }
    }
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
/// variants — see [`EnvValue`]. Conditional variants may gate on
/// `install_source = "dev"` / `"registry"` (filtered by hpm at install
/// time) or on `houdini` / `os` / `python` (compiled into Houdini's
/// expression form per <https://www.sidefx.com/docs/houdini/ref/plugins.html>).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestEnvEntry {
    pub method: EnvMethod,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<EnvValue>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub required: bool,
}

pub(super) fn is_false(b: &bool) -> bool {
    !*b
}

impl ManifestEnvEntry {
    /// Convert to a Houdini environment variable value for a published
    /// (non-dev) consumer.
    ///
    /// Returns `Ok(None)` for required-but-unsupplied placeholders, and
    /// for conditional values whose every branch is gated to a
    /// non-matching `install_source`. Returns `Err` for malformed
    /// `when` selectors — the prior implementation silently dropped
    /// these via `.ok().flatten()`, masking authoring mistakes that
    /// validate would otherwise catch.
    ///
    /// No substitution is applied — the returned value reflects the
    /// manifest verbatim, so `$HPM_PACKAGE_ROOT` is preserved. Use
    /// [`Self::lower`] when you have a concrete package path and install
    /// context to substitute in.
    pub fn to_houdini_env_value(&self) -> Result<Option<HoudiniEnvValue>, ExpressionError> {
        self.lower(&[], false)
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
        let method = self.method.houdini_method();
        // `set` promises "this is THE value", which for a path-registered
        // variable (OCIO, PYTHONPATH, HOUDINI_*_PATH — anything Houdini
        // treats as a merge-able path list) only a flat string delivers.
        // Houdini seeds those variables flat-first from its own
        // `$HFS/packages/*.json`, then treats them as always-mergeable, so
        // a list-form `replace` *appends* onto the seed
        // (`builtin;project`) instead of replacing it — the OCIO crash on
        // H22.0.367. A later flat string overwrites the seed cleanly,
        // load-order-independent, which is exactly what `set` means.
        // Verified against real Houdini (H21.0.729 / H22.0.367).
        //
        // `prepend` / `append` still emit single-element lists: those
        // methods only merge when the variable's first definition is a
        // list, and for a *custom* var a flat first definition would make
        // it non-mergeable so every later entry silently overwrites it.
        // The trade-off is deliberate — see [`EnvMethod::houdini_method`].
        let is_set = self.method == EnvMethod::Set;
        let lowered = match value {
            EnvValue::Flat(s) => {
                let mut out = s.clone();
                for (from, to) in substitutions {
                    out = out.replace(from, to);
                }
                if is_set {
                    HoudiniEnvValue::Simple(out)
                } else {
                    HoudiniEnvValue::Detailed {
                        method,
                        value: vec![out],
                    }
                }
            }
            EnvValue::Conditional(variants) => {
                match lower_conditional(variants, substitutions, is_dev)? {
                    // First surviving branch is unconditional — the value
                    // is effectively flat in this install context, so it
                    // follows the flat-value rules above: `set` emits a
                    // bare string, other methods a single-element list.
                    LoweredConditional::Unconditional(value) if is_set => {
                        HoudiniEnvValue::Simple(value)
                    }
                    LoweredConditional::Unconditional(value) => HoudiniEnvValue::Detailed {
                        method,
                        value: vec![value],
                    },
                    // Genuinely conditional `set` (a `{ when, set }` list
                    // with more than the fallback surviving) can't collapse
                    // to a flat string — Houdini's conditional-object array
                    // form requires the `{ method, value: [...] }` shape.
                    // So conditional `set` stays list-form `replace` and
                    // does NOT get the path-registered-var overwrite fix; a
                    // version/OS-gated OCIO would still append onto the
                    // seed. Left as a known, narrow gap: gate the *package*
                    // on the condition instead, or set OCIO flat.
                    LoweredConditional::Branches(branches) => {
                        if branches.is_empty() {
                            // Every branch filtered out by install_source —
                            // treat the entry as inert in this install
                            // context.
                            return Ok(None);
                        }
                        HoudiniEnvValue::DetailedConditional {
                            method,
                            value: branches,
                        }
                    }
                }
            }
        };
        Ok(Some(lowered))
    }
}

/// Validate a `[runtime]`-shaped table. The `section` label is used
/// verbatim in error messages so the source is obvious to authors.
pub(super) fn validate_env_table(
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
            Some(EnvValue::Flat(_)) => {}
            Some(EnvValue::Conditional(variants)) => {
                if variants.is_empty() {
                    return Err(format!(
                        "{section} var '{key}' has an empty conditional value list"
                    ));
                }
                for variant in variants {
                    crate::env_value::compile_condition(&variant.when)
                        .map_err(|e| format!("{section} var '{key}': {e}"))?;
                }
            }
        }
    }
    Ok(())
}
