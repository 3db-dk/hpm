//! PEP 503 distribution name normalization.
//!
//! Python wheel installers canonicalize distribution names before laying
//! down `*.dist-info/` directories: lowercase, with runs of `-`, `_`, or
//! `.` collapsed to a single underscore. Any time HPM compares a package
//! name we collected from a requirement string (e.g. `Foo-Bar==1.0`) to a
//! directory it finds on disk (e.g. `foo_bar-1.0.dist-info`), both sides
//! must go through this normalizer.

/// Canonicalize a Python distribution name per PEP 503.
///
/// Lowercases the input and folds `-` / `.` to `_`. Empty input is
/// preserved as empty.
pub fn normalize(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c == '-' || c == '.' {
            out.push('_');
        } else {
            out.push(c.to_ascii_lowercase());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowercases() {
        assert_eq!(normalize("NumPy"), "numpy");
        assert_eq!(normalize("REQUESTS"), "requests");
    }

    #[test]
    fn folds_separators() {
        assert_eq!(normalize("Foo-Bar"), "foo_bar");
        assert_eq!(normalize("foo.bar"), "foo_bar");
        assert_eq!(normalize("foo_bar"), "foo_bar");
    }

    #[test]
    fn already_canonical_is_identity() {
        assert_eq!(normalize("requests"), "requests");
        assert_eq!(normalize("foo_bar_baz"), "foo_bar_baz");
    }

    #[test]
    fn empty_stays_empty() {
        assert_eq!(normalize(""), "");
    }
}
