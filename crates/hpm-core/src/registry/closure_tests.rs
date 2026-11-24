use super::*;
use crate::registry::test_support::entry;
use proptest::prelude::*;
use std::collections::BTreeMap;

/// `package@version` requiring each `(name, req)` of `deps`.
fn version(package: &str, version: &str, deps: &[(&str, &str)]) -> RegistryEntry {
    RegistryEntry {
        deps: deps
            .iter()
            .map(|(n, r)| RegistryDep {
                name: n.to_string(),
                req: r.to_string(),
            })
            .collect(),
        ..entry(package, version, false)
    }
}

fn listed(name: &str, pin: Option<&str>, versions: Vec<RegistryEntry>) -> Candidates {
    Candidates {
        name: name.to_string(),
        pin: pin.map(str::to_string),
        versions,
        on_disk: false,
    }
}

/// A source over a fixed map, recording what it was asked for. A name the
/// map does not hold fails, or is absent when `absent` is set.
#[derive(Default)]
struct MapSource {
    versions: BTreeMap<String, Vec<RegistryEntry>>,
    asked: Vec<String>,
    absent: bool,
}

impl MapSource {
    fn new(entries: Vec<RegistryEntry>) -> Self {
        let mut versions: BTreeMap<String, Vec<RegistryEntry>> = BTreeMap::new();
        for e in entries {
            versions.entry(e.name.clone()).or_default().push(e);
        }
        Self {
            versions,
            ..Self::default()
        }
    }
}

impl VersionSource for MapSource {
    async fn versions(&mut self, name: &str) -> Result<Vec<RegistryEntry>, Unsupplied> {
        self.asked.push(name.to_string());
        match self.versions.get(name) {
            Some(v) => Ok(v.clone()),
            None if self.absent => Err(Unsupplied::Absent("offline".into())),
            None => Err(Unsupplied::Failed("not found".into())),
        }
    }
}

fn chosen(selections: &[Selection]) -> Vec<(String, String)> {
    selections
        .iter()
        .map(|s| (s.name.clone(), s.version.clone()))
        .collect()
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(n, v)| (n.to_string(), v.to_string()))
        .collect()
}

fn manifest(toml: &str) -> PackageManifest {
    hpm_package::parse_manifest_str(toml).unwrap()
}

/// The case from hpm#17: a listed package that requires one the project
/// does not list brings it in, at the newest version that fits.
#[tokio::test]
async fn a_required_package_the_project_does_not_list_is_added() {
    let rig = vec![version("t/rig", "1.25.4", &[("t/kit", ">=0.3.0")])];
    let mut source = MapSource::new(vec![
        version("t/kit", "0.2.0", &[]),
        version("t/kit", "0.3.1", &[]),
        version("t/kit", "0.4.0", &[]),
    ]);
    let got = select_closed(
        &[listed("t/rig", Some("1.25.4"), rig)],
        &[],
        None,
        &mut source,
        |_| false,
    )
    .await
    .unwrap();
    assert_eq!(
        chosen(&got),
        pairs(&[("t/rig", "1.25.4"), ("t/kit", "0.4.0")])
    );
    assert_eq!(source.asked, ["t/kit"]);
}

/// Requirements are followed through any depth, each package fetched once.
#[tokio::test]
async fn requirements_are_followed_to_the_end_of_the_chain() {
    let a = vec![version("t/a", "1.0.0", &[("t/b", "*")])];
    let mut source = MapSource::new(vec![
        version("t/b", "1.0.0", &[("t/c", "*"), ("t/d", "*")]),
        version("t/c", "1.0.0", &[("t/d", "^1")]),
        version("t/d", "1.0.0", &[]),
        version("t/d", "2.0.0", &[]),
    ]);
    let got = select_closed(&[listed("t/a", None, a)], &[], None, &mut source, |_| false)
        .await
        .unwrap();
    let mut got = chosen(&got);
    got.sort();
    assert_eq!(
        got,
        pairs(&[
            ("t/a", "1.0.0"),
            ("t/b", "1.0.0"),
            ("t/c", "1.0.0"),
            ("t/d", "1.0.0")
        ])
    );
    let mut asked = source.asked.clone();
    asked.sort();
    asked.dedup();
    assert_eq!(asked.len(), source.asked.len(), "a package fetched twice");
}

