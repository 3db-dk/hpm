//! `[stage]` filtering: workspace include/exclude globs, per-platform
//! `place` rules, ignore-rule construction, and the staging walk shared by
//! `hpm pack` and `hpm build`.

use glob::Pattern;
use hpm_package::IoOp;
use hpm_package::manifest::StageConfig;
use hpm_package::path_util::relative_path_to_forward_slash;
use hpm_package::platform::Platform;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use super::PackError;

/// A single `from -> to` placement rule compiled from `[stage.platform.*]`.
struct CompiledPlaceRule {
    from: Pattern,
    /// Archive-path prefix or full path, depending on whether `to` ends
    /// with `/` in the manifest. See [`StageFilter::archive_path_for`].
    to: String,
    /// Whether `to` was authored as a directory (ends with `/`).
    to_is_dir: bool,
}

/// Derived from `[stage]` at pack time. Combines:
///   - workspace include/exclude globs from `[stage]`
///   - per-platform `place` rules (with `from`/`to` paths) from
///     `[stage.platform.*]`, filtered to the target platform plus exclusion
///     of files matched only by other platforms' rules.
pub struct StageFilter {
    include: Vec<Pattern>,
    exclude: Vec<Pattern>,
    /// `from` patterns for the target platform. A file matched by one of
    /// these is included and its archive path is rewritten via the
    /// matching rule's `to`.
    target_rules: Vec<CompiledPlaceRule>,
    /// `from` patterns claimed only by non-target platforms. A file matched
    /// only by these is excluded.
    other_platform_patterns: Vec<Pattern>,
}

impl StageFilter {
    /// Build a filter from `[stage]` for the given target platform.
    /// Pass `target = None` to pack without per-platform placement (used
    /// when the package declares no `[compat].platforms`).
    pub fn new(stage: &StageConfig, target: Option<&Platform>) -> Result<Self, PackError> {
        let include = compile_patterns(&stage.include)?;
        let exclude = compile_patterns(&stage.exclude)?;

        let mut target_rules = Vec::new();
        let mut other_platform_patterns = Vec::new();

        if let Some(target) = target {
            for (platform, rules) in &stage.platform.entries {
                for rule in &rules.place {
                    let from = Pattern::new(&rule.from)
                        .map_err(|e| PackError::GlobPattern(e.to_string()))?;
                    if platform == target {
                        let trimmed = rule.to.trim();
                        // The confined form drops `.` components and refuses
                        // a `to` that would leave the install image.
                        let confined = rule
                            .confined_to()
                            .map_err(|e| PackError::PlaceTarget(e.to_string()))?;
                        // A `to` naming the image root (empty, `.`, `./`) or
                        // ending in `/` or `/.` is a directory: each file
                        // keeps its name under it. Read as a literal file
                        // path, an empty one would put files at the root
                        // itself and `lib/.` would become a file `lib`.
                        let to_is_dir = confined.as_os_str().is_empty()
                            || trimmed.ends_with('/')
                            || trimmed.ends_with("/.");
                        let to = relative_path_to_forward_slash(&confined);
                        target_rules.push(CompiledPlaceRule {
                            from,
                            to,
                            to_is_dir,
                        });
                    } else {
                        other_platform_patterns.push(from);
                    }
                }
            }
        }

        Ok(Self {
            include,
            exclude,
            target_rules,
            other_platform_patterns,
        })
    }

    /// Returns the archive-relative path for `rel_path`, or `None` if the
    /// file should be excluded from this platform's archive.
    pub fn archive_path_for(&self, rel_path: &str) -> Option<String> {
        // Explicit excludes always win.
        if self.exclude.iter().any(|p| p.matches(rel_path)) {
            return None;
        }
        // Explicit includes ("only ship these as common content") narrow
        // the set when present, but never override a target-platform
        // `from` match.
        let target_match = self
            .target_rules
            .iter()
            .find(|rule| rule.from.matches(rel_path));
        if let Some(rule) = target_match {
            return Some(rewrite_archive_path(rel_path, rule));
        }
        let other_match = self
            .other_platform_patterns
            .iter()
            .any(|p| p.matches(rel_path));
        if other_match {
            return None;
        }
        if !self.include.is_empty() && !self.include.iter().any(|p| p.matches(rel_path)) {
            return None;
        }
        Some(rel_path.to_string())
    }
}

fn compile_patterns(globs: &[String]) -> Result<Vec<Pattern>, PackError> {
    globs
        .iter()
        .map(|g| Pattern::new(g).map_err(|e| PackError::GlobPattern(e.to_string())))
        .collect()
}

