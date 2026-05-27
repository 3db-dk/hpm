//! Shared test fixtures for CLI command tests.

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

/// Global mutex serializing tests that mutate the process-wide current directory.
///
/// Any test calling `env::set_current_dir()` or `env::current_dir()` must acquire
/// this lock via [`CwdGuard`] to avoid races with parallel tests whose TempDirs
/// may be dropped out from under them.
static CWD_MUTEX: Mutex<()> = Mutex::new(());

/// RAII guard that locks the cwd mutex, changes directory, and restores it on drop.
///
/// Tests that mutate the current directory should construct one of these at the
/// start of the test and let it drop at the end.
pub(crate) struct CwdGuard {
    _guard: MutexGuard<'static, ()>,
    original: PathBuf,
}

impl CwdGuard {
    /// Lock the cwd mutex and change directory to `new_dir`, saving the original.
    pub(crate) fn enter(new_dir: &Path) -> Self {
        let guard = CWD_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let original = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        std::env::set_current_dir(new_dir).unwrap();
        Self {
            _guard: guard,
            original,
        }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.original);
    }
}

/// Options for generating a test hpm.toml manifest.
#[derive(Default)]
pub(crate) struct TestManifestOpts {
    /// Include HPM dependencies section.
    pub include_deps: bool,
    /// Include Python dependencies section.
    pub include_python_deps: bool,
    /// Use HTTP (insecure) URLs instead of HTTPS for git deps.
    pub use_http: bool,
}

/// Write a test hpm.toml manifest file at `dir/hpm.toml`.
pub(crate) fn write_test_manifest(dir: &Path, opts: TestManifestOpts) -> Result<()> {
    let scheme = if opts.use_http { "http" } else { "https" };

    let mut content = r#"[package]
path = "studio/test-package"
name = "test-package"
version = "1.0.0"
description = "A test package"
authors = ["Test Author <test@example.com>"]
license = "MIT"

[compat]
houdini = ">=20.5"
"#
    .to_string();

    if opts.include_deps {
        content.push_str(&format!(
            r#"
[dependencies]
utility-nodes = {{ url = "{scheme}://example.com/packages/utility-nodes/1.0.0/utility-nodes-1.0.0.zip", version = "1.0.0" }}
material-library = {{ path = "../material-library", optional = true }}
"#
        ));
    }

    if opts.include_python_deps {
        content.push_str(
            r#"
[python_dependencies]
numpy = ">=1.20.0"
requests = { version = ">=2.25.0", extras = ["security"] }
matplotlib = { version = "^3.5.0", optional = true }
"#,
        );
    }

    std::fs::write(dir.join("hpm.toml"), content)?;
    Ok(())
}
