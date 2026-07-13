//! Executable model of how Houdini's package system applies `env` entries,
//! encoding the semantics verified against a real Houdini (21.0.688 /
//! 21.0.729) with the `hconfig` harness in `houdini_conformance_tests.rs`:
//!
//! - Package files in a directory are processed in byte-wise ascending
//!   filename order; `env` entries within a file in array order.
//! - A variable whose FIRST definition uses a flat string value is
//!   non-mergeable: every later entry overwrites it wholesale, whatever
//!   its `method` says (`WARNING: var X overwritten with ...`). This is
//!   the failure mode behind hpm's pre-0.28 flat-string emission.
//! - A variable whose first definition uses a JSON list value is
//!   mergeable: `append` pushes elements at the end, `prepend` inserts
//!   each element at the front in element order (so a multi-element block
//!   comes out reversed), `replace` resets the variable to the entry's
//!   elements. The method of the first definition itself is irrelevant —
//!   it just defines the elements.
//! - A flat-string entry applied to a mergeable variable acts as a
//!   single-element list under its method.
//! - The only methods Houdini accepts are `prepend` / `append` /
//!   `replace`; anything else (notably `set`) draws
//!   `WARNING: Unsupported method value`.
//!
//! The model deliberately panics on anything hpm must never emit
//! (unknown methods, non-string / non-list values), so a property test
//! running emitted files through it fails loudly on format regressions.

use std::collections::HashMap;

/// Final state of one variable after all package files are applied.
#[derive(Debug, Clone, PartialEq)]
pub enum VarState {
    /// First-defined by a flat string: non-mergeable, holds the last
    /// value written.
    Flat(String),
    /// First-defined by a list: mergeable, holds the merged elements.
    List(Vec<String>),
}

impl VarState {
    pub fn elements(&self) -> Vec<String> {
        match self {
            VarState::Flat(s) => vec![s.clone()],
            VarState::List(v) => v.clone(),
        }
    }
}

/// Apply `(filename, package.json)` pairs in Houdini's processing order
/// (byte-wise ascending filename) and return the final variable states.
pub fn apply_package_files(files: &[(String, serde_json::Value)]) -> HashMap<String, VarState> {
    let mut ordered: Vec<&(String, serde_json::Value)> = files.iter().collect();
    ordered.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));

    let mut vars = HashMap::new();
    for (file, json) in ordered {
        let Some(env) = json.get("env") else { continue };
        if env.is_null() {
            // HoudiniPackage serializes an absent env as `"env": null`,
            // which Houdini tolerates.
            continue;
        }
        let entries = env
            .as_array()
            .unwrap_or_else(|| panic!("{file}: env must be an array"));
        for entry in entries {
            let map = entry
                .as_object()
                .unwrap_or_else(|| panic!("{file}: env entry must be an object"));
            for (key, value) in map {
                apply_entry(&mut vars, key, value, file);
            }
        }
    }
    vars
}

fn apply_entry(
    vars: &mut HashMap<String, VarState>,
    key: &str,
    value: &serde_json::Value,
    file: &str,
) {
    // A bare (method-less) value defines/overwrites the variable; hpm only
    // emits this shape for the per-package PKG_* variable, which is defined
    // exactly once, so the default-method subtleties never arise.
    let (method, payload) = match value {
        serde_json::Value::String(_) | serde_json::Value::Array(_) => ("replace", value),
        serde_json::Value::Object(obj) => {
            let method = obj
                .get("method")
                .and_then(|m| m.as_str())
                .unwrap_or("replace");
            let payload = obj
                .get("value")
                .unwrap_or_else(|| panic!("{file}: env entry for {key} has no value"));
            (method, payload)
        }
        other => panic!("{file}: unsupported env value shape for {key}: {other}"),
    };

    if !matches!(method, "prepend" | "append" | "replace") {
        panic!(
            "{file}: Houdini rejects method '{method}' for {key} \
             (WARNING: Unsupported method value)"
        );
    }

    // Flat strings and list values diverge; conditional-object arrays are
    // out of the model's scope — the tests that use it only generate
    // unconditional values.
    let elements: Vec<String> = match payload {
        serde_json::Value::String(s) => {
            return apply_flat(vars, key, method, s.clone());
        }
        serde_json::Value::Array(items) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .unwrap_or_else(|| {
                        panic!(
                            "{file}: non-string list element for {key} \
                             (conditionals are outside the model)"
                        )
                    })
                    .to_string()
            })
            .collect(),
        other => panic!("{file}: unsupported value payload for {key}: {other}"),
    };

    match vars.get_mut(key) {
        // First definition, or overwrite of a non-mergeable variable:
        // the entry's elements become the state, method irrelevant.
        // (Post-overwrite mergeability of a flat-first variable is
        // unverified in Houdini; hpm-emitted directories never have a
        // flat-first variable, so the branch only matters for the state's
        // shape, not for any assertion.)
        None | Some(VarState::Flat(_)) => {
            vars.insert(key.to_string(), VarState::List(elements));
        }
        Some(VarState::List(current)) => match method {
            "replace" => *current = elements,
            "append" => current.extend(elements),
            // Element-wise front insertion: a multi-element block reverses.
            "prepend" => {
                for element in elements {
                    current.insert(0, element);
                }
            }
            _ => unreachable!("method validated above"),
        },
    }
}

fn apply_flat(vars: &mut HashMap<String, VarState>, key: &str, method: &str, value: String) {
    match vars.get_mut(key) {
        // First definition with a flat string: the variable is
        // non-mergeable from here on.
        None | Some(VarState::Flat(_)) => {
            vars.insert(key.to_string(), VarState::Flat(value));
        }
        // Flat entry on a mergeable variable: single-element list.
        Some(VarState::List(current)) => match method {
            "replace" => *current = vec![value],
            "append" => current.push(value),
            "prepend" => current.insert(0, value),
            _ => unreachable!("method validated above"),
        },
    }
}
