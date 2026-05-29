//! `hpm migrate` — rewrite a pre-0.16 `hpm.toml` to the current schema.
//!
//! Reading the old format is transparent everywhere (see
//! [`hpm_package::parse_manifest_str`]); this command makes the conversion
//! permanent by writing the current shape back to disk. The lossy
//! `[native]` -> `[stage]` step is best-effort — the derived place-rule
//! destinations are flagged for review both in the terminal and as a comment
//! block at the top of the rewritten file.

use anyhow::{Context, Result};
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;

use hpm_package::{LEGACY_MANIFEST_SUNSET, MigrationReport};

use super::manifest_utils::determine_manifest_path;
use crate::console::Console;

/// Run the migration. Returns `true` when the manifest was (or needed to be)
/// migrated, `false` when it was already on the current format — the caller
/// maps that onto the process exit status for `--check`.
pub async fn migrate_manifest(
    manifest: Option<PathBuf>,
    stdout: bool,
    check: bool,
    console: &mut Console,
) -> Result<bool> {
    let path = determine_manifest_path(manifest)?;
    let content = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read manifest: {}", path.display()))?;

    let (migrated, report) = hpm_package::parse_manifest_str(&content)
        .with_context(|| format!("Failed to parse manifest: {}", path.display()))?;

    let Some(report) = report else {
        console.success(format!(
            "{} is already on the current format; nothing to migrate.",
            path.display()
        ));
        return Ok(false);
    };

    if check {
        console.warn(format!(
            "{} uses the pre-0.16 format and needs migration (run `hpm migrate`).",
            path.display()
        ));
        return Ok(true);
    }

    let body =
        toml::to_string_pretty(&migrated).context("Failed to serialize migrated manifest")?;
    let output = prepend_header(&body, &report);

    if stdout {
        print!("{output}");
        return Ok(true);
    }

    // Back up the original next to it as `hpm.toml.bak` before overwriting.
    let backup = append_extension(&path, "bak");
    fs::copy(&path, &backup)
        .with_context(|| format!("Failed to back up manifest to {}", backup.display()))?;
    fs::write(&path, &output)
        .with_context(|| format!("Failed to write migrated manifest: {}", path.display()))?;

    console.success(format!(
        "Migrated {} to the current format (original saved as {}).",
        path.display(),
        backup.display()
    ));
    for w in &report.warnings {
        console.warn(format!("review: {}", w));
    }

    Ok(true)
}

/// Prepend a provenance + review comment block to the serialized manifest.
fn prepend_header(body: &str, report: &MigrationReport) -> String {
    let mut header = String::new();
    header.push_str(&format!(
        "# Migrated from the pre-0.16 hpm.toml format by `hpm migrate`.\n\
         # Legacy support is removed in {LEGACY_MANIFEST_SUNSET}.\n"
    ));
    if !report.is_empty() {
        header.push_str("# Review the following before publishing:\n");
        for w in &report.warnings {
            header.push_str(&format!("#   - {w}\n"));
        }
    }
    header.push('\n');
    header.push_str(body);
    header
}

/// Append an extension to a path, keeping the existing one
/// (`hpm.toml` -> `hpm.toml.bak`), unlike `Path::with_extension` which would
/// replace it.
fn append_extension(path: &std::path::Path, ext: &str) -> PathBuf {
    let mut s: OsString = path.as_os_str().to_owned();
    s.push(".");
    s.push(ext);
    PathBuf::from(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::Console;
    use tempfile::TempDir;

    const LEGACY: &str = r#"
[package]
path = "studio/my-pkg"
name = "My Pkg"
version = "1.0.0"

[houdini]
min_version = "20.5"
max_version = "21.0"

[native]
platforms = ["linux-x86_64"]

[native.linux-x86_64]
files = ["dso/linux-x86_64/*"]
"#;

    const CURRENT: &str = r#"
[package]
path = "studio/my-pkg"
name = "My Pkg"
version = "1.0.0"

[compat]
houdini = ">=20.5"
"#;

    fn write_manifest(content: &str) -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("hpm.toml");
        fs::write(&path, content).unwrap();
        (dir, path)
    }

    #[tokio::test]
    async fn default_writes_in_place_with_backup() {
        let (_dir, path) = write_manifest(LEGACY);
        let mut console = Console::new();

        let needed = migrate_manifest(Some(path.clone()), false, false, &mut console)
            .await
            .unwrap();
        assert!(needed);

        // Backup preserves the original.
        let backup = append_extension(&path, "bak");
        assert!(backup.exists());
        assert_eq!(fs::read_to_string(&backup).unwrap(), LEGACY);

        // The rewritten file is now on the current format.
        let rewritten = fs::read_to_string(&path).unwrap();
        let (_, report) = hpm_package::parse_manifest_str(&rewritten).unwrap();
        assert!(report.is_none(), "rewritten manifest is current-format");
        assert!(rewritten.contains("[compat]"));
        assert!(rewritten.contains("stage.platform.linux-x86_64"));
        // Provenance + review comment header is present.
        assert!(rewritten.starts_with("# Migrated from the pre-0.16"));
    }

    #[tokio::test]
    async fn stdout_leaves_file_untouched() {
        let (_dir, path) = write_manifest(LEGACY);
        let mut console = Console::new();

        let needed = migrate_manifest(Some(path.clone()), true, false, &mut console)
            .await
            .unwrap();
        assert!(needed);
        assert_eq!(fs::read_to_string(&path).unwrap(), LEGACY);
        assert!(!append_extension(&path, "bak").exists());
    }

    #[tokio::test]
    async fn check_reports_without_writing() {
        let (_dir, path) = write_manifest(LEGACY);
        let mut console = Console::new();

        let needed = migrate_manifest(Some(path.clone()), false, true, &mut console)
            .await
            .unwrap();
        assert!(needed, "legacy manifest needs migration");
        assert_eq!(fs::read_to_string(&path).unwrap(), LEGACY);
        assert!(!append_extension(&path, "bak").exists());
    }

    #[tokio::test]
    async fn current_manifest_needs_nothing() {
        let (_dir, path) = write_manifest(CURRENT);
        let mut console = Console::new();

        let needed = migrate_manifest(Some(path.clone()), false, false, &mut console)
            .await
            .unwrap();
        assert!(!needed);
        assert_eq!(fs::read_to_string(&path).unwrap(), CURRENT);
        assert!(!append_extension(&path, "bak").exists());
    }
}
