//! Minimal ELF reader for the pack-time platform lint.
//!
//! Deliberately not a general ELF crate: it answers exactly the two questions
//! [`super::platform_lint`] asks of a Linux payload — which glibc symbol
//! versions the object references, and whether it carries a library search
//! path of its own. Both come from the `PT_DYNAMIC` segment, which is what the
//! runtime loader itself reads, so the answers hold even for an object whose
//! section headers have been stripped.
//!
//! Every parse failure is reported as "not inspectable" rather than an error.
//! A lint that cannot read a file must not be able to fail a release.

use std::path::Path;

/// A glibc `major.minor` version. Patch levels don't exist in glibc's symbol
/// versioning (`GLIBC_2.39`), so two components is the whole grammar.
///
/// Lives here rather than in the manifest because it is a fact *read off a
/// binary*, never something an author writes. An earlier revision made it a
/// `[compat].glibc` manifest key checked against a hardcoded baseline; that
/// was the wrong shape twice over — it named one platform's mechanism in a
/// cross-platform schema, and because `[compat]` rejects unknown fields, the
/// escape hatch from the check would itself have broken older hpm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GlibcVersion {
    pub major: u32,
    pub minor: u32,
}

impl GlibcVersion {
    /// glibc 2.28 — the [VFX Reference Platform](https://vfxplatform.com/)
    /// requirement for CY2025 and CY2026, i.e. the Houdini 21 and Houdini 22
    /// series, and in practice an EL8-era host. (Even the CY2027 draft only
    /// moves to 2.34.) Used solely to phrase a warning; nothing is enforced
    /// against it, so it carries no policy and can go stale harmlessly.
    pub const VFX_PLATFORM_BASELINE: GlibcVersion = GlibcVersion {
        major: 2,
        minor: 28,
    };

    #[cfg(test)]
    pub const fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }
}

impl std::fmt::Display for GlibcVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

impl std::str::FromStr for GlibcVersion {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (major, minor) = s.trim().split_once('.').ok_or(())?;
        Ok(Self {
            major: major.parse().map_err(|_| ())?,
            minor: minor.parse().map_err(|_| ())?,
        })
    }
}

/// What the lint could learn about one ELF object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ElfInfo {
    /// Highest `GLIBC_x.y` version referenced by the object's symbol version
    /// requirements. `None` when it references none (a static binary, or one
    /// that links no versioned glibc symbol).
    pub max_glibc: Option<GlibcVersion>,
    /// Whether the object carries `DT_RUNPATH` or `DT_RPATH`.
    pub has_runpath: bool,
    /// Whether the object is a shared object (`ET_DYN`) rather than an
    /// executable. Both are `ET_DYN` for a PIE, so this is only used to soften
    /// advice, never to gate an error.
    pub is_shared_object: bool,
}

const ELF_MAGIC: &[u8; 4] = b"\x7fELF";

// e_ident
const EI_CLASS: usize = 4;
const EI_DATA: usize = 5;
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;

// Program header types
const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;

// Dynamic tags
const DT_NULL: i64 = 0;
const DT_STRTAB: i64 = 5;
const DT_RPATH: i64 = 15;
const DT_RUNPATH: i64 = 29;
const DT_VERNEED: i64 = 0x6fff_fffe;
const DT_VERNEEDNUM: i64 = 0x6fff_ffff;

/// True if `bytes` opens with the ELF magic. Cheap pre-filter so the lint only
/// pays for a full parse on files that could possibly be Linux payloads.
pub(crate) fn is_elf(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && &bytes[..4] == ELF_MAGIC
}

