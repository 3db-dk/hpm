//! Recognising a full disk, from an `io::Error` or from a child's message.
//!
//! A full disk reaches us two ways: as an `io::Error` from our own writes,
//! and as text from a tool we ran (uv prints Rust's `io::Error` display,
//! `"No space left on device (os error 28)"`). Both are answered here so a
//! caller deciding what to do, or what to tell the user, asks one rule.
//!
//! Codes are matched per platform because they collide across them: 28 is
//! `ENOSPC` on Linux and macOS but `ERROR_OUT_OF_PAPER` on Windows, and 112 is
//! Windows' `ERROR_DISK_FULL` but `EHOSTDOWN` on Linux. The English messages
//! are matched as well because Windows localises its own, and the code is the
//! part that survives translation.

use std::io;

/// Raw OS error codes that mean the volume is out of space on this platform.
/// `EDQUOT` (an exhausted quota) is included: it reads the same to the user.
#[cfg(target_os = "linux")]
const DISK_FULL_CODES: &[i32] = &[28 /* ENOSPC */, 122 /* EDQUOT */];
#[cfg(target_os = "macos")]
const DISK_FULL_CODES: &[i32] = &[28 /* ENOSPC */, 69 /* EDQUOT */];
#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
const DISK_FULL_CODES: &[i32] = &[28 /* ENOSPC */];
#[cfg(windows)]
const DISK_FULL_CODES: &[i32] = &[
    39,  /* ERROR_HANDLE_DISK_FULL */
    112, /* ERROR_DISK_FULL */
];
#[cfg(not(any(unix, windows)))]
const DISK_FULL_CODES: &[i32] = &[];

const DISK_FULL_PHRASES: &[&str] = &[
    "no space left on device",
    "not enough space on the disk",
    "disk quota exceeded",
];

/// Whether `err` is the volume running out of space.
pub fn is_disk_full(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::StorageFull
        || err.kind() == io::ErrorKind::QuotaExceeded
        || err
            .raw_os_error()
            .is_some_and(|c| DISK_FULL_CODES.contains(&c))
}

/// Whether `message` (a child's stderr, or an error rendered to text)
/// reports the volume running out of space.
pub fn is_disk_full_message(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    DISK_FULL_PHRASES.iter().any(|p| lower.contains(p))
        || DISK_FULL_CODES
            .iter()
            .any(|c| lower.contains(&format!("(os error {c})")))
}

/// Whether any error in `err`'s source chain is the volume running out of
/// space, as an `io::Error` or in its rendered text.
pub fn chain_is_disk_full(err: &(dyn std::error::Error + 'static)) -> bool {
    let mut current = Some(err);
    while let Some(e) = current {
        if e.downcast_ref::<io::Error>().is_some_and(is_disk_full)
            || is_disk_full_message(&e.to_string())
        {
            return true;
        }
        current = e.source();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_uv_s_full_disk_message() {
        // Verbatim from a user's log: uv rewriting a managed CPython's
        // sysconfig on a full disk.
        let stderr = "error: failed to write to file `<HOME>/.hpm/uv-python/\
                      cpython-3.13.1-macos-aarch64-none/lib/python3.13/\
                      _sysconfigdata__darwin_darwin.py`: No space left on device (os error 28)";
        assert!(is_disk_full_message(stderr));
    }

    #[test]
    fn the_damage_a_full_disk_leaves_is_not_itself_a_full_disk() {
        // What every later run printed once the disk had space again: the
        // repair has to be allowed to run on this.
        assert!(!is_disk_full_message("error: Unexpected end of file"));
    }

    #[test]
    fn recognises_windows_message() {
        assert!(is_disk_full_message(
            "There is not enough space on the disk. (os error 112)"
        ));
    }

    #[test]
    fn unrelated_errors_are_not_disk_full() {
        for msg in [
            "Permission denied (os error 13)",
            "error: No interpreter found for Python 3.13.1",
            "connection refused",
            "",
        ] {
            assert!(!is_disk_full_message(msg), "{msg}");
        }
    }

    #[test]
    fn io_error_kinds_and_codes() {
        assert!(is_disk_full(&io::Error::from(io::ErrorKind::StorageFull)));
        assert!(is_disk_full(&io::Error::from(io::ErrorKind::QuotaExceeded)));
        assert!(!is_disk_full(&io::Error::from(io::ErrorKind::NotFound)));
        #[cfg(unix)]
        assert!(is_disk_full(&io::Error::from_raw_os_error(28)));
        #[cfg(windows)]
        assert!(is_disk_full(&io::Error::from_raw_os_error(112)));
    }

    #[test]
    fn chain_finds_a_wrapped_io_error() {
        #[derive(Debug, thiserror::Error)]
        #[error("failed to write the archive")]
        struct Wrapper(#[source] io::Error);
        let err = Wrapper(io::Error::from(io::ErrorKind::StorageFull));
        assert!(chain_is_disk_full(&err));
        let other = Wrapper(io::Error::from(io::ErrorKind::NotFound));
        assert!(!chain_is_disk_full(&other));
    }
}
