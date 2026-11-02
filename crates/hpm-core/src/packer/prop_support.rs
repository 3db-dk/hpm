//! Proptest generators for workspace trees and ignore rules, shared by the
//! staging-walk properties (`packer::tests`) and the dev-copy properties
//! (`storage::dev_install::tests`).
//!
//! The vocabularies are deliberately small, so random draws often collide on
//! the same directory names. That puts ignore rules, `!` re-includes and
//! `[stage]` globs on the same paths, where the interesting cases are.

use proptest::prelude::*;
use std::collections::BTreeSet;
use std::path::Path;

/// Directory names, including the ones real workspaces ignore wholesale.
const DIRS: &[&str] = &["a", "build", "target", "src", ".git", "resolver", "dso"];
/// File names. Disjoint from [`DIRS`], so a path can never be both a file and
/// a directory.
const FILES: &[&str] = &["f.txt", "x.log", "lib.so", "keep.txt"];

/// gitignore-style lines: directory-only, anchored, `**`, wildcards, and
/// negations, both ones that can take effect and ones that can't (a file
/// re-included under an excluded parent).
const IGNORE_LINES: &[&str] = &[
    "build/",
    "target/",
    "**/target/",
    "src/",
    "resolver/",
    "dso",
    "/a",
    "*.log",
    "build/*",
    "a/**/lib.so",
    "!keep.txt",
    "!build/",
    "!build/**",
    "!resolver/",
    "!resolver/**",
    "!dso/keep.txt",
    "!*.so",
    "!a/",
];

/// `[stage]` include globs.
pub(crate) const STAGE_INCLUDES: &[&str] = &[
    "**/*.so",
    "a/**",
    "resolver/**",
    "*.txt",
    "build/**",
    "dso/**",
];
/// `[stage]` exclude globs.
pub(crate) const STAGE_EXCLUDES: &[&str] = &["**/*.log", "a/a/**", "**/keep.txt"];

/// A random set of workspace-relative file paths (forward slashes), 1–4
/// segments deep, never `hpm.toml`.
pub(crate) fn file_tree() -> impl Strategy<Value = BTreeSet<String>> {
    let path = (
        proptest::collection::vec(proptest::sample::select(DIRS), 0..4),
        proptest::sample::select(FILES),
    )
        .prop_map(|(dirs, file)| {
            let mut parts: Vec<&str> = dirs;
            parts.push(file);
            parts.join("/")
        });
    proptest::collection::btree_set(path, 1..24)
}

/// Random contents for an ignore file (possibly empty).
pub(crate) fn ignore_file() -> impl Strategy<Value = String> {
    proptest::collection::vec(proptest::sample::select(IGNORE_LINES), 0..6)
        .prop_map(|lines| lines.join("\n"))
}

/// A random subset of `pool`, as owned strings.
pub(crate) fn glob_subset(pool: &'static [&'static str]) -> impl Strategy<Value = Vec<String>> {
    proptest::sample::subsequence(pool, 0..=pool.len())
        .prop_map(|v| v.into_iter().map(str::to_string).collect())
}

/// Write each path in `files` under `root`, with its own path as the content.
pub(crate) fn write_tree(root: &Path, files: &BTreeSet<String>) {
    for rel in files {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, rel).unwrap();
    }
}