fn rewrite_archive_path(rel_path: &str, rule: &CompiledPlaceRule) -> String {
    if rule.to_is_dir {
        // Take the basename of `rel_path` and append it under `to/`. This
        // matches the common case of `from = "build/Release/*.dylib"`,
        // `to = "dso/macos-aarch64/"`.
        let basename = rel_path.rsplit_once('/').map_or(rel_path, |(_, name)| name);
        if rule.to.is_empty() {
            basename.to_string()
        } else {
            format!("{}/{}", rule.to, basename)
        }
    } else {
        // `to` is a literal full archive path; use it verbatim. Useful when
        // exactly one file is being relocated under a renamed name.
        rule.to.clone()
    }
}

/// Build gitignore-style rules for filtering archive contents.
///
/// Always excludes `.git/` and `.hpm/`. Additionally loads `.gitignore` and
/// `.hpmignore` if they exist in the package directory.
pub fn build_ignore_rules(dir: &Path) -> Result<Gitignore, PackError> {
    let mut builder = GitignoreBuilder::new(dir);

    // Always exclude .git/ and .hpm/
    builder.add_line(None, ".git/")?;
    builder.add_line(None, ".hpm/")?;

    // Load .gitignore if present
    let gitignore = dir.join(".gitignore");
    if gitignore.exists() {
        builder.add(gitignore);
    }

    // Load .hpmignore if present
    let hpmignore = dir.join(".hpmignore");
    if hpmignore.exists() {
        builder.add(hpmignore);
    }

    Ok(builder.build()?)
}

/// Collect the install image: every workspace file that survives the ignore
/// rules and `[stage]` filter, paired with its archive-relative destination
/// path. Sorted for deterministic output.
///
/// `skip_prefix` (relative to `package_dir`) excludes a subtree — used by
/// [`stage_to_dir`] when the output directory sits inside the package root,
/// so a re-run never re-stages its own previous output.
///
/// Also backs dev copies (`StorageManager::install_as_dev_copy`), so a dev
/// install carries exactly the image a registry consumer would get.
pub(crate) fn collect_stage_entries(
    package_dir: &Path,
    ignore: &Gitignore,
    stage_filter: Option<&StageFilter>,
    skip_prefix: Option<&Path>,
) -> Result<Vec<(PathBuf, String)>, PackError> {
    let mut entries: Vec<(PathBuf, String)> = Vec::new();
    // Archive path -> the workspace path staged there.
    let mut placed: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    // Prune ignored directories instead of descending into them: `.git/`,
    // cargo `target/` and CMake build trees can hold tens of thousands of
    // files, and walking them only to discard every entry turns a dev copy of
    // a native package into a long stat storm.
    //
    // Pruning is only exact when no `!` rule exists. The per-entry check below
    // (`matched_path_or_any_parents`) lets the *nearest* matching rule win, so
    // unlike git a re-include such as `!build/Release/*.dll` does take effect
    // under an excluded `build/`, and packages rely on that. With any
    // whitelist present, walk everything and let the per-entry check decide.
    // `prop_pruned_walk_matches_full_walk` pins the equivalence.
    let can_prune = ignore.num_whitelists() == 0;
    let walker = WalkDir::new(package_dir)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| {
            if !can_prune || !entry.file_type().is_dir() {
                return true;
            }
            let relative = entry
                .path()
                .strip_prefix(package_dir)
                .unwrap_or(entry.path());
            relative == Path::new("") || !ignore.matched(relative, true).is_ignore()
        });
    // Links are resolved against the package's real location, so one that
    // points outside the tree can be told apart from one inside it.
    let canonical_root = fs::canonicalize(package_dir)
        .map_err(|e| IoOp::wrap("resolve package directory", package_dir, e))?;
    for entry in walker {
        let entry = entry.map_err(|e| {
            IoOp::wrap(
                "walk package source tree",
                package_dir,
                std::io::Error::other(e),
            )
        })?;

        let path = entry.path();
        let relative = path.strip_prefix(package_dir).unwrap_or(path);

        // Skip the root directory itself
        if relative == Path::new("") {
            continue;
        }

        if let Some(skip) = skip_prefix
            && relative.starts_with(skip)
        {
            continue;
        }

        // Check ignore rules
        let is_dir = entry.file_type().is_dir();
        if ignore
            .matched_path_or_any_parents(relative, is_dir)
            .is_ignore()
        {
            continue;
        }

        // `file_type()` reports the link itself, not its target, because the
        // walk deliberately doesn't follow links. Testing it alone therefore
        // dropped every symlink from the archive silently — a package laying
        // out `libfoo.so -> libfoo.so.1.2.3`, the normal Unix convention,
        // shipped without `libfoo.so` and nothing said so. Resolve the link
        // instead: a symlink to a file is packed as an ordinary file holding
        // the target's bytes, which is also what keeps archives symlink-free
        // (extraction refuses to materialise links — see
        // `archive_fetcher::extract`).
        let packable = if entry.file_type().is_file() {
            true
        } else if entry.file_type().is_symlink() {
            match std::fs::metadata(path) {
                // Symlink to a directory: the walk won't descend into it
                // without `follow_links`, so its contents would go missing
                // just as quietly. Say so rather than half-packing.
                Ok(md) if md.is_dir() => {
                    tracing::warn!(
                        "Skipping symlinked directory {} — its contents will not \
                         be packed. Replace it with a real directory if the \
                         package needs them.",
                        relative.display()
                    );
                    false
                }
                // A link to a file outside the package tree would ship that
                // file (a key in the home directory, a CI environment);
                // only links within the tree are packed.
                Ok(_) => {
                    let inside = fs::canonicalize(path)
                        .is_ok_and(|target| target.starts_with(&canonical_root));
                    if !inside {
                        return Err(PackError::LinkOutsidePackage(relative.to_path_buf()));
                    }
                    true
                }
                // Broken link: nothing to read, and `File::open` would other-
                // wise fail the whole pack later with a less obvious message.
                Err(e) => {
                    tracing::warn!("Skipping broken symlink {}: {}", relative.display(), e);
                    false
                }
            }
        } else {
            false
        };
        if !packable {
            continue;
        }

        // Manifest globs (e.g. `build/Release/*.dylib`) use forward slashes,
        // so the input must too — `to_string_lossy()` would emit backslashes
        // on Windows and silently fail to match.
        let rel_str = relative_path_to_forward_slash(relative);
        let archive_path = match stage_filter {
            Some(filter) => match filter.archive_path_for(&rel_str) {
                Some(p) => p,
                None => continue,
            },
            None => rel_str.clone(),
        };
        // Every writer (`hpm build`, `hpm pack`, dev copies) joins this
        // path under its own root; refuse here what would leave it.
        hpm_package::path_util::confined_relative_path(Path::new(&archive_path))
            .map_err(|e| PackError::PlaceTarget(e.to_string()))?;
        // Two files placed at one path: `hpm pack` would stop on a duplicate
        // zip entry, while `hpm build` and dev copies kept whichever was
        // walked last. Refuse it the same way for all of them.
        if let Some(earlier) = placed.insert(archive_path.clone(), rel_str) {
            return Err(PackError::PlaceTarget(format!(
                "'{}' and '{}' would both be placed at '{}'; give each its own `to`, \
                 or end `to` with '/' so each keeps its name",
                earlier, placed[&archive_path], archive_path
            )));
        }
        entries.push((path.to_path_buf(), archive_path));
    }
    Ok(entries)
}

