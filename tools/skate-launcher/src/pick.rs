//! Native folder and file pickers on every OS: the Windows Forms dialogs through PowerShell, the macOS dialogs through
//! osascript, zenity or kdialog on Linux.
use std::path::PathBuf;
use std::process::Command;

fn first_line(out: std::process::Output) -> Option<PathBuf> {
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (out.status.success() && !text.is_empty()).then(|| PathBuf::from(text.lines().next().unwrap_or_default()))
}

fn has(cmd: &str) -> bool {
    Command::new("sh").arg("-c").arg(format!("command -v {cmd}")).output().is_ok_and(|o| o.status.success())
}

fn ps_quote(s: &str) -> String {
    s.replace('\'', "''")
}

/// A folder, or None when cancelled.
pub fn folder(title: &str) -> Option<PathBuf> {
    if cfg!(windows) {
        let script = format!("Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.FolderBrowserDialog; $d.Description = '{}'; if ($d.ShowDialog() -eq 'OK') {{ $d.SelectedPath }}", ps_quote(title));
        return first_line(Command::new("powershell.exe").args(["-NoProfile", "-STA", "-Command", &script]).output().ok()?);
    }
    if cfg!(target_os = "macos") {
        return first_line(Command::new("osascript").args(["-e", &format!("POSIX path of (choose folder with prompt \"{}\")", title.replace('"', "'"))]).output().ok()?);
    }
    if has("zenity") {
        return first_line(Command::new("zenity").args(["--file-selection", "--directory", &format!("--title={title}")]).output().ok()?);
    }
    if has("kdialog") {
        return first_line(Command::new("kdialog").args(["--getexistingdirectory", ".", "--title", title]).output().ok()?);
    }
    None
}

/// A file, or None when cancelled. `filter`: a Windows filter (`ISO or default.xex|*.iso;default.xex|All files|*.*`).
pub fn file(title: &str, filter: &str) -> Option<PathBuf> {
    if cfg!(windows) {
        let script = format!("Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.OpenFileDialog; $d.Title = '{}'; $d.Filter = '{}'; if ($d.ShowDialog() -eq 'OK') {{ $d.FileName }}", ps_quote(title), ps_quote(filter));
        return first_line(Command::new("powershell.exe").args(["-NoProfile", "-STA", "-Command", &script]).output().ok()?);
    }
    if cfg!(target_os = "macos") {
        return first_line(Command::new("osascript").args(["-e", &format!("POSIX path of (choose file with prompt \"{}\")", title.replace('"', "'"))]).output().ok()?);
    }
    if has("zenity") {
        return first_line(Command::new("zenity").args(["--file-selection", &format!("--title={title}")]).output().ok()?);
    }
    if has("kdialog") {
        return first_line(Command::new("kdialog").args(["--getopenfilename", ".", "--title", title]).output().ok()?);
    }
    None
}