/// A working copy is held at its declared version and brings what its
/// `hpm.toml` requires; path and url dependencies in it are not followed.
#[tokio::test]
async fn a_working_copy_brings_its_registry_requirements() {
    let rig = manifest(
        "[package]\npath = \"t/rig\"\nname = \"Rig\"\nversion = \"1.26.0\"\n\n\
         [dependencies]\n\"t/kit\" = \">=0.3.0\"\n\"t/local\" = { path = \"../local\" }\n",
    );
    let copy = working_copy("t/rig", &rig);
    assert!(copy.on_disk);
    assert_eq!(copy.pin.as_deref(), Some("1.26.0"));
    let mut source = MapSource::new(vec![version("t/kit", "0.3.0", &[])]);
    let got = select_closed(&[copy], &[], None, &mut source, |_| false)
        .await
        .unwrap();
    assert_eq!(
        chosen(&got),
        pairs(&[("t/rig", "1.26.0"), ("t/kit", "0.3.0")])
    );
    assert_eq!(
        source.asked,
        ["t/kit"],
        "a path dependency is not looked up"
    );
}

/// A requirement on a working copy holds like one on any pin.
#[tokio::test]
async fn a_requirement_a_working_copy_does_not_meet_is_a_conflict() {
    let kit = manifest("[package]\npath = \"t/kit\"\nname = \"Kit\"\nversion = \"0.2.5\"\n");
    let rig = vec![version("t/rig", "1.25.4", &[("t/kit", ">=0.3.0")])];
    let mut source = MapSource::default();
    let err = select_closed(
        &[
            listed("t/rig", Some("1.25.4"), rig),
            working_copy("t/kit", &kit),
        ],
        &[],
        None,
        &mut source,
        |_| false,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, ClosureError::Solve(SolveError::Conflict { .. })),
        "{err}"
    );
}

/// A supplied package is chosen exactly when required, at its pin, and the
/// source is never asked for it.
#[tokio::test]
async fn a_supplied_package_is_used_only_when_required() {
    let rig = vec![version("t/rig", "1.0.0", &[("t/kit", ">=0.3.0")])];
    let kit = listed("t/kit", Some("0.3.0"), vec![version("t/kit", "0.3.0", &[])]);
    let unused = listed(
        "t/other",
        Some("1.0.0"),
        vec![version("t/other", "1.0.0", &[])],
    );
    let mut source = MapSource::new(vec![version("t/kit", "0.9.0", &[])]);
    let got = select_closed(
        &[listed("t/rig", None, rig)],
        &[kit, unused],
        None,
        &mut source,
        |_| false,
    )
    .await
    .unwrap();
    assert_eq!(
        chosen(&got),
        pairs(&[("t/rig", "1.0.0"), ("t/kit", "0.3.0")])
    );
    assert!(source.asked.is_empty());
}

/// A source that fails refuses the choice, naming the package and who
/// required it.
#[tokio::test]
async fn a_source_failure_refuses_the_choice() {
    let rig = vec![version("t/rig", "1.0.0", &[("t/kit", "*")])];
    let err = select_closed(
        &[listed("t/rig", None, rig)],
        &[],
        None,
        &mut MapSource::default(),
        |_| false,
    )
    .await
    .unwrap_err();
    assert_eq!(
        err,
        ClosureError::Unsupplied {
            name: "t/kit".into(),
            requirer: "t/rig 1.0.0".into(),
            reason: "not found".into(),
        }
    );
}

/// A package with no version here passes the choice to a version that does
/// not need it; only when none works is it named.
#[tokio::test]
async fn an_absent_package_is_worked_around_or_named() {
    let rig = vec![
        version("t/rig", "1.0.0", &[]),
        version("t/rig", "2.0.0", &[("t/kit", "*")]),
    ];
    let mut source = MapSource {
        absent: true,
        ..MapSource::default()
    };
    let got = select_closed(
        &[listed("t/rig", None, rig)],
        &[],
        None,
        &mut source,
        |_| false,
    )
    .await
    .unwrap();
    assert_eq!(chosen(&got), pairs(&[("t/rig", "1.0.0")]));

    let pinned = vec![version("t/rig", "2.0.0", &[("t/kit", "*")])];
    let err = select_closed(
        &[listed("t/rig", Some("2.0.0"), pinned)],
        &[],
        None,
        &mut source,
        |_| false,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&err, ClosureError::Absent(a) if a[0].name == "t/kit"),
        "{err}"
    );
}

