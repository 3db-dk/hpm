//! Package validation: what `hpm check` reports and `hpm pack` gates on.
//!
//! [`check_package`] runs every pass over a package directory and returns a
//! [`Report`] of [`Finding`]s. A report is valid exactly when it holds no
//! [`Severity::Error`]; nothing stores validity separately. Rendering
//! (prefixes, colours, JSON) belongs to the caller.

use hpm_package::PackageManifest;
use std::fs;
use std::path::Path;

/// How much a finding matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The package cannot be published as it stands.
    Error,
    /// Worth fixing, but the package is valid.
    Warning,
    /// A check that passed.
    Passed,
    /// Neutral information.
    Info,
}

/// One thing a check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    pub message: String,
}

/// Every finding of one validation run, in the order the checks ran.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub findings: Vec<Finding>,
}

impl Report {
    /// Whether the package passed: no finding is an [`Severity::Error`].
    pub fn is_valid(&self) -> bool {
        !self.findings.iter().any(|f| f.severity == Severity::Error)
    }

    /// The messages of the findings with `severity`, in order.
    pub fn messages(&self, severity: Severity) -> impl Iterator<Item = &str> {
        self.findings
            .iter()
            .filter(move |f| f.severity == severity)
            .map(|f| f.message.as_str())
    }

    fn push(&mut self, severity: Severity, message: impl Into<String>) {
        self.findings.push(Finding {
            severity,
            message: message.into(),
        });
    }

    fn error(&mut self, message: impl Into<String>) {
        self.push(Severity::Error, message);
    }

    fn warning(&mut self, message: impl Into<String>) {
        self.push(Severity::Warning, message);
    }

    fn passed(&mut self, message: impl Into<String>) {
        self.push(Severity::Passed, message);
    }

    fn info(&mut self, message: impl Into<String>) {
        self.push(Severity::Info, message);
    }
}

/// Validate the package in `project_dir`.
///
/// Every problem, including an `hpm.toml` that is missing, unreadable or
/// does not parse, is a finding; the passes after the manifest run only
/// once it parses.
pub fn check_package(project_dir: &Path) -> Report {
    let mut report = Report::default();
    let Some(manifest) = check_manifest_file(&project_dir.join("hpm.toml"), &mut report) else {
        return report;
    };
    check_manifest_content(&manifest, &mut report);
    check_project_structure(project_dir, &manifest, &mut report);
    check_houdini_compatibility(&manifest, &mut report);
    check_best_practices(project_dir, &manifest, &mut report);
    report
}

fn check_manifest_file(manifest_path: &Path, report: &mut Report) -> Option<PackageManifest> {
    if !manifest_path.exists() {
        report.error("No hpm.toml found in current directory");
        return None;
    }
    report.passed("hpm.toml found");

    let Ok(content) = fs::read_to_string(manifest_path) else {
        report.error("Failed to validate manifest: Failed to read hpm.toml");
        return None;
    };
    match hpm_package::parse_manifest_str(&content) {
        Ok(manifest) => {
            report.passed("hpm.toml has valid TOML syntax");
            Some(manifest)
        }
        Err(e) => {
            report.error(format!("Invalid TOML syntax in hpm.toml: {}", e));
            None
        }
    }
}

fn check_manifest_content(manifest: &PackageManifest, report: &mut Report) {
    let validation = manifest.validate_with(hpm_package::ValidationLevel::Publish);
    if validation.is_ok() {
        report.passed("Package manifest validation passed");
    }
    for err in validation.errors {
        report.error(format!("Manifest validation failed: {}", err));
    }
    for warning in validation.warnings {
        report.warning(warning);
    }
}

fn check_project_structure(project_dir: &Path, manifest: &PackageManifest, report: &mut Report) {
    let expected_dirs = ["otls", "python", "scripts", "presets", "config"];
    let mut found_dirs = Vec::new();
    for dir_name in expected_dirs {
        if project_dir.join(dir_name).is_dir() {
            found_dirs.push(dir_name);
            report.passed(format!("Found {} directory", dir_name));
        }
    }
    if found_dirs.is_empty() {
        report.warning(
            "No standard Houdini directories found (otls, python, scripts, presets, config)",
        );
    }

    let readme_files = ["README.md", "README.txt", "README"];
    if let Some(readme) = readme_files
        .iter()
        .find(|readme| project_dir.join(readme).exists())
    {
        report.passed(format!("Found {}", readme));
    } else if let Some(readme_path) = &manifest.package.readme {
        if project_dir.join(readme_path).exists() {
            report.passed(format!("Found specified README: {}", readme_path));
        } else {
            report.error(format!(
                "README file specified in manifest not found: {}",
                readme_path
            ));
        }
    } else {
        report.warning("No README file found - consider adding documentation");
    }

    if found_dirs.contains(&"otls") {
        check_otls_directory(project_dir, report);
    }
}

