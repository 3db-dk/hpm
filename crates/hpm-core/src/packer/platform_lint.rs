//! Pack-time inspection of a platform archive's native payload.
//!
//! A per-platform archive is the one part of a package that CI builds but
//! nobody looks at. Three separate faults have shipped that way, all with the
//! same shape: the Windows leg works, the Unix leg is malformed, and nothing
//! notices until a user clicks a button. Losing the executable bit was one
//! (now fixed in the packer); requiring a glibc newer than the host is the
//! other, and it is worse, because the loader rejects the binary before any of
//! the package's own code runs and the error names a symbol version rather
//! than a package.
//!
//! So the archive gets read before it ships. The lint is deliberately timid
//! about what it *cannot* determine — an unparseable file is simply not
//! linted — and firm about what it can: a binary that demonstrably cannot load
//! on the declared floor fails the pack.

use std::path::PathBuf;

use hpm_package::manifest::compat::{CompatConfig, GlibcVersion};

use super::PackError;
use super::elf;

/// A non-fatal observation about the payload. Returned rather than logged so
/// the caller decides how to present it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformWarning {
    pub path: String,
    pub message: String,
}

/// Inspect every staged file destined for a Linux archive.
///
/// `entries` is the `(source path, archive path)` list the archive will be
/// built from, so the lint sees exactly what ships — not what happens to be in
/// the working tree.
///
/// Errors when a binary requires a newer glibc than `compat`'s floor. Warnings
/// cover the advisory case (a shared object with no search path of its own).
/// Non-Linux archives are skipped: glibc is not a concept there, and the
/// packer already carries the executable bit for every platform.
pub fn lint_linux_payload(
    entries: &[(PathBuf, String)],
    compat: &CompatConfig,
) -> Result<Vec<PlatformWarning>, PackError> {
    let floor = compat.glibc_floor();
    let mut warnings = Vec::new();
    let mut too_new: Vec<(String, GlibcVersion)> = Vec::new();

    for (source, archive_path) in entries {
        let Some(info) = elf::inspect_path(source) else {
            continue;
        };

        if let Some(required) = info.max_glibc
            && required > floor
        {
            too_new.push((archive_path.clone(), required));
        }

        // A shared object with no RUNPATH/RPATH can only resolve dependencies
        // that are already loaded or sit on the default search path. That is
        // fine for a Houdini plugin whose every dependency is Houdini's own —
        // and silently fatal the day it ships a sibling library, because the
        // env var packages reach for (`LD_LIBRARY_PATH`) is read once at
        // process start and cannot affect a later `dlopen`.
        if info.is_shared_object && !info.has_runpath && archive_path.ends_with(".so") {
            warnings.push(PlatformWarning {
                path: archive_path.clone(),
                message: "shared object declares no RUNPATH/RPATH; it can only resolve libraries \
                     already loaded by the host process. If it ever ships alongside its own \
                     dependencies, link it with -Wl,-rpath,'$ORIGIN' — setting LD_LIBRARY_PATH \
                     from a package cannot fix this, as glibc reads it only at process start."
                    .to_string(),
            });
        }
    }

    if too_new.is_empty() {
        return Ok(warnings);
    }

    too_new.sort();
    let highest = too_new.iter().map(|(_, v)| *v).max().unwrap_or(floor);
    let listed = too_new
        .iter()
        .map(|(p, v)| format!("  {} requires glibc {}", p, v))
        .collect::<Vec<_>>()
        .join("\n");
    let floor_origin = floor_origin_hint(compat);

    Err(PackError::PlatformPayload(format!(
        "this archive's Linux binaries require a newer glibc than the package \
         declares:\n{listed}\n\n\
         The declared floor is glibc {floor}{floor_origin}. A host with an older \
         glibc cannot load these at all — the dynamic loader rejects them before \
         any package code runs, so the failure surfaces as \"version \
         `GLIBC_{highest}' not found\", or for a plugin as one that silently \
         never loads, rather than as anything naming this package.\n\n\
         Either build on an older host so the binaries match the floor, or — if \
         dropping those hosts is intended — say so: [compat] glibc = \"{highest}\".\n\n\
         The requirement is often accidental: a toolchain links newer symbol \
         versions purely because of the host it built on, even when the code \
         uses nothing new."
    )))
}

