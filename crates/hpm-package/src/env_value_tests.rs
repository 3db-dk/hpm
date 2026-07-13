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

/// `=` is documented as an exact-version alias of `==`; it used to fall
/// through to the bare-version (caret) parser and error out.
#[test]
fn single_equals_aliases_exact_version() {
    assert_eq!(
        compile_houdini_req("=21").unwrap(),
        "houdini_version == '21'"
    );
    assert_eq!(
        compile_houdini_req("==21").unwrap(),
        "houdini_version == '21'"
    );
}

#[test]
fn os_translates_to_houdini_os() {
    assert_eq!(
        compile_condition(&Condition {
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
        compile_condition(&Condition {
            os: Some("bsd".to_string()),
            ..Default::default()
        })
        .is_err()
    );
}

#[test]
fn python_translates_with_or_without_python_prefix() {
    assert_eq!(
        compile_condition(&Condition {
            python: Some("3.11".to_string()),
            ..Default::default()
        })
        .unwrap()
        .unwrap(),
        "houdini_python == 'python3.11'"
    );
    assert_eq!(
        compile_condition(&Condition {
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
    let s = compile_condition(&Condition {
        houdini: Some(HoudiniRange::parse("^21").unwrap()),
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
    assert!(compile_condition(&Condition::default()).unwrap().is_none());
}

/// Helper: unwrap the Branches variant.
fn branches(lowered: LoweredConditional) -> Vec<std::collections::HashMap<String, String>> {
    match lowered {
        LoweredConditional::Branches(b) => b,
        other => panic!("expected Branches, got {other:?}"),
    }
}

fn houdini_branch(req: &str, set: &str) -> EnvValueBranch {
    EnvValueBranch {
        when: Condition {
            houdini: Some(HoudiniRange::parse(req).unwrap()),
            ..Default::default()
        },
        set: set.to_string(),
    }
}

#[test]
fn lower_conditional_substitutes_pkg_root_per_branch() {
    let variants = vec![
        houdini_branch("^21", "$HPM_PACKAGE_ROOT/h21/x"),
        houdini_branch("^22", "$HPM_PACKAGE_ROOT/h22/x"),
    ];
    let lowered = branches(
        lower_conditional(&variants, &[("$HPM_PACKAGE_ROOT", "/abs/pkg")], false).unwrap(),
    );
    assert_eq!(lowered.len(), 2);
    let key = lowered[0].keys().next().unwrap();
    assert!(key.contains("21"));
    assert_eq!(lowered[0][key], "/abs/pkg/h21/x");
}

/// Houdini applies every matching conditional-array element, so later
/// branches must carry the negation of every earlier branch to deliver
/// the documented first-match semantics. Negation is comparison-flipping
/// joined with `or` — the expression grammar has no `not`.
#[test]
fn lower_conditional_compiles_branches_mutually_exclusive() {
    let variants = vec![
        houdini_branch("^21", "a"),
        houdini_branch("^22", "b"),
        EnvValueBranch {
            when: Condition::default(),
            set: "fallback".to_string(),
        },
    ];
    let lowered = branches(lower_conditional(&variants, &[], false).unwrap());
    assert_eq!(lowered.len(), 3);
    assert_eq!(
        lowered[0].keys().next().unwrap(),
        "houdini_version >= '21' and houdini_version < '22'"
    );
    assert_eq!(
        lowered[1].keys().next().unwrap(),
        "houdini_version >= '22' and houdini_version < '23' \
         and ( houdini_version < '21' or houdini_version >= '22' )"
    );
    // The fallback's expression is exactly "no earlier branch matched" —
    // there is no always-true expression in Houdini's grammar.
    assert_eq!(
        lowered[2].keys().next().unwrap(),
        "( houdini_version < '21' or houdini_version >= '22' ) \
         and ( houdini_version < '22' or houdini_version >= '23' )"
    );
    assert_eq!(lowered[2].values().next().unwrap(), "fallback");
}

/// A lone empty `when` collapses to an unconditional value — the broken
/// `{"true": ...}` encoding (Houdini has no `true` literal; it defined a
/// stray variable named `true`) must never be emitted.
#[test]
fn empty_when_lowers_unconditional() {
    let variants = vec![EnvValueBranch {
        when: Condition::default(),
        set: "default".to_string(),
    }];
    assert_eq!(
        lower_conditional(&variants, &[], false).unwrap(),
        LoweredConditional::Unconditional("default".to_string())
    );
}

/// Branches after an unconditional branch are unreachable under
/// first-match semantics and must be dropped.
#[test]
fn branches_after_unconditional_are_pruned() {
    let variants = vec![
        EnvValueBranch {
            when: Condition::default(),
            set: "wins".to_string(),
        },
        houdini_branch("^21", "unreachable"),
    ];
    assert_eq!(
        lower_conditional(&variants, &[], false).unwrap(),
        LoweredConditional::Unconditional("wins".to_string())
    );

    // Same when the unconditional branch is not first: it terminates the
    // list after itself.
    let variants = vec![
        houdini_branch("^21", "a"),
        EnvValueBranch {
            when: Condition::default(),
            set: "fallback".to_string(),
        },
        houdini_branch("^22", "unreachable"),
    ];
    let lowered = branches(lower_conditional(&variants, &[], false).unwrap());
    assert_eq!(lowered.len(), 2);
    assert_eq!(lowered[1].values().next().unwrap(), "fallback");
}

#[test]
fn install_source_dev_filters_out_for_registry_install() {
    // The canonical HDK pattern: dev build dir, published dso fallback.
    let variants = vec![
        EnvValueBranch {
            when: Condition {
                install_source: Some("dev".to_string()),
                ..Default::default()
            },
            set: "build/Release".to_string(),
        },
        EnvValueBranch {
            when: Condition::default(),
            set: "dso".to_string(),
        },
    ];
    // Registry install: dev variant drops, fallback is unconditional.
    assert_eq!(
        lower_conditional(&variants, &[], false).unwrap(),
        LoweredConditional::Unconditional("dso".to_string())
    );
    // Dev install: the dev branch is unconditional at runtime and wins;
    // the dso fallback is unreachable and must NOT also fire (it used to,
    // shadowing the dev build under `prepend`).
    assert_eq!(
        lower_conditional(&variants, &[], true).unwrap(),
        LoweredConditional::Unconditional("build/Release".to_string())
    );
}

#[test]
fn install_source_strips_from_runtime_expression() {
    // A dev branch that also has a Houdini constraint should emit only
    // the Houdini constraint to the runtime expression — install_source
    // is hpm-side, not Houdini-side.
    let variants = vec![EnvValueBranch {
        when: Condition {
            houdini: Some(HoudiniRange::parse("^21").unwrap()),
            install_source: Some("dev".to_string()),
            ..Default::default()
        },
        set: "x".to_string(),
    }];
    let lowered = branches(lower_conditional(&variants, &[], true).unwrap());
    assert_eq!(lowered.len(), 1);
    let key = lowered[0].keys().next().unwrap();
    assert!(key.contains("houdini_version"));
    assert!(!key.contains("install_source"));
}

#[test]
fn install_source_registry_filters_out_for_dev_install() {
    let variants = vec![EnvValueBranch {
        when: Condition {
            install_source: Some("registry".to_string()),
            ..Default::default()
        },
        set: "dso".to_string(),
    }];
    assert_eq!(
        lower_conditional(&variants, &[], true).unwrap(),
        LoweredConditional::Branches(Vec::new())
    );
    assert_eq!(
        lower_conditional(&variants, &[], false).unwrap(),
        LoweredConditional::Unconditional("dso".to_string())
    );
}

#[test]
fn unknown_install_source_rejected() {
    let variants = vec![EnvValueBranch {
        when: Condition {
            install_source: Some("ci".to_string()),
            ..Default::default()
        },
        set: "x".to_string(),
    }];
    assert!(lower_conditional(&variants, &[], true).is_err());
}

#[test]
fn env_value_round_trips_flat() {
    let toml_str = r#"value = "hello""#;
    #[derive(Deserialize, Serialize)]
    struct Holder {
        value: EnvValue,
    }
    let h: Holder = toml::from_str(toml_str).unwrap();
    assert_eq!(h.value.as_flat(), Some("hello"));
}

#[test]
fn env_value_round_trips_conditional() {
    let toml_str = r#"
value = [
  { when = { houdini = "^21" }, set = "a" },
  { when = { houdini = "^22" }, set = "b" },
]
"#;
    #[derive(Deserialize)]
    struct Holder {
        value: EnvValue,
    }
    let h: Holder = toml::from_str(toml_str).unwrap();
    match h.value {
        EnvValue::Conditional(v) => {
            assert_eq!(v.len(), 2);
            assert_eq!(v[0].set, "a");
            assert_eq!(
                v[1].when.houdini.as_ref().map(HoudiniRange::as_str),
                Some("^22")
            );
        }
        EnvValue::Flat(_) => panic!("expected conditional"),
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
        value: EnvValue,
    }
    let res: Result<Holder, _> = toml::from_str(toml_str);
    assert!(res.is_err(), "deny_unknown_fields should reject 'weather'");
}
