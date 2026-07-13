//! `hpm update` — recompute the highest matching version per registry
//! dependency and bump `hpm.toml` to that exact version, then re-install.
//!
//! ## Range semantics
//!
//! Each dependency's spec is parsed as a `semver::VersionReq`. The
//! registry is queried for all versions of the package; non-yanked entries
//! matching the requirement are sorted and the highest picked. If that
//! differs from the currently-locked version it's reported as an update.
//!
//! ## Apply
//!
//! Because today's install path resolves each dep against an *exact*
//! version (it calls `Registry::get_version(name, version_string)` with
//! the manifest's version verbatim), applying an update rewrites the
//! manifest spec to the resolved exact version. Users who want continued
//! range tracking after an update need to re-add `^`/`~` manually. This
//! trade is documented; the alternative — lockfile-driven install — is
//! a larger architectural change.
//!
//! ## Python dependencies
//!
//! Python deps are not touched here. UV already picks the latest version
//! matching the spec on every install; re-running `hpm install` is
//! enough to update them.

use super::manifest_utils::{determine_manifest_path, load_manifest};
use crate::console::Console;
use crate::output::OutputFormat;
use anyhow::{Context, Result, bail};
use hpm_config::Config;
use hpm_core::project::manifest_edit;
use hpm_core::{LockFile, registry::RegistrySet};
use hpm_package::DependencySpec;
use std::collections::HashSet;
use std::path::PathBuf;
use tracing::warn;

#[derive(Debug, Clone)]
pub struct UpdateOptions {
    pub package: Option<PathBuf>,
    pub packages: Vec<String>,
    pub dry_run: bool,
    pub yes: bool,
    pub output: OutputFormat,
}

impl Default for UpdateOptions {
    fn default() -> Self {
        Self {
            package: None,
            packages: Vec::new(),
            dry_run: false,
            yes: false,
            output: OutputFormat::Human,
        }
    }
}

/// A single dependency the user could update.
#[derive(Debug, Clone)]
struct Candidate {
    name: String,
    locked: Option<String>,
    latest: String,
    /// True when the registry confirmed the *currently-locked* version is
    /// yanked. The upgrade itself filters yanks; this flag drives a
    /// per-package WARN so users know their existing pin is unsafe.
    locked_is_yanked: bool,
}

pub async fn update_packages(config: &Config, options: UpdateOptions) -> Result<()> {
    let manifest_path = determine_manifest_path(options.package.clone())?;
    let manifest = load_manifest(&manifest_path)?;
    let project_dir = manifest_path
        .parent()
        .context("Manifest file has no parent directory")?;
    let lock_path = project_dir.join("hpm.lock");

    let existing_lock = LockFile::load(&lock_path).ok();
    let registry_set = RegistrySet::from_config(config)?;
    if registry_set.is_empty() {
        bail!(
            "Cannot check for updates: no registries are configured. \
             Run `hpm registry add <url>` first."
        );
    }

    let filter: HashSet<&str> = options.packages.iter().map(|s| s.as_str()).collect();

    let candidates =
        collect_candidates(&registry_set, &manifest, existing_lock.as_ref(), &filter).await?;

    if candidates.is_empty() {
        Console::new().success("All HPM packages are up to date");
        return Ok(());
    }

    print_candidates(&candidates, options.output);

    if options.dry_run {
        return Ok(());
    }

    if !options.yes
        && matches!(options.output, OutputFormat::Human)
        && !Console::new().confirm("Apply these updates?")?
    {
        println!("Update cancelled");
        return Ok(());
    }

    // Rewrite each candidate's spec to the resolved exact version through
    // the formatting-preserving editor, then run install to fetch + lock.
    apply_updates(&manifest, &manifest_path, &candidates)
        .with_context(|| format!("Failed to write {}", manifest_path.display()))?;

    super::install::install_dependencies(config, Some(manifest_path), false)
        .await
        .context("Failed to install the updated dependency set")?;

    Console::new().success(format!("Updated {} package(s)", candidates.len()));
    Ok(())
}

/// Walk the manifest's `[dependencies]`, query each registry-resolvable
/// entry, return one `Candidate` per dep whose latest-matching version
/// differs from the lockfile (or that has no lockfile entry yet).
async fn collect_candidates(
    registry_set: &RegistrySet,
    manifest: &hpm_package::PackageManifest,
    existing_lock: Option<&LockFile>,
    filter: &HashSet<&str>,
) -> Result<Vec<Candidate>> {
    if manifest.dependencies.is_empty() {
        return Ok(Vec::new());
    }

    let mut candidates = Vec::new();
    for (name, spec) in &manifest.dependencies {
        if !filter.is_empty() && !filter.contains(name.as_str()) {
            continue;
        }
        // URL and Path deps don't update through a registry.
        let ver_req_str = match spec {
            DependencySpec::Simple(v) | DependencySpec::Registry { version: v, .. } => v,
            DependencySpec::Url { .. } | DependencySpec::Path { .. } => continue,
        };

        let req = match semver::VersionReq::parse(ver_req_str) {
            Ok(r) => r,
            Err(e) => {
                warn!(
                    "Skipping {}: invalid version requirement '{}': {}",
                    name, ver_req_str, e
                );
                continue;
            }
        };

        let entries = registry_set
            .get_versions(name)
            .await
            .with_context(|| format!("Failed to query registry for {name}"))?;

        let locked = existing_lock
            .and_then(|l| l.get_dependency(name))
            .map(|d| d.version.clone());

        let locked_is_yanked = if let Some(ref l) = locked {
            entries.iter().any(|e| e.version == *l && e.yanked)
        } else {
            false
        };

        let latest = match hpm_core::registry::highest_matching(&entries, &req) {
            Some(entry) => entry.version.clone(),
            None => continue,
        };

        if Some(&latest) != locked.as_ref() {
            candidates.push(Candidate {
                name: name.clone(),
                locked,
                latest,
                locked_is_yanked,
            });
        }
    }
    Ok(candidates)
}

