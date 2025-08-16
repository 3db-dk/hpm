//! Version and version constraint handling
//!
//! This module implements semantic versioning compatible version handling
//! and constraint matching for dependency resolution.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

/// Semantic version following semver specification
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Vec<String>,
    pub build: Vec<String>,
}

impl Version {
    pub fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
            pre: Vec::new(),
            build: Vec::new(),
        }
    }

    pub fn with_pre(mut self, pre: Vec<String>) -> Self {
        self.pre = pre;
        self
    }

    pub fn with_build(mut self, build: Vec<String>) -> Self {
        self.build = build;
        self
    }

    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }

    pub fn is_compatible_with(&self, other: &Version) -> bool {
        if self.major != other.major {
            return false;
        }

        match self.cmp(other) {
            Ordering::Greater | Ordering::Equal => true,
            Ordering::Less => false,
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;

        if !self.pre.is_empty() {
            write!(f, "-{}", self.pre.join("."))?;
        }

        if !self.build.is_empty() {
            write!(f, "+{}", self.build.join("."))?;
        }

        Ok(())
    }
}

impl FromStr for Version {
    type Err = VersionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.is_empty() {
            return Err(VersionError::InvalidFormat(
                "empty version string".to_string(),
            ));
        }

        // Split off build metadata
        let (core_and_pre, build) = if let Some(pos) = s.find('+') {
            let (core, build_part) = s.split_at(pos);
            let build_metadata = build_part[1..].split('.').map(|s| s.to_string()).collect();
            (core, build_metadata)
        } else {
            (s, Vec::new())
        };

        // Split off prerelease
        let (core, pre) = if let Some(pos) = core_and_pre.find('-') {
            let (core_part, pre_part) = core_and_pre.split_at(pos);
            let prerelease = pre_part[1..].split('.').map(|s| s.to_string()).collect();
            (core_part, prerelease)
        } else {
            (core_and_pre, Vec::new())
        };

        // Parse core version numbers
        let parts: Vec<&str> = core.split('.').collect();
        if parts.len() != 3 {
            return Err(VersionError::InvalidFormat(format!(
                "expected major.minor.patch, got: {}",
                core
            )));
        }

        let major = parts[0].parse::<u64>().map_err(|_| {
            VersionError::InvalidFormat(format!("invalid major version: {}", parts[0]))
        })?;
        let minor = parts[1].parse::<u64>().map_err(|_| {
            VersionError::InvalidFormat(format!("invalid minor version: {}", parts[1]))
        })?;
        let patch = parts[2].parse::<u64>().map_err(|_| {
            VersionError::InvalidFormat(format!("invalid patch version: {}", parts[2]))
        })?;

        Ok(Version {
            major,
            minor,
            patch,
            pre,
            build,
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        // Compare core version
        match (
            self.major.cmp(&other.major),
            self.minor.cmp(&other.minor),
            self.patch.cmp(&other.patch),
        ) {
            (Ordering::Equal, Ordering::Equal, Ordering::Equal) => {}
            (Ordering::Equal, Ordering::Equal, patch_cmp) => return patch_cmp,
            (Ordering::Equal, minor_cmp, _) => return minor_cmp,
            (major_cmp, _, _) => return major_cmp,
        }

        // Compare prerelease
        match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater, // No prerelease > prerelease
            (false, true) => Ordering::Less,    // Prerelease < no prerelease
            (false, false) => {
                // Compare prerelease identifiers lexically
                for (a, b) in self.pre.iter().zip(&other.pre) {
                    // Try to parse as numbers first
                    match (a.parse::<u64>(), b.parse::<u64>()) {
                        (Ok(num_a), Ok(num_b)) => match num_a.cmp(&num_b) {
                            Ordering::Equal => continue,
                            other => return other,
                        },
                        (Ok(_), Err(_)) => return Ordering::Less, // Numeric < alpha
                        (Err(_), Ok(_)) => return Ordering::Greater, // Alpha > numeric
                        (Err(_), Err(_)) => match a.cmp(b) {
                            Ordering::Equal => continue,
                            other => return other,
                        },
                    }
                }
                self.pre.len().cmp(&other.pre.len())
            }
        }
    }
}

