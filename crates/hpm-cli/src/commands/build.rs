//! `hpm build` — materialise the install image into a directory.
//!
//! Runs `[stage].prepack` scripts (compile DSO, collapse expanded HDAs,
//! etc.) in sequence, then copies workspace files into the output dir
//! using the same include/exclude/place rules `hpm pack` would apply to
//! a direct archive. The result is a directory layout identical to what
//! a registry consumer's install would see.
//!
//! Output location:
//! - Default: `[stage].output_dir` (typically `dist/`) under the manifest dir.
//! - Overridable per invocation via `--output <dir>`. Absolute paths used
//!   verbatim; relative paths resolve against the manifest dir.
//!
//! Per-session staging: users running multiple Houdini sessions in
//! parallel typically point each at its own `--output <tmpdir>`, so
//! rebuilding one session's image doesn't fight another session's loaded
//! DSOs on Windows. HPM is a one-shot copier — managing those output
//! paths is the user's responsibility, not a background-service concern.

use anyhow::{Context, Result, bail};
use hpm_core::packer::StageFilter;
use hpm_package::path_util::relative_path_to_forward_slash;
use hpm_package::{PackageManifest, Platform};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::commands::manifest_utils::{determine_manifest_path, load_manifest};
use crate::commands::run::run_script;
use crate::console::Console;

pub struct BuildOptions {
    /// Manifest or its containing directory. None = cwd.
    pub manifest: Option<PathBuf>,
    /// Target platform; defaults to host when `[compat].platforms` is declared.
    pub platform: Option<String>,
    /// Override `[stage].output_dir`. Absolute paths are used verbatim;
    /// relative paths resolve against the manifest directory. Useful when
    /// running multiple Houdini sessions, each with its own staged image
    /// in a separate temp directory.
    pub output: Option<PathBuf>,
    /// Build profile selecting a `[stage.profile.<name>]` table (default
    /// `"release"`). Always exposed to prepack scripts as `HPM_BUILD_PROFILE`.
    pub profile: String,
    /// Skip `[stage].prepack`. CI sometimes runs prepack steps separately.
    pub no_prepack: bool,
    /// Wipe the output directory before populating it. Default true; users
    /// running multiple sessions against distinct `--output` directories
    /// typically leave this on.
    pub clean: bool,
}