/// An excluded package is never fetched or added.
#[tokio::test]
async fn an_excluded_package_is_left_out() {
    let rig = vec![version("t/rig", "1.0.0", &[("t/kit", "*")])];
    let mut source = MapSource::new(vec![version("t/kit", "1.0.0", &[])]);
    let got = select_closed(&[listed("t/rig", None, rig)], &[], None, &mut source, |n| {
        n == "t/kit"
    })
    .await
    .unwrap();
    assert_eq!(chosen(&got), pairs(&[("t/rig", "1.0.0")]));
    assert!(source.asked.is_empty());
}

#[test]
fn registry_requirements_take_registry_dependencies_by_name() {
    let m = manifest(
        "[package]\npath = \"t/a\"\nname = \"A\"\nversion = \"1.0.0\"\n\n\
         [dependencies]\n\"t/z\" = \"^1\"\n\"t/b\" = { version = \"0.2.0\", registry = \"studio\" }\n\
         \"t/p\" = { path = \"../p\" }\n\"t/u\" = { url = \"https://x.invalid/u.zip\", version = \"1.0.0\" }\n",
    );
    assert_eq!(
        registry_requirements(&m),
        vec![
            RegistryDep {
                name: "t/b".into(),
                req: "0.2.0".into()
            },
            RegistryDep {
                name: "t/z".into(),
                req: "^1".into()
            },
        ]
    );
}

const NAMES: [&str; 5] = ["t/a", "t/b", "t/c", "t/d", "t/e"];
const VERSIONS: [&str; 3] = ["0.1.0", "0.2.0", "0.3.0"];

fn arb_req() -> impl Strategy<Value = &'static str> {
    prop_oneof![
        Just("*"),
        Just(">=0.2.0"),
        Just("<0.3.0"),
        Just("0.1.0"),
        Just("^0.3"),
    ]
}