/// Version constraint specification
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VersionConstraint {
    Exact(Version),
    GreaterThan(Version),
    GreaterThanOrEqual(Version),
    LessThan(Version),
    LessThanOrEqual(Version),
    Compatible(Version), // Caret (^1.2.3)
    Tilde(Version),      // Tilde (~1.2.3)
    Range(VersionRange),
    Any,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionRange {
    pub min: Option<Version>,
    pub max: Option<Version>,
    pub include_min: bool,
    pub include_max: bool,
}

impl VersionConstraint {
    pub fn matches(&self, version: &Version) -> bool {
        match self {
            VersionConstraint::Exact(v) => version == v,
            VersionConstraint::GreaterThan(v) => version > v,
            VersionConstraint::GreaterThanOrEqual(v) => version >= v,
            VersionConstraint::LessThan(v) => version < v,
            VersionConstraint::LessThanOrEqual(v) => version <= v,
            VersionConstraint::Compatible(v) => {
                // Compatible (^): allows patch and minor updates but not major
                version.major == v.major && version >= v
            }
            VersionConstraint::Tilde(v) => {
                // Tilde (~): allows patch updates only
                version.major == v.major && version.minor == v.minor && version >= v
            }
            VersionConstraint::Range(range) => {
                let min_ok = match &range.min {
                    None => true,
                    Some(min) => {
                        if range.include_min {
                            version >= min
                        } else {
                            version > min
                        }
                    }
                };
                let max_ok = match &range.max {
                    None => true,
                    Some(max) => {
                        if range.include_max {
                            version <= max
                        } else {
                            version < max
                        }
                    }
                };
                min_ok && max_ok
            }
            VersionConstraint::Any => true,
        }
    }

    pub fn intersect(&self, other: &VersionConstraint) -> Option<VersionConstraint> {
        match (self, other) {
            (VersionConstraint::Any, other) => Some(other.clone()),
            (other, VersionConstraint::Any) => Some(other.clone()),

            (VersionConstraint::Exact(v1), VersionConstraint::Exact(v2)) => {
                if v1 == v2 {
                    Some(VersionConstraint::Exact(v1.clone()))
                } else {
                    None // No intersection
                }
            }

            (VersionConstraint::Exact(v), constraint)
            | (constraint, VersionConstraint::Exact(v)) => {
                if constraint.matches(v) {
                    Some(VersionConstraint::Exact(v.clone()))
                } else {
                    None
                }
            }

            // Handle range intersections properly
            (VersionConstraint::GreaterThan(v1), VersionConstraint::LessThan(v2))
            | (VersionConstraint::LessThan(v2), VersionConstraint::GreaterThan(v1)) => {
                // >v1 && <v2 -> Range(v1, v2) exclusive
                if v1 < v2 {
                    Some(VersionConstraint::Range(VersionRange {
                        min: Some(v1.clone()),
                        max: Some(v2.clone()),
                        include_min: false,
                        include_max: false,
                    }))
                } else {
                    None // No valid intersection
                }
            }

            (VersionConstraint::GreaterThanOrEqual(v1), VersionConstraint::LessThanOrEqual(v2))
            | (VersionConstraint::LessThanOrEqual(v2), VersionConstraint::GreaterThanOrEqual(v1)) =>
            {
                // >=v1 && <=v2 -> Range(v1, v2) inclusive
                if v1 <= v2 {
                    Some(VersionConstraint::Range(VersionRange {
                        min: Some(v1.clone()),
                        max: Some(v2.clone()),
                        include_min: true,
                        include_max: true,
                    }))
                } else {
                    None // No valid intersection
                }
            }

            (VersionConstraint::GreaterThan(v1), VersionConstraint::LessThanOrEqual(v2))
            | (VersionConstraint::LessThanOrEqual(v2), VersionConstraint::GreaterThan(v1)) => {
                // >v1 && <=v2 -> Range(v1, v2)
                if v1 < v2 {
                    Some(VersionConstraint::Range(VersionRange {
                        min: Some(v1.clone()),
                        max: Some(v2.clone()),
                        include_min: false,
                        include_max: true,
                    }))
                } else {
                    None // No valid intersection
                }
            }

            (VersionConstraint::GreaterThanOrEqual(v1), VersionConstraint::LessThan(v2))
            | (VersionConstraint::LessThan(v2), VersionConstraint::GreaterThanOrEqual(v1)) => {
                // >=v1 && <v2 -> Range(v1, v2)
                if v1 < v2 {
                    Some(VersionConstraint::Range(VersionRange {
                        min: Some(v1.clone()),
                        max: Some(v2.clone()),
                        include_min: true,
                        include_max: false,
                    }))
                } else {
                    None // No valid intersection
                }
            }

            // Same constraint types - take the more restrictive
            (VersionConstraint::GreaterThan(v1), VersionConstraint::GreaterThan(v2))
            | (
                VersionConstraint::GreaterThanOrEqual(v1),
                VersionConstraint::GreaterThanOrEqual(v2),
            ) => Some(if v1 > v2 { self.clone() } else { other.clone() }),

            (VersionConstraint::LessThan(v1), VersionConstraint::LessThan(v2))
            | (VersionConstraint::LessThanOrEqual(v1), VersionConstraint::LessThanOrEqual(v2)) => {
                Some(if v1 < v2 { self.clone() } else { other.clone() })
            }

            // For other combinations, conservative approach - no intersection
            _ => None,
        }
    }

    pub fn is_exact(&self) -> bool {
        matches!(self, VersionConstraint::Exact(_))
    }

    pub fn is_compatible(&self) -> bool {
        matches!(self, VersionConstraint::Compatible(_))
    }

    pub fn is_tilde(&self) -> bool {
        matches!(self, VersionConstraint::Tilde(_))
    }

    pub fn specificity(&self) -> u32 {
        match self {
            VersionConstraint::Exact(_) => 100,
            VersionConstraint::Tilde(_) => 80,
            VersionConstraint::Compatible(_) => 60,
            VersionConstraint::Range(_) => 40,
            VersionConstraint::GreaterThanOrEqual(_) | VersionConstraint::LessThanOrEqual(_) => 20,
            VersionConstraint::GreaterThan(_) | VersionConstraint::LessThan(_) => 10,
            VersionConstraint::Any => 0,
        }
    }
}

impl fmt::Display for VersionConstraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VersionConstraint::Exact(v) => write!(f, "={}", v),
            VersionConstraint::GreaterThan(v) => write!(f, ">{}", v),
            VersionConstraint::GreaterThanOrEqual(v) => write!(f, ">={}", v),
            VersionConstraint::LessThan(v) => write!(f, "<{}", v),
            VersionConstraint::LessThanOrEqual(v) => write!(f, "<={}", v),
            VersionConstraint::Compatible(v) => write!(f, "^{}", v),
            VersionConstraint::Tilde(v) => write!(f, "~{}", v),
            VersionConstraint::Range(range) => match (&range.min, &range.max) {
                (Some(min), Some(max)) => {
                    write!(
                        f,
                        "{}{} {} {}{}",
                        if range.include_min { "[" } else { "(" },
                        min,
                        if range.include_max { "<=" } else { "<" },
                        max,
                        if range.include_max { "]" } else { ")" }
                    )
                }
                (Some(min), None) => write!(f, ">={}", min),
                (None, Some(max)) => write!(f, "<={}", max),
                (None, None) => write!(f, "*"),
            },
            VersionConstraint::Any => write!(f, "*"),
        }
    }
}

