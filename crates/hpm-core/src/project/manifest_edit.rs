//! Formatting-preserving edits to a project's `hpm.toml`.
//!
//! All programmatic manifest mutation goes through here. Edits use
//! `toml_edit::DocumentMut` so user comments and formatting survive, and
//! writes are atomic. Serde round-trips (deserialize, mutate, re-serialize)
//! are forbidden for editing: they destroy comments and reorder content.

use crate::project::ProjectError;
use hpm_package::{DependencySpec, IoOp, ManifestLoadError};
use std::path::Path;
use tracing::info;

/// Read `hpm.toml` at `path`, apply `f` to the parsed document, and write it
/// back atomically. Formatting and comments outside the edited nodes are
/// preserved.
pub fn with_manifest_edit(
    path: &Path,
    f: impl FnOnce(&mut toml_edit::DocumentMut) -> Result<(), ProjectError>,
) -> Result<(), ProjectError> {
    let content = std::fs::read_to_string(path).map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            ProjectError::Manifest(ManifestLoadError::NotFound {
                path: path.to_path_buf(),
            })
        } else {
            ProjectError::Io(IoOp::wrap("read project manifest", path, source))
        }
    })?;

    let mut doc: toml_edit::DocumentMut =
        content
            .parse()
            .map_err(|source: toml_edit::TomlError| ProjectError::ManifestEdit {
                path: path.to_path_buf(),
                source,
            })?;

    f(&mut doc)?;

    hpm_package::atomic_write(path, doc.to_string()).map_err(ProjectError::Io)
}

/// Render a [`DependencySpec`] as the `toml_edit` item it takes in
/// `[dependencies]`: the bare version-string shorthand for a no-options
/// registry spec, an inline table for everything else. Mirrors the serde
/// shapes in `hpm_package::dependency`.
fn dependency_item(spec: &DependencySpec) -> toml_edit::Item {
    let mut inline = toml_edit::InlineTable::new();
    match spec {
        DependencySpec::Registry {
            version,
            registry: None,
            optional: false,
        } => return toml_edit::value(version),
        DependencySpec::Registry {
            version,
            registry,
            optional,
        } => {
            inline.insert("version", version.as_str().into());
            if let Some(registry) = registry {
                inline.insert("registry", registry.as_str().into());
            }
            if *optional {
                inline.insert("optional", true.into());
            }
        }
        DependencySpec::Url {
            url,
            version,
            optional,
        } => {
            inline.insert("url", url.as_str().into());
            inline.insert("version", version.as_str().into());
            if *optional {
                inline.insert("optional", true.into());
            }
        }
        DependencySpec::Path {
            path,
            optional,
            link,
        } => {
            inline.insert("path", path.as_str().into());
            if *optional {
                inline.insert("optional", true.into());
            }
            if *link {
                inline.insert("link", true.into());
            }
        }
    }
    toml_edit::Item::Value(toml_edit::Value::InlineTable(inline))
}

