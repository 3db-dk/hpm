//! Validated package identifier (`creator/slug`).
//!
//! A `PackagePath` is the canonical identifier of a package in HPM. It is
//! always two ASCII kebab-case segments separated by a single `/` — both
//! `creator` and `slug` are non-empty, contain only `[a-z0-9-]`, and never
//! start or end with `-`. The wire format is the original string ("`a/b`"),
//! so TOML and JSON round-trip identically.
//!
//! Validation runs at deserialization. Any consumer holding a
//! `PackagePath` can call [`creator`] / [`slug`] without an `Option` —
//! the well-formed shape is guaranteed by the type.
//!
//! [`creator`]: PackagePath::creator
//! [`slug`]: PackagePath::slug

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Canonical scoped package identifier in `creator/slug` form.
///
/// Stored as a single string with the `/` index cached so `creator()` and
/// `slug()` are O(1) substring slices.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PackagePath {
    full: String,
    /// Index of the `/` separator inside `full`. Cached at construction.
    sep: usize,
}

/// Parse failure for a [`PackagePath`].
#[derive(Debug, thiserror::Error)]
pub enum PackagePathError {
    #[error("package path cannot be empty")]
    Empty,
    #[error("package path '{0}' must be in 'creator/slug' form (one '/' separator)")]
    Shape(String),
    #[error(
        "'{segment}' in package path '{full}' must be lowercase ASCII alphanumeric \
         or hyphens, with no leading or trailing hyphen"
    )]
    Segment { full: String, segment: String },
}

impl PackagePath {
    /// Parse `input` as a package path. Validates kebab-case on both
    /// segments; returns [`PackagePathError`] otherwise.
    pub fn new(input: impl Into<String>) -> Result<Self, PackagePathError> {
        let full = input.into();
        if full.is_empty() {
            return Err(PackagePathError::Empty);
        }

        // Exactly one '/' splitting two non-empty segments.
        let sep = match full.find('/') {
            Some(i) => i,
            None => return Err(PackagePathError::Shape(full)),
        };
        if full[sep + 1..].contains('/') {
            return Err(PackagePathError::Shape(full));
        }

        let creator = &full[..sep];
        let slug = &full[sep + 1..];
        if !is_valid_segment(creator) {
            let bad = creator.to_string();
            return Err(PackagePathError::Segment { full, segment: bad });
        }
        if !is_valid_segment(slug) {
            let bad = slug.to_string();
            return Err(PackagePathError::Segment { full, segment: bad });
        }

        Ok(Self { full, sep })
    }

    /// The full path, e.g. `"tumblehead/fire-fx"`.
    pub fn as_str(&self) -> &str {
        &self.full
    }

    /// The creator segment, e.g. `"tumblehead"`.
    pub fn creator(&self) -> &str {
        &self.full[..self.sep]
    }

    /// The package slug, e.g. `"fire-fx"`.
    pub fn slug(&self) -> &str {
        &self.full[self.sep + 1..]
    }

    /// Consume `self` and return the underlying owned string.
    pub fn into_string(self) -> String {
        self.full
    }
}

fn is_valid_segment(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && !s.ends_with('-')
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

impl fmt::Display for PackagePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.full)
    }
}

impl AsRef<str> for PackagePath {
    fn as_ref(&self) -> &str {
        &self.full
    }
}

impl PartialEq<str> for PackagePath {
    fn eq(&self, other: &str) -> bool {
        self.full == other
    }
}

impl PartialEq<&str> for PackagePath {
    fn eq(&self, other: &&str) -> bool {
        self.full == *other
    }
}

impl FromStr for PackagePath {
    type Err = PackagePathError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s.to_string())
    }
}

impl TryFrom<String> for PackagePath {
    type Error = PackagePathError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<PackagePath> for String {
    fn from(value: PackagePath) -> Self {
        value.full
    }
}

// Round-trip as a plain string in TOML/JSON — the wire format is the
// original `creator/slug` literal. Deserialization runs full validation,
// so any `PackagePath` held in memory is always well-formed.
impl Serialize for PackagePath {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&self.full)
    }
}

impl<'de> Deserialize<'de> for PackagePath {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s = String::deserialize(de)?;
        Self::new(s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn well_formed_paths_parse() {
        let p = PackagePath::new("tumblehead/fire-fx").unwrap();
        assert_eq!(p.creator(), "tumblehead");
        assert_eq!(p.slug(), "fire-fx");
        assert_eq!(p.as_str(), "tumblehead/fire-fx");
    }

    #[test]
    fn empty_rejected() {
        assert!(matches!(PackagePath::new(""), Err(PackagePathError::Empty)));
    }

    #[test]
    fn missing_separator_rejected() {
        assert!(matches!(
            PackagePath::new("flatname"),
            Err(PackagePathError::Shape(_))
        ));
    }

    #[test]
    fn extra_separator_rejected() {
        assert!(matches!(
            PackagePath::new("a/b/c"),
            Err(PackagePathError::Shape(_))
        ));
    }

    #[test]
    fn empty_segment_rejected() {
        assert!(matches!(
            PackagePath::new("/slug"),
            Err(PackagePathError::Segment { .. })
        ));
        assert!(matches!(
            PackagePath::new("creator/"),
            Err(PackagePathError::Segment { .. })
        ));
    }

    #[test]
    fn uppercase_rejected() {
        assert!(matches!(
            PackagePath::new("Tumblehead/fire-fx"),
            Err(PackagePathError::Segment { .. })
        ));
    }

    #[test]
    fn leading_or_trailing_hyphen_rejected() {
        assert!(PackagePath::new("-foo/bar").is_err());
        assert!(PackagePath::new("foo/-bar").is_err());
        assert!(PackagePath::new("foo-/bar").is_err());
        assert!(PackagePath::new("foo/bar-").is_err());
    }

    #[test]
    fn digits_allowed() {
        assert!(PackagePath::new("creator123/pkg456").is_ok());
    }

    #[test]
    fn deserializes_from_toml_string() {
        #[derive(Deserialize)]
        struct Wrap {
            path: PackagePath,
        }
        let w: Wrap = toml::from_str(r#"path = "creator/slug""#).unwrap();
        assert_eq!(w.path.creator(), "creator");
        assert_eq!(w.path.slug(), "slug");
    }

    #[test]
    fn deserialize_rejects_malformed_toml_string() {
        #[derive(Debug, Deserialize)]
        struct Wrap {
            #[allow(dead_code)]
            path: PackagePath,
        }
        let err = toml::from_str::<Wrap>(r#"path = "Bad/Slug""#).unwrap_err();
        assert!(err.to_string().contains("Bad"));
    }

    #[test]
    fn round_trips_through_toml() {
        #[derive(Serialize, Deserialize)]
        struct Wrap {
            path: PackagePath,
        }
        let original = Wrap {
            path: PackagePath::new("creator/slug").unwrap(),
        };
        let s = toml::to_string(&original).unwrap();
        let back: Wrap = toml::from_str(&s).unwrap();
        assert_eq!(back.path, original.path);
    }
}
