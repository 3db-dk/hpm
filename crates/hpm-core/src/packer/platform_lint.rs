//! Pack-time inspection of a platform archive's native payload.
//!
//! A per-platform archive is the one part of a package CI builds and nobody
//! looks at, and the most damaging way to get it wrong leaves no trace: a
//! toolchain links newer glibc symbol versions purely because of the host it
//! built on, even when the code uses nothing new, and the resulting binary is
//! rejected outright by the dynamic loader anywhere older. The error names a
//! symbol version rather than a package, a shared library simply never loads,
//! and the Windows archive works fine — so it reads as a platform bug.
//!
//! This module **reports**; it does not enforce. What a payload requires is a
//! fact read off the binary. What a package is willing to support is a policy,
//! and a package manager guessing at that policy — against a hardcoded
//! industry baseline, no less — would fail builds over a number the author
//! never wrote. So pack surfaces the requirement (a console warning plus a
//! field in `--json`) and leaves the judgement to whoever owns the release; a
//! CI job wanting a hard gate can assert on the JSON.
//!
//! That is also why there is no manifest key here. An earlier revision added
//! `[compat].glibc` with a VFX-platform default and failed the pack against
//! it. That was the wrong shape twice over: it put one platform's mechanism
//! into a cross-platform schema, and since `[compat]` rejects unknown fields,
//! declaring your way out of the check would itself have broken every older
//! hpm. Recording the discovered requirement into the version record, so
//! `hpm install` can say "this build needs glibc 2.39, you have 2.34" at the
//! point it actually matters, is the natural next step — and needs no schema
//! of its own either.

use std::path::PathBuf;

use super::elf;
// Re-exported: the requirement type is part of this module's public
// surface (`PayloadRequirements::glibc`), while `elf` itself stays private.
pub use super::elf::GlibcVersion;

/// What a platform archive's payload turned out to require.
///
/// Everything here is derived, never declared. An absent field means "nothing
/// detected", not "no requirement" — a payload hpm cannot parse contributes
/// nothing rather than a guess.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PayloadRequirements {
    /// Highest glibc version any shipped ELF references.
    pub glibc: Option<GlibcVersion>,
}

impl PayloadRequirements {
    /// Rendered for `pack --json` as `{"glibc": "2.39"}`. Empty when nothing
    /// was detected, so a consumer can tell "none found" from "found zero".
    pub fn to_json(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        if let Some(g) = self.glibc {
            map.insert("glibc".into(), serde_json::Value::String(g.to_string()));
        }
        serde_json::Value::Object(map)
    }
}

/// A non-fatal observation about the payload. Returned rather than logged so
/// the caller decides how to present it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformWarning {
    pub path: String,
    pub message: String,
}

/// What one inspection pass produced.
#[derive(Debug, Clone, Default)]
pub struct PayloadReport {
    pub requirements: PayloadRequirements,
    pub warnings: Vec<PlatformWarning>,
}

