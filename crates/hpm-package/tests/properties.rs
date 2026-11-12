//! Property-based tests for hpm-package — proptest strategies plus the
//! `proptest!` harness exercising manifest validation, TOML round-trips,
//! Houdini package generation, and dependency-spec shape consistency.

use proptest::prelude::*;

use hpm_package::manifest::operators::{OperatorDecl, OperatorKind, OperatorSource};
use hpm_package::manifest::stage::{ProfileStaging, StageProfileRules};
use hpm_package::{
    CompatConfig, Condition, DependencySpec, EnvMethod, EnvValue, EnvValueBranch, HoudiniEnvValue,
    HoudiniRange, InstallSource, ManifestEnvEntry, OsKey, PackageInfo, PackageManifest,
    PackagePath, PackageScripts, PlaceRule, Platform, PlatformStaging, PythonDependencySpec,
    RegistryConfig, RegistryType, ScriptEntry, ScriptEnv, StageConfig, StagePlatformRules,
};

/// Strategy to generate valid slug segments (kebab-case)
pub fn slug_strategy() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-z][a-z0-9-]{1,25}")
        .unwrap()
        .prop_filter("Slug must not end with hyphen", |name| {
            !name.ends_with('-') && name.len() >= 2 && name.len() <= 25
        })
}

/// Strategy to generate valid package paths (creator/slug). Returns the
/// validated [`PackagePath`] directly so consumers can drop it straight
/// into a [`PackageInfo`].
pub fn package_path_strategy() -> impl Strategy<Value = PackagePath> {
    (slug_strategy(), slug_strategy()).prop_map(|(creator, slug)| {
        PackagePath::new(format!("{}/{}", creator, slug))
            .expect("slug_strategy yields kebab-case segments")
    })
}

/// Strategy to generate freeform display names
pub fn package_name_strategy() -> impl Strategy<Value = String> {
    prop::string::string_regex("[A-Z][A-Za-z0-9 ]{1,30}")
        .unwrap()
        .prop_filter("Name must not be empty", |name| !name.trim().is_empty())
}

/// Strategy to generate semantic version strings
pub fn version_strategy() -> impl Strategy<Value = String> {
    (0u32..100, 0u32..100, 0u32..100)
        .prop_map(|(major, minor, patch)| format!("{}.{}.{}", major, minor, patch))
}

/// Strategy to generate version requirement strings
pub fn version_req_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("*".to_string()),
        version_strategy(),
        version_strategy().prop_map(|v| format!("^{}", v)),
        version_strategy().prop_map(|v| format!("~{}", v)),
        version_strategy().prop_map(|v| format!(">={}", v)),
    ]
}

/// Strategy to generate license identifiers
pub fn license_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("MIT".to_string()),
        Just("Apache-2.0".to_string()),
        Just("GPL-3.0".to_string()),
        Just("BSD-3-Clause".to_string()),
        Just("ISC".to_string()),
    ]
}

/// Strategy to generate author strings
pub fn author_strategy() -> impl Strategy<Value = String> {
    ("[A-Z][a-z]{3,15}", "[a-z0-9._%+-]+@[a-z0-9.-]+\\.[a-z]{2,}")
        .prop_map(|(name, email)| format!("{} <{}>", name, email))
}

/// Strategy to generate URLs
pub fn url_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("https://github.com/user/repo".to_string()),
        Just("https://example.com/project".to_string()),
        Just("https://docs.example.com".to_string()),
    ]
}

/// Strategy to generate `[compat].houdini` range strings.
pub fn houdini_version_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(">=20.5".to_string()),
        Just(">=21.0".to_string()),
        Just(">=22.0".to_string()),
        Just("^21".to_string()),
        Just("^22".to_string()),
        Just(">=20.5, <22".to_string()),
        Just("~21.5".to_string()),
    ]
}

