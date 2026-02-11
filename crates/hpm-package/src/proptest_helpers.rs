//! Property-based testing strategies for package types.
//!
//! This module provides proptest strategies for generating test data
//! across the hpm-package crate.

use proptest::prelude::*;

use crate::dependency::DependencySpec;
use crate::houdini::HoudiniEnvValue;
use crate::manifest::{HoudiniConfig, PackageInfo, PackageManifest};
use crate::python::PythonDependencySpec;

/// Strategy to generate valid package names
pub fn package_name_strategy() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-z][a-z0-9-]{1,50}")
        .unwrap()
        .prop_filter("Package name must not end with hyphen", |name| {
            !name.ends_with('-') && name.len() >= 2 && name.len() <= 50
        })
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

/// Strategy to generate Houdini version strings
pub fn houdini_version_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("19.5".to_string()),
        Just("20.0".to_string()),
        Just("20.5".to_string()),
        Just("21.0".to_string()),
    ]
}

/// Strategy to generate dependency specifications
pub fn dependency_spec_strategy() -> impl Strategy<Value = DependencySpec> {
    prop_oneof![
        // Git dependency with version (for release artifact download)
        (url_strategy(), version_strategy(), any::<bool>()).prop_map(|(git, version, optional)| {
            DependencySpec::Git {
                git,
                version,
                optional,
            }
        }),
        // Path dependency
        (
            prop::string::string_regex(r"\.?\./[a-z0-9-/]{3,30}").unwrap(),
            any::<bool>()
        )
            .prop_map(|(path, optional)| DependencySpec::Path { path, optional }),
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

/// Strategy to generate package manifests
pub fn package_manifest_strategy() -> impl Strategy<Value = PackageManifest> {
    (
        package_name_strategy(),
        version_strategy(),
        prop::option::of("[A-Za-z0-9 ]{10,100}"),
        prop::option::of(prop::collection::vec(author_strategy(), 1..4)),
        prop::option::of(license_strategy()),
        prop::option::of(houdini_version_strategy()),
        prop::option::of(houdini_version_strategy()),
    )
        .prop_map(
            |(name, version, description, authors, license, min_houdini, max_houdini)| {
                PackageManifest {
                    package: PackageInfo {
                        name,
                        version,
                        description,
                        authors,
                        license,
                        readme: Some("README.md".to_string()),
                        homepage: None,
                        repository: None,
                        documentation: None,
                        keywords: Some(vec!["houdini".to_string()]),
                        categories: None,
                    },
                    houdini: Some(HoudiniConfig {
                        min_version: min_houdini,
                        max_version: max_houdini,
                    }),
                    dependencies: None,
                    python_dependencies: None,
                    scripts: None,
                }
            },
        )
}

/// Strategy to generate malformed package names
pub fn malformed_package_name_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("".to_string()),      // Empty name
        Just("-".to_string()),     // Just hyphen
        Just("-test".to_string()), // Starts with hyphen
        Just("test-".to_string()), // Ends with hyphen
        r"[A-Z]{5,20}",            // All uppercase
        r"test[_]{1,5}package",    // Contains underscores
        r"test[\s]{1,3}package",   // Contains spaces
        r"test[!@#$%^&*()]{1,3}",  // Special characters
        r"[0-9]{1,10}",            // All numeric
        r"[a-z]{100,200}",         // Too long
        Just("PACKAGE".to_string()),
        Just("Test-Package".to_string()),
        Just("test__package".to_string()),
        Just("test@package".to_string()),
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

// Property-based tests using the strategies above

proptest! {
    /// Test that valid package manifests always pass validation
    #[test]
    fn prop_valid_manifests_pass_validation(manifest in package_manifest_strategy()) {
        prop_assert!(manifest.validate().is_ok());
    }

    /// Test that manifest serialization/deserialization is consistent
    #[test]
    fn prop_manifest_serialization_roundtrip(manifest in package_manifest_strategy()) {
        let toml_str = toml::to_string(&manifest).unwrap();
        let deserialized: PackageManifest = toml::from_str(&toml_str).unwrap();

        prop_assert_eq!(manifest.package.name, deserialized.package.name);
        prop_assert_eq!(manifest.package.version, deserialized.package.version);
        prop_assert_eq!(manifest.package.description, deserialized.package.description);
    }

    /// Test that dependency specifications serialize/deserialize correctly
    #[test]
    fn prop_dependency_spec_roundtrip(spec in dependency_spec_strategy()) {
        let json_str = serde_json::to_string(&spec).unwrap();
        let deserialized: DependencySpec = serde_json::from_str(&json_str).unwrap();

        // Compare serialized forms since the enum variants might differ in structure
        let original_json = serde_json::to_string(&spec).unwrap();
        let roundtrip_json = serde_json::to_string(&deserialized).unwrap();
        prop_assert_eq!(original_json, roundtrip_json);
    }

    /// Test that Python dependency specifications serialize/deserialize correctly
    #[test]
    fn prop_python_dependency_spec_roundtrip(spec in python_dependency_spec_strategy()) {
        let json_str = serde_json::to_string(&spec).unwrap();
        let deserialized: PythonDependencySpec = serde_json::from_str(&json_str).unwrap();

        let original_json = serde_json::to_string(&spec).unwrap();
        let roundtrip_json = serde_json::to_string(&deserialized).unwrap();
        prop_assert_eq!(original_json, roundtrip_json);
    }

    /// Test that Houdini package generation is consistent and valid
    #[test]
    fn prop_houdini_package_generation(manifest in package_manifest_strategy()) {
        let houdini_pkg = manifest.generate_houdini_package();

        // Generated package should always have hpath and env
        prop_assert!(houdini_pkg.hpath.is_some());
        prop_assert!(houdini_pkg.env.is_some());

        // If Houdini config exists with versions, enable should be set
        if let Some(houdini_config) = &manifest.houdini {
            if houdini_config.min_version.is_some() || houdini_config.max_version.is_some() {
                prop_assert!(houdini_pkg.enable.is_some());
            }
        }

        // hpath should contain otls directory
        let hpath = houdini_pkg.hpath.unwrap();
        prop_assert!(hpath.iter().any(|path| path.contains("otls")));

        // env should contain PYTHONPATH and HOUDINI_SCRIPT_PATH
        let env = houdini_pkg.env.unwrap();
        let has_python_path = env.iter().any(|env_map| env_map.contains_key("PYTHONPATH"));
        let has_script_path = env.iter().any(|env_map| env_map.contains_key("HOUDINI_SCRIPT_PATH"));
        prop_assert!(has_python_path);
        prop_assert!(has_script_path);
    }

    /// Test that package name validation works correctly
    #[test]
    fn prop_package_name_validation(
        valid_name in package_name_strategy(),
        invalid_chars in r"[A-Z_]{1,5}"
    ) {
        // Valid names should pass validation
        let valid_manifest = PackageManifest::new(
            valid_name,
            "1.0.0".to_string(),
            None,
            None,
            None,
        );
        prop_assert!(valid_manifest.validate().is_ok());

        // Names with invalid characters should fail validation
        let invalid_manifest = PackageManifest::new(
            format!("test{}", invalid_chars),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );
        prop_assert!(invalid_manifest.validate().is_err());
    }

    /// Test that semver validation works correctly
    #[test]
    fn prop_semver_validation(
        valid_version in version_strategy(),
        invalid_version in r"[a-zA-Z]{3,10}|[0-9]+\\.[0-9]+|[0-9]+"
    ) {
        // Valid semver should pass validation
        let valid_manifest = PackageManifest::new(
            "test-package".to_string(),
            valid_version,
            None,
            None,
            None,
        );
        prop_assert!(valid_manifest.validate().is_ok());

        // Invalid versions should fail (unless they accidentally match semver)
        let invalid_manifest = PackageManifest::new(
            "test-package".to_string(),
            invalid_version.clone(),
            None,
            None,
            None,
        );

        // Only assert failure if it's clearly not semver
        if !invalid_version.chars().all(|c| c.is_ascii_digit() || c == '.') ||
           invalid_version.split('.').count() != 3 {
            prop_assert!(invalid_manifest.validate().is_err());
        }
    }

    /// Test that malformed package names are rejected consistently
    #[test]
    fn prop_malformed_package_names_rejected(malformed_name in malformed_package_name_strategy()) {
        let manifest = PackageManifest::new(
            malformed_name.clone(),
            "1.0.0".to_string(),
            None,
            None,
            None,
        );

        let result = manifest.validate();

        // Most malformed names should fail validation
        if malformed_name.is_empty() ||
           malformed_name.starts_with('-') ||
           malformed_name.ends_with('-') ||
           malformed_name.contains(' ') ||
           malformed_name.contains('_') ||
           malformed_name.chars().any(|c| c.is_uppercase()) ||
           malformed_name.chars().any(|c| !c.is_alphanumeric() && c != '-') {
            prop_assert!(result.is_err(),
                "Malformed package name '{}' should fail validation", malformed_name);
        }
    }

    /// Test that malformed versions are rejected consistently
    #[test]
    fn prop_malformed_versions_rejected(malformed_version in malformed_version_strategy()) {
        let manifest = PackageManifest::new(
            "test-package".to_string(),
            malformed_version.clone(),
            None,
            None,
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
        // Test Git dependency spec
        let git_spec = DependencySpec::Git {
            git: git_url.clone(),
            version: "1.0.0".to_string(),
            optional: true,
        };
        let git_json = serde_json::to_string(&git_spec);
        prop_assert!(git_json.is_ok(), "Git spec serialization should always work");

        // Test Path dependency spec
        let path_spec = DependencySpec::Path {
            path: path.clone(),
            optional: false,
        };
        let path_json = serde_json::to_string(&path_spec);
        prop_assert!(path_json.is_ok(), "Path spec serialization should always work");
    }

    /// Test that Houdini package generation handles missing/invalid data
    #[test]
    fn prop_houdini_package_generation_robustness(
        name in prop_oneof![package_name_strategy(), malformed_package_name_strategy()],
        version in prop_oneof![version_strategy(), malformed_version_strategy()],
        min_version in prop::option::of(houdini_version_strategy()),
        max_version in prop::option::of(houdini_version_strategy())
    ) {
        let manifest = PackageManifest {
            package: PackageInfo {
                name,
                version,
                description: None,
                authors: None,
                license: None,
                readme: None,
                homepage: None,
                repository: None,
                documentation: None,
                keywords: None,
                categories: None,
            },
            houdini: Some(HoudiniConfig {
                min_version,
                max_version,
            }),
            dependencies: None,
            python_dependencies: None,
            scripts: None,
        };

        // Houdini package generation should never panic, even with bad input
        let houdini_pkg = manifest.generate_houdini_package();

        // Should always generate basic structure
        prop_assert!(houdini_pkg.hpath.is_some());
        prop_assert!(houdini_pkg.env.is_some());

        // Environment variables should be properly formatted
        if let Some(env) = &houdini_pkg.env {
            for env_map in env {
                for (key, value) in env_map {
                    prop_assert!(!key.is_empty(), "Environment variable key should not be empty");
                    match value {
                        HoudiniEnvValue::Simple(s) => {
                            prop_assert!(!s.is_empty(), "Simple env value should not be empty");
                        }
                        HoudiniEnvValue::Detailed { method, value } => {
                            prop_assert!(!method.is_empty(), "Env method should not be empty");
                            prop_assert!(!value.is_empty(), "Detailed env value should not be empty");
                        }
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
            "test-package".to_string(),
            "1.0.0".to_string(),
            None,
            Some(vec![author_string.clone()]),
            None,
        );

        // Validation should not fail due to author format (it's not strictly validated)
        // but serialization should work
        let toml_result = toml::to_string(&manifest);
        prop_assert!(toml_result.is_ok(),
            "Manifest with author '{}' should serialize", author_string);
    }
}