/// Inspect every staged file destined for a Linux archive.
///
/// `entries` is the `(source path, archive path)` list the archive will be
/// built from, so this sees exactly what ships — not whatever happens to be
/// lying around the working tree. Never fails: a file that can't be read, or
/// isn't an inspectable ELF, simply contributes nothing.
pub fn inspect_linux_payload(entries: &[(PathBuf, String)]) -> PayloadReport {
    let mut report = PayloadReport::default();
    // Files carrying the current maximum, so the warning can name the ones
    // actually responsible rather than every binary in the archive.
    let mut carriers: Vec<String> = Vec::new();

    for (source, archive_path) in entries {
        let Some(info) = elf::inspect_path(source) else {
            continue;
        };

        if let Some(required) = info.max_glibc {
            match report.requirements.glibc {
                Some(current) if required < current => {}
                Some(current) if required == current => carriers.push(archive_path.clone()),
                _ => {
                    // A new high-water mark: previous carriers no longer
                    // explain the requirement.
                    carriers.clear();
                    carriers.push(archive_path.clone());
                    report.requirements.glibc = Some(required);
                }
            }
        }

        // A shared object with no RUNPATH/RPATH can only resolve dependencies
        // already loaded or on the default search path. Fine for a Houdini
        // plugin whose every dependency is Houdini's own — and silently fatal
        // the day it ships a sibling library, because the env var packages
        // reach for (`LD_LIBRARY_PATH`) is read once at process start and
        // cannot affect a later `dlopen`.
        if info.is_shared_object && !info.has_runpath && archive_path.ends_with(".so") {
            report.warnings.push(PlatformWarning {
                path: archive_path.clone(),
                message: "shared object declares no RUNPATH/RPATH; it can only resolve libraries \
                     already loaded by the host process. If it ever ships alongside its own \
                     dependencies, link it with -Wl,-rpath,'$ORIGIN' — setting LD_LIBRARY_PATH \
                     from a package cannot fix this, as glibc reads it only at process start."
                    .to_string(),
            });
        }
    }

    // Only worth remarking on when it exceeds what a Houdini host is likely to
    // have. At or below the baseline there is nothing to say.
    if let Some(required) = report.requirements.glibc
        && required > GlibcVersion::VFX_PLATFORM_BASELINE
    {
        carriers.sort();
        report.warnings.push(PlatformWarning {
            path: carriers.join(", "),
            message: format!(
                "payload requires glibc {required}, above the VFX Reference Platform \
                 baseline of {baseline} (the CY2025/CY2026 requirement — Houdini 21 and \
                 22, in practice an EL8-era host). A host with an older glibc cannot load \
                 these at all: the loader rejects them before any package code runs, so it \
                 surfaces as \"version `GLIBC_{required}' not found\", or for a plugin as one \
                 that silently never loads. This is usually accidental — a toolchain links \
                 the newest symbol versions its build host offers even when the code uses \
                 nothing new — so the fix is normally to build on an older host rather than \
                 to change the code.",
                baseline = GlibcVersion::VFX_PLATFORM_BASELINE
            ),
        });
    }

    report
}