impl FromStr for VersionConstraint {
    type Err = VersionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();

        if s == "*" {
            return Ok(VersionConstraint::Any);
        }

        if let Some(version_str) = s.strip_prefix("^") {
            let version = Version::from_str(version_str)?;
            return Ok(VersionConstraint::Compatible(version));
        }

        if let Some(version_str) = s.strip_prefix("~") {
            let version = Version::from_str(version_str)?;
            return Ok(VersionConstraint::Tilde(version));
        }

        if let Some(version_str) = s.strip_prefix(">=") {
            let version = Version::from_str(version_str)?;
            return Ok(VersionConstraint::GreaterThanOrEqual(version));
        }

        if let Some(version_str) = s.strip_prefix("<=") {
            let version = Version::from_str(version_str)?;
            return Ok(VersionConstraint::LessThanOrEqual(version));
        }

        if let Some(version_str) = s.strip_prefix(">") {
            let version = Version::from_str(version_str)?;
            return Ok(VersionConstraint::GreaterThan(version));
        }

        if let Some(version_str) = s.strip_prefix("<") {
            let version = Version::from_str(version_str)?;
            return Ok(VersionConstraint::LessThan(version));
        }

        if let Some(version_str) = s.strip_prefix("=") {
            let version = Version::from_str(version_str)?;
            return Ok(VersionConstraint::Exact(version));
        }

