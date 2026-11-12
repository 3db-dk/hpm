//! [`pack_package`]: everything `hpm pack` does between reading the package
//! and reporting the archive, for every embedder to share.

use hpm_package::{
    IoOp, NativePackageTarget, PackageManifest, Platform, validate_hpackage_descriptor,
};
use std::fmt;
use std::path::{Path, PathBuf};

use super::{ArchiveLayout, PackError, PackResult, SigningKey};
use crate::asset_index::AssetIndex;
use crate::check::Severity;

/// One pack of the package in `dir`.
pub struct PackRequest {
    /// The package directory (the one holding `hpm.toml`).
    pub dir: PathBuf,
    /// Where the archive is written.
    pub output_dir: PathBuf,
    /// Signs the archive when present.
    pub key: Option<SigningKey>,
    /// The requested target platform; `None` targets the host when the
    /// package declares `[compat].platforms`.
    pub platform: Option<Platform>,
    /// The shape of the bundled `{slug}.json`.
    pub target: NativePackageTarget,
    /// Fail, and remove the archive, when a declared operator source is not
    /// in it, instead of warning.
    pub verify_assets: bool,
}

/// A packed archive with its operator index, the manifest it was packed
/// from, and what the pack warned about.
#[derive(Debug)]
pub struct PackOutcome {
    pub result: PackResult,
    pub manifest: PackageManifest,
    pub asset_index: AssetIndex,
    pub warnings: Vec<PackWarning>,
}

/// Something a pack reports without failing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackWarning {
    /// A warning from the validation the pack runs first.
    Validation(String),
    /// A file whose name differs from `{slug}.json` only by case was not
    /// shipped as the descriptor; one was generated instead.
    CaseDifferingDescriptor { found: String, expected: String },
    /// An authored descriptor's `hpackage.version` that SideFX's repository
    /// stores trimmed.
    HpackageVersionTrimmed {
        filename: String,
        declared: String,
        stored: String,
    },
    /// A declared `[[operators]]` source that is not in the archive.
    MissingOperatorSource(String),
}

impl fmt::Display for PackWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(message) => f.write_str(message),
            Self::CaseDifferingDescriptor { found, expected } => write!(
                f,
                "Ignoring {found}: the bundled Houdini descriptor must be named exactly \
                 {expected} (after the package slug), so {found} is a different file. \
                 Generating the descriptor from hpm.toml instead — rename it to {expected} \
                 to ship it verbatim."
            ),
            Self::HpackageVersionTrimmed {
                filename,
                declared,
                stored,
            } => write!(
                f,
                "{filename} declares hpackage.version {declared}, which SideFX's \
                 hpackage repository stores as {stored} — the published version and \
                 the served archive's URL will both read {stored}."
            ),
            Self::MissingOperatorSource(source) => write!(
                f,
                "Declared operator source not found in archive: {source} (pass --verify-assets to fail the pack)"
            ),
        }
    }
}

/// How a directory's entries match a filename needed by exact spelling.
enum NameMatch {
    /// An entry is spelled exactly as asked.
    Exact,
    /// No exact entry, but one differs only by case. Carries its real name.
    CaseDiffers(String),
    /// Nothing resembling the name is there.
    Missing,
}

/// Look `name` up among `dir`'s entries by exact spelling.
///
/// `Path::exists` asks the filesystem, and APFS and NTFS answer
/// case-insensitively: a repo holding `FLOPs.json` reports `flops.json` as
/// present, while the same tree on Linux reports it absent. Anything keyed
/// off that answer produces a different archive depending on who built it.
/// Houdini looks the descriptor up by exact name and the archive entry is
/// written under the exact name, so a case-folded match is a different file.
/// Comparing directory entries gives the same answer everywhere.
fn find_exact_entry(dir: &Path, name: &str) -> Result<NameMatch, PackError> {
    let read_err = |e| IoOp::wrap("read directory", dir, e);
    let mut case_differs = None;
    for entry in std::fs::read_dir(dir).map_err(read_err)? {
        let file_name = entry.map_err(read_err)?.file_name();
        let Some(found) = file_name.to_str() else {
            continue;
        };
        if found == name {
            return Ok(NameMatch::Exact);
        }
        if case_differs.is_none() && found.eq_ignore_ascii_case(name) {
            case_differs = Some(found.to_string());
        }
    }
    Ok(match case_differs {
        Some(found) => NameMatch::CaseDiffers(found),
        None => NameMatch::Missing,
    })
}

