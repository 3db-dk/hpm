//! Whether a recorded process still lives, decided by asking the OS.
//!
//! A content copy may only be deleted when nothing still reads it, and the
//! reader that matters is a *process*: Houdini, hython or a package script
//! reading files out of the copy for as long as it runs, opening most of them
//! lazily (an HDA when a node is first created, a Python module when first
//! imported). On Windows a file held open blocks a rename of its directory,
//! which a careful delete can lean on; on macOS and Linux nothing does, so a
//! record naming the process is the only protection there is.
//!
//! A PID alone does not name a process: the OS reuses PIDs, so a record
//! carries the OS's start time for the PID too, and a live PID whose start
//! time disagrees is a different process. Every case that cannot be settled
//! (the OS could not be asked, a start time could not be read) resolves to
//! "still alive": a copy kept too long costs disk, one deleted too early costs
//! a running session.
//!
//! This is the one rule for both layers of dev copies: hpm's `_dev` copies
//! (see `storage::dev_users`) and an embedder's per-session copies made from
//! them, which record their Houdini the same way.

use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate};

/// One process, as recorded by whoever started it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessRecord {
    pub pid: u32,
    /// The OS's start time for `pid`, in seconds since the Unix epoch, as
    /// `sysinfo` reports it. `None` when it could not be read; the PID alone
    /// then decides, which can only err toward "alive".
    pub started_at: Option<u64>,
}

/// What the OS says about one PID right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Gone,
    /// The OS could not be asked. Read as alive by [`ProcessRecord::is_live`].
    Unknown,
    /// Alive, with the OS start time when it was readable.
    Alive {
        started_at: Option<u64>,
    },
}

/// Start times are whole seconds derived from boot time plus a tick count on
/// some platforms, so two reads of the same process can disagree by one. A
/// recycled PID is a different process started at a different time, seconds
/// apart at the very least, so a small tolerance loses nothing.
pub const START_TIME_TOLERANCE_SECS: u64 = 2;

impl ProcessRecord {
    /// A record of `pid` as it is now, start time included when readable.
    pub fn of(pid: u32) -> Self {
        let started_at = match process_state(pid) {
            ProcessState::Alive { started_at } => started_at,
            ProcessState::Gone | ProcessState::Unknown => None,
        };
        Self { pid, started_at }
    }

    /// A record of this process.
    pub fn current() -> Self {
        Self::of(std::process::id())
    }

    /// Whether the recorded process still runs, given the OS's answer for its
    /// PID. Gone is dead; a live PID started at a different time is a
    /// recycled PID, so also dead; everything else (unknown, or no start time
    /// on either side) is alive.
    pub fn is_live(&self, state_of: impl FnOnce(u32) -> ProcessState) -> bool {
        match state_of(self.pid) {
            ProcessState::Gone => false,
            ProcessState::Unknown => true,
            ProcessState::Alive { started_at } => match (self.started_at, started_at) {
                (Some(recorded), Some(now)) => recorded.abs_diff(now) <= START_TIME_TOLERANCE_SECS,
                _ => true,
            },
        }
    }
}

/// Ask the OS about one PID. Refreshes only that PID, never the whole table.
pub fn process_state(pid: u32) -> ProcessState {
    let Ok(mut sys) = sysinfo::System::new() else {
        return ProcessState::Unknown;
    };
    let target = Pid::from_u32(pid);
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[target]),
        true,
        ProcessRefreshKind::nothing(),
    );
    match sys.process(target) {
        Some(p) => ProcessState::Alive {
            started_at: Some(p.start_time()).filter(|&t| t > 0),
        },
        None => ProcessState::Gone,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: u64 = 1_758_000_000;

    /// Every combination of OS answer and recorded start time, enumerated:
    /// only "gone" and "alive but started at a clearly different time" are
    /// dead.
    #[test]
    fn liveness_over_every_case() {
        let recorded = [None, Some(T)];
        let answers = [
            ProcessState::Gone,
            ProcessState::Unknown,
            ProcessState::Alive { started_at: None },
            ProcessState::Alive {
                started_at: Some(T),
            },
            ProcessState::Alive {
                started_at: Some(T + START_TIME_TOLERANCE_SECS),
            },
            ProcessState::Alive {
                started_at: Some(T + START_TIME_TOLERANCE_SECS + 1),
            },
            ProcessState::Alive {
                started_at: Some(T - START_TIME_TOLERANCE_SECS - 1),
            },
        ];
        for started_at in recorded {
            for answer in answers {
                let rec = ProcessRecord { pid: 7, started_at };
                let expected = match (answer, started_at) {
                    (ProcessState::Gone, _) => false,
                    (
                        ProcessState::Alive {
                            started_at: Some(now),
                        },
                        Some(r),
                    ) => r.abs_diff(now) <= START_TIME_TOLERANCE_SECS,
                    _ => true,
                };
                assert_eq!(
                    rec.is_live(|_| answer),
                    expected,
                    "{answer:?} vs {started_at:?}"
                );
            }
        }
    }

    #[test]
    fn this_process_is_live() {
        assert!(ProcessRecord::current().is_live(process_state));
    }

    /// The wire shape embedders already store (`{"pid":..,"startedAt":..}`).
    #[test]
    fn record_reads_the_camel_case_shape() {
        let rec: ProcessRecord = serde_json::from_str(r#"{"pid":12,"startedAt":34}"#).unwrap();
        assert_eq!(
            rec,
            ProcessRecord {
                pid: 12,
                started_at: Some(34)
            }
        );
    }
}
