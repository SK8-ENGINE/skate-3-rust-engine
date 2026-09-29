//! Helper executable and shared-library file names per platform.

/// `"skate3setup"` -> `"skate3setup.exe"` on Windows, unchanged elsewhere.
pub fn exe_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_owned()
    }
}

/// Steamworks runtime library shipped next to the Steam relay helper.
pub fn steam_api_library() -> &'static str {
    if cfg!(windows) {
        "steam_api64.dll"
    } else {
        "libsteam_api.so"
    }
}
