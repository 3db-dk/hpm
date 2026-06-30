//! `hpm run <script>` — execute a `[scripts]` entry from `hpm.toml`.
//!
//! Thin CLI wrapper: resolves the manifest, then delegates to the shared
//! [`hpm_core::script_run`] runner with a [`ConsoleSink`]. The env contract
//! (per-script venv, package environment, `HPM_PACKAGE_ROOT`) lives in
//! hpm-core so every embedder runs scripts the same way.

use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::commands::manifest_utils::{determine_manifest_path, load_manifest};
use crate::console::Console;
use crate::script_sink::ConsoleSink;

/// Run the named `[scripts]` entry in `hpm.toml`, forwarding `extra_args`.
///
/// Returns the script's exit code so the caller can propagate it as the
/// `hpm` exit status.
pub async fn run_script(
    script: &str,
    extra_args: &[String],
    directory: Option<PathBuf>,
    extra_env: &HashMap<String, String>,
    console: &mut Console,
) -> Result<i32> {
    let manifest_path = determine_manifest_path(directory)?;
    let manifest = load_manifest(&manifest_path)?;
    let package_root = manifest_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    let mut sink = ConsoleSink::new(console);
    hpm_core::script_run::run_script(
        &manifest,
        script,
        &package_root,
        extra_args,
        extra_env,
        &mut sink,
    )
    .await
}
