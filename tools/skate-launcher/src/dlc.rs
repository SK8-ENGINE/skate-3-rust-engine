//! Tools > Import DLC, as `launcher.ps1` Import-Dlc / Copy-Dlc (doc 44a 1b): every Skate 3 DLC package (checked by
//! its header) copied to `<dlc>/<display name>/<file>`; other files are skipped.
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::state::{setting, Launcher};

/// The DLC folder: config.json `dlc_dir`, else `dlc` in the state folder.
pub fn dlc_dir(l: &Launcher) -> PathBuf {
    setting(&l.config, "dlc_dir").and_then(Value::as_str).map_or_else(|| l.state_dir.join("dlc"), PathBuf::from)
}

/// `Dlc-Name`: a Skate 3 package's display name (Xbox 360 LIVE / PIRS / CON, marketplace content 2, title 454108E6).
pub fn dlc_name(path: &Path) -> Option<String> {
    let mut h = vec![0u8; 0x492];
    std::fs::File::open(path).ok()?.read_exact(&mut h).ok()?;
    if !matches!(&h[0..4], b"LIVE" | b"PIRS" | b"CON ") {
        return None;
    }
    let be = |o: usize| u32::from_be_bytes([h[o], h[o + 1], h[o + 2], h[o + 3]]);
    if be(0x344) != 2 || be(0x360) != 0x454108E6 {
        return None;
    }
    let units: Vec<u16> = h[0x411..0x411 + 0x80].chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
    let name = String::from_utf16_lossy(&units);
    let name = name.split('\0').next().unwrap_or_default().trim().to_owned();
    Some(if name.is_empty() { path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() } else { name })
}

fn packages(folder: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(folder) else { return };
    for entry in read.flatten() {
        let p = entry.path();
        if p.is_dir() {
            packages(&p, out);
        } else if std::fs::metadata(&p).is_ok_and(|m| m.len() >= 0xB000) {
            out.push(p);
        }
    }
}

/// The dialogs when no path is given: files (one or many) or a folder; None = canceled.
fn pick() -> Option<Vec<PathBuf>> {
    let lines = |o: std::process::Output| -> Vec<String> {
        String::from_utf8_lossy(&o.stdout).lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned).collect()
    };
    let picked: Vec<String> = if cfg!(windows) {
        let script = "Add-Type -AssemblyName System.Windows.Forms; $o = New-Object System.Windows.Forms.Form -Property @{ TopMost = $true; ShowInTaskbar = $false; Width = 1; Height = 1 }; $o.Show(); $o.Hide(); \
            $nl = [Environment]::NewLine; $a = [System.Windows.Forms.MessageBox]::Show($o, \"Pick DLC package files?$nl$nl\" + \"Yes: choose one or more package files.$nl\" + 'No: choose a folder (every package inside it).', 'Import DLC', 'YesNoCancel', 'Question'); \
            if ($a -eq 'Yes') { $d = New-Object System.Windows.Forms.OpenFileDialog; $d.Title = 'Skate 3 DLC packages'; $d.Multiselect = $true; $d.Filter = 'All files (*.*)|*.*'; if ($d.ShowDialog($o) -eq 'OK') { $d.FileNames } } \
            elseif ($a -eq 'No') { $d = New-Object System.Windows.Forms.FolderBrowserDialog; $d.Description = 'The folder with your Skate 3 DLC packages (subfolders too)'; if ($d.ShowDialog($o) -eq 'OK') { 'DIR:' + $d.SelectedPath } }";
        lines(crate::proc::cmd("powershell.exe").args(["-NoProfile", "-STA", "-Command", script]).output().ok()?)
    } else if cfg!(target_os = "macos") {
        let script = "set c to button returned of (display dialog \"Pick DLC package files, or a folder with every package inside it?\" buttons {\"Cancel\", \"Folder\", \"Files\"} default button \"Files\" with title \"Import DLC\")\n\
            if c is \"Files\" then\nset fs to choose file with prompt \"Skate 3 DLC packages\" with multiple selections allowed\nset out to \"\"\nrepeat with f in fs\nset out to out & POSIX path of f & linefeed\nend repeat\nreturn out\n\
            else\nreturn \"DIR:\" & POSIX path of (choose folder with prompt \"The folder with your Skate 3 DLC packages\")\nend if";
        lines(crate::proc::cmd("osascript").args(["-e", script]).output().ok()?)
    } else {
        let has = |c: &str| crate::proc::cmd("sh").arg("-c").arg(format!("command -v {c}")).output().is_ok_and(|o| o.status.success());
        if has("zenity") {
            let files = crate::proc::cmd("zenity").args(["--question", "--title=Import DLC", "--text=Pick DLC package files?\n\nYes: choose one or more package files.\nNo: choose a folder (every package inside it).", "--ok-label=Files", "--cancel-label=Folder"]).status().ok()?.success();
            if files {
                lines(crate::proc::cmd("zenity").args(["--file-selection", "--multiple", "--separator=\n", "--title=Skate 3 DLC packages"]).output().ok()?)
            } else {
                let o = crate::proc::cmd("zenity").args(["--file-selection", "--directory", "--title=The folder with your Skate 3 DLC packages"]).output().ok()?;
                lines(o).into_iter().map(|p| format!("DIR:{p}")).collect()
            }
        } else if has("kdialog") {
            let o = crate::proc::cmd("kdialog").args(["--getexistingdirectory", ".", "--title", "The folder with your Skate 3 DLC packages"]).output().ok()?;
            lines(o).into_iter().map(|p| format!("DIR:{p}")).collect()
        } else {
            Vec::new()
        }
    };
    if picked.is_empty() {
        return None;
    }
    let mut files = Vec::new();
    for p in picked {
        match p.strip_prefix("DIR:") {
            Some(dir) => packages(Path::new(dir), &mut files),
            None => files.push(PathBuf::from(p)),
        }
    }
    Some(files)
}