/// Insert or replace `name` in `[dependencies]`, creating the table when
/// missing. Returns `true` when the dependency was already present (and got
/// replaced).
pub fn upsert_dependency(
    manifest_path: &Path,
    name: &str,
    spec: &DependencySpec,
) -> Result<bool, ProjectError> {
    let mut replaced = false;
    with_manifest_edit(manifest_path, |doc| {
        if !doc.contains_key("dependencies") {
            doc["dependencies"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let deps_table =
            doc["dependencies"]
                .as_table_mut()
                .ok_or_else(|| ProjectError::ManifestStructure {
                    path: manifest_path.to_path_buf(),
                    message: "[dependencies] is not a table".to_string(),
                })?;
        replaced = deps_table.contains_key(name);
        deps_table[name] = dependency_item(spec);
        Ok(())
    })?;
    info!("Updated hpm.toml dependency: {}", name);
    Ok(replaced)
}

/// Remove `name` from `[dependencies]`. Returns `true` when an entry was
/// actually removed. A missing manifest is treated as nothing-to-remove.
pub fn remove_dependency(manifest_path: &Path, name: &str) -> Result<bool, ProjectError> {
    if !manifest_path.exists() {
        return Ok(false);
    }
    let mut removed = false;
    with_manifest_edit(manifest_path, |doc| {
        if let Some(deps) = doc.get_mut("dependencies")
            && let Some(table) = deps.as_table_mut()
            && table.contains_key(name)
        {
            table.remove(name);
            removed = true;
            info!("Removed {} from [dependencies]", name);
        }
        Ok(())
    })?;
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const MANIFEST: &str = r#"# Project manifest — comment must survive edits.
[package]
path = "studio/test"
name = "Test" # inline comment
version = "1.0.0"

[dependencies]
existing = "1.2.3" # keep me
"#;

    fn write_manifest(dir: &TempDir) -> std::path::PathBuf {
        let path = dir.path().join("hpm.toml");
        std::fs::write(&path, MANIFEST).unwrap();
        path
    }

    #[test]
    fn upsert_preserves_comments_and_formatting() {
        let dir = TempDir::new().unwrap();
        let path = write_manifest(&dir);

        let added = upsert_dependency(
            &path,
            "acme/tools",
            &DependencySpec::registry("2.0.0", None),
        )
        .unwrap();
        assert!(!added, "new entry is not a replacement");

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("comment must survive edits"));
        assert!(content.contains("# inline comment"));
        assert!(content.contains("# keep me"));
        assert!(content.contains(r#""acme/tools" = "2.0.0""#));
    }

    /// A `--registry` pin has to survive the write, otherwise the dependency
    /// silently reverts to resolving across every configured registry on the
    /// next install.
    #[test]
    fn upsert_persists_a_registry_pin() {
        let dir = TempDir::new().unwrap();
        let path = write_manifest(&dir);

        upsert_dependency(
            &path,
            "acme/tools",
            &DependencySpec::registry("2.0.0", Some("houdinihub".to_string())),
        )
        .unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            content.contains(r#"registry = "houdinihub""#),
            "pin missing from manifest:\n{content}"
        );

        // And it must parse back out as a pin, not just be inert text.
        let reloaded = hpm_package::PackageManifest::from_path(&path).unwrap();
        let spec = reloaded.dependencies.get("acme/tools").unwrap();
        assert_eq!(spec.registry_name(), Some("houdinihub"));
    }

    #[test]
    fn upsert_replaces_existing_entry() {
        let dir = TempDir::new().unwrap();
        let path = write_manifest(&dir);

        let replaced =
            upsert_dependency(&path, "existing", &DependencySpec::registry("9.9.9", None)).unwrap();
        assert!(replaced);
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains(r#"existing = "9.9.9""#));
    }

    #[test]
    fn upsert_writes_inline_table_specs() {
        let dir = TempDir::new().unwrap();
        let path = write_manifest(&dir);

        upsert_dependency(
            &path,
            "dev-pkg",
            &DependencySpec::Path {
                path: "../dev-pkg".to_string(),
                optional: false,
                link: true,
            },
        )
        .unwrap();

        // The written shape must round-trip through the typed parser.
        let manifest = hpm_package::PackageManifest::from_path(&path).unwrap();
        let spec = manifest.dependencies.get("dev-pkg").unwrap();
        assert!(matches!(
            spec,
            DependencySpec::Path {
                link: true,
                optional: false,
                ..
            }
        ));
    }

    #[test]
    fn remove_reports_presence() {
        let dir = TempDir::new().unwrap();
        let path = write_manifest(&dir);

        assert!(remove_dependency(&path, "existing").unwrap());
        assert!(!remove_dependency(&path, "existing").unwrap());
        assert!(!remove_dependency(&path, "never-there").unwrap());

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(!content.contains("existing = "));
        assert!(content.contains("comment must survive edits"));
    }
}