/// Refuse to put a private key in anything that leaves the workspace (an
/// archive, `hpm build` output). A file whose first line is a PEM private
/// key header is refused, whatever its name and however it got into the
/// tree; a public key or a document that mentions keys further in is not.
pub(super) fn refuse_private_keys(entries: &[(PathBuf, String)]) -> Result<(), PackError> {
    use std::io::Read;
    for (source, archive_path) in entries {
        let mut head = [0u8; 128];
        let n = fs::File::open(source)
            .and_then(|mut f| f.read(&mut head))
            .map_err(|e| IoOp::wrap("read staged file", source, e))?;
        let text = String::from_utf8_lossy(&head[..n]);
        let first_line = text.trim_start().lines().next().unwrap_or("");
        if first_line.starts_with("-----BEGIN ") && first_line.ends_with("PRIVATE KEY-----") {
            return Err(PackError::PrivateKey(archive_path.clone()));
        }
    }
    Ok(())
}

/// Materialise the install image into `output_dir` — the same file set and
/// placement [`create_archive`] would put in a zip, as a directory tree.
/// Returns the number of files copied. Backs `hpm build`.
///
/// [`create_archive`]: super::create_archive
pub fn stage_to_dir(
    package_dir: &Path,
    output_dir: &Path,
    ignore: &Gitignore,
    stage_filter: Option<&StageFilter>,
) -> Result<usize, PackError> {
    // If the output dir sits inside the package root, exclude it from the
    // walk — otherwise a re-run would recursively stage the previous output.
    let skip = output_dir
        .strip_prefix(package_dir)
        .ok()
        .map(Path::to_path_buf);
    let entries = collect_stage_entries(package_dir, ignore, stage_filter, skip.as_deref())?;
    refuse_private_keys(&entries)?;

    for (source, archive_path) in &entries {
        let dest = output_dir.join(archive_path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| IoOp::wrap("create staging directory", parent, e))?;
        }
        fs::copy(source, &dest).map_err(|e| IoOp::wrap("copy staged file to", &dest, e))?;
    }
    Ok(entries.len())
}