/// Strategy to generate dependency specifications
pub fn dependency_spec_strategy() -> impl Strategy<Value = DependencySpec> {
    prop_oneof![
        // Bare version shorthand (registry spec with no options)
        version_strategy().prop_map(|version| DependencySpec::registry(version, None)),
        // URL dependency
        (url_strategy(), version_strategy(), any::<bool>()).prop_map(|(url, version, optional)| {
            DependencySpec::Url {
                url,
                version,
                optional,
            }
        }),
        // Path dependency
        (
            prop::string::string_regex(r"\.?\./[a-z0-9-/]{3,30}").unwrap(),
            any::<bool>(),
            any::<bool>()
        )
            .prop_map(|(path, optional, link)| DependencySpec::Path {
                path,
                optional,
                link
            }),
        // Registry dependency with options
        (
            version_strategy(),
            prop::option::of(Just("custom-registry".to_string())),
            any::<bool>()
        )
            .prop_map(|(version, registry, optional)| DependencySpec::Registry {
                version,
                registry,
                optional,
            }),
    ]
}

/// Strategy to generate Python dependency specifications
pub fn python_dependency_spec_strategy() -> impl Strategy<Value = PythonDependencySpec> {
    prop_oneof![
        version_req_strategy().prop_map(PythonDependencySpec::Simple),
        (
            prop::option::of(version_req_strategy()),
            prop::option::of(any::<bool>()),
            prop::option::of(prop::collection::vec("[a-z]{3,10}", 0..3)),
        )
            .prop_map(|(version, optional, extras)| {
                PythonDependencySpec::Detailed {
                    version,
                    optional,
                    extras,
                }
            })
    ]
}

/// Small key domains so generated maps collide and order matters.
fn dep_key_strategy() -> impl Strategy<Value = String> {
    prop::sample::select(vec!["acme/tools", "acme/rig", "other/tools", "studio/fx"])
        .prop_map(str::to_string)
}

fn env_key_strategy() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "PATH",
        "PYTHONPATH",
        "OCIO",
        "MY_TOOL_ROOT",
        "HOUDINI_PATH",
    ])
    .prop_map(str::to_string)
}

fn platform_strategy() -> impl Strategy<Value = Platform> {
    prop::sample::select(vec![
        Platform::LinuxX86_64,
        Platform::LinuxAarch64,
        Platform::MacosX86_64,
        Platform::MacosAarch64,
        Platform::WindowsX86_64,
        Platform::WindowsAarch64,
        Platform::Universal,
    ])
}

fn os_key_strategy() -> impl Strategy<Value = OsKey> {
    prop::sample::select(vec![OsKey::Linux, OsKey::Macos, OsKey::Windows])
}

/// Free text for values, including non-ASCII, quotes and backslashes,
/// which TOML must escape.
fn text_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        "[a-zA-Z0-9_ ./$-]{0,24}",
        Just(String::new()),
        Just("café/ø/日本".to_string()),
        Just("quote\"back\\slash".to_string()),
        Just("$HPM_PACKAGE_ROOT/python".to_string()),
    ]
}

/// A `when` selector valid for `[runtime]` branches (every axis).
fn runtime_condition_strategy() -> impl Strategy<Value = Condition> {
    (
        prop::option::of(houdini_version_strategy()),
        prop::option::of(os_key_strategy()),
        prop::option::of(prop::sample::select(vec!["3.10", "3.11", "3.12"])),
        prop::option::of(prop::sample::select(vec![
            InstallSource::Dev,
            InstallSource::Registry,
        ])),
    )
        .prop_map(|(houdini, os, python, install_source)| Condition {
            houdini: houdini.map(|r| HoudiniRange::parse(r).expect("strategy yields valid ranges")),
            os,
            python: python.map(str::to_string),
            install_source,
        })
}

fn env_value_strategy(cond: BoxedStrategy<Condition>) -> impl Strategy<Value = EnvValue> {
    prop_oneof![
        text_strategy().prop_map(EnvValue::Flat),
        prop::collection::vec(
            (cond, text_strategy()).prop_map(|(when, set)| EnvValueBranch { when, set }),
            1..4
        )
        .prop_map(EnvValue::Conditional),
    ]
}

fn runtime_entry_strategy() -> impl Strategy<Value = ManifestEnvEntry> {
    (
        prop::sample::select(vec![EnvMethod::Set, EnvMethod::Prepend, EnvMethod::Append]),
        prop::option::of(env_value_strategy(runtime_condition_strategy().boxed())),
        any::<bool>(),
    )
        .prop_map(|(method, value, required)| ManifestEnvEntry {
            method,
            // A missing value is only legal as a required placeholder.
            required: required || value.is_none(),
            value,
        })
}

