//! Child processes without console windows. The launcher is a windowed program on Windows (Steam starts it), so a
//! console tool it starts (git, cargo, PowerShell, cmd for PLAY.bat) would open a console window of its own; this
//! gives every child CREATE_NO_WINDOW (SkateLauncher.cs: CreateNoWindow = true).
use std::ffi::OsStr;
use std::process::Command;

pub fn cmd<S: AsRef<OsStr>>(program: S) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c
}

/// Started from a terminal, the text commands print there (a windowed program has no console of its own).
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn attach_parent_console() {
    // SAFETY: a plain Win32 call; it fails harmlessly when the parent has no console (Steam, Explorer).
    unsafe { windows_sys::Win32::System::Console::AttachConsole(windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS) };
}

#[cfg(not(windows))]
pub fn attach_parent_console() {}
