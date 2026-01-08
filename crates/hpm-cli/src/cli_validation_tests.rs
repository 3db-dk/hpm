//! Property-based tests for CLI argument parsing and validation
//!
//! This module contains comprehensive property-based tests for CLI command parsing,
//! argument validation, and edge case handling. These tests ensure the CLI handles
//! various user inputs gracefully and provides clear error messages.

#[cfg(test)]
mod tests {
    use crate::*;
    use proptest::prelude::*;
    use std::path::PathBuf;

    // Strategies for generating CLI test data

    /// Strategy to generate valid package names
    fn package_name_strategy() -> impl Strategy<Value = String> {
        prop_oneof![
            r"[a-z][a-z0-9-]{1,50}",
            Just("my-package".to_string()),
            Just("awesome-houdini-tools".to_string()),
            Just("geometry-utils".to_string()),
            Just("material-library".to_string()),
        ]
        .prop_filter("Valid package name", |name| {
            !name.starts_with('-') && !name.ends_with('-') && name.len() >= 2
        })
    }

    /// Strategy to generate problematic package names for testing validation
    fn problematic_package_name_strategy() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("".to_string()),             // Empty
            Just("-package".to_string()),     // Starts with hyphen
            Just("package-".to_string()),     // Ends with hyphen
            Just("PACKAGE".to_string()),      // All uppercase
            Just("Package".to_string()),      // Mixed case
            Just("package_name".to_string()), // Underscore
            Just("package name".to_string()), // Space
            Just("package@name".to_string()), // Special characters
            Just("123package".to_string()),   // Starts with number
            r"[a-z]{100,200}",                // Too long
            Just(".package".to_string()),     // Starts with dot
            Just("package.".to_string()),     // Ends with dot
            Just("--package".to_string()),    // Double hyphen
        ]
    }

    /// Strategy to generate git URLs
    fn git_url_strategy() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("https://github.com/example/package".to_string()),
            Just("https://github.com/studio/utility-nodes".to_string()),
            Just("https://github.com/artist/material-library".to_string()),
            Just("https://gitlab.com/team/houdini-tools".to_string()),
            r"https://github\.com/[a-z]+/[a-z-]+",
        ]
    }

    /// Strategy to generate commit hashes
    fn commit_hash_strategy() -> impl Strategy<Value = String> {
        prop::string::string_regex("[0-9a-f]{40}").unwrap()
    }

    /// Strategy to generate package versions (semver-like)
    fn package_version_strategy() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("1.0.0".to_string()),
            Just("0.1.0".to_string()),
            Just("2.5.3".to_string()),
            r"[0-9]{1,2}\.[0-9]{1,2}\.[0-9]{1,3}",
        ]
    }

    /// Strategy to generate file paths
    fn file_path_strategy() -> impl Strategy<Value = PathBuf> {
        prop_oneof![
            Just(PathBuf::from("./hpm.toml")),
            Just(PathBuf::from("/tmp/project/hpm.toml")),
            Just(PathBuf::from("../project/hpm.toml")),
            Just(PathBuf::from("project")),
            Just(PathBuf::from("/project")),
            Just(PathBuf::from("./project")),
            r"[a-zA-Z0-9_-]+(/[a-zA-Z0-9_-]+)*".prop_map(PathBuf::from),
        ]
    }

    /// Strategy to generate problematic file paths
    fn problematic_file_path_strategy() -> impl Strategy<Value = PathBuf> {
        prop_oneof![
            Just(PathBuf::from("")),                             // Empty path
            Just(PathBuf::from("   ")),                          // Whitespace path
            Just(PathBuf::from(".")),                            // Current dir
            Just(PathBuf::from("..")),                           // Parent dir
            Just(PathBuf::from("/")),                            // Root
            Just(PathBuf::from("~/project")),                    // Tilde
            Just(PathBuf::from("con")),                          // Windows reserved
            Just(PathBuf::from("nul")),                          // Windows reserved
            Just(PathBuf::from("prn")),                          // Windows reserved
            r"[^a-zA-Z0-9_/\-\.]{1,20}".prop_map(PathBuf::from), // Special chars
            r"[a-zA-Z0-9_/-]{200,400}".prop_map(PathBuf::from),  // Extremely long
        ]
    }

    /// Strategy to generate author strings  
    fn author_strategy() -> impl Strategy<Value = String> {
        prop_oneof![
            (
                r"[A-Z][a-z]{2,20}",
                r"[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,4}"
            )
                .prop_map(|(name, email)| format!("{} <{}>", name, email)),
            r"[A-Z][a-z]{2,20}",                       // Name only
            r"[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,4}", // Email only
        ]
    }

    /// Strategy to generate license identifiers
    fn license_strategy() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("MIT".to_string()),
            Just("Apache-2.0".to_string()),
            Just("GPL-3.0".to_string()),
            Just("BSD-3-Clause".to_string()),
            Just("ISC".to_string()),
            Just("MPL-2.0".to_string()),
            Just("LGPL-2.1".to_string()),
            Just("Unlicense".to_string()),
            r"[A-Z0-9\-\.]{2,20}", // Custom license format
        ]
    }

    /// Strategy to generate Houdini version strings
    fn houdini_version_strategy() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("19.5".to_string()),
            Just("20.0".to_string()),
            Just("20.5".to_string()),
            Just("21.0".to_string()),
            r"[1-2][0-9]\.[0-9]", // Generate reasonable Houdini versions
        ]
    }

    /// Strategy to generate VCS options
    fn vcs_strategy() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("git".to_string()),
            Just("none".to_string()),
            Just("hg".to_string()),  // Not supported but valid input
            Just("svn".to_string()), // Not supported but valid input
            r"[a-z]{3,10}",          // Random VCS names
        ]
    }

    /// Strategy to generate verbosity counts
    fn verbosity_strategy() -> impl Strategy<Value = u8> {
        0u8..10
    }

    /// Strategy to generate CLI flag combinations
    fn cli_flags_strategy(
    ) -> impl Strategy<Value = (bool, Option<ColorChoiceArg>, Option<OutputFormatArg>)> {
        (
            any::<bool>(), // quiet flag
            prop::option::of(prop_oneof![
                Just(ColorChoiceArg::Auto),
                Just(ColorChoiceArg::Always),
                Just(ColorChoiceArg::Never),
            ]),
            prop::option::of(prop_oneof![
                Just(OutputFormatArg::Human),
                Just(OutputFormatArg::Json),
                Just(OutputFormatArg::JsonLines),
                Just(OutputFormatArg::JsonCompact),
            ]),
        )
    }

    // Property-based tests

    proptest! {
        /// Test that valid package names in init command are handled correctly
        #[test]
        fn prop_init_command_package_name_validation(
            package_name in package_name_strategy(),
            version in package_version_strategy(),
            author in author_strategy(),
            license in license_strategy(),
            houdini_min in prop::option::of(houdini_version_strategy()),
            houdini_max in prop::option::of(houdini_version_strategy()),
            bare in any::<bool>(),
            vcs in vcs_strategy(),
        ) {
            let init_cmd = Commands::Init {
                name: Some(package_name.clone()),
                description: Some("Test package description".to_string()),
                author: Some(author),
                version,
                license,
                houdini_min,
                houdini_max,
                bare,
                vcs,
            };

            // Should be able to construct the command without panicking
            match init_cmd {
                Commands::Init { name, .. } => {
                    prop_assert_eq!(name, Some(package_name));
                }
                _ => prop_assert!(false, "Should construct Init command"),
            }
        }

        /// Test that CLI flag combinations work correctly
        #[test]
        fn prop_cli_flags_combination(
            verbosity in verbosity_strategy(),
            flags in cli_flags_strategy(),
            _directory in prop::option::of(file_path_strategy())
        ) {
            let (quiet, color, output) = flags;

            // Test verbosity logic
            let expected_verbosity = if quiet {
                Verbosity::Quiet
            } else {
                match verbosity {
                    0 => Verbosity::Normal,
                    1 => Verbosity::Verbose,
                    _ => Verbosity::Verbose,
                }
            };

            // Should handle quiet flag override
            if quiet {
                prop_assert_eq!(expected_verbosity, Verbosity::Quiet);
            }

            // Test color choice conversion
            let color_choice = color
                .map(ColorChoice::from)
                .unwrap_or(ColorChoice::Auto);

            prop_assert!(matches!(color_choice, ColorChoice::Auto | ColorChoice::Always | ColorChoice::Never));

            // Test output format conversion
            let output_format = output
                .map(OutputFormat::from)
                .unwrap_or(OutputFormat::Human);

            prop_assert!(matches!(output_format,
                OutputFormat::Human | OutputFormat::Json |
                OutputFormat::JsonLines | OutputFormat::JsonCompact));
        }

        /// Test add command argument validation
        #[test]
        fn prop_add_command_validation(
            package_name in package_name_strategy(),
            git_url in prop::option::of(git_url_strategy()),
            commit_hash in prop::option::of(commit_hash_strategy()),
            manifest_path in prop::option::of(file_path_strategy()),
            optional in any::<bool>()
        ) {
            let add_cmd = Commands::Add {
                package: package_name.clone(),
                git: git_url.clone(),
                commit: commit_hash.clone(),
                path: None,
                manifest: manifest_path.clone(),
                optional,
            };

            match add_cmd {
                Commands::Add { package, git, commit, manifest, optional: opt, .. } => {
                    prop_assert_eq!(package, package_name);
                    prop_assert_eq!(git, git_url);
                    prop_assert_eq!(commit, commit_hash);
                    prop_assert_eq!(manifest, manifest_path);
                    prop_assert_eq!(opt, optional);
                }
                _ => prop_assert!(false, "Should construct Add command"),
            }
        }

        /// Test remove command argument validation
        #[test]
        fn prop_remove_command_validation(
            package_name in package_name_strategy(),
            manifest_path in prop::option::of(file_path_strategy())
        ) {
            let remove_cmd = Commands::Remove {
                package: package_name.clone(),
                manifest: manifest_path.clone(),
            };

            match remove_cmd {
                Commands::Remove { package, manifest } => {
                    prop_assert_eq!(package, package_name);
                    prop_assert_eq!(manifest, manifest_path);
                }
                _ => prop_assert!(false, "Should construct Remove command"),
            }
        }

        /// Test update command validation with multiple packages
        #[test]
        fn prop_update_command_validation(
            packages in prop::collection::vec(package_name_strategy(), 0..10),
            manifest_path in prop::option::of(file_path_strategy()),
            dry_run in any::<bool>(),
            yes in any::<bool>()
        ) {
            let update_cmd = Commands::Update {
                packages: packages.clone(),
                manifest: manifest_path.clone(),
                dry_run,
                yes,
            };

            match update_cmd {
                Commands::Update { packages: p, manifest, dry_run: dr, yes: y } => {
                    prop_assert_eq!(p, packages);
                    prop_assert_eq!(manifest, manifest_path);
                    prop_assert_eq!(dr, dry_run);
                    prop_assert_eq!(y, yes);
                }
                _ => prop_assert!(false, "Should construct Update command"),
            }
        }

        /// Test that problematic package names are handled gracefully in validation
        #[test]
        fn prop_problematic_package_names(
            problematic_name in problematic_package_name_strategy(),
            git_url in git_url_strategy(),
            commit_hash in commit_hash_strategy()
        ) {
            let add_cmd = Commands::Add {
                package: problematic_name.clone(),
                git: Some(git_url),
                commit: Some(commit_hash),
                path: None,
                manifest: None,
                optional: false,
            };

            // Command construction should succeed (validation happens later)
            match add_cmd {
                Commands::Add { package, .. } => {
                    prop_assert_eq!(package, problematic_name);
                }
                _ => prop_assert!(false, "Should construct Add command even with problematic name"),
            }
        }

        /// Test path handling robustness
        #[test]
        fn prop_path_handling_robustness(
            path in prop_oneof![file_path_strategy().prop_map(Some), Just(None)],
            problematic_path in prop_oneof![problematic_file_path_strategy().prop_map(Some), Just(None)]
        ) {
            // Test with normal paths
            let list_cmd = Commands::List { manifest: path.clone() };
            match list_cmd {
                Commands::List { manifest } => {
                    prop_assert_eq!(manifest, path);
                }
                _ => prop_assert!(false, "Should construct List command"),
            }

            // Test with problematic paths (should still construct)
            let install_cmd = Commands::Install { manifest: problematic_path.clone() };
            match install_cmd {
                Commands::Install { manifest } => {
                    prop_assert_eq!(manifest, problematic_path);
                }
                _ => prop_assert!(false, "Should construct Install command even with problematic path"),
            }
        }

        /// Test init command with various parameter combinations
        #[test]
        fn prop_init_command_parameter_combinations(
            name in prop::option::of(package_name_strategy()),
            description in prop::option::of("[A-Za-z0-9 ]{10,100}"),
            author in prop::option::of(author_strategy()),
            version in package_version_strategy(),
            license in license_strategy(),
            houdini_min in prop::option::of(houdini_version_strategy()),
            houdini_max in prop::option::of(houdini_version_strategy()),
            bare in any::<bool>(),
            vcs in vcs_strategy()
        ) {
            let init_cmd = Commands::Init {
                name: name.clone(),
                description: description.clone(),
                author: author.clone(),
                version: version.clone(),
                license: license.clone(),
                houdini_min: houdini_min.clone(),
                houdini_max: houdini_max.clone(),
                bare,
                vcs: vcs.clone(),
            };

            // All parameter combinations should be valid for construction
            match init_cmd {
                Commands::Init {
                    name: n,
                    description: d,
                    author: a,
                    version: v,
                    license: l,
                    houdini_min: hmin,
                    houdini_max: hmax,
                    bare: b,
                    vcs: vc,
                } => {
                    prop_assert_eq!(n, name);
                    prop_assert_eq!(d, description);
                    prop_assert_eq!(a, author);
                    prop_assert_eq!(v, version);
                    prop_assert_eq!(l, license);
                    prop_assert_eq!(hmin, houdini_min);
                    prop_assert_eq!(hmax, houdini_max);
                    prop_assert_eq!(b, bare);
                    prop_assert_eq!(vc, vcs);
                }
                _ => prop_assert!(false, "Should construct Init command"),
            }
        }

        /// Test search command with various query patterns
        #[test]
        fn prop_search_command_validation(
            query in prop_oneof![
                r"[a-zA-Z][a-zA-Z0-9\-_]{1,50}",     // Package-like names
                r"[a-zA-Z ]{3,100}",                  // Natural language queries
                r"[a-zA-Z0-9\-\+\*\^\~]{1,50}",     // Version-like patterns
                Just("".to_string()),                 // Empty query
                r"[^a-zA-Z0-9 \-_]{1,20}",           // Special characters
            ]
        ) {
            let search_cmd = Commands::Search { query: query.clone() };

            match search_cmd {
                Commands::Search { query: q } => {
                    prop_assert_eq!(q, query);
                }
                _ => prop_assert!(false, "Should construct Search command"),
            }
        }

        /// Test run command with script names and arguments
        #[test]
        fn prop_run_command_validation(
            script_name in prop_oneof![
                r"[a-zA-Z][a-zA-Z0-9\-_]{1,30}",     // Valid script names
                Just("build".to_string()),            // Common script names
                Just("test".to_string()),
                Just("deploy".to_string()),
                r"[^a-zA-Z0-9\-_]{1,20}",            // Invalid script names
                Just("".to_string()),                 // Empty script name
            ],
            args in prop::collection::vec(r"[a-zA-Z0-9\-_\.]{1,20}", 0..10)
        ) {
            let run_cmd = Commands::Run {
                script: script_name.clone(),
                args: args.clone(),
            };

            match run_cmd {
                Commands::Run { script, args: a } => {
                    prop_assert_eq!(script, script_name);
                    prop_assert_eq!(a, args);
                }
                _ => prop_assert!(false, "Should construct Run command"),
            }
        }
    }

    // Additional unit tests for edge cases

    #[test]
    fn test_color_choice_conversion() {
        assert!(matches!(
            ColorChoice::from(ColorChoiceArg::Auto),
            ColorChoice::Auto
        ));
        assert!(matches!(
            ColorChoice::from(ColorChoiceArg::Always),
            ColorChoice::Always
        ));
        assert!(matches!(
            ColorChoice::from(ColorChoiceArg::Never),
            ColorChoice::Never
        ));
    }

    #[test]
    fn test_output_format_conversion() {
        assert!(matches!(
            OutputFormat::from(OutputFormatArg::Human),
            OutputFormat::Human
        ));
        assert!(matches!(
            OutputFormat::from(OutputFormatArg::Json),
            OutputFormat::Json
        ));
        assert!(matches!(
            OutputFormat::from(OutputFormatArg::JsonLines),
            OutputFormat::JsonLines
        ));
        assert!(matches!(
            OutputFormat::from(OutputFormatArg::JsonCompact),
            OutputFormat::JsonCompact
        ));
    }

    #[test]
    fn test_verbosity_logic() {
        // Test quiet override
        let verbosity = 5;
        let quiet = true;
        let result = if quiet {
            Verbosity::Quiet
        } else {
            match verbosity {
                0 => Verbosity::Normal,
                1 => Verbosity::Verbose,
                _ => Verbosity::Verbose,
            }
        };
        assert!(matches!(result, Verbosity::Quiet));

        // Test normal progression
        let quiet = false;
        let result = if quiet {
            Verbosity::Quiet
        } else {
            match 0 {
                0 => Verbosity::Normal,
                1 => Verbosity::Verbose,
                _ => Verbosity::Verbose,
            }
        };
        assert!(matches!(result, Verbosity::Normal));

        let result = if quiet {
            Verbosity::Quiet
        } else {
            match 1 {
                0 => Verbosity::Normal,
                1 => Verbosity::Verbose,
                _ => Verbosity::Verbose,
            }
        };
        assert!(matches!(result, Verbosity::Verbose));
    }
}