fn script_entry_strategy() -> impl Strategy<Value = ScriptEntry> {
    let os_only = prop::option::of(os_key_strategy())
        .prop_map(|os| Condition {
            os,
            ..Condition::default()
        })
        .boxed();
    prop_oneof![
        text_strategy().prop_map(ScriptEntry::Plain),
        (
            env_value_strategy(os_only),
            prop::option::of(prop::sample::select(vec!["3.10", "3.11"])),
            prop::collection::vec("[a-z]{2,8}(==1\\.[0-9])?", 0..3),
            prop::option::of(text_strategy()),
            prop::option::of(text_strategy()),
            any::<bool>(),
        )
            .prop_map(
                |(cmd, python, requirements, label, description, package_env)| {
                    ScriptEntry::WithEnv(ScriptEnv {
                        cmd,
                        python: python.map(str::to_string),
                        requirements,
                        label,
                        description,
                        package_env,
                    })
                }
            ),
    ]
}

fn registry_config_strategy() -> impl Strategy<Value = RegistryConfig> {
    (
        prop::sample::select(vec!["tumbletrove", "studio", "mirror"]),
        url_strategy(),
        prop::sample::select(vec![RegistryType::Api, RegistryType::Git]),
    )
        .prop_map(|(name, url, registry_type)| RegistryConfig {
            name: name.to_string(),
            url,
            registry_type,
        })
}

fn non_blank_strategy() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_/.]{0,16}"
}

fn place_rules_strategy() -> impl Strategy<Value = StagePlatformRules> {
    prop::collection::vec(
        (
            non_blank_strategy(),
            prop_oneof![Just("./".to_string()), non_blank_strategy()],
        )
            .prop_map(|(from, to)| PlaceRule { from, to }),
        0..3,
    )
    .prop_map(|place| StagePlatformRules { place })
}

/// Per-platform staging keyed only by platforms in `declared`, so the
/// manifest stays valid.
fn platform_staging_strategy(declared: Vec<Platform>) -> BoxedStrategy<PlatformStaging> {
    if declared.is_empty() {
        return Just(PlatformStaging::default()).boxed();
    }
    prop::collection::vec(
        (prop::sample::select(declared), place_rules_strategy()),
        0..3,
    )
    .prop_map(|entries| PlatformStaging {
        entries: entries
            .into_iter()
            .map(|(p, r)| (p.to_string(), r))
            .collect(),
    })
    .boxed()
}

fn stage_strategy(declared: Vec<Platform>) -> impl Strategy<Value = StageConfig> {
    let globs = || prop::collection::vec("[a-z*/.]{1,12}", 0..3);
    let profile = (
        globs(),
        globs(),
        globs(),
        platform_staging_strategy(declared.clone()),
    )
        .prop_map(|(prepack, include, exclude, platform)| StageProfileRules {
            prepack,
            include,
            exclude,
            platform,
        });
    (
        prop::option::of("[a-z]{1,8}"),
        globs(),
        globs(),
        globs(),
        platform_staging_strategy(declared),
        prop::collection::vec(
            (prop::sample::select(vec!["debug", "release"]), profile),
            0..2,
        ),
    )
        .prop_map(
            |(output_dir, prepack, include, exclude, platform, profiles)| StageConfig {
                output_dir,
                prepack,
                include,
                exclude,
                platform,
                profile: ProfileStaging {
                    entries: profiles
                        .into_iter()
                        .map(|(k, v)| (k.to_string(), v))
                        .collect(),
                },
            },
        )
}

fn operator_strategy(declared: Vec<Platform>) -> impl Strategy<Value = OperatorDecl> {
    let per_platform = if declared.is_empty() {
        Just(None).boxed()
    } else {
        prop::collection::vec(
            (prop::sample::select(declared), "dso/[a-z]{1,8}\\.so"),
            1..3,
        )
        .prop_map(|v| {
            Some(OperatorSource::PerPlatform(
                v.into_iter().map(|(p, s)| (p.to_string(), s)).collect(),
            ))
        })
        .boxed()
    };
    (
        prop::sample::select(vec![OperatorKind::Hda, OperatorKind::Dso]),
        "[a-z][a-z0-9_:.]{0,20}",
        prop::sample::select(vec!["Sop", "Lop", "Object", "Driver"]),
        prop::option::of(text_strategy()),
        prop::option::of(text_strategy()),
        prop::option::of(text_strategy()),
        prop_oneof![
            Just(None),
            "otls/[a-z]{1,8}\\.hda".prop_map(|s| Some(OperatorSource::Single(s))),
            per_platform,
        ],
    )
        .prop_map(
            |(kind, type_name, category, label, tab_submenu, icon, source)| OperatorDecl {
                kind,
                type_name,
                category: category.to_string(),
                label,
                tab_submenu,
                icon,
                source,
            },
        )
}

