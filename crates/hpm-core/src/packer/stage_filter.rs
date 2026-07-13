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
            let target_str = target.as_str();
            for (platform_str, rules) in &stage.platform.entries {
                for rule in &rules.place {
                    let from = Pattern::new(&rule.from)
                        .map_err(|e| PackError::GlobPattern(e.to_string()))?;
                    if platform_str == target_str {
                        let trimmed = rule.to.trim();
                        let to_is_dir = trimmed.ends_with('/') || trimmed == ".";
                        let to = if trimmed == "." || trimmed == "./" {
                            String::new()
                        } else if to_is_dir {
                            trimmed.trim_end_matches('/').to_string()
                        } else {
                            trimmed.to_string()
                        };
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
pub(super) fn collect_stage_entries(
    package_dir: &Path,
    ignore: &Gitignore,
    stage_filter: Option<&StageFilter>,
    skip_prefix: Option<&Path>,
) -> Result<Vec<(PathBuf, String)>, PackError> {
    let mut entries: Vec<(PathBuf, String)> = Vec::new();
    for entry in WalkDir::new(package_dir).sort_by_file_name() {
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

        if !entry.file_type().is_file() {
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
            None => rel_str,
        };
        entries.push((path.to_path_buf(), archive_path));
    }
    Ok(entries)
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
