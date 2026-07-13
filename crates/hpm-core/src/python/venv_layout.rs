//! On-disk layout of a venv created by `uv venv`.
//!
//! Every path derived from a venv root goes through here — the
//! `bin`/`Scripts` and `lib/pythonX.Y`/`Lib` split is platform lore that
//! must not be re-encoded per call site.

use super::types::PythonVersion;
use std::path::{Path, PathBuf};

/// Directory holding executables (`bin` on Unix, `Scripts` on Windows).
/// Prepended to `PATH` so `python` resolves to the venv interpreter.
pub fn bin_dir(venv_path: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        venv_path.join("Scripts")
    }
    #[cfg(not(target_os = "windows"))]
    {
        venv_path.join("bin")
    }
}

/// Absolute path to the Python interpreter inside the venv.
pub fn python_executable(venv_path: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        bin_dir(venv_path).join("python.exe")
    }
    #[cfg(not(target_os = "windows"))]
    {
        bin_dir(venv_path).join("python")
    }
}

/// The venv's `site-packages` directory. The caller supplies the Python
/// version (already known from the resolved dependency set) so we don't
/// have to parse `pyvenv.cfg`.
pub fn site_packages_dir(venv_path: &Path, python_version: &PythonVersion) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let _ = python_version; // Windows venvs share one Lib/site-packages
        venv_path.join("Lib").join("site-packages")
    }
    #[cfg(not(target_os = "windows"))]
    {
        venv_path
            .join("lib")
            .join(format!(
                "python{}.{}",
                python_version.major, python_version.minor
            ))
            .join("site-packages")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_uv() {
        let root = Path::new("/tmp/venv");
        #[cfg(target_os = "windows")]
        {
            assert!(bin_dir(root).ends_with("Scripts"));
            assert!(python_executable(root).ends_with("Scripts/python.exe"));
            assert!(
                site_packages_dir(root, &PythonVersion::new(3, 11, None))
                    .ends_with("Lib/site-packages")
            );
        }
        #[cfg(not(target_os = "windows"))]
        {
            assert!(bin_dir(root).ends_with("bin"));
            assert!(python_executable(root).ends_with("bin/python"));
            assert!(
                site_packages_dir(root, &PythonVersion::new(3, 11, None))
                    .ends_with("lib/python3.11/site-packages")
            );
        }
    }
}