/// Strategy to generate package manifests covering every section.
pub fn package_manifest_strategy() -> impl Strategy<Value = PackageManifest> {
    prop::collection::vec(platform_strategy(), 0..4).prop_flat_map(|mut platforms| {
        platforms.dedup();
        let info = (
            package_path_strategy(),
            package_name_strategy(),
            version_strategy(),
            prop::option::of("[A-Za-z0-9 ]{10,100}"),
            prop::collection::vec(author_strategy(), 0..4),
            prop::option::of(license_strategy()),
            prop::option::of(text_strategy()),
            prop::option::of(url_strategy()),
            prop::option::of(url_strategy()),
            prop::option::of(url_strategy()),
            prop::collection::vec("[a-z]{2,10}", 0..3),
            prop::collection::vec("[a-z]{2,10}", 0..3),
        )
            .prop_map(
                |(
                    path,
                    name,
                    version,
                    description,
                    authors,
                    license,
                    readme,
                    homepage,
                    repository,
                    documentation,
                    keywords,
                    categories,
                )| PackageInfo {
                    path,
                    name,
                    version,
                    description,
                    authors,
                    license,
                    readme,
                    homepage,
                    repository,
                    documentation,
                    keywords,
                    categories,
                },
            );
        let sections = (
            prop::option::of(houdini_version_strategy()),
            stage_strategy(platforms.clone()),
            prop::collection::vec(registry_config_strategy(), 0..3),
            prop::collection::vec((dep_key_strategy(), dependency_spec_strategy()), 0..4),
            prop::collection::vec(
                (
                    prop::sample::select(vec!["numpy", "requests", "pyyaml"]),
                    python_dependency_spec_strategy(),
                ),
                0..3,
            ),
            prop::collection::vec((env_key_strategy(), runtime_entry_strategy()), 0..4),
            prop::collection::vec(
                (
                    prop::sample::select(vec!["build", "test", "serve"]),
                    script_entry_strategy(),
                ),
                0..3,
            ),
            prop::collection::vec(operator_strategy(platforms.clone()), 0..3),
        );
        (info, sections, Just(platforms)).prop_map(
            |(
                package,
                (houdini, stage, registries, deps, py_deps, runtime, scripts, operators),
                platforms,
            )| PackageManifest {
                package,
                compat: CompatConfig {
                    houdini: houdini
                        .map(|r| HoudiniRange::parse(r).expect("strategy yields valid ranges")),
                    platforms,
                },
                stage,
                registries,
                dependencies: deps.into_iter().collect(),
                python_dependencies: py_deps
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v))
                    .collect(),
                runtime: runtime.into_iter().collect(),
                scripts: PackageScripts {
                    commands: scripts
                        .into_iter()
                        .map(|(k, v)| (k.to_string(), v))
                        .collect(),
                },
                operators,
            },
        )
    })
}

/// Strategy to generate malformed package paths
pub fn malformed_package_path_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("".to_string()),                   // Empty
        Just("flat-name".to_string()),          // No slash
        Just("/slug".to_string()),              // Missing creator
        Just("creator/".to_string()),           // Missing slug
        Just("a/b/c".to_string()),              // Too many segments
        Just("-bad/slug".to_string()),          // Creator starts with hyphen
        Just("creator/-bad".to_string()),       // Slug starts with hyphen
        Just("Upper/case".to_string()),         // Uppercase
        Just("creator/UPPER".to_string()),      // Uppercase slug
        Just("test_under/slug".to_string()),    // Underscore in creator
        Just("creator/test_under".to_string()), // Underscore in slug
        Just("creator/test@pkg".to_string()),   // @ in slug
    ]
}