/// Validate, pack and index the package `request` names.
///
/// In order: the validation gate ([`crate::check::check_package`]); the
/// target platform ([`hpm_package::CompatConfig::resolve_target_platform`]
/// against this host); for [`NativePackageTarget::SideFxHpackage`], the
/// README hpackage's uploader insists on; the bundled `{slug}.json`, shipped
/// verbatim when the package has one by that exact name and generated
/// otherwise, checked against hpackage's rules under that target; the
/// archive, in Houdini's hpackage layout (`{slug}.json` at the root, content
/// under `{slug}/`); and the operator index. The archive is removed when
/// indexing fails, or when `verify_assets` finds a declared source missing.
pub fn pack_package(request: PackRequest) -> Result<PackOutcome, PackError> {
    let PackRequest {
        dir,
        output_dir,
        key,
        platform,
        target,
        verify_assets,
    } = request;
    let mut warnings = Vec::new();

    let validation = crate::check::check_package(&dir);
    warnings.extend(
        validation
            .messages(Severity::Warning)
            .map(|w| PackWarning::Validation(w.to_string())),
    );
    if !validation.is_valid() {
        return Err(PackError::Validation {
            errors: validation
                .messages(Severity::Error)
                .map(str::to_string)
                .collect(),
        });
    }

    let manifest = PackageManifest::from_path(&dir.join("hpm.toml"))?;
    let platform = manifest
        .compat
        .resolve_target_platform(platform, Platform::current())?;

    if target == NativePackageTarget::SideFxHpackage {
        check_sidefx_readme(&dir)?;
    }

    let (native_filename, json_bytes) = bundled_descriptor(&dir, &manifest, target, &mut warnings)?;

    let content_prefix = manifest.package.slug().to_string();
    let inject_files = vec![(native_filename, json_bytes)];
    let result = super::pack(
        &dir,
        &manifest.package.name,
        &manifest.package.version,
        &output_dir,
        key.as_ref(),
        platform.as_ref(),
        &manifest.stage,
        ArchiveLayout {
            inject_files: &inject_files,
            content_prefix: Some(&content_prefix),
        },
    )?;

    // An index that silently dropped operators, or one advertising files the
    // archive does not ship, is a packaging bug; don't leave the archive for
    // CI to publish.
    let remove_archive = |path: &Path| {
        let _ = std::fs::remove_file(path);
    };
    let asset_index = match crate::asset_index::collect_assets(
        &result.archive_path,
        &manifest.operators,
        platform.as_ref(),
        Some(&content_prefix),
    ) {
        Ok(index) => index,
        Err(e) => {
            remove_archive(&result.archive_path);
            return Err(PackError::AssetIndex(e));
        }
    };
    if !asset_index.missing_sources.is_empty() {
        if verify_assets {
            remove_archive(&result.archive_path);
            return Err(PackError::MissingOperatorSources {
                sources: asset_index.missing_sources,
            });
        }
        warnings.extend(
            asset_index
                .missing_sources
                .iter()
                .cloned()
                .map(PackWarning::MissingOperatorSource),
        );
    }

    Ok(PackOutcome {
        result,
        manifest,
        asset_index,
        warnings,
    })
}

/// hpackage's uploader reads `{package_dir}/README.md` by that literal name
/// and sends its text as the package description; a missing file raises
/// rather than warning, and so does the placeholder its own scaffold writes.
/// hpm's own README check accepts `README.txt` and a bare `README` and only
/// warns, so a package can pass validation and still be refused on upload.
fn check_sidefx_readme(dir: &Path) -> Result<(), PackError> {
    match find_exact_entry(dir, "README.md")? {
        NameMatch::Exact => {}
        NameMatch::CaseDiffers(found) => return Err(PackError::SidefxReadmeCase { found }),
        NameMatch::Missing => {
            return Err(PackError::SidefxReadmeMissing {
                path: dir.join("README.md"),
            });
        }
    }
    let path = dir.join("README.md");
    let readme = std::fs::read_to_string(&path).map_err(|e| IoOp::wrap("read", &path, e))?;
    if readme.contains("<enter a one-line description") {
        return Err(PackError::SidefxReadmePlaceholder);
    }
    Ok(())
}

