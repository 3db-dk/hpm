//! Shared test fixtures for CLI command tests.

use anyhow::Result;
use std::path::Path;

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
name = "test-package"
version = "1.0.0"
description = "A test package"
authors = ["Test Author <test@example.com>"]
license = "MIT"

[houdini]
min_version = "20.0"
"#
    .to_string();

    if opts.include_deps {
        content.push_str(&format!(
            r#"
[dependencies]
utility-nodes = {{ git = "{scheme}://github.com/studio/utility-nodes", version = "1.0.0" }}
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