/// Strategy to generate malformed version strings
pub fn malformed_version_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("".to_string()),        // Empty version
        Just("v1.0.0".to_string()),  // With 'v' prefix
        Just("1".to_string()),       // Missing minor.patch
        Just("1.0".to_string()),     // Missing patch
        Just("1.0.0.0".to_string()), // Too many components
        Just("1.0.0-".to_string()),  // Trailing hyphen
        Just("1.0.0+".to_string()),  // Trailing plus
        Just("1.0.0-+".to_string()), // Invalid metadata
        Just("01.0.0".to_string()),  // Leading zeros
        Just("1.00.0".to_string()),
        Just("1.0.00".to_string()),
        r"[a-zA-Z]{1,10}",              // Non-numeric
        r"[0-9]+\\.[a-zA-Z]+\\.[0-9]+", // Mixed alpha-numeric
        r"[0-9]+\\.[0-9]+\\.[a-zA-Z]+",
        Just("-1.0.0".to_string()), // Negative numbers
        Just("1.-1.0".to_string()),
        Just("1.0.-1".to_string()),
    ]
}

/// Strategy to generate problematic TOML content
pub fn malformed_toml_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("[package]\nname =".to_string()), // Incomplete assignment
        Just("[package\nname = \"test\"".to_string()), // Missing bracket
        Just("[package]]\nname = \"test\"".to_string()), // Extra bracket
        Just("package]\nname = \"test\"".to_string()), // Missing opening bracket
        Just("[package]\nname = test".to_string()), // Unquoted string
        Just("[package]\nname = \"test\nversion = \"1.0.0\"".to_string()), // Unclosed quote
        Just("[package]\nname = 123".to_string()), // Wrong type
        Just("[package]\nname = \"test\"\nversion = true".to_string()), // Wrong type
        Just("invalid toml content {]".to_string()), // Invalid syntax
        Just("[package]\n\"name\" = \"test\"".to_string()), // Quoted key
        Just("[package]\nname = 'test'".to_string()), // Single quotes
    ]
}

/// Strategy to generate edge case URLs
pub fn edge_case_url_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("".to_string()),                                     // Empty URL
        Just("not-a-url".to_string()),                            // Not a URL
        Just("http://".to_string()),                              // Incomplete URL
        Just("ftp://example.com".to_string()),                    // Different protocol
        Just("https://".to_string()),                             // Just protocol
        Just("https://example".to_string()),                      // No TLD
        Just("https://example.".to_string()),                     // Trailing dot
        Just("https://example.com:".to_string()),                 // Incomplete port
        Just("https://example.com:abc".to_string()),              // Invalid port
        Just("https://ex ample.com".to_string()),                 // Space in domain
        Just("https://example.com/path with spaces".to_string()), // Spaces in path
        r"https://[a-z]{1,5}\\.[a-z]{200,300}",                   // Extremely long domain
        Just("mailto:test@example.com".to_string()),              // Wrong protocol
    ]
}

/// Strategy to generate problematic email addresses
pub fn malformed_email_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("".to_string()),                      // Empty email
        Just("not-an-email".to_string()),          // No @ symbol
        Just("@example.com".to_string()),          // No local part
        Just("user@".to_string()),                 // No domain
        Just("user@@example.com".to_string()),     // Double @
        Just("user@ex ample.com".to_string()),     // Space in domain
        Just("user name@example.com".to_string()), // Space in local part
        Just("user@example".to_string()),          // No TLD
        Just("user@example.".to_string()),         // Trailing dot
        Just("user@.example.com".to_string()),     // Leading dot in domain
        Just("user.@example.com".to_string()),     // Trailing dot in local
        Just(".user@example.com".to_string()),     // Leading dot in local
        r"[a-z]{100,200}@example.com",             // Extremely long local part
        r"user@[a-z]{100,200}\\.com",              // Extremely long domain
    ]
}

/// TOML needs a table at the top level; wrap a single value in one.
#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct Wrapped<T> {
    dep: T,
}

// Property-based tests using the strategies above