fn print_candidates(candidates: &[Candidate], output: OutputFormat) {
    match output {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            let payload: Vec<_> = candidates
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "name": c.name,
                        "current": c.locked,
                        "latest": c.latest,
                        "currentIsYanked": c.locked_is_yanked,
                    })
                })
                .collect();
            let body = serde_json::json!({"updates": payload});
            let s = if matches!(output, OutputFormat::JsonCompact) {
                body.to_string()
            } else {
                serde_json::to_string_pretty(&body).unwrap()
            };
            println!("{}", s);
        }
        OutputFormat::JsonLines => {
            for c in candidates {
                println!(
                    "{}",
                    serde_json::json!({
                        "name": c.name,
                        "current": c.locked,
                        "latest": c.latest,
                        "currentIsYanked": c.locked_is_yanked,
                    })
                );
            }
        }
        OutputFormat::Human => {
            println!("Available updates:");
            for c in candidates {
                let current = c.locked.as_deref().unwrap_or("<not locked>");
                println!("  {}: {} -> {}", c.name, current, c.latest);
                if c.locked_is_yanked {
                    println!(
                        "    note: the locked {} {} has been yanked",
                        c.name, current
                    );
                }
            }
        }
    }
}

/// Mutate `manifest.dependencies` so each candidate's spec becomes the
/// resolved exact version. `Simple` becomes `Simple(new_version)`;
/// `Registry { version, .. }` retains its `registry` / `optional` fields
/// with `version` replaced.
fn apply_updates(
    manifest: &hpm_package::PackageManifest,
    manifest_path: &std::path::Path,
    candidates: &[Candidate],
) -> Result<()> {
    for c in candidates {
        let Some(spec) = manifest.dependencies.get(&c.name) else {
            continue;
        };
        let new_spec = match spec {
            DependencySpec::Simple(_) => DependencySpec::Simple(c.latest.clone()),
            DependencySpec::Registry {
                registry, optional, ..
            } => DependencySpec::Registry {
                version: c.latest.clone(),
                registry: registry.clone(),
                optional: *optional,
            },
            DependencySpec::Url { .. } | DependencySpec::Path { .. } => {
                // collect_candidates already filtered these out.
                continue;
            }
        };
        manifest_edit::upsert_dependency(manifest_path, &c.name, &new_spec)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_options_default() {
        let options = UpdateOptions::default();
        assert!(options.packages.is_empty());
        assert!(!options.dry_run);
        assert!(!options.yes);
        assert!(matches!(options.output, OutputFormat::Human));
    }

    #[test]
    fn apply_updates_rewrites_simple_specs() {
        use hpm_package::PackageManifest;

        let dir = tempfile::TempDir::new().unwrap();
        let manifest_path = dir.path().join("hpm.toml");
        std::fs::write(
            &manifest_path,
            r#"# top comment
[package]
path = "studio/project"
name = "project"
version = "1.0.0"

[dependencies]
foo = "^1.0.0"
bar = { version = "^2.0.0", registry = "main" }
"#,
        )
        .unwrap();
        let manifest = PackageManifest::from_path(&manifest_path).unwrap();

        let candidates = vec![
            Candidate {
                name: "foo".to_string(),
                locked: Some("1.0.0".to_string()),
                latest: "1.0.5".to_string(),
                locked_is_yanked: false,
            },
            Candidate {
                name: "bar".to_string(),
                locked: Some("2.0.1".to_string()),
                latest: "2.1.0".to_string(),
                locked_is_yanked: false,
            },
        ];

        apply_updates(&manifest, &manifest_path, &candidates).unwrap();

        let updated = PackageManifest::from_path(&manifest_path).unwrap();
        match &updated.dependencies["foo"] {
            DependencySpec::Simple(v) => assert_eq!(v, "1.0.5"),
            other => panic!("expected Simple, got {:?}", other),
        }
        match &updated.dependencies["bar"] {
            DependencySpec::Registry {
                version, registry, ..
            } => {
                assert_eq!(version, "2.1.0");
                assert_eq!(registry.as_deref(), Some("main"));
            }
            other => panic!("expected Registry, got {:?}", other),
        }
        // Formatting-preserving edit: comments survive.
        let content = std::fs::read_to_string(&manifest_path).unwrap();
        assert!(content.contains("# top comment"));
    }
}