/// Where the floor came from, so the message doesn't imply the author wrote a
/// number they never wrote.
fn floor_origin_hint(compat: &CompatConfig) -> &'static str {
    if compat.glibc.is_some() {
        " (from [compat].glibc)"
    } else {
        " (the VFX Reference Platform baseline, used because [compat].glibc is unset)"
    }
}

/// True when this archive's payload should be checked for glibc compatibility.
pub fn targets_linux(platform: Option<&hpm_package::platform::Platform>) -> bool {
    platform.is_some_and(|p| p.to_string().starts_with("linux"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn compat_with(glibc: Option<GlibcVersion>) -> CompatConfig {
        CompatConfig {
            glibc,
            ..Default::default()
        }
    }

    #[test]
    fn non_elf_entries_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("hpm.toml");
        std::fs::write(&f, b"[package]").unwrap();
        let entries = vec![(f, "hpm.toml".to_string())];
        assert!(
            lint_linux_payload(&entries, &compat_with(None))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn missing_file_is_ignored_rather_than_failing_the_pack() {
        let entries = vec![(
            Path::new("/nonexistent/definitely/not/here").to_path_buf(),
            "bin/tool".to_string(),
        )];
        assert!(
            lint_linux_payload(&entries, &compat_with(None))
                .unwrap()
                .is_empty()
        );
    }

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

    fn staged(dir: &Path, name: &str, bytes: &[u8]) -> Vec<(PathBuf, String)> {
        let p = dir.join(name.replace('/', "_"));
        std::fs::write(&p, bytes).unwrap();
        vec![(p, name.to_string())]
    }

    /// The case this lint exists for: TumblePipe shipped Linux binaries needing
    /// GLIBC_2.39 against a VFX platform baseline of 2.28, and nothing noticed
    /// until a user's loader refused them.
    #[test]
    fn binary_above_the_floor_fails_the_pack() {
        let dir = tempfile::tempdir().unwrap();
        let entries = staged(dir.path(), "bin/tt_setup", &elf_requiring_glibc(2, 39));

        let err = lint_linux_payload(&entries, &compat_with(None)).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("bin/tt_setup"), "{msg}");
        assert!(msg.contains("2.39"), "{msg}");
        assert!(
            msg.contains("2.28"),
            "should name the floor it violated: {msg}"
        );
        assert!(
            msg.contains("VFX Reference Platform"),
            "an unset floor must say where the number came from: {msg}"
        );
    }

    /// Declaring the higher floor is the sanctioned escape hatch — it makes the
    /// requirement visible in the manifest instead of silent in the archive.
    #[test]
    fn declaring_the_higher_floor_permits_the_binary() {
        let dir = tempfile::tempdir().unwrap();
        let entries = staged(dir.path(), "bin/tt_setup", &elf_requiring_glibc(2, 39));

        lint_linux_payload(&entries, &compat_with(Some(GlibcVersion::new(2, 39))))
            .expect("declared floor should admit its own binaries");
    }

    #[test]
    fn binary_at_or_below_the_floor_passes() {
        let dir = tempfile::tempdir().unwrap();
        let entries = staged(dir.path(), "bin/tool", &elf_requiring_glibc(2, 17));
        assert!(
            lint_linux_payload(&entries, &compat_with(None))
                .unwrap()
                .is_empty()
        );
    }

    /// A shared object with no search path of its own is advisory, never fatal:
    /// it works fine while every dependency is the host's, which is the normal
    /// case for a Houdini plugin.
    #[test]
    fn shared_object_without_runpath_only_warns() {
        let dir = tempfile::tempdir().unwrap();
        let entries = staged(
            dir.path(),
            "lib/tumbleResolver.so",
            &elf_requiring_glibc(2, 17),
        );

        let warnings = lint_linux_payload(&entries, &compat_with(None)).unwrap();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].message.contains("RUNPATH"), "{warnings:?}");
        assert!(
            warnings[0].message.contains("LD_LIBRARY_PATH"),
            "should explain why the env-var workaround cannot help: {warnings:?}"
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

    #[test]
    fn floor_defaults_to_the_vfx_baseline_and_says_so() {
        assert_eq!(
            compat_with(None).glibc_floor(),
            GlibcVersion::VFX_PLATFORM_BASELINE
        );
        assert!(floor_origin_hint(&compat_with(None)).contains("VFX"));
        assert!(
            floor_origin_hint(&compat_with(Some(GlibcVersion::new(2, 39))))
                .contains("[compat].glibc")
        );
    }
}
