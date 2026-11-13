//! Cross-platform path helpers used by integrity hashes, archive entries,
//! and glob matching — anywhere a path string must look the same on
//! Windows and Unix.

use std::path::{Path, PathBuf};

/// Locate the user's home directory via the platform's canonical env var
/// (`HOME` on Unix, `USERPROFILE` on Windows). Returns `None` when the
/// variable is unset.
///
/// Sidesteps the `dirs` / `home` crates so the workspace doesn't grow a
/// supply-chain dependency for a one-line lookup. Multiple crates need
/// this (`hpm-config` for the user config directory, `hpm_core::python`
/// for `~/.hpm/venvs/`), so it lives in `hpm-package` as the lowest
/// shared layer.
pub fn user_home() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

/// A path that does not stay inside the directory it is meant for.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "'{0}' is not a relative path inside the target directory \
     (no root, drive, '..', '\\', ':' or NUL)"
)]
pub struct UnconfinedPath(pub String);

/// The relative path `path` names inside a target directory, built only from
/// plain name components; an empty result names the directory itself.
///
/// This is the single rule for every path hpm writes on someone else's
/// behalf: archive entries on extraction, and `[stage]` place targets in
/// dev copies, `hpm build` output and packed archives. `.` components are
/// dropped (so `./`-prefixed tar members and `to = "./"` work). A root, a
/// drive or UNC prefix, a `..` component, and a backslash, colon or NUL
/// anywhere are refused, on every platform: on Windows those spell a
/// separator, a drive or an alternate data stream, and refusing them
/// everywhere means one input yields one tree on every host.
pub fn confined_relative_path(path: &Path) -> Result<PathBuf, UnconfinedPath> {
    use std::path::Component;
    let refuse = || UnconfinedPath(path.display().to_string());
    if path.to_string_lossy().contains(['\\', ':', '\0']) {
        return Err(refuse());
    }
    let mut relative = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(name) => relative.push(name),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(refuse());
            }
        }
    }
    Ok(relative)
}

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
pub fn relative_path_to_forward_slash(relative: &Path) -> String {
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

    mod properties {
        use super::*;
        use proptest::prelude::*;
        use std::path::Component;

        /// Paths made of the pieces that decide confinement: separators of
        /// both kinds, roots, drives, `..`, `.`, colons, NUL and names.
        fn hostile_path() -> impl Strategy<Value = String> {
            let piece = prop::sample::select(vec![
                "a",
                "b.txt",
                ".hidden",
                "..",
                ".",
                "/",
                "\\",
                "C:",
                "c:\\",
                "\\\\?\\",
                "//srv/share",
                ":",
                "\0",
                "x..y",
                "...",
                " ",
                "~",
            ]);
            prop_oneof![
                3 => prop::collection::vec(piece, 0..8).prop_map(|p| p.concat()),
                1 => any::<String>(),
            ]
        }

        fn plain_segment() -> impl Strategy<Value = String> {
            "[A-Za-z0-9_][A-Za-z0-9_. -]{0,8}"
                .prop_filter("not a dot name", |s| s != "." && s != ".." && s.trim() == s)
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(1024))]

            /// An accepted path is only plain names, so joined to any
            /// directory it stays inside it, on every platform.
            #[test]
            fn accepted_paths_are_plain_names(raw in hostile_path()) {
                if let Ok(relative) = confined_relative_path(Path::new(&raw)) {
                    prop_assert!(
                        relative.components().all(|c| matches!(c, Component::Normal(_))),
                        "{:?} -> {:?}", raw, relative
                    );
                    prop_assert!(!relative.to_string_lossy().contains(['\\', ':', '\0']));
                    let root = Path::new("/target/dir");
                    prop_assert!(root.join(&relative).starts_with(root));
                    // Applying the rule again, to the `/` form a writer
                    // records, changes nothing.
                    let rendered = relative_path_to_forward_slash(&relative);
                    prop_assert_eq!(confined_relative_path(Path::new(&rendered)).unwrap(), relative);
                } else {
                    prop_assert!(
                        raw.contains(['\\', ':', '\0'])
                            || Path::new(&raw).components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)),
                        "{:?} refused", raw
                    );
                }
            }

            /// A relative path of plain names, with `./` anywhere, is
            /// accepted and keeps its names in order.
            #[test]
            fn plain_relative_paths_are_kept(
                segments in prop::collection::vec((plain_segment(), any::<bool>()), 0..6),
                trailing in any::<bool>(),
            ) {
                let mut raw = String::new();
                for (segment, dot) in &segments {
                    if *dot {
                        raw.push_str("./");
                    }
                    raw.push_str(segment);
                    raw.push('/');
                }
                if !trailing {
                    raw.pop();
                }
                let expected: PathBuf = segments.iter().map(|(s, _)| s.as_str()).collect();
                prop_assert_eq!(confined_relative_path(Path::new(&raw)).unwrap(), expected);
            }
        }
    }
}