/// `Import-Dlc`: the result line and how many packages were copied.
pub fn import(l: &Launcher, from: &str) -> (String, usize) {
    let files = if from.is_empty() {
        match pick() {
            Some(f) => f,
            None => return ("Import DLC: cancelled.".into(), 0),
        }
    } else {
        let p = Path::new(from);
        if !p.exists() {
            return (format!("Import DLC: {from} not found."), 0);
        }
        if p.is_file() { vec![p.to_path_buf()] } else { let mut v = Vec::new(); packages(p, &mut v); v }
    };
    let dir = dlc_dir(l);
    let (mut added, mut kept, mut skipped) = (Vec::new(), Vec::new(), 0usize);
    for file in files {
        let Some(name) = dlc_name(&file) else { skipped += 1; continue };
        let safe: String = name.chars().map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c }).collect();
        let safe = safe.trim_matches(|c| c == '.' || c == ' ').to_owned();
        let target = dir.join(&safe).join(file.file_name().unwrap_or_default());
        let same = std::fs::metadata(&target).ok().zip(std::fs::metadata(&file).ok()).is_some_and(|(a, b)| a.len() == b.len());
        if same {
            kept.push(name);
            continue;
        }
        println!("  Copying {name}...");
        let _ = std::fs::create_dir_all(target.parent().unwrap_or(&dir));
        if std::fs::copy(&file, &target).is_ok() {
            added.push(name);
        }
    }
    let mut r = format!("Import DLC: {} added", added.len());
    if !added.is_empty() {
        r.push_str(&format!(" ({})", added.join(", ")));
    }
    if !kept.is_empty() {
        r.push_str(&format!(", {} already there", kept.len()));
    }
    if skipped > 0 {
        r.push_str(&format!(", {skipped} other files skipped (not Skate 3 DLC packages)"));
    }
    r.push('.');
    (r, added.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_skate_3_package_header_gives_its_name_and_anything_else_none() {
        let dir = std::env::temp_dir().join(format!("skate-launcher-dlc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut h = vec![0u8; 0x492];
        h[0..4].copy_from_slice(b"LIVE");
        h[0x344..0x348].copy_from_slice(&2u32.to_be_bytes());
        h[0x360..0x364].copy_from_slice(&0x454108E6u32.to_be_bytes());
        for (i, u) in "Maloof Money Cup".encode_utf16().enumerate() {
            h[0x411 + i * 2..0x413 + i * 2].copy_from_slice(&u.to_be_bytes());
        }
        std::fs::write(dir.join("pack"), &h).unwrap();
        assert_eq!(dlc_name(&dir.join("pack")).as_deref(), Some("Maloof Money Cup"));
        h[0x360] = 0;
        std::fs::write(dir.join("other"), &h).unwrap();
        assert_eq!(dlc_name(&dir.join("other")), None, "another title is skipped");
        std::fs::remove_dir_all(&dir).ok();
    }
}