proptest! {
    /// Test that valid package manifests always pass validation
    #[test]
    fn prop_valid_manifests_pass_validation(manifest in package_manifest_strategy()) {
        prop_assert!(manifest.validate().is_ok());
    }

    /// hpm.toml round-trips exactly: every section, every field.
    #[test]
    fn prop_manifest_serialization_roundtrip(manifest in package_manifest_strategy()) {
        let toml_str = toml::to_string(&manifest).unwrap();
        let deserialized: PackageManifest = toml::from_str(&toml_str).unwrap();
        prop_assert_eq!(deserialized, manifest);
    }

    /// A dependency spec round-trips through TOML, the format it is stored in.
    #[test]
    fn prop_dependency_spec_roundtrip(spec in dependency_spec_strategy()) {
        let table = Wrapped { dep: spec };
        let toml_str = toml::to_string(&table).unwrap();
        let back: Wrapped<DependencySpec> = toml::from_str(&toml_str).unwrap();
        prop_assert_eq!(back, table);
    }

    /// A Python dependency spec round-trips through TOML.
    #[test]
    fn prop_python_dependency_spec_roundtrip(spec in python_dependency_spec_strategy()) {
        let table = Wrapped { dep: spec };
        let toml_str = toml::to_string(&table).unwrap();
        let back: Wrapped<PythonDependencySpec> = toml::from_str(&toml_str).unwrap();
        prop_assert_eq!(back, table);
    }

    /// The native (hpackage-style) package JSON round-trips through serde_json
    /// for both targets, whenever the manifest can produce one.
    #[test]
    fn prop_houdini_native_package_json_roundtrip(manifest in package_manifest_strategy()) {
        for target in [hpm_package::NativePackageTarget::Generic, hpm_package::NativePackageTarget::SideFxHpackage] {
            if let Ok((_, pkg)) = manifest.generate_houdini_native_package_for(target) {
                let json = serde_json::to_string(&pkg).unwrap();
                let back: hpm_package::HoudiniNativePackage = serde_json::from_str(&json).unwrap();
                prop_assert_eq!(back, pkg);
            }
        }
    }

    /// The bundled `{slug}.json` exists for every valid manifest, is named
    /// for the slug, roots `hpath` at `$HOUDINI_PACKAGE_PATH/{slug}`, carries
    /// the version, and has an `enable` exactly when `[compat].houdini` is set.
    #[test]
    fn prop_houdini_package_generation(manifest in package_manifest_strategy()) {
        let (file, pkg) = manifest
            .generate_houdini_native_package()
            .expect("strategy yields valid manifests");
        let slug = manifest.package.path.slug();
        prop_assert_eq!(file, format!("{slug}.json"));
        prop_assert_eq!(&pkg.name, slug);
        prop_assert_eq!(pkg.hpath, format!("$HOUDINI_PACKAGE_PATH/{slug}"));
        prop_assert_eq!(&pkg.hpackage.version, &manifest.package.version);
        prop_assert_eq!(pkg.enable.is_some(), manifest.compat.houdini.is_some());
    }

    // prop_package_path_validation removed: PackagePath::new enforces
    // validity at construction, so the field type makes this property
    // structurally true. Malformed-path rejection is covered directly by
    // PackagePath's unit tests in package_path.rs.

    /// Test that semver validation works correctly
    #[test]
    fn prop_semver_validation(
        valid_version in version_strategy(),
        invalid_version in r"[a-zA-Z]{3,10}|[0-9]+\\.[0-9]+|[0-9]+"
    ) {
        // Valid semver should pass validation
        let valid_manifest = PackageManifest::new(
            PackagePath::new("studio/test-package").unwrap(),
            "Test Package".to_string(),
            valid_version,
            None,
            Vec::new(),
            None,
        );
        prop_assert!(valid_manifest.validate().is_ok());

        // Invalid versions should fail (unless they accidentally match semver)
        let invalid_manifest = PackageManifest::new(
            PackagePath::new("studio/test-package").unwrap(),
            "Test Package".to_string(),
            invalid_version.clone(),
            None,
            Vec::new(),
            None,
        );

        // Only assert failure if it's clearly not semver
        if !invalid_version.chars().all(|c| c.is_ascii_digit() || c == '.') ||
           invalid_version.split('.').count() != 3 {
            prop_assert!(invalid_manifest.validate().is_err());
        }
    }

    /// Test that malformed package paths are rejected at the type
    /// boundary, not deep inside `PackageManifest::validate`.
    #[test]
    fn prop_malformed_package_paths_rejected(malformed_path in malformed_package_path_strategy()) {
        let result = PackagePath::new(malformed_path.clone());
        prop_assert!(result.is_err(),
            "Malformed package path '{}' should be rejected", malformed_path);
    }

    /// Test that malformed versions are rejected consistently
    #[test]
    fn prop_malformed_versions_rejected(malformed_version in malformed_version_strategy()) {
        let manifest = PackageManifest::new(
            PackagePath::new("studio/test-package").unwrap(),
            "Test Package".to_string(),
            malformed_version.clone(),
            None,
            Vec::new(),
            None,
        );

        let result = manifest.validate();

        // Check if this should fail based on version format
        let parts: Vec<&str> = malformed_version.split('.').collect();
        let is_valid_semver = parts.len() == 3 &&
            parts.iter().all(|part| !part.is_empty() && part.parse::<u32>().is_ok());

        if !is_valid_semver || malformed_version.is_empty() {
            prop_assert!(result.is_err(),
                "Malformed version '{}' should fail validation", malformed_version);
        }
    }

    /// Test that TOML parsing handles malformed input gracefully
    #[test]
    fn prop_toml_parsing_graceful_failure(malformed_toml in malformed_toml_strategy()) {
        let result = toml::from_str::<PackageManifest>(&malformed_toml);

        // Should either parse successfully (if accidentally valid) or fail with informative error
        if let Err(error) = result {
            let error_msg = error.to_string();
            prop_assert!(
                !error_msg.is_empty(),
                "Error message should not be empty for malformed TOML: '{}'", malformed_toml
            );

            // Error should be descriptive
            prop_assert!(
                error_msg.to_lowercase().contains("toml") ||
                error_msg.to_lowercase().contains("parse") ||
                error_msg.to_lowercase().contains("syntax") ||
                error_msg.to_lowercase().contains("invalid") ||
                error_msg.to_lowercase().contains("expected"),
                "Error should be descriptive for malformed TOML: '{}'", malformed_toml
            );
        }
    }

    /// Test that dependency specifications handle edge cases
    #[test]
    fn prop_dependency_spec_edge_cases(
        git_url in prop_oneof![
            url_strategy(),
            edge_case_url_strategy(),
        ],
        path in prop::string::string_regex(r"\.?\./[a-z0-9-/]{3,30}").unwrap()
    ) {
        // Odd URLs and paths are stored verbatim: they round-trip through
        // TOML unchanged (hpm does not normalise them).
        for spec in [
            DependencySpec::Url { url: git_url.clone(), version: "1.0.0".to_string(), optional: true },
            DependencySpec::Path { path: path.clone(), optional: false, link: false },
        ] {
            let table = Wrapped { dep: spec };
            let back: Wrapped<DependencySpec> =
                toml::from_str(&toml::to_string(&table).unwrap()).unwrap();
            prop_assert_eq!(back, table);
        }
    }

    /// Test that Houdini package generation handles missing/invalid data.
    /// `path` is always a valid `PackagePath` because the field type
    /// enforces it; the malformed-version branch still exercises the
    /// generator's tolerance for non-semver versions.
    #[test]
    fn prop_houdini_package_generation_robustness(
        path in package_path_strategy(),
        name in package_name_strategy(),
        version in prop_oneof![version_strategy(), malformed_version_strategy()],
        houdini_req in prop::option::of(houdini_version_strategy()),
    ) {
        let manifest = PackageManifest {
            package: PackageInfo {
                path,
                name,
                version,
                description: None,
                authors: Vec::new(),
                license: None,
                readme: None,
                homepage: None,
                repository: None,
                documentation: None,
                keywords: Vec::new(),
                categories: Vec::new(),
            },
            compat: CompatConfig {
                houdini: houdini_req
                    .map(|r| hpm_package::HoudiniRange::parse(r).expect("strategy yields valid ranges")),
                platforms: Vec::new(),
            },
            stage: Default::default(),
            registries: Vec::new(),
            dependencies: indexmap::IndexMap::new(),
            python_dependencies: indexmap::IndexMap::new(),
            runtime: indexmap::IndexMap::new(),
            scripts: Default::default(),
                operators: Vec::new(),
        };

        // Generation either succeeds or returns Err for a manifest it
        // cannot lower; both are documented outcomes. The strategy mixes malformed
        // versions in for robustness coverage.
        let Ok((_, houdini_pkg)) = manifest.generate_houdini_native_package() else {
            return Ok(());
        };

        // Environment variables should be properly formatted
        for env_map in &houdini_pkg.env {
            for (key, value) in env_map {
                prop_assert!(!key.is_empty(), "Environment variable key should not be empty");
                match value {
                    HoudiniEnvValue::Simple(s) => {
                        prop_assert!(!s.is_empty(), "Simple env value should not be empty");
                    }
                    // `method` is a typed enum (HoudiniMethod), so only the
                    // value shape needs checking here.
                    HoudiniEnvValue::Detailed { value, .. } => {
                        prop_assert!(!value.is_empty(), "Detailed env value should not be empty");
                    }
                    HoudiniEnvValue::DetailedConditional { value, .. } => {
                        prop_assert!(
                            !value.is_empty(),
                            "Conditional env value list should not be empty"
                        );
                    }
                }
            }
        }
    }

    /// Test validation behavior consistency across multiple calls
    #[test]
    fn prop_validation_consistency(manifest in package_manifest_strategy()) {
        let result1 = manifest.validate();
        let result2 = manifest.validate();
        let result3 = manifest.validate();

        prop_assert_eq!(result1.is_ok(), result2.is_ok(),
            "Validation should be consistent across calls");
        prop_assert_eq!(result2.is_ok(), result3.is_ok(),
            "Validation should be consistent across calls");

        // Error messages should also be consistent
        if let (Err(e1), Err(e2)) = (&result1, &result2) {
            prop_assert_eq!(e1, e2, "Error messages should be consistent");
        }
    }

    /// Test that author field parsing handles various email formats
    #[test]
    fn prop_author_field_parsing(
        name in "[A-Za-z ]{3,30}",
        email in prop_oneof![
            "[a-z0-9._%+-]+@[a-z0-9.-]+\\.[a-z]{2,}",
            malformed_email_strategy(),
        ]
    ) {
        let author_string = if email.contains('@') && !email.starts_with('@') && !email.ends_with('@') {
            format!("{} <{}>", name, email)
        } else {
            format!("{} <invalid-email>", name)
        };

        let manifest = PackageManifest::new(
            PackagePath::new("studio/test-package").unwrap(),
            "Test Package".to_string(),
            "1.0.0".to_string(),
            None,
            vec![author_string.clone()],
            None,
        );

        // Authors are free-form: any author string validates and is stored
        // verbatim through a TOML round-trip.
        prop_assert!(manifest.validate().is_ok());
        let back: PackageManifest = toml::from_str(&toml::to_string(&manifest).unwrap()).unwrap();
        prop_assert_eq!(back.package.authors, vec![author_string]);
    }
}

