//! Crash reporting helpers shared by the supervisor.
use std::path::Path;

/// Kernel/OS version string for diagnostic reports. Windows reads it natively
/// in the game's crash module; other platforms use uname(2) via std env.
#[cfg(not(windows))]
pub fn os_version() -> String {
    std::process::Command::new("uname")
        .arg("-sr")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "OS version unavailable".into())
}

/// Show the saved report to the user. On Windows a hidden PowerShell window
/// renders the report UI; elsewhere the console message already points at the
/// saved file, so this is a no-op success.
pub fn report_popup(path: &Path, #[cfg_attr(not(windows), allow(unused))] windows_script: &str) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        let status = crate::process::detached(
            std::process::Command::new("powershell.exe")
                .args([
                    "-NoProfile",
                    "-STA",
                    "-NonInteractive",
                    "-WindowStyle",
                    "Hidden",
                    "-Command",
                    windows_script,
                ])
                .env("SKATE_REPORT_PATH", path),
        )
        .status()?;
        if !status.success() {
            return Err(std::io::Error::other("PowerShell UI failed"));
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
    }
    Ok(())
}