/// Inspect an ELF object.
///
/// Returns `None` when the file isn't an ELF we can read: not ELF at all, not
/// 64-bit little-endian (the only class Houdini's Linux platforms use), or
/// structurally malformed. Callers treat that as "nothing to say about this
/// file", never as a failure.
pub(crate) fn inspect(bytes: &[u8]) -> Option<ElfInfo> {
    if !is_elf(bytes) {
        return None;
    }
    // 64-bit LE only. x86_64 and aarch64 Linux are both ELFCLASS64/LSB; a
    // 32-bit or big-endian object is out of scope rather than an error.
    if *bytes.get(EI_CLASS)? != ELFCLASS64 || *bytes.get(EI_DATA)? != ELFDATA2LSB {
        return None;
    }

    let e_type = u16(bytes, 0x10)?;
    let e_phoff = u64_at(bytes, 0x20)? as usize;
    let e_phentsize = u16(bytes, 0x36)? as usize;
    let e_phnum = u16(bytes, 0x38)? as usize;

    // Collect PT_LOAD segments so virtual addresses in the dynamic section can
    // be mapped back to file offsets, and find PT_DYNAMIC.
    let mut loads: Vec<(u64, u64, u64)> = Vec::new(); // (vaddr, offset, filesz)
    let mut dynamic: Option<(usize, usize)> = None; // (offset, size)
    for i in 0..e_phnum {
        let ph = e_phoff.checked_add(i.checked_mul(e_phentsize)?)?;
        let p_type = u32_at(bytes, ph)?;
        let p_offset = u64_at(bytes, ph + 0x08)?;
        let p_vaddr = u64_at(bytes, ph + 0x10)?;
        let p_filesz = u64_at(bytes, ph + 0x20)?;
        match p_type {
            PT_LOAD => loads.push((p_vaddr, p_offset, p_filesz)),
            PT_DYNAMIC => dynamic = Some((p_offset as usize, p_filesz as usize)),
            _ => {}
        }
    }

    let to_offset = |vaddr: u64| -> Option<usize> {
        loads
            .iter()
            .find(|(va, _, sz)| vaddr >= *va && vaddr < va.saturating_add(*sz))
            .map(|(va, off, _)| (off + (vaddr - va)) as usize)
    };

    let mut info = ElfInfo {
        max_glibc: None,
        has_runpath: false,
        // ET_DYN == 3. A PIE executable is also ET_DYN, which is why this only
        // ever softens wording.
        is_shared_object: e_type == 3,
    };

    let Some((dyn_off, dyn_size)) = dynamic else {
        return Some(info);
    };

    let mut strtab: Option<u64> = None;
    let mut verneed: Option<u64> = None;
    let mut verneednum: u64 = 0;
    for i in 0..(dyn_size / 16) {
        let e = dyn_off.checked_add(i * 16)?;
        let tag = i64_at(bytes, e)?;
        let val = u64_at(bytes, e + 8)?;
        match tag {
            DT_NULL => break,
            DT_STRTAB => strtab = Some(val),
            DT_RPATH | DT_RUNPATH => info.has_runpath = true,
            DT_VERNEED => verneed = Some(val),
            DT_VERNEEDNUM => verneednum = val,
            _ => {}
        }
    }

    // Version requirements name their versions through the dynamic string
    // table; without it there is nothing to read.
    let (Some(strtab), Some(verneed)) = (strtab, verneed) else {
        return Some(info);
    };
    let (Some(str_off), Some(vn_off)) = (to_offset(strtab), to_offset(verneed)) else {
        return Some(info);
    };

    info.max_glibc = max_glibc_version(bytes, str_off, vn_off, verneednum);
    Some(info)
}

