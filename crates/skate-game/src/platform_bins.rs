//! Portable helper binary names. Windows packages ship `.exe`/`.dll`;
//! macOS/Linux ship extension-less Mach-O/ELF binaries and `.dylib`/`.so`.
use std::path::{Path, PathBuf};

pub(crate) fn support_bin(root: &Path, stem: &str) -> PathBuf {
    if cfg!(windows) {
        root.join(format!("support/{stem}.exe"))
    } else {
        root.join(format!("support/{stem}"))
    }
}

pub(crate) fn setup_helper(root: &Path) -> PathBuf {
    support_bin(root, "skate3setup")
}

pub(crate) fn update_helper(root: &Path) -> PathBuf {
    support_bin(root, "skate3update")
}

pub(crate) fn relay_bin(dir: &Path) -> PathBuf {
    if cfg!(windows) {
        dir.join("steam-relay/skate-steam-relay.exe")
    } else {
        dir.join("steam-relay/skate-steam-relay")
    }
}

pub(crate) fn steam_api_lib(dir: &Path) -> PathBuf {
    if cfg!(windows) {
        dir.join("steam-relay/steam_api64.dll")
    } else if cfg!(target_os = "macos") {
        dir.join("steam-relay/libsteam_api.dylib")
    } else {
        dir.join("steam-relay/libsteam_api.so")
    }
}

pub(crate) fn setup_missing_message() -> String {
    if cfg!(windows) {
        "This copy has not been set up. Use the complete Windows package, or --assets DIRECTORY for development.".into()
    } else if cfg!(target_os = "macos") {
        "This copy has not been set up. Use the complete macOS package, or --assets DIRECTORY for development.".into()
    } else {
        "This copy has not been set up. Use the complete Linux package, or --assets DIRECTORY for development.".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn helper_names_follow_platform_conventions() {
        let root = Path::new("/game");
        let (setup, update, relay, api) = if cfg!(windows) {
            ("support/skate3setup.exe", "support/skate3update.exe", "steam-relay/skate-steam-relay.exe", "steam-relay/steam_api64.dll")
        } else if cfg!(target_os = "macos") {
            ("support/skate3setup", "support/skate3update", "steam-relay/skate-steam-relay", "steam-relay/libsteam_api.dylib")
        } else {
            ("support/skate3setup", "support/skate3update", "steam-relay/skate-steam-relay", "steam-relay/libsteam_api.so")
        };
        assert_eq!(setup_helper(root), Path::new("/game").join(setup));
        assert_eq!(update_helper(root), Path::new("/game").join(update));
        assert_eq!(relay_bin(root), Path::new("/game").join(relay));
        assert_eq!(steam_api_lib(root), Path::new("/game").join(api));
        assert!(setup_missing_message().contains("--assets DIRECTORY"));
    }
}
