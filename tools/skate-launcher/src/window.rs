//! Where the window opens and how big it is, and one launcher per folder (doc 44a 2, SkateLauncher.cs StartScreen /
//! Main): 62 % of the work area's height at the panel's 1240 x 760 shape, centred, on the main monitor or (Tools >
//! Open on, or `--same-monitor`) on the monitor of the window it was started from.
use std::fs::File;
use std::path::{Path, PathBuf};

/// The Tools setting: present = open on the monitor the launcher was started from.
pub fn same_monitor_file(state_dir: &Path) -> PathBuf {
    state_dir.join("open-on-launching-monitor.txt")
}

/// The window's size for a work area of `w` x `h`.
pub fn fit(w: f32, h: f32) -> (f32, f32) {
    let height = (h * 0.62).min(w * 0.62 * 760.0 / 1240.0).floor();
    ((height * 1240.0 / 760.0).floor(), height)
}

/// The chosen monitor's work area in points (x, y, w, h). Windows only; elsewhere the window centres itself and sizes
/// from the monitor on its first frame.
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn start_area(same_monitor: bool) -> Option<(f32, f32, f32, f32)> {
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTOPRIMARY};
    use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, MDT_EFFECTIVE_DPI};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetForegroundWindow};
    // SAFETY: plain Win32 queries with valid out pointers; the awareness call is the one winit makes anyway (a second
    // call fails harmlessly).
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let fg = if same_monitor { GetForegroundWindow() } else { std::ptr::null_mut() };
        // No foreground window: the monitor under the cursor (SkateLauncher.cs StartScreen).
        let mut cursor = POINT { x: 0, y: 0 };
        if same_monitor && fg.is_null() { GetCursorPos(&mut cursor); }
        let mon = if fg.is_null() { MonitorFromPoint(cursor, MONITOR_DEFAULTTOPRIMARY) } else { MonitorFromWindow(fg, MONITOR_DEFAULTTOPRIMARY) };
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, rcMonitor: RECT { left: 0, top: 0, right: 0, bottom: 0 }, rcWork: RECT { left: 0, top: 0, right: 0, bottom: 0 }, dwFlags: 0 };
        if GetMonitorInfoW(mon, &mut info) == 0 {
            return None;
        }
        let (mut dx, mut dy) = (96u32, 96u32);
        let scale = if GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) == 0 { dx as f32 / 96.0 } else { 1.0 };
        let r = info.rcWork;
        Some((r.left as f32 / scale, r.top as f32 / scale, (r.right - r.left) as f32 / scale, (r.bottom - r.top) as f32 / scale))
    }
}

#[cfg(not(windows))]
pub fn start_area(_same_monitor: bool) -> Option<(f32, f32, f32, f32)> {
    None
}

static INSTANCE: std::sync::Mutex<Option<File>> = std::sync::Mutex::new(None);

/// One launcher per state folder. True for the first (it holds the lock until it exits or hands over); false when
/// one already runs: that one was asked to come to the front (`raise.txt`, polled by the window).
pub fn single_instance(state_dir: &Path) -> bool {
    let _ = std::fs::create_dir_all(state_dir);
    let Ok(file) = File::options().create(true).write(true).truncate(false).open(state_dir.join("launcher.lock")) else { return true };
    match file.try_lock() {
        Ok(()) => {
            if let Ok(mut g) = INSTANCE.lock() { *g = Some(file); }
            true
        }
        Err(_) => {
            let _ = std::fs::write(raise_file(state_dir), "raise\n");
            false
        }
    }
}

/// Lets an updated launcher started by this one take the folder (SkateLauncher.cs ReleaseInstance).
pub fn release_instance() {
    if let Ok(mut g) = INSTANCE.lock() { *g = None; }
}

/// Free space on the drive holding `path`, in bytes.
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn free_bytes(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    let mut dir = path.to_path_buf();
    while !dir.exists() { dir = dir.parent()?.to_path_buf(); }
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut free = 0u64;
    // SAFETY: a NUL-terminated path and valid out pointers.
    let ok = unsafe { windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) };
    (ok != 0).then_some(free)
}

/// Free space on the drive holding `path`, in bytes (`df -Pk`).
#[cfg(not(windows))]
pub fn free_bytes(path: &Path) -> Option<u64> {
    let mut dir = path.to_path_buf();
    while !dir.exists() { dir = dir.parent()?.to_path_buf(); }
    let out = crate::proc::cmd("df").arg("-Pk").arg(&dir).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let kb: u64 = text.lines().nth(1)?.split_whitespace().nth(3)?.parse().ok()?;
    Some(kb * 1024)
}

/// Lets the game the launcher starts take the foreground (Steam gives the foreground window its controller layout).
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn allow_foreground() {
    // SAFETY: a plain Win32 call; ASFW_ANY lets any process the launcher starts set the foreground window.
    unsafe { windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(windows_sys::Win32::UI::WindowsAndMessaging::ASFW_ANY) };
}

#[cfg(not(windows))]
pub fn allow_foreground() {}

pub fn raise_file(state_dir: &Path) -> PathBuf {
    state_dir.join("raise.txt")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_is_62_percent_of_the_work_area_at_the_panel_shape() {
        assert_eq!(fit(1920.0, 1032.0), (1042.0, 639.0));
        // A narrow screen: limited by the width.
        assert_eq!(fit(1001.0, 1200.0), (620.0, 380.0));
    }

    #[test]
    fn a_second_launcher_asks_the_first_to_come_forward() {
        let dir = std::env::temp_dir().join(format!("skate-launcher-instance-{}", std::process::id()));
        assert!(single_instance(&dir));
        assert!(!single_instance(&dir));
        assert!(raise_file(&dir).exists());
        release_instance();
        assert!(single_instance(&dir));
        release_instance();
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// This thread's top-level window with Windows' own open / close animation switched off (the classic's layered window
/// has none; it hid the start of the swing). Returns whether the window is on screen yet.
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn own_window_shown() -> bool {
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_TRANSITIONS_FORCEDISABLED};
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumThreadWindows, IsWindowVisible};
    unsafe extern "system" fn each(h: HWND, shown: LPARAM) -> BOOL {
        let off: BOOL = 1;
        // SAFETY: h comes from EnumThreadWindows; the attribute is a BOOL read during the call; shown points at a bool
        // that outlives the enumeration.
        unsafe {
            DwmSetWindowAttribute(h, DWMWA_TRANSITIONS_FORCEDISABLED as u32, (&off as *const BOOL).cast(), 4);
            if IsWindowVisible(h) != 0 { *(shown as *mut bool) = true; }
        }
        1
    }
    let mut shown = false;
    // SAFETY: enumerates this thread's windows with a callback that only touches them and the local flag.
    unsafe { EnumThreadWindows(GetCurrentThreadId(), Some(each), (&mut shown as *mut bool) as LPARAM); }
    shown
}
#[cfg(not(windows))]
pub fn own_window_shown() -> bool {
    true
}