/// True when this archive's payload is worth inspecting for glibc.
pub fn targets_linux(platform: Option<&hpm_package::platform::Platform>) -> bool {
    platform.is_some_and(|p| p.to_string().starts_with("linux"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Build a minimal but structurally real ELF64-LE shared object whose
    /// dynamic section declares one version requirement, `GLIBC_<major.minor>`.
    /// Hand-assembled rather than checked in as a fixture so the test states
    /// exactly which bytes drive the result.
    fn elf_requiring_glibc(major: u32, minor: u32) -> Vec<u8> {
        let name = format!("GLIBC_{major}.{minor}\0");
        // Layout (file offset == vaddr, one PT_LOAD covering everything):
        //   0x000 ELF header
        //   0x040 program headers (2 x 56)
        //   0x100 .dynamic
        //   0x200 strtab
        //   0x300 verneed + vernaux
        let mut b = vec![0u8; 0x400];
        b[..4].copy_from_slice(b"\x7fELF");
        b[4] = 2; // ELFCLASS64
        b[5] = 1; // ELFDATA2LSB
        b[6] = 1; // EV_CURRENT
        b[0x10..0x12].copy_from_slice(&3u16.to_le_bytes()); // e_type = ET_DYN
        b[0x20..0x28].copy_from_slice(&0x40u64.to_le_bytes()); // e_phoff
        b[0x36..0x38].copy_from_slice(&56u16.to_le_bytes()); // e_phentsize
        b[0x38..0x3A].copy_from_slice(&2u16.to_le_bytes()); // e_phnum

        // PT_LOAD covering the whole file, vaddr == offset.
        let ph = 0x40;
        b[ph..ph + 4].copy_from_slice(&1u32.to_le_bytes()); // PT_LOAD
        b[ph + 0x08..ph + 0x10].copy_from_slice(&0u64.to_le_bytes()); // p_offset
        b[ph + 0x10..ph + 0x18].copy_from_slice(&0u64.to_le_bytes()); // p_vaddr
        b[ph + 0x20..ph + 0x28].copy_from_slice(&0x400u64.to_le_bytes()); // p_filesz

        // PT_DYNAMIC
        let ph = 0x40 + 56;
        b[ph..ph + 4].copy_from_slice(&2u32.to_le_bytes()); // PT_DYNAMIC
        b[ph + 0x08..ph + 0x10].copy_from_slice(&0x100u64.to_le_bytes()); // p_offset
        b[ph + 0x10..ph + 0x18].copy_from_slice(&0x100u64.to_le_bytes()); // p_vaddr
        b[ph + 0x20..ph + 0x28].copy_from_slice(&0x80u64.to_le_bytes()); // p_filesz

        // .dynamic: STRTAB, VERNEED, VERNEEDNUM, NULL
        let mut d = 0x100;
        let put = |b: &mut Vec<u8>, d: &mut usize, tag: i64, val: u64| {
            b[*d..*d + 8].copy_from_slice(&tag.to_le_bytes());
            b[*d + 8..*d + 16].copy_from_slice(&val.to_le_bytes());
            *d += 16;
        };
        put(&mut b, &mut d, 5, 0x200); // DT_STRTAB
        put(&mut b, &mut d, 0x6fff_fffe, 0x300); // DT_VERNEED
        put(&mut b, &mut d, 0x6fff_ffff, 1); // DT_VERNEEDNUM
        put(&mut b, &mut d, 0, 0); // DT_NULL

        // strtab: byte 0 is the customary empty string; the version name at 1.
        b[0x201..0x201 + name.len()].copy_from_slice(name.as_bytes());

        // Elf64_Verneed { version, cnt, file, aux, next }
        let vn = 0x300;
        b[vn..vn + 2].copy_from_slice(&1u16.to_le_bytes()); // vn_version
        b[vn + 2..vn + 4].copy_from_slice(&1u16.to_le_bytes()); // vn_cnt
        b[vn + 4..vn + 8].copy_from_slice(&0u32.to_le_bytes()); // vn_file
        b[vn + 8..vn + 12].copy_from_slice(&16u32.to_le_bytes()); // vn_aux
        b[vn + 12..vn + 16].copy_from_slice(&0u32.to_le_bytes()); // vn_next
        // Elf64_Vernaux { hash, flags, other, name, next }
        let va = vn + 16;
        b[va + 8..va + 12].copy_from_slice(&1u32.to_le_bytes()); // vna_name -> strtab+1
        b[va + 12..va + 16].copy_from_slice(&0u32.to_le_bytes()); // vna_next
        b
    }

    fn staged(dir: &Path, name: &str, bytes: &[u8]) -> (PathBuf, String) {
        let p = dir.join(name.replace('/', "_"));
        std::fs::write(&p, bytes).unwrap();
        (p, name.to_string())
    }

    #[test]
    fn non_elf_and_missing_entries_contribute_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let toml = staged(dir.path(), "hpm.toml", b"[package]");
        let gone = (
            Path::new("/nonexistent/definitely/not/here").to_path_buf(),
            "bin/tool".to_string(),
        );
        let report = inspect_linux_payload(&[toml, gone]);
        assert_eq!(report.requirements, PayloadRequirements::default());
        assert!(report.warnings.is_empty());
    }

    /// The case this exists for: TumblePipe shipped Linux binaries needing
    /// GLIBC_2.39 against a platform baseline of 2.28, and nothing noticed
    /// until a user's loader refused them. Reported and warned, never enforced.
    #[test]
    fn requirement_above_the_baseline_is_reported_and_warned() {
        let dir = tempfile::tempdir().unwrap();
        let report = inspect_linux_payload(&[staged(
            dir.path(),
            "bin/tt_setup",
            &elf_requiring_glibc(2, 39),
        )]);

        assert_eq!(report.requirements.glibc, Some(GlibcVersion::new(2, 39)));
        assert_eq!(
            report.requirements.to_json(),
            serde_json::json!({ "glibc": "2.39" })
        );

        let warning = report
            .warnings
            .iter()
            .find(|w| w.message.contains("glibc 2.39"))
            .expect("should warn");
        assert!(warning.path.contains("bin/tt_setup"), "{warning:?}");
        assert!(warning.message.contains("2.28"), "names the baseline");
    }

    /// Below the baseline there is nothing to remark on — but the requirement
    /// is still recorded, because it is a fact about the archive either way.
    #[test]
    fn requirement_below_the_baseline_is_recorded_without_a_warning() {
        let dir = tempfile::tempdir().unwrap();
        let report =
            inspect_linux_payload(&[staged(dir.path(), "bin/tool", &elf_requiring_glibc(2, 17))]);

        assert_eq!(report.requirements.glibc, Some(GlibcVersion::new(2, 17)));
        assert!(
            report
                .warnings
                .iter()
                .all(|w| !w.message.contains("baseline")),
            "{:?}",
            report.warnings
        );
    }

    /// The reported requirement is the highest across the payload, and the
    /// warning names only the files that actually carry it.
    #[test]
    fn highest_requirement_wins_and_names_its_carriers() {
        let dir = tempfile::tempdir().unwrap();
        let report = inspect_linux_payload(&[
            staged(dir.path(), "bin/old", &elf_requiring_glibc(2, 17)),
            staged(dir.path(), "bin/new", &elf_requiring_glibc(2, 39)),
        ]);

        assert_eq!(report.requirements.glibc, Some(GlibcVersion::new(2, 39)));
        let warning = report
            .warnings
            .iter()
            .find(|w| w.message.contains("glibc 2.39"))
            .unwrap();
        assert!(warning.path.contains("bin/new"), "{warning:?}");
        assert!(
            !warning.path.contains("bin/old"),
            "the 2.17 binary is not what forces 2.39: {warning:?}"
        );
    }

    /// Order must not change the outcome — a lower requirement seen after a
    /// higher one must not displace it or its carrier list.
    #[test]
    fn carrier_tracking_is_order_independent() {
        let dir = tempfile::tempdir().unwrap();
        let high = staged(dir.path(), "bin/new", &elf_requiring_glibc(2, 39));
        let low = staged(dir.path(), "bin/old", &elf_requiring_glibc(2, 17));

        let forward = inspect_linux_payload(&[low.clone(), high.clone()]);
        let reverse = inspect_linux_payload(&[high, low]);

        assert_eq!(forward.requirements, reverse.requirements);
        let carriers = |r: &PayloadReport| {
            r.warnings
                .iter()
                .find(|w| w.message.contains("glibc 2.39"))
                .map(|w| w.path.clone())
                .unwrap()
        };
        assert_eq!(carriers(&forward), carriers(&reverse));
        assert_eq!(carriers(&forward), "bin/new");
    }

    #[test]
    fn shared_object_without_runpath_warns() {
        let dir = tempfile::tempdir().unwrap();
        let report = inspect_linux_payload(&[staged(
            dir.path(),
            "lib/tumbleResolver.so",
            &elf_requiring_glibc(2, 17),
        )]);

        let warning = report
            .warnings
            .iter()
            .find(|w| w.message.contains("RUNPATH"))
            .expect("should warn");
        assert!(
            warning.message.contains("LD_LIBRARY_PATH"),
            "should explain why the env-var workaround cannot help: {warning:?}"
        );
    }

    #[test]
    fn empty_requirements_serialise_to_an_empty_object() {
        assert_eq!(
            PayloadRequirements::default().to_json(),
            serde_json::json!({})
        );
    }

    #[test]
    fn linux_platform_detection() {
        use hpm_package::platform::Platform;
        assert!(targets_linux(Some(&Platform::LinuxX86_64)));
        assert!(!targets_linux(Some(&Platform::MacosAarch64)));
        assert!(!targets_linux(Some(&Platform::WindowsX86_64)));
        assert!(!targets_linux(None));
    }
}