/// Walk the `Elf64_Verneed` chain and return the highest `GLIBC_x.y` found.
///
/// Each `Verneed` names one needed object and chains `Vernaux` entries for the
/// versions required from it. Only `GLIBC_`-prefixed names matter here; the
/// `GCC_`/`CXXABI_`/`GLIBCXX_` sets belong to libgcc and libstdc++, which ship
/// with the toolchain rather than the host C library.
fn max_glibc_version(
    bytes: &[u8],
    str_off: usize,
    vn_off: usize,
    count: u64,
) -> Option<GlibcVersion> {
    let read_str = |off: usize| -> Option<&str> {
        let start = str_off.checked_add(off)?;
        let rest = bytes.get(start..)?;
        let end = rest.iter().position(|b| *b == 0)?;
        std::str::from_utf8(&rest[..end]).ok()
    };

    let mut max: Option<GlibcVersion> = None;
    let mut cursor = vn_off;
    // `count` bounds the walk, and `vn_next == 0` terminates it. Both are
    // honoured so a malformed chain can't spin.
    for _ in 0..count.min(4096) {
        let vn_cnt = u16(bytes, cursor + 2)? as usize;
        let vn_aux = u32_at(bytes, cursor + 8)? as usize;
        let vn_next = u32_at(bytes, cursor + 12)? as usize;

        let mut aux = cursor.checked_add(vn_aux)?;
        for _ in 0..vn_cnt.min(4096) {
            let vna_name = u32_at(bytes, aux + 8)? as usize;
            let vna_next = u32_at(bytes, aux + 12)? as usize;
            if let Some(name) = read_str(vna_name)
                && let Some(v) = parse_glibc_symbol_version(name)
            {
                max = Some(max.map_or(v, |m: GlibcVersion| m.max(v)));
            }
            if vna_next == 0 {
                break;
            }
            aux = aux.checked_add(vna_next)?;
        }

        if vn_next == 0 {
            break;
        }
        cursor = cursor.checked_add(vn_next)?;
    }
    max
}

/// `"GLIBC_2.39"` -> `2.39`. Anything else (including `GLIBC_PRIVATE`) is None.
fn parse_glibc_symbol_version(name: &str) -> Option<GlibcVersion> {
    name.strip_prefix("GLIBC_")?.parse().ok()
}

fn u16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}
fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}
fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}
fn i64_at(b: &[u8], at: usize) -> Option<i64> {
    Some(i64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// Read `path` and inspect it, returning `None` for anything unreadable or not
/// an inspectable ELF.
pub(crate) fn inspect_path(path: &Path) -> Option<ElfInfo> {
    inspect(&std::fs::read(path).ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_elf_is_not_inspectable() {
        assert!(inspect(b"[package]\nname = \"x\"").is_none());
        assert!(inspect(b"").is_none());
        assert!(!is_elf(b"MZ"));
    }

    #[test]
    fn truncated_elf_does_not_panic() {
        // Header magic and class/data set, then nothing — every field read
        // past the end must fold into None rather than panicking.
        let mut b = vec![0u8; 8];
        b[..4].copy_from_slice(ELF_MAGIC);
        b[EI_CLASS] = ELFCLASS64;
        b[EI_DATA] = ELFDATA2LSB;
        assert!(inspect(&b).is_none());
    }

    #[test]
    fn thirty_two_bit_and_big_endian_are_out_of_scope() {
        let mut b = vec![0u8; 128];
        b[..4].copy_from_slice(ELF_MAGIC);
        b[EI_CLASS] = 1; // ELFCLASS32
        b[EI_DATA] = ELFDATA2LSB;
        assert!(inspect(&b).is_none());

        b[EI_CLASS] = ELFCLASS64;
        b[EI_DATA] = 2; // ELFDATA2MSB
        assert!(inspect(&b).is_none());
    }

    #[test]
    fn parses_glibc_symbol_versions() {
        assert_eq!(
            parse_glibc_symbol_version("GLIBC_2.39"),
            Some(GlibcVersion::new(2, 39))
        );
        assert_eq!(
            parse_glibc_symbol_version("GLIBC_2.2.5"),
            None,
            "three components is not the glibc symbol grammar"
        );
        assert_eq!(parse_glibc_symbol_version("GLIBC_PRIVATE"), None);
        assert_eq!(parse_glibc_symbol_version("GCC_3.0"), None);
        assert_eq!(parse_glibc_symbol_version("GLIBCXX_3.4"), None);
    }

    /// Ordering is what the floor comparison rests on, and it must be numeric
    /// rather than lexical — "2.9" is older than "2.28", not newer.
    #[test]
    fn versions_order_numerically() {
        assert!(GlibcVersion::new(2, 28) < GlibcVersion::new(2, 39));
        assert!(GlibcVersion::new(2, 9) < GlibcVersion::new(2, 28));
        assert!(GlibcVersion::new(2, 34) > GlibcVersion::new(2, 28));
    }
}