/// The `{slug}.json` to ship at the archive root: the package's own by that
/// exact name, verbatim, or else one generated from the manifest.
///
/// Under [`NativePackageTarget::SideFxHpackage`] the descriptor that will
/// actually ship is checked, authored or generated: an authored one is
/// shipped without normalization, so checking only the generated one would
/// make the target do nothing on exactly the packages whose author wrote the
/// file by hand. The generator and the check encode the same hpackage rules
/// from opposite ends, so checking generated output catches them drifting.
fn bundled_descriptor(
    dir: &Path,
    manifest: &PackageManifest,
    target: NativePackageTarget,
    warnings: &mut Vec<PackWarning>,
) -> Result<(String, Vec<u8>), PackError> {
    let (filename, native_pkg) = manifest
        .generate_houdini_native_package_for(target)
        .map_err(PackError::DescriptorGeneration)?;
    let generated = || serde_json::to_vec_pretty(&native_pkg).map_err(PackError::DescriptorJson);
    let (bytes, authored) = match find_exact_entry(dir, &filename)? {
        NameMatch::Exact => {
            let path = dir.join(&filename);
            let bytes = std::fs::read(&path)
                .map_err(|e| IoOp::wrap("read hand-written Houdini package json", &path, e))?;
            (bytes, true)
        }
        NameMatch::CaseDiffers(found) => {
            warnings.push(PackWarning::CaseDifferingDescriptor {
                found,
                expected: filename.clone(),
            });
            (generated()?, false)
        }
        NameMatch::Missing => (generated()?, false),
    };

    if target == NativePackageTarget::SideFxHpackage {
        let descriptor: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|error| PackError::DescriptorNotJson {
                filename: filename.clone(),
                error,
            })?;
        let stored = validate_hpackage_descriptor(&descriptor, &filename).map_err(|why| {
            if authored {
                PackError::DescriptorRefused(why)
            } else {
                PackError::GeneratedDescriptorRefused(why)
            }
        })?;
        // hpackage stores the trimmed version and builds the served
        // archive's URL from it. The generator already trims, so this only
        // fires on an authored file.
        let declared = descriptor
            .get("hpackage")
            .and_then(|hpackage| hpackage.get("version"))
            .and_then(|version| version.as_str())
            .unwrap_or_default();
        if declared != stored {
            warnings.push(PackWarning::HpackageVersionTrimmed {
                filename: filename.clone(),
                declared: declared.to_string(),
                stored,
            });
        }
    }
    Ok((filename, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: the bundled descriptor was chosen with `Path::exists`,
    /// which APFS and NTFS answer case-insensitively, so `FLOPs.json` shipped
    /// as the root `flops.json` on macOS and Windows and not on Linux.
    #[test]
    fn exact_entry_lookup_does_not_fold_case() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("FLOPs.json"), "{}").unwrap();
        match find_exact_entry(dir.path(), "flops.json").unwrap() {
            NameMatch::CaseDiffers(found) => assert_eq!(found, "FLOPs.json"),
            NameMatch::Exact => panic!("case-folded match reported as exact"),
            NameMatch::Missing => panic!("the case-differing file should be reported"),
        }
        assert!(matches!(
            find_exact_entry(dir.path(), "FLOPs.json").unwrap(),
            NameMatch::Exact
        ));
        assert!(matches!(
            find_exact_entry(dir.path(), "README.md").unwrap(),
            NameMatch::Missing
        ));
    }

    /// A directory that cannot be read is an error, not "no such file".
    #[test]
    fn exact_entry_lookup_fails_on_an_unreadable_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_exact_entry(&dir.path().join("nope"), "README.md").is_err());
    }

    #[derive(Debug, Clone, Copy)]
    enum Descriptor {
        Absent,
        Authored,
        CaseDiffers,
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(24))]

        /// Over generated package trees: exactly one `{slug}.json`, at the
        /// archive root; an authored one ships byte-identical; a
        /// case-differing one is not the descriptor and is warned about.
        #[test]
        fn exactly_one_root_descriptor(
            files in crate::packer::prop_support::file_tree(),
            descriptor in proptest::prop_oneof![
                proptest::strategy::Just(Descriptor::Absent),
                proptest::strategy::Just(Descriptor::Authored),
                proptest::strategy::Just(Descriptor::CaseDiffers),
            ],
        ) {
            let dir = tempfile::tempdir().unwrap();
            let pkg = dir.path().join("pkg");
            for file in &files {
                let path = pkg.join(file);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, file.as_bytes()).unwrap();
            }
            std::fs::write(
                pkg.join("hpm.toml"),
                "[package]\npath = \"acme/demo\"\nname = \"Demo\"\nversion = \"1.0.0\"\ndescription = \"d\"\nauthors = [\"A\"]\nkeywords = [\"k\"]\n",
            )
            .unwrap();
            let authored = b"{\"hpath\": \"by hand\"}".to_vec();
            match descriptor {
                Descriptor::Absent => {}
                Descriptor::Authored => std::fs::write(pkg.join("demo.json"), &authored).unwrap(),
                Descriptor::CaseDiffers => std::fs::write(pkg.join("Demo.json"), &authored).unwrap(),
            }

            let outcome = pack_package(PackRequest {
                dir: pkg.clone(),
                output_dir: dir.path().join("out"),
                key: None,
                platform: None,
                target: NativePackageTarget::Generic,
                verify_assets: false,
            })
            .unwrap();

            let mut zip = zip::ZipArchive::new(
                std::fs::File::open(&outcome.result.archive_path).unwrap(),
            )
            .unwrap();
            let names: Vec<String> = zip.file_names().map(|n| n.unwrap().into_owned()).collect();
            let roots: Vec<&str> = names
                .iter()
                .map(String::as_str)
                .filter(|n| n.rsplit('/').next() == Some("demo.json"))
                .collect();
            proptest::prop_assert_eq!(roots, vec!["demo.json"]);
            let mut shipped = Vec::new();
            std::io::Read::read_to_end(&mut zip.by_name("demo.json").unwrap(), &mut shipped)
                .unwrap();
            proptest::prop_assert_eq!(
                shipped == authored,
                matches!(descriptor, Descriptor::Authored)
            );
            let warned = outcome
                .warnings
                .iter()
                .any(|w| matches!(w, PackWarning::CaseDifferingDescriptor { .. }));
            proptest::prop_assert_eq!(warned, matches!(descriptor, Descriptor::CaseDiffers));
        }
    }
}