        // Default to exact version
        let version = Version::from_str(s)?;
        Ok(VersionConstraint::Exact(version))
    }
}

#[derive(Debug, Error)]
pub enum VersionError {
    #[error("Invalid version format: {0}")]
    InvalidFormat(String),

    #[error("Invalid version constraint: {0}")]
    InvalidConstraint(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // Custom strategies for generating test data

    /// Strategy to generate valid version components (0-9999 for major/minor, 0-999 for patch)
    fn version_component_strategy() -> impl Strategy<Value = (u64, u64, u64)> {
        (0u64..10000, 0u64..10000, 0u64..1000)
    }

    /// Strategy to generate prerelease identifiers
    fn prerelease_identifier_strategy() -> impl Strategy<Value = String> {
        prop_oneof![
            "[a-z]{1,10}",          // alpha identifiers
            "[0-9]{1,3}",           // numeric identifiers
            "[a-z]{1,5}[0-9]{1,3}", // mixed identifiers (no dots inside)
            Just("alpha".to_string()),
            Just("beta".to_string()),
            Just("rc".to_string()),
            Just("pre".to_string()),
        ]
    }

    /// Strategy to generate build metadata identifiers
    fn build_identifier_strategy() -> impl Strategy<Value = String> {
        prop_oneof![
            "[a-zA-Z0-9]{1,10}",
            Just("build".to_string()),
            Just("snapshot".to_string()),
            Just("dev".to_string()),
        ]
    }

    /// Strategy to generate complete versions with optional prerelease and build metadata
    fn version_strategy() -> impl Strategy<Value = Version> {
        (
            version_component_strategy(),
            prop::option::of(prop::collection::vec(
                prerelease_identifier_strategy(),
                1..4,
            )),
            prop::option::of(prop::collection::vec(build_identifier_strategy(), 1..3)),
        )
            .prop_map(|((major, minor, patch), pre, build)| Version {
                major,
                minor,
                patch,
                pre: pre.unwrap_or_default(),
                build: build.unwrap_or_default(),
            })
    }

    /// Strategy to generate version constraint types
    fn version_constraint_strategy() -> impl Strategy<Value = VersionConstraint> {
        prop_oneof![
            version_strategy().prop_map(VersionConstraint::Exact),
            version_strategy().prop_map(VersionConstraint::GreaterThan),
            version_strategy().prop_map(VersionConstraint::GreaterThanOrEqual),
            version_strategy().prop_map(VersionConstraint::LessThan),
            version_strategy().prop_map(VersionConstraint::LessThanOrEqual),
            version_strategy().prop_map(VersionConstraint::Compatible),
            version_strategy().prop_map(VersionConstraint::Tilde),
            Just(VersionConstraint::Any),
        ]
    }

    /// Strategy to generate version constraint strings
    fn version_constraint_string_strategy() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("*".to_string()),
            version_component_strategy()
                .prop_map(|(major, minor, patch)| format!("{}.{}.{}", major, minor, patch)),
            version_component_strategy()
                .prop_map(|(major, minor, patch)| format!("^{}.{}.{}", major, minor, patch)),
            version_component_strategy()
                .prop_map(|(major, minor, patch)| format!("~{}.{}.{}", major, minor, patch)),
            version_component_strategy()
                .prop_map(|(major, minor, patch)| format!(">={}.{}.{}", major, minor, patch)),
            version_component_strategy()
                .prop_map(|(major, minor, patch)| format!("<={}.{}.{}", major, minor, patch)),
            version_component_strategy()
                .prop_map(|(major, minor, patch)| format!(">{}.{}.{}", major, minor, patch)),
            version_component_strategy()
                .prop_map(|(major, minor, patch)| format!("<{}.{}.{}", major, minor, patch)),
            version_component_strategy()
                .prop_map(|(major, minor, patch)| format!("={}.{}.{}", major, minor, patch)),
        ]
    }

    /// Strategy to generate invalid version strings
    fn invalid_version_string_strategy() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("".to_string()),
            Just("1".to_string()),
            Just("1.2".to_string()),
            Just("1.2.3.4".to_string()),
            Just("a.b.c".to_string()),
            Just("1.b.3".to_string()),
            Just("1.2.c".to_string()),
            r"[^0-9\.\-\+a-zA-Z]{1,10}",
            Just("1.-2.3".to_string()),
            Just("-1.2.3".to_string()),
            Just("1.2.-3".to_string()),
            Just("..".to_string()),
            Just("1..3".to_string()),
        ]
    }

    // Property-based tests

    proptest! {
        /// Test that valid versions can be parsed and formatted consistently
        #[test]
        fn prop_version_roundtrip(version in version_strategy()) {
            let version_str = version.to_string();
            let parsed = Version::from_str(&version_str);

            prop_assert!(parsed.is_ok(), "Failed to parse generated version: {}", version_str);
            let parsed_version = parsed.unwrap();
            prop_assert_eq!(version.major, parsed_version.major);
            prop_assert_eq!(version.minor, parsed_version.minor);
            prop_assert_eq!(version.patch, parsed_version.patch);
            prop_assert_eq!(version.pre, parsed_version.pre);
            prop_assert_eq!(version.build, parsed_version.build);
        }

        /// Test that version comparison is consistent and transitive
        #[test]
        fn prop_version_comparison_transitivity(
            v1 in version_strategy(),
            v2 in version_strategy(),
            v3 in version_strategy()
        ) {
            // Test reflexivity
            prop_assert_eq!(v1.cmp(&v1), Ordering::Equal);

            // Test symmetry
            let cmp_12 = v1.cmp(&v2);
            let cmp_21 = v2.cmp(&v1);
            prop_assert_eq!(cmp_12, cmp_21.reverse());

            // Test transitivity
            if v1 < v2 && v2 < v3 {
                prop_assert!(v1 < v3, "Transitivity violated: {} < {} < {} but {} >= {}", v1, v2, v3, v1, v3);
            }
        }

        /// Test that prerelease versions are always less than normal versions
        #[test]
        fn prop_prerelease_comparison(
            base_version in version_component_strategy(),
            prerelease in prop::collection::vec(prerelease_identifier_strategy(), 1..4)
        ) {
            let (major, minor, patch) = base_version;
            let normal = Version::new(major, minor, patch);
            let pre = Version::new(major, minor, patch).with_pre(prerelease);

            if !pre.pre.is_empty() {
                prop_assert!(pre < normal, "Prerelease {} should be < normal {}", pre, normal);
            }
        }

        /// Test that version constraints can be parsed and serialized consistently
        #[test]
        fn prop_version_constraint_roundtrip(constraint_str in version_constraint_string_strategy()) {
            let parsed = VersionConstraint::from_str(&constraint_str);

            if parsed.is_ok() {
                let constraint = parsed.unwrap();
                let formatted = constraint.to_string();
                let re_parsed = VersionConstraint::from_str(&formatted);

                prop_assert!(re_parsed.is_ok(), "Re-parsing failed for {}", formatted);

                // Both constraints should behave identically
                let test_version = Version::new(1, 2, 3);
                prop_assert_eq!(
                    constraint.matches(&test_version),
                    re_parsed.unwrap().matches(&test_version),
                    "Roundtrip failed for constraint: {}", constraint_str
                );
            }
        }

        /// Test that version constraint matching is consistent with comparison
        #[test]
        fn prop_version_constraint_matching_consistency(
            constraint in version_constraint_strategy(),
            version in version_strategy()
        ) {
            let matches = constraint.matches(&version);

            // Test specific constraint type behaviors
            match &constraint {
                VersionConstraint::Exact(v) => {
                    prop_assert_eq!(matches, version == *v);
                }
                VersionConstraint::GreaterThan(v) => {
                    prop_assert_eq!(matches, version > *v);
                }
                VersionConstraint::GreaterThanOrEqual(v) => {
                    prop_assert_eq!(matches, version >= *v);
                }
                VersionConstraint::LessThan(v) => {
                    prop_assert_eq!(matches, version < *v);
                }
                VersionConstraint::LessThanOrEqual(v) => {
                    prop_assert_eq!(matches, version <= *v);
                }
                VersionConstraint::Compatible(v) => {
                    let expected = version.major == v.major && version >= *v;
                    prop_assert_eq!(matches, expected);
                }
                VersionConstraint::Tilde(v) => {
                    let expected = version.major == v.major && version.minor == v.minor && version >= *v;
                    prop_assert_eq!(matches, expected);
                }
                VersionConstraint::Any => {
                    prop_assert!(matches, "Any constraint should always match");
                }
                VersionConstraint::Range(_) => {
                    // Range matching is more complex, tested separately
                }
            }
        }

        /// Test that constraint intersection is commutative
        #[test]
        fn prop_constraint_intersection_commutative(
            c1 in version_constraint_strategy(),
            c2 in version_constraint_strategy()
        ) {
            let intersection_12 = c1.intersect(&c2);
            let intersection_21 = c2.intersect(&c1);

            match (intersection_12, intersection_21) {
                (Some(i1), Some(i2)) => {
                    // Both intersections should match the same versions
                    let test_versions = vec![
                        Version::new(1, 0, 0),
                        Version::new(1, 2, 3),
                        Version::new(2, 0, 0),
                        Version::new(0, 1, 0),
                    ];

                    for version in test_versions {
                        prop_assert_eq!(
                            i1.matches(&version),
                            i2.matches(&version),
                            "Intersection not commutative for constraints: {} ∩ {} ≠ {} ∩ {}",
                            c1, c2, c2, c1
                        );
                    }
                }
                (None, None) => {
                    // Both indicate no intersection - this is correct
                }
                _ => {
                    prop_assert!(false, "Intersection not commutative: one is None, other is Some");
                }
            }
        }

        /// Test that specificity ordering is consistent
        #[test]
        fn prop_constraint_specificity_ordering(constraint in version_constraint_strategy()) {
            let specificity = constraint.specificity();

            // Test that specificity values are within expected ranges
            prop_assert!(specificity <= 100, "Specificity too high: {}", specificity);

            // Test relative ordering
            let any_constraint = VersionConstraint::Any;
            if !matches!(constraint, VersionConstraint::Any) {
                prop_assert!(
                    constraint.specificity() > any_constraint.specificity(),
                    "Non-Any constraint should be more specific than Any"
                );
            }

            // Exact constraints should be most specific (except ranges can be complex)
            if matches!(constraint, VersionConstraint::Exact(_)) {
                prop_assert_eq!(specificity, 100, "Exact constraints should have maximum specificity");
            }
        }

        /// Test that invalid version strings fail to parse gracefully
        #[test]
        fn prop_invalid_version_parsing(invalid_str in invalid_version_string_strategy()) {
            let result = Version::from_str(&invalid_str);

            // Should either parse successfully (if accidentally valid) or fail with informative error
            if let Err(error) = result {
                let error_msg = error.to_string();
                prop_assert!(
                    !error_msg.is_empty(),
                    "Error message should not be empty for invalid input: '{}'", invalid_str
                );
                prop_assert!(
                    error_msg.to_lowercase().contains("format") ||
                    error_msg.to_lowercase().contains("invalid") ||
                    error_msg.to_lowercase().contains("parse"),
                    "Error message should be descriptive for: '{}'", invalid_str
                );
            }
        }

        /// Test version compatibility rules
        #[test]
        fn prop_version_compatibility(
            base_version in version_strategy(),
            test_version in version_strategy()
        ) {
            let is_compatible = base_version.is_compatible_with(&test_version);

            // Compatibility rules:
            // - Different major versions are incompatible
            if base_version.major != test_version.major {
                prop_assert!(!is_compatible,
                    "Different major versions should be incompatible: {} vs {}",
                    base_version, test_version);
            }

            // - Same major version with base_version >= test_version should be compatible
            if base_version.major == test_version.major && base_version >= test_version {
                prop_assert!(is_compatible,
                    "Same major version with {} >= {} should be compatible",
                    base_version, test_version);
            }
        }

        /// Test that build metadata is ignored in version comparison
        #[test]
        fn prop_build_metadata_ignored_in_comparison(
            base_version in version_component_strategy(),
            pre in prop::option::of(prop::collection::vec(prerelease_identifier_strategy(), 1..3)),
            build1 in prop::collection::vec(build_identifier_strategy(), 1..3),
            build2 in prop::collection::vec(build_identifier_strategy(), 1..3)
        ) {
            let (major, minor, patch) = base_version;
            let version1 = Version {
                major, minor, patch,
                pre: pre.clone().unwrap_or_default(),
                build: build1,
            };
            let version2 = Version {
                major, minor, patch,
                pre: pre.unwrap_or_default(),
                build: build2,
            };

            prop_assert_eq!(
                version1.cmp(&version2),
                Ordering::Equal,
                "Versions with different build metadata should compare as equal: {} vs {}",
                version1, version2
            );
        }
    }

    // Traditional unit tests for edge cases and specific scenarios

    #[test]
    fn test_version_parsing() {
        assert_eq!(Version::from_str("1.2.3").unwrap(), Version::new(1, 2, 3));

        let version_with_pre = Version::from_str("1.2.3-alpha.1").unwrap();
        assert_eq!(version_with_pre.major, 1);
        assert_eq!(version_with_pre.minor, 2);
        assert_eq!(version_with_pre.patch, 3);
        assert_eq!(
            version_with_pre.pre,
            vec!["alpha".to_string(), "1".to_string()]
        );

        let version_with_build = Version::from_str("1.2.3+build.1").unwrap();
        assert_eq!(
            version_with_build.build,
            vec!["build".to_string(), "1".to_string()]
        );
    }

    #[test]
    fn test_version_comparison() {
        let v1 = Version::new(1, 2, 3);
        let v2 = Version::new(1, 2, 4);
        let v3 = Version::new(1, 3, 0);
        let v4 = Version::new(2, 0, 0);

        assert!(v1 < v2);
        assert!(v2 < v3);
        assert!(v3 < v4);

        // Prerelease comparison
        let v_pre = Version::from_str("1.2.3-alpha").unwrap();
        let v_normal = Version::new(1, 2, 3);
        assert!(v_pre < v_normal);
    }

    #[test]
    fn test_constraint_matching() {
        let version = Version::new(1, 2, 3);

        assert!(VersionConstraint::Exact(version.clone()).matches(&version));
        assert!(VersionConstraint::GreaterThanOrEqual(Version::new(1, 2, 0)).matches(&version));
        assert!(VersionConstraint::LessThan(Version::new(1, 2, 4)).matches(&version));
        assert!(VersionConstraint::Compatible(Version::new(1, 2, 0)).matches(&version));
        assert!(VersionConstraint::Tilde(Version::new(1, 2, 0)).matches(&version));
        assert!(VersionConstraint::Any.matches(&version));
    }

    #[test]
    fn test_constraint_parsing() {
        assert!(matches!(
            VersionConstraint::from_str("^1.2.3").unwrap(),
            VersionConstraint::Compatible(_)
        ));

        assert!(matches!(
            VersionConstraint::from_str("~1.2.3").unwrap(),
            VersionConstraint::Tilde(_)
        ));

        assert!(matches!(
            VersionConstraint::from_str(">=1.2.3").unwrap(),
            VersionConstraint::GreaterThanOrEqual(_)
        ));

        assert!(matches!(
            VersionConstraint::from_str("*").unwrap(),
            VersionConstraint::Any
        ));
    }

    #[test]
    fn test_constraint_specificity() {
        assert!(
            VersionConstraint::Exact(Version::new(1, 0, 0)).specificity()
                > VersionConstraint::Compatible(Version::new(1, 0, 0)).specificity()
        );

        assert!(
            VersionConstraint::Compatible(Version::new(1, 0, 0)).specificity()
                > VersionConstraint::Any.specificity()
        );
    }

    #[test]
    fn test_constraint_intersection() {
        let exact = VersionConstraint::Exact(Version::new(1, 2, 3));
        let compatible = VersionConstraint::Compatible(Version::new(1, 2, 0));

        let intersection = exact.intersect(&compatible);
        assert!(matches!(intersection, Some(VersionConstraint::Exact(_))));

        let incompatible = VersionConstraint::Exact(Version::new(2, 0, 0));
        let no_intersection = exact.intersect(&incompatible);
        assert!(no_intersection.is_none());
    }
}
