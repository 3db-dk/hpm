//! Process-spawn helpers shared across subsystems.

/// `CREATE_NO_WINDOW`: suppress the console window flash a CLI child (git,
/// uv) would otherwise show when spawned from a GUI parent on Windows.
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Hide the child's console window on Windows; no-op elsewhere.
pub(crate) fn hide_console_std(cmd: &mut std::process::Command) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = cmd;
    }
}

/// Hide the child's console window on Windows; no-op elsewhere.
pub(crate) fn hide_console_tokio(cmd: &mut tokio::process::Command) {
    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = cmd;
    }
}