/// A world of five packages, each with up to three versions requiring some
/// of the others, and which of them the project lists (pinned or not).
fn arb_world() -> impl Strategy<Value = (Vec<RegistryEntry>, Vec<(usize, Option<usize>)>)> {
    let one_version = (
        any::<bool>(),
        prop::collection::btree_map(0usize..5, arb_req(), 0..3),
    );
    let package = prop::collection::vec(one_version, 3);
    (
        prop::collection::vec(package, 5),
        prop::collection::btree_map(0usize..5, prop::option::of(0usize..3), 1..3),
    )
        .prop_map(|(packages, listed)| {
            let mut entries = Vec::new();
            for (p, versions) in packages.iter().enumerate() {
                for (v, (present, deps)) in versions.iter().enumerate() {
                    if !present && v != 0 {
                        continue;
                    }
                    let deps: Vec<(&str, &str)> = deps
                        .iter()
                        .filter(|(d, _)| **d != p)
                        .map(|(d, r)| (NAMES[*d], *r))
                        .collect();
                    entries.push(version(NAMES[p], VERSIONS[v], &deps));
                }
            }
            (entries, listed.into_iter().collect())
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// The loop only fetches: its answer is the one the solver gives when
    /// every package the project does not list is offered up front, and it
    /// fails exactly when that does. Every package it adds is required by a
    /// chosen version, and it asks for each package at most once and never
    /// for a listed one.
    #[test]
    fn prop_the_loop_chooses_what_offering_everything_chooses((entries, roots) in arb_world()) {
        let of = |name: &str| -> Vec<RegistryEntry> {
            entries.iter().filter(|e| e.name == name).cloned().collect()
        };
        let listed: Vec<Candidates> = roots
            .iter()
            .map(|(p, pin)| {
                let versions = of(NAMES[*p]);
                let pin = pin.and_then(|v| versions.get(v)).map(|e| e.version.clone());
                Candidates { name: NAMES[*p].to_string(), pin, versions, on_disk: false }
            })
            .collect();
        let everything: Vec<Candidates> = NAMES
            .iter()
            .filter(|n| !listed.iter().any(|c| c.name == **n))
            .map(|n| Candidates { name: n.to_string(), pin: None, versions: of(n), on_disk: false })
            .collect();
        let oracle = select(&listed, &everything, None);

        let mut source = MapSource::new(entries.clone());
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let got = runtime.block_on(select_closed(&listed, &[], None, &mut source, |_| false));
        match (&got, &oracle) {
            (Ok(got), Ok(want)) => {
                let (mut g, mut w) = (chosen(got), chosen(want));
                prop_assert_eq!(&g[..listed.len()], &w[..listed.len()], "listed first, in order");
                g.sort();
                w.sort();
                prop_assert_eq!(g, w);
                for sel in &got[listed.len()..] {
                    prop_assert!(
                        got.iter().any(|s| s.entry.iter().any(|e| e.deps.iter().any(|d| d.name == sel.name))),
                        "{} was added but nothing chosen requires it", sel.name
                    );
                }
            }
            (Err(_), Err(_)) => {}
            _ => prop_assert!(false, "loop {:?} but offering everything {:?}", got, oracle),
        }
        let mut asked = source.asked.clone();
        asked.sort();
        asked.dedup();
        prop_assert_eq!(asked.len(), source.asked.len(), "a package was fetched twice");
        prop_assert!(asked.iter().all(|n| !listed.iter().any(|c| &c.name == n)));
    }

    /// Keeping a lock: the answer is always a valid set (every chosen
    /// version's requirements on the others hold, the project's pins stand,
    /// every listed package is chosen and every other one is required); it
    /// keeps every locked version whenever a set with them all fits; when
    /// none does it is the fresh choice, and a frozen choice fails instead.
    #[test]
    fn prop_a_lock_is_kept_while_it_fits(
        (entries, roots) in arb_world(),
        lock in prop::collection::btree_map(0usize..5, 0usize..3, 0..5),
        frozen in any::<bool>(),
        excluded in prop::collection::btree_set(0usize..5, 0..3),
    ) {
        let is_excluded = |name: &str| excluded.iter().any(|p| NAMES[*p] == name);
        let of = |name: &str| -> Vec<RegistryEntry> {
            entries.iter().filter(|e| e.name == name).cloned().collect()
        };
        let locked_version = |name: &str| -> Option<String> {
            let p = NAMES.iter().position(|n| *n == name)?;
            let v = VERSIONS[*lock.get(&p)?];
            of(name).iter().any(|e| e.version == v).then(|| v.to_string())
        };
        let fresh: Vec<Candidates> = roots
            .iter()
            .map(|(p, pin)| {
                let versions = of(NAMES[*p]);
                let pin = pin.and_then(|v| versions.get(v)).map(|e| e.version.clone());
                Candidates { name: NAMES[*p].to_string(), pin, versions, on_disk: false }
            })
            .collect();
        let kept_listed: Vec<Candidates> = fresh
            .iter()
            .map(|c| Candidates {
                pin: c.pin.clone().or_else(|| locked_version(&c.name)),
                ..c.clone()
            })
            .collect();
        let supplied: Vec<Candidates> = NAMES
            .iter()
            .filter(|n| !fresh.iter().any(|c| c.name == **n))
            .filter_map(|n| {
                let v = locked_version(n)?;
                let e = of(n).into_iter().find(|e| e.version == v)?;
                Some(Candidates { name: n.to_string(), pin: Some(v), versions: vec![e], on_disk: false })
            })
            .collect();

        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let got = runtime.block_on(select_keeping(
            Kept { listed: &kept_listed, supplied: &supplied },
            &fresh,
            None,
            &mut MapSource::new(entries.clone()),
            frozen,
            is_excluded,
        ));
        let with_lock = runtime.block_on(select_closed(
            &kept_listed,
            &supplied,
            None,
            &mut MapSource::new(entries.clone()),
            is_excluded,
        ));
        let anew = runtime.block_on(select_closed(
            &fresh,
            &[],
            None,
            &mut MapSource::new(entries.clone()),
            is_excluded,
        ));

        if let Ok(sel) = &got {
            let version_of = |name: &str| sel.iter().find(|s| s.name == name).map(|s| s.version.clone());
            for s in sel {
                for d in s.entry.iter().flat_map(|e| e.deps.iter()) {
                    if let Some(v) = version_of(&d.name) {
                        let req = crate::registry::solve::requirement(&d.req).unwrap();
                        prop_assert!(
                            req.matches(&semver::Version::parse(&v).unwrap()),
                            "{} {} requires {} {} but {} was chosen", s.name, s.version, d.name, d.req, v
                        );
                    }
                }
            }
            for c in &fresh {
                prop_assert!(version_of(&c.name).is_some(), "{} is listed but not chosen", c.name);
                if let Some(pin) = &c.pin {
                    prop_assert_eq!(version_of(&c.name), Some(pin.clone()), "a pin moved");
                }
            }
            for s in sel.iter().filter(|s| !fresh.iter().any(|c| c.name == s.name)) {
                prop_assert!(
                    sel.iter().any(|o| o.entry.iter().any(|e| e.deps.iter().any(|d| d.name == s.name))),
                    "{} was chosen but nothing requires it", s.name
                );
            }
            for s in sel.iter().filter(|s| !fresh.iter().any(|c| c.name == s.name)) {
                prop_assert!(!is_excluded(&s.name), "{} is excluded but was chosen", s.name);
            }
        }
        match &with_lock {
            Ok(want) => {
                prop_assert!(got.is_ok(), "a set keeping the lock fits, yet {:?}", got);
                let sel = got.as_ref().unwrap();
                for s in sel {
                    let pinned_by_project = fresh.iter().any(|c| c.name == s.name && c.pin.is_some());
                    if let (false, Some(v)) = (pinned_by_project, locked_version(&s.name)) {
                        prop_assert_eq!(&s.version, &v, "{}: the locked version was not kept", s.name);
                    }
                }
                prop_assert_eq!(chosen(sel), chosen(want));
            }
            Err(_) if frozen => prop_assert!(got.is_err(), "frozen, but the lock did not fit and {:?}", got),
            Err(_) => prop_assert_eq!(
                got.as_ref().map(|s| chosen(s)).map_err(|_| ()),
                anew.as_ref().map(|s| chosen(s)).map_err(|_| ())
            ),
        }
    }

    /// Excluding a package is offering the solver everything but it: the
    /// loop's answer is the one offering every package the project neither
    /// lists nor excludes gives, a requirement on an excluded package
    /// constrains nothing, the source is never asked for one, and one
    /// offered as supplied is not chosen either.
    #[test]
    fn prop_an_excluded_package_is_never_chosen_or_fetched(
        (entries, roots) in arb_world(),
        excluded in prop::collection::btree_set(0usize..5, 1..3),
        supply_excluded in any::<bool>(),
    ) {
        let is_excluded = |name: &str| excluded.iter().any(|p| NAMES[*p] == name);
        let of = |name: &str| -> Vec<RegistryEntry> {
            entries.iter().filter(|e| e.name == name).cloned().collect()
        };
        let listed: Vec<Candidates> = roots
            .iter()
            .map(|(p, pin)| {
                let versions = of(NAMES[*p]);
                let pin = pin.and_then(|v| versions.get(v)).map(|e| e.version.clone());
                Candidates { name: NAMES[*p].to_string(), pin, versions, on_disk: false }
            })
            .collect();
        let unlisted = |n: &str| !listed.iter().any(|c| c.name == n);
        let allowed: Vec<Candidates> = NAMES
            .iter()
            .filter(|n| unlisted(n) && !is_excluded(n))
            .map(|n| Candidates { name: n.to_string(), pin: None, versions: of(n), on_disk: false })
            .collect();
        let supplied: Vec<Candidates> = if supply_excluded {
            NAMES
                .iter()
                .filter(|n| unlisted(n) && is_excluded(n))
                .map(|n| Candidates { name: n.to_string(), pin: None, versions: of(n), on_disk: false })
                .collect()
        } else {
            Vec::new()
        };
        let oracle = select(&listed, &allowed, None);

        let mut source = MapSource::new(entries.clone());
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let got = runtime.block_on(select_closed(&listed, &supplied, None, &mut source, is_excluded));

        prop_assert!(
            source.asked.iter().all(|n| !is_excluded(n) || listed.iter().any(|c| &c.name == n)),
            "an excluded package was fetched: {:?}", source.asked
        );
        match (&got, &oracle) {
            (Ok(got), Ok(want)) => {
                let (mut g, mut w) = (chosen(got), chosen(want));
                g.sort();
                w.sort();
                prop_assert_eq!(g, w);
                for s in got.iter().filter(|s| !listed.iter().any(|c| c.name == s.name)) {
                    prop_assert!(!is_excluded(&s.name), "{} is excluded but was chosen", s.name);
                }
            }
            (Err(_), Err(_)) => {}
            _ => prop_assert!(false, "loop {:?} but the world without the excluded {:?}", got, oracle),
        }
    }
}