/// Digital-asset library extensions Houdini loads from an `otls` directory.
/// `otl` is the historical spelling of `hda`; the `lc`/`nc` variants are the
/// limited-commercial and non-commercial saves of the same format (see
/// <https://www.sidefx.com/docs/houdini/assets/install.html>).
const HDA_EXTENSIONS: [&str; 6] = ["hda", "otl", "hdalc", "otllc", "hdanc", "otlnc"];

fn check_otls_directory(project_dir: &Path, report: &mut Report) {
    let entries = match fs::read_dir(project_dir.join("otls")) {
        Ok(entries) => entries,
        Err(e) => {
            report.warning(format!("Could not read otls directory: {}", e));
            return;
        }
    };
    let mut has_assets = false;
    for entry in entries.flatten() {
        let path = entry.path();
        let is_asset = path
            .extension()
            .is_some_and(|ext| HDA_EXTENSIONS.iter().any(|known| ext == *known));
        if is_asset {
            has_assets = true;
            report.passed(format!(
                "Found Houdini asset: {}",
                entry.file_name().to_string_lossy()
            ));
        }
    }
    if !has_assets {
        report.warning(format!(
            "otls directory exists but contains no digital assets ({})",
            HDA_EXTENSIONS
                .iter()
                .map(|e| format!(".{e}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
}

fn check_houdini_compatibility(manifest: &PackageManifest, report: &mut Report) {
    let Some(range) = &manifest.compat.houdini else {
        return;
    };
    report.passed(format!(
        "Houdini compatibility: {} (compiles to `{}`)",
        range,
        range.to_enable_expression()
    ));

    // DSOs compiled against one Houdini major typically won't load in the
    // next, so a native package unbounded above installs cleanly on a newer
    // Houdini and then fails at load.
    if !manifest.compat.platforms.is_empty() && !range.has_upper_bound() {
        report.warning(format!(
            "[compat].platforms is declared but [compat].houdini = \"{}\" \
             has no upper bound. Native binaries compiled against one \
             Houdini major typically won't load in the next. Consider \
             \"^21\" (Houdini 21.x only) or an explicit range like \
             \">=20.5, <22\".",
            range
        ));
    }
}

fn check_best_practices(project_dir: &Path, manifest: &PackageManifest, report: &mut Report) {
    let license_files = ["LICENSE", "LICENSE.md", "LICENSE.txt", "COPYING"];
    if let Some(license) = license_files
        .iter()
        .find(|license| project_dir.join(license).exists())
    {
        report.passed(format!("Found license file: {}", license));
    } else if manifest.package.license.is_some() {
        report.warning("License specified in manifest but no LICENSE file found");
    }

    if project_dir.join(".git").exists() {
        report.passed("Git repository initialized");
        if !project_dir.join(".gitignore").exists() {
            report.warning("No .gitignore file found - consider adding one");
        }
    } else {
        report.info("No version control detected - consider initializing git");
    }

    check_scripts(manifest, report);
    check_package_size(project_dir, manifest, report);
}

fn check_scripts(manifest: &PackageManifest, report: &mut Report) {
    if manifest.scripts.commands.is_empty() {
        return;
    }
    report.passed(format!(
        "Package defines {} script(s)",
        manifest.scripts.commands.len()
    ));

    let host_os = hpm_package::Platform::current().and_then(|p| p.os_key().map(|os| os.as_str()));
    for (name, entry) in &manifest.scripts.commands {
        match entry.resolve_cmd(host_os) {
            Some(cmd) if cmd.trim().is_empty() => {
                report.warning(format!("Script '{}' has empty command", name));
            }
            Some(_) => {
                if let Some(desc) = entry.description() {
                    let display = entry.label().unwrap_or(name);
                    report.info(format!("Script '{}': {}", display, desc));
                } else if let Some(label) = entry.label() {
                    report.info(format!("Script '{}' labelled '{}'", name, label));
                }
            }
            // Legitimate (the script exists only for other platforms), but
            // worth a hint so authors notice typos in their `when` axes.
            None => report.info(format!(
                "Script '{}' has no command for host OS — only matches other platforms",
                name
            )),
        }
    }
}

const LARGE_FILE: u64 = 10 * 1024 * 1024;
const LARGE_PACKAGE: u64 = 100 * 1024 * 1024;

/// Warn about a large install image and the large files in it. The image is
/// what `hpm pack` would ship: the stage walk with the same ignore rules
/// (`.git/`, `.hpm/`, `.gitignore`, `.hpmignore`) and `[stage]` globs, so a
/// gitignored build tree is not counted. A failure to walk it is an error
/// finding, since pack would fail the same way.
fn check_package_size(project_dir: &Path, manifest: &PackageManifest, report: &mut Report) {
    let entries = crate::packer::build_ignore_rules(project_dir).and_then(|ignore| {
        let filter = crate::packer::StageFilter::new(&manifest.stage, None)?;
        crate::packer::collect_stage_entries(project_dir, &ignore, Some(&filter), None)
    });
    let entries = match entries {
        Ok(entries) => entries,
        Err(e) => {
            report.error(format!("Could not measure the package contents: {}", e));
            return;
        }
    };

    let mut total_size = 0u64;
    let mut large_files = Vec::new();
    for (path, _) in &entries {
        let size = match fs::metadata(path) {
            Ok(metadata) => metadata.len(),
            Err(e) => {
                report.error(format!(
                    "Could not measure the package contents: {}: {}",
                    path.display(),
                    e
                ));
                return;
            }
        };
        total_size += size;
        if size > LARGE_FILE {
            large_files.push((path, size));
        }
    }

    let megabytes = |size: u64| size as f64 / (1024.0 * 1024.0);
    if total_size > LARGE_PACKAGE {
        report.warning(format!(
            "Package size is large ({:.1} MB) - consider if all files are necessary",
            megabytes(total_size)
        ));
    }
    for (path, size) in large_files {
        let relative = path.strip_prefix(project_dir).unwrap_or(path);
        report.warning(format!(
            "Large file detected: {} ({:.1} MB)",
            hpm_package::path_util::relative_path_to_forward_slash(relative),
            megabytes(size)
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn package(manifest: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("hpm.toml"), manifest).unwrap();
        dir
    }

    fn warnings(report: &Report) -> Vec<&str> {
        report.messages(Severity::Warning).collect()
    }

    /// Validity is exactly the absence of an error finding.
    #[test]
    fn a_report_is_valid_exactly_when_it_holds_no_error() {
        let mut report = Report::default();
        assert!(report.is_valid());
        report.warning("w");
        report.passed("p");
        report.info("i");
        assert!(report.is_valid());
        report.error("e");
        assert!(!report.is_valid());
        assert_eq!(report.messages(Severity::Error).collect::<Vec<_>>(), ["e"]);
    }

    #[test]
    fn a_missing_manifest_is_the_only_finding() {
        let dir = TempDir::new().unwrap();
        let report = check_package(dir.path());
        assert_eq!(
            report.findings,
            [Finding {
                severity: Severity::Error,
                message: "No hpm.toml found in current directory".into(),
            }]
        );
    }

    #[test]
    fn warns_when_platforms_declared_with_unbounded_houdini() {
        let dir = package(
            "[package]\npath = \"studio/needs-bound\"\nname = \"Needs Bound\"\nversion = \"1.0.0\"\n\n[compat]\nhoudini = \">=21\"\nplatforms = [\"linux-x86_64\"]\n",
        );
        let report = check_package(dir.path());
        assert!(
            warnings(&report)
                .iter()
                .any(|w| w.contains("no upper bound") && w.contains("Native binaries")),
            "{report:?}"
        );
    }

    #[test]
    fn no_upper_bound_warning_for_a_bounded_range_or_without_platforms() {
        for compat in [
            "houdini = \"^21\"\nplatforms = [\"linux-x86_64\"]",
            "houdini = \">=21\"",
        ] {
            let dir = package(&format!(
                "[package]\npath = \"studio/p\"\nname = \"P\"\nversion = \"1.0.0\"\n\n[compat]\n{compat}\n"
            ));
            let report = check_package(dir.path());
            assert!(
                !warnings(&report)
                    .iter()
                    .any(|w| w.contains("no upper bound")),
                "{compat}: {report:?}"
            );
        }
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(48))]

        /// The size check measures exactly what `hpm pack` ships: over
        /// generated trees, ignore files and `[stage]` globs, the large-file
        /// warnings name exactly the staged files above the threshold, and
        /// the large-package warning appears exactly when the staged total
        /// is above its limit. Files are sparse, so sizes cost nothing.
        #[test]
        fn the_size_check_measures_the_pack_image(
            files in crate::packer::prop_support::file_tree(),
            large in proptest::collection::vec(proptest::bool::ANY, 24),
            gitignore in crate::packer::prop_support::ignore_file(),
            include in crate::packer::prop_support::glob_subset(crate::packer::prop_support::STAGE_INCLUDES),
            exclude in crate::packer::prop_support::glob_subset(crate::packer::prop_support::STAGE_EXCLUDES),
        ) {
            let dir = TempDir::new().unwrap();
            let quote = |globs: &[String]| {
                globs.iter().map(|g| format!("\"{g}\"")).collect::<Vec<_>>().join(", ")
            };
            std::fs::write(
                dir.path().join("hpm.toml"),
                format!(
                    "[package]\npath = \"studio/p\"\nname = \"P\"\nversion = \"1.0.0\"\n\n[stage]\ninclude = [{}]\nexclude = [{}]\n",
                    quote(&include),
                    quote(&exclude)
                ),
            )
            .unwrap();
            std::fs::write(dir.path().join(".gitignore"), &gitignore).unwrap();
            for (file, big) in files.iter().zip(&large) {
                let path = dir.path().join(file);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                let size = if *big { LARGE_FILE + 1 } else { 3 };
                std::fs::File::create(&path).unwrap().set_len(size).unwrap();
            }

            let manifest = PackageManifest::from_path(&dir.path().join("hpm.toml")).unwrap();
            let ignore = crate::packer::build_ignore_rules(dir.path()).unwrap();
            let filter = crate::packer::StageFilter::new(&manifest.stage, None).unwrap();
            let staged = crate::packer::collect_stage_entries(dir.path(), &ignore, Some(&filter), None).unwrap();
            let sizes: Vec<(String, u64)> = staged
                .iter()
                .map(|(path, _)| {
                    let rel = path.strip_prefix(dir.path()).unwrap();
                    (
                        hpm_package::path_util::relative_path_to_forward_slash(rel),
                        std::fs::metadata(path).unwrap().len(),
                    )
                })
                .collect();
            let mut expected_large: Vec<String> = sizes
                .iter()
                .filter(|(_, size)| *size > LARGE_FILE)
                .map(|(path, size)| format!("Large file detected: {} ({:.1} MB)", path, *size as f64 / (1024.0 * 1024.0)))
                .collect();
            expected_large.sort();
            let total: u64 = sizes.iter().map(|(_, size)| size).sum();

            let report = check_package(dir.path());
            let mut large_warnings: Vec<String> = warnings(&report)
                .into_iter()
                .filter(|w| w.starts_with("Large file detected"))
                .map(str::to_string)
                .collect();
            large_warnings.sort();
            proptest::prop_assert_eq!(large_warnings, expected_large);
            proptest::prop_assert_eq!(
                warnings(&report).iter().any(|w| w.starts_with("Package size is large")),
                total > LARGE_PACKAGE
            );
        }
    }

    /// Files the stage walk leaves out (gitignored, `[stage].exclude`) do not
    /// count towards the package size.
    #[test]
    fn package_size_counts_only_what_pack_would_ship() {
        let dir = package(
            "[package]\npath = \"studio/p\"\nname = \"P\"\nversion = \"1.0.0\"\n\n[stage]\nexclude = [\"raw/**\"]\n",
        );
        for sub in ["build", "raw", "data"] {
            std::fs::create_dir_all(dir.path().join(sub)).unwrap();
        }
        std::fs::write(dir.path().join(".gitignore"), "build/\n").unwrap();
        for file in ["build/a.bin", "raw/b.bin", "data/c.bin"] {
            std::fs::File::create(dir.path().join(file))
                .unwrap()
                .set_len(LARGE_FILE + 1)
                .unwrap();
        }
        let report = check_package(dir.path());
        let large: Vec<_> = warnings(&report)
            .into_iter()
            .filter(|w| w.starts_with("Large file"))
            .collect();
        assert_eq!(large, ["Large file detected: data/c.bin (10.0 MB)"]);
    }
}
