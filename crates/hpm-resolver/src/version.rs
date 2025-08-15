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

            // For simplicity, handle other combinations later
            // This is a basic implementation for core functionality
            _ => {
                // For now, if both constraints are not exact, create a range
                // This would need more sophisticated logic in a full implementation
                Some(self.clone())
            }
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