/// The manifest generator reaches every section often enough for the
/// round-trip property to mean something.
#[test]
fn manifest_strategy_covers_every_section() {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;

    let mut runner = TestRunner::deterministic();
    let strategy = package_manifest_strategy();
    let n = 500;
    let mut counts = std::collections::BTreeMap::<&str, usize>::new();
    for _ in 0..n {
        let m = strategy.new_tree(&mut runner).unwrap().current();
        let mut hit = |k| *counts.entry(k).or_default() += 1;
        if m.compat.houdini.is_some() {
            hit("houdini");
        }
        if !m.compat.platforms.is_empty() {
            hit("platforms");
        }
        if !m.stage.platform.is_empty() {
            hit("stage.platform");
        }
        if !m.stage.profile.is_empty() {
            hit("stage.profile");
        }
        if !m.registries.is_empty() {
            hit("registries");
        }
        if m.dependencies.len() >= 2 {
            hit("dependencies>=2");
        }
        if !m.python_dependencies.is_empty() {
            hit("python_dependencies");
        }
        if m.runtime
            .values()
            .any(|e| matches!(e.value, Some(EnvValue::Conditional(_))))
        {
            hit("runtime conditional");
        }
        if m.runtime.values().any(|e| e.value.is_none()) {
            hit("runtime placeholder");
        }
        if m.scripts
            .commands
            .values()
            .any(|e| matches!(e, ScriptEntry::WithEnv(_)))
        {
            hit("scripts table");
        }
        if m.operators
            .iter()
            .any(|o| matches!(o.source, Some(OperatorSource::PerPlatform(_))))
        {
            hit("operators per-platform");
        }
    }
    for (k, c) in &counts {
        assert!(*c * 10 >= n, "{k} generated only {c}/{n} times");
    }
    assert_eq!(counts.len(), 11, "some section never generated: {counts:?}");
}