pub async fn build(options: BuildOptions, console: &mut Console) -> Result<()> {
    let manifest_path = determine_manifest_path(options.manifest.clone())?;
    let manifest = load_manifest(&manifest_path)?;
    manifest
        .validate()
        .map_err(|e| anyhow::anyhow!("Invalid manifest: {}", e))?;

    let package_root = manifest_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    if manifest.stage.is_empty() {
        anyhow::bail!("Package has no [stage] section — nothing to build");
    }

    let platform = resolve_target_platform(&manifest, options.platform.as_deref())?;

    // A non-default profile that names no declared table still sets
    // HPM_BUILD_PROFILE, but its absence is usually a typo worth surfacing.
    if options.profile != "release" && !manifest.stage.has_profile(&options.profile) {
        console.warn(format!(
            "profile '{}' has no [stage.profile.{}] table; using base [stage]",
            options.profile, options.profile
        ));
    }
    let stage = manifest.stage.resolved_for_profile(&options.profile);

    // Context exposed to prepack scripts (and any [scripts] they invoke):
    // the selected build profile and, when known, the target platform.
    let mut prepack_env: HashMap<String, String> = HashMap::new();
    prepack_env.insert("HPM_BUILD_PROFILE".to_string(), options.profile.clone());
    if let Some(p) = &platform {
        prepack_env.insert("HPM_PLATFORM".to_string(), p.as_str().to_string());
    }

    if !options.no_prepack && !stage.prepack.is_empty() {
        run_prepack(
            &manifest,
            &stage.prepack,
            options.manifest.clone(),
            &prepack_env,
            console,
        )
        .await?;
    }

    let output_dir = match &options.output {
        Some(p) if p.is_absolute() => p.clone(),
        Some(p) => package_root.join(p),
        None => package_root.join(stage.effective_output_dir()),
    };
    if options.clean && output_dir.exists() {
        fs::remove_dir_all(&output_dir)
            .with_context(|| format!("Failed to clear {}", output_dir.display()))?;
    }
    fs::create_dir_all(&output_dir)
        .with_context(|| format!("Failed to create {}", output_dir.display()))?;

    let filter = StageFilter::new(&stage, platform.as_ref())
        .map_err(|e| anyhow::anyhow!("Failed to build stage filter: {}", e))?;
    let ignore = build_ignore_rules(&package_root)
        .with_context(|| format!("Failed to read ignore rules in {}", package_root.display()))?;

    let mut copied = 0usize;
    for entry in WalkDir::new(&package_root).sort_by_file_name() {
        let entry = entry.map_err(|e| anyhow::anyhow!("Walk error: {}", e))?;
        let path = entry.path();
        if path == package_root {
            continue;
        }
        let relative = path.strip_prefix(&package_root).unwrap_or(path);

        // Always skip the output_dir if it sits inside the package root —
        // otherwise a re-run would recursively pack the previous staging
        // output. External --output paths (a temp dir per Houdini session,
        // typically) live outside the workspace so this check is a no-op.
        if let Ok(out_rel) = output_dir.strip_prefix(&package_root)
            && relative.starts_with(out_rel)
        {
            continue;
        }

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
        let rel_str = relative_path_to_forward_slash(relative);
        let archive_path = match filter.archive_path_for(&rel_str) {
            Some(p) => p,
            None => continue,
        };
        let dest = output_dir.join(&archive_path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        fs::copy(path, &dest)
            .with_context(|| format!("Failed to copy {} -> {}", path.display(), dest.display()))?;
        copied += 1;
    }

    console.success(format!(
        "Built {} v{} ({} file(s), profile={}{})",
        manifest.package.name,
        manifest.package.version,
        copied,
        options.profile,
        platform
            .as_ref()
            .map(|p| format!(", platform={}", p))
            .unwrap_or_default()
    ));
    println!("  output: {}", output_dir.display());

    Ok(())
}

fn resolve_target_platform(
    manifest: &PackageManifest,
    requested: Option<&str>,
) -> Result<Option<Platform>> {
    let declared = &manifest.compat.platforms;
    match (requested, declared.is_empty()) {
        (Some(_), true) => {
            bail!("--platform was specified but package has no [compat].platforms")
        }
        (Some(p), false) => {
            let platform: Platform = p.parse().map_err(|e: String| anyhow::anyhow!(e))?;
            if !declared.contains(&platform) {
                bail!(
                    "Platform '{}' is not declared in [compat].platforms: {:?}",
                    platform,
                    declared
                );
            }
            Ok(Some(platform))
        }
        (None, false) => {
            let detected = Platform::current()
                .context("Could not detect host platform; pass --platform explicitly")?;
            if !declared.contains(&detected) {
                bail!(
                    "Host platform '{}' is not declared in [compat].platforms: {:?}. \
                     Pass --platform to target a different one.",
                    detected,
                    declared
                );
            }
            Ok(Some(detected))
        }
        (None, true) => Ok(None),
    }
}

async fn run_prepack(
    manifest: &PackageManifest,
    names: &[String],
    manifest_arg: Option<PathBuf>,
    extra_env: &HashMap<String, String>,
    console: &mut Console,
) -> Result<()> {
    for name in names {
        if manifest.script_for(name).is_none() {
            bail!(
                "[stage].prepack references '{}' but no such [scripts] entry exists",
                name
            );
        }
        console.info(format!("prepack: {}", name));
        let code = run_script(name, &[], manifest_arg.clone(), extra_env, console).await?;
        if code != 0 {
            bail!(
                "Prepack script '{}' exited with status {} — aborting build",
                name,
                code
            );
        }
    }
    Ok(())
}

fn build_ignore_rules(dir: &Path) -> Result<Gitignore> {
    let mut builder = GitignoreBuilder::new(dir);
    builder
        .add_line(None, ".git/")
        .context("Adding .git/ to ignore rules")?;
    builder
        .add_line(None, ".hpm/")
        .context("Adding .hpm/ to ignore rules")?;
    let gitignore = dir.join(".gitignore");
    if gitignore.exists() {
        builder.add(gitignore);
    }
    let hpmignore = dir.join(".hpmignore");
    if hpmignore.exists() {
        builder.add(hpmignore);
    }
    Ok(builder.build()?)
}
