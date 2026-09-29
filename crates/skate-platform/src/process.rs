//! Detached helper-process spawning. Windows helpers must not pop a console
//! window; elsewhere a plain spawn already detaches from any console.
use std::process::Command;

/// CREATE_NO_WINDOW on Windows; a no-op elsewhere.
pub fn detached(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}
