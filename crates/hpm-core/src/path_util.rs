//! Cross-platform path helpers used by integrity hashes, archive entries,
//! and glob matching — anywhere a path string must look the same on
//! Windows and Unix.

use std::path::Path;

/// Render a relative path with `/` separators, regardless of host OS.
///
/// Used wherever a path string is consumed by something that expects
/// POSIX-style separators: ZIP entry names (APPNOTE 4.4.17.1 mandates `/`),
/// glob patterns from manifests, and content hashes that should match
/// across platforms. On Unix the result matches `to_string_lossy()`; on
/// Windows it normalizes `\` to `/`.
///
/// Non-`Normal` components (root, prefix, `.`, `..`) are silently dropped,
/// so this is only safe for relative paths inside a known tree — pass an
/// absolute or `..`-laden path and you will get a sanitized result, not
/// the original.
pub(crate) fn relative_path_to_forward_slash(relative: &Path) -> String {
    relative
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn relative_path_to_forward_slash_normalizes_separators() {
        let p: PathBuf = ["config", ".gitkeep"].iter().collect();
        assert_eq!(relative_path_to_forward_slash(&p), "config/.gitkeep");

        let nested: PathBuf = ["lib", "windows-x86_64", "foo.dll"].iter().collect();
        assert_eq!(
            relative_path_to_forward_slash(&nested),
            "lib/windows-x86_64/foo.dll"
        );

        let flat = PathBuf::from("hpm.toml");
        assert_eq!(relative_path_to_forward_slash(&flat), "hpm.toml");
    }

    #[test]
    fn relative_path_to_forward_slash_drops_non_normal_components() {
        let p = PathBuf::from("./foo/../bar");
        assert_eq!(relative_path_to_forward_slash(&p), "foo/bar");
    }
}
